//! Query-side embedding resolution (`AstLowerer`-free AST rewrite).
//!
//! Split from `resolve.rs` (size hygiene): the `QUERY` collect → batch →
//! apply pipeline, the shared embed-target names and batch helpers, plus its
//! tests. The statement dispatcher and the upsert half stay in `resolve`,
//! which imports the shared names from here, so the dependency runs one way
//! (`resolve` → `resolve_query`).

use std::future::Future;
use std::pin::Pin;

use qql_core::ast::{
    Prefetch, PrefetchSource, QueryExpr, QueryInput, QueryStmt, VectorKind, VectorTarget,
    VectorValue,
};
use qql_core::error::QqlError;

use crate::embedder::Embedder;
use crate::sparse::SparseVector;

/// Default named dense vector for auto-embedding.
pub const DENSE_VECTOR_NAME: &str = "dense";
/// Default named sparse vector for auto-embedding.
pub const SPARSE_VECTOR_NAME: &str = "sparse";

#[derive(Debug, Clone, Copy)]
struct EmbedTarget {
    kind: VectorKind,
    multi: bool,
}

/// Resolve embed target for a `USING` clause.
///
/// - No `USING` → single dense.
/// - `USING name AS …` / schema-filled kind → that kind; `multi` from AS MULTI or schema.
/// - `USING name` with `kind: None` → error.
fn require_embed_target(target: &Option<VectorTarget>) -> Result<EmbedTarget, QqlError> {
    match target {
        None => Ok(EmbedTarget {
            kind: VectorKind::Dense,
            multi: false,
        }),
        Some(t) => match t.kind {
            Some(kind) => Ok(EmbedTarget {
                kind,
                multi: t.multi,
            }),
            None => Err(crate::topology::unknown_using_kind_error(&t.name)),
        },
    }
}

pub(crate) fn ensure_batch_len(got: usize, expected: usize, model: &str) -> Result<(), QqlError> {
    if got != expected {
        return Err(QqlError::execution(
            "QQL-EMBEDDING",
            format!(
                "embed_dense_batch returned {got} vectors for {expected} texts (model={model})"
            ),
            None,
        ));
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
#[cfg(target_arch = "wasm32")]
type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

pub(crate) async fn resolve_query_embeddings(
    query: &mut QueryStmt,
    embedder: &dyn Embedder,
) -> Result<(), QqlError> {
    let mut jobs = Vec::new();
    collect_query_jobs(query, &mut jobs)?;

    let outputs = batch_query_jobs(embedder, &jobs).await?;

    let mut cursor = ApplyCursor::new(&jobs, outputs);
    apply_query_embeddings(query, &mut cursor).await?;
    cursor.finish()
}

// ── Ordered embed jobs: collect produces one list, apply consumes it ────
//
// One AST walk fills a single job list in walk order (CTEs, then expression;
// prefetch members in order; positive before negative; Hybrid dense leg
// before its sparse leg). Batching groups positions by (modality, model) for
// one RPC per group and scatters results back into job order. Apply walks the
// same order and checks every step against the job list, so a future drift
// between the two walks fails closed instead of silently mis-binding vectors.

/// One query-side embedding job in walk order.
#[derive(Debug)]
struct QueryJob {
    modality: JobModality,
    model: String,
    text: String,
}

/// Embedding modality: selects the batch RPC and the apply rewrite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JobModality {
    Dense,
    Sparse,
    Multi,
    Image,
}

/// One batched result, aligned 1:1 with [`QueryJob`] by index.
#[derive(Debug)]
enum JobOutput {
    Dense(Vec<f32>),
    Sparse(SparseVector),
    Multi(Vec<Vec<f32>>),
    Image(Vec<f32>),
}

/// Error code + leg noun per modality, matching the legacy messages.
fn modality_code(modality: JobModality) -> (&'static str, &'static str) {
    match modality {
        JobModality::Dense => ("QQL-EMBEDDING", "dense"),
        JobModality::Sparse => ("QQL-EMBEDDING-SPARSE", "sparse"),
        JobModality::Multi => ("QQL-EMBEDDING-MULTI", "multi"),
        JobModality::Image => ("QQL-EMBEDDING-IMAGE", "image"),
    }
}

fn collect_query_jobs(query: &QueryStmt, jobs: &mut Vec<QueryJob>) -> Result<(), QqlError> {
    for cte in &query.ctes {
        collect_expr_jobs(&cte.query.expression, jobs)?;
    }
    collect_expr_jobs(&query.expression, jobs)
}

fn collect_prefetches_jobs(
    prefetches: &[Prefetch],
    jobs: &mut Vec<QueryJob>,
) -> Result<(), QqlError> {
    for pref in prefetches {
        if let PrefetchSource::Query(sub) = &pref.source {
            collect_query_jobs(sub, jobs)?;
        }
    }
    Ok(())
}

fn collect_expr_jobs(expr: &QueryExpr, jobs: &mut Vec<QueryJob>) -> Result<(), QqlError> {
    match expr {
        QueryExpr::Nearest {
            input,
            using,
            prefetch,
            ..
        } => {
            collect_input_jobs(input, require_embed_target(using)?, "default", jobs)?;
            collect_prefetches_jobs(prefetch, jobs)?;
        }
        QueryExpr::Recommend {
            positive,
            negative,
            using,
            prefetch,
            ..
        } => {
            let target = require_embed_target(using)?;
            for input in positive.iter().chain(negative.iter()) {
                collect_input_jobs(input, target, "default", jobs)?;
            }
            collect_prefetches_jobs(prefetch, jobs)?;
        }
        QueryExpr::Context {
            pairs,
            using,
            prefetch,
            ..
        } => {
            let target = require_embed_target(using)?;
            for pair in pairs {
                collect_input_jobs(&pair.positive, target, "default", jobs)?;
                collect_input_jobs(&pair.negative, target, "default", jobs)?;
            }
            collect_prefetches_jobs(prefetch, jobs)?;
        }
        QueryExpr::Discover {
            target,
            context,
            using,
            prefetch,
            ..
        } => {
            let emb = require_embed_target(using)?;
            collect_input_jobs(target, emb, "default", jobs)?;
            for pair in context {
                collect_input_jobs(&pair.positive, emb, "default", jobs)?;
                collect_input_jobs(&pair.negative, emb, "default", jobs)?;
            }
            collect_prefetches_jobs(prefetch, jobs)?;
        }
        QueryExpr::Fusion { prefetch, .. } | QueryExpr::Formula { prefetch, .. } => {
            collect_prefetches_jobs(prefetch, jobs)?;
        }
        QueryExpr::RelevanceFeedback {
            target,
            feedback,
            using,
            prefetch,
            ..
        } => {
            let emb = require_embed_target(using)?;
            collect_input_jobs(target, emb, "default", jobs)?;
            for fb in feedback {
                collect_input_jobs(&fb.example, emb, "default", jobs)?;
            }
            collect_prefetches_jobs(prefetch, jobs)?;
        }
        QueryExpr::Hybrid { text, model, .. } => {
            // The sparse leg shares `Hybrid.model` with the dense leg (there
            // is no dedicated sparse-model field); apply consumes dense first.
            let m = model.as_deref().unwrap_or("default").to_string();
            jobs.push(QueryJob {
                modality: JobModality::Dense,
                model: m.clone(),
                text: text.clone(),
            });
            jobs.push(QueryJob {
                modality: JobModality::Sparse,
                model: m,
                text: text.clone(),
            });
        }
        QueryExpr::CrossRerank { prefetch, .. } => {
            // Query string is scored by the pair model, not embedded.
            collect_prefetches_jobs(prefetch, jobs)?;
        }
        QueryExpr::Rerank {
            input,
            model,
            using,
            prefetch,
            ..
        } => {
            // RERANK uses the MODEL string for dense/multi, not "default".
            let emb = require_embed_target(using)?;
            // RERANK is always dense-family; multi comes from USING / schema.
            if emb.kind == VectorKind::Sparse {
                return Err(QqlError::execution(
                    "QQL-VECTOR-KIND",
                    "RERANK requires a dense (or multivector) target, not sparse",
                    None,
                ));
            }
            collect_input_jobs(input, emb, model.as_str(), jobs)?;
            collect_prefetches_jobs(prefetch, jobs)?;
        }
        _ => {}
    }
    Ok(())
}

/// Classify one query input into its modality batch. TEXT joins the dense
/// batch only for single-vector dense targets; sparse and multi batch
/// separately. IMAGE always joins the image batch (target validated here so
/// misconfigured queries fail before any batch RPC runs — same errors as
/// [`apply_input`]).
fn collect_input_jobs(
    input: &QueryInput,
    target: EmbedTarget,
    default_model: &str,
    jobs: &mut Vec<QueryJob>,
) -> Result<(), QqlError> {
    match input {
        QueryInput::Text { text, model, .. } => {
            let m = model.as_deref().unwrap_or(default_model).to_string();
            let modality = if target.kind == VectorKind::Sparse {
                JobModality::Sparse
            } else if target.multi {
                JobModality::Multi
            } else {
                JobModality::Dense
            };
            jobs.push(QueryJob {
                modality,
                model: m,
                text: text.clone(),
            });
        }
        QueryInput::Image { source, model, .. } => {
            check_image_target(target)?;
            let m = model.as_deref().unwrap_or(default_model).to_string();
            jobs.push(QueryJob {
                modality: JobModality::Image,
                model: m,
                text: source.clone(),
            });
        }
        _ => {}
    }
    Ok(())
}

/// Group job positions by model (sorted runs) so each model needs one batch
/// RPC. Sort-based grouping avoids a per-operation `HashMap`; the common
/// single-model case skips the sort entirely. Walk order is restored on
/// scatter via the position map.
fn group_positions_by_model(jobs: &[(String, String)]) -> Vec<Vec<usize>> {
    if jobs.is_empty() {
        return Vec::new();
    }
    if jobs.iter().all(|(model, _)| model == &jobs[0].0) {
        return vec![(0..jobs.len()).collect()];
    }
    let mut order: Vec<usize> = (0..jobs.len()).collect();
    order.sort_by(|&a, &b| jobs[a].0.cmp(&jobs[b].0));
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for idx in order {
        match groups.last_mut() {
            Some(last) if jobs[last[0]].0 == jobs[idx].0 => last.push(idx),
            _ => groups.push(vec![idx]),
        }
    }
    groups
}

/// Drain a scatter buffer, failing closed on a missing slot (batch/impl
/// cardinality bugs must never surface as silently dropped inputs).
fn drain_batch_out<T>(
    out: Vec<Option<T>>,
    code: &'static str,
    kind: &str,
) -> Result<Vec<T>, QqlError> {
    out.into_iter()
        .enumerate()
        .map(|(i, v)| {
            v.ok_or_else(|| {
                QqlError::execution(
                    code,
                    format!("missing {kind} embedding at job index {i}"),
                    None,
                )
            })
        })
        .collect()
}

/// One modality's batch RPC behind the generic [`batch_by_model`] skeleton.
///
/// A plain `Fn` parameter cannot name the lifetime tying the boxed future to
/// its inputs, so the call is a trait method with an explicit method
/// lifetime instead of a closure.
trait BatchCall<T> {
    fn call<'x>(
        &self,
        embedder: &'x dyn Embedder,
        texts: &'x [String],
        model: &'x str,
    ) -> BoxFut<'x, Result<Vec<T>, QqlError>>;
}

struct DenseBatchCall;
struct SparseBatchCall;
struct MultiBatchCall;
struct ImageBatchCall;

impl BatchCall<Vec<f32>> for DenseBatchCall {
    fn call<'x>(
        &self,
        embedder: &'x dyn Embedder,
        texts: &'x [String],
        model: &'x str,
    ) -> BoxFut<'x, Result<Vec<Vec<f32>>, QqlError>> {
        Box::pin(embedder.embed_dense_batch(texts, model))
    }
}

impl BatchCall<SparseVector> for SparseBatchCall {
    fn call<'x>(
        &self,
        embedder: &'x dyn Embedder,
        texts: &'x [String],
        model: &'x str,
    ) -> BoxFut<'x, Result<Vec<SparseVector>, QqlError>> {
        Box::pin(embedder.embed_sparse_query_batch(texts, model))
    }
}

impl BatchCall<Vec<Vec<f32>>> for MultiBatchCall {
    fn call<'x>(
        &self,
        embedder: &'x dyn Embedder,
        texts: &'x [String],
        model: &'x str,
    ) -> BoxFut<'x, Result<Vec<Vec<Vec<f32>>>, QqlError>> {
        Box::pin(embedder.embed_multi_batch(texts, model))
    }
}

impl BatchCall<Vec<f32>> for ImageBatchCall {
    fn call<'x>(
        &self,
        embedder: &'x dyn Embedder,
        texts: &'x [String],
        model: &'x str,
    ) -> BoxFut<'x, Result<Vec<Vec<f32>>, QqlError>> {
        Box::pin(embedder.embed_image_batch(texts, model))
    }
}

/// Group jobs by model, run one batch RPC per model, restore walk order.
///
/// The single generic skeleton behind every modality batch: `call` is the
/// modality RPC, `check_len` reproduces that modality's cardinality error,
/// and (`code`, `kind`) feed the shared missing-slot drain.
async fn batch_by_model<T>(
    embedder: &dyn Embedder,
    jobs: &[(String, String)],
    code: &'static str,
    kind: &'static str,
    check_len: impl Fn(usize, usize, &str) -> Result<(), QqlError>,
    call: impl BatchCall<T>,
) -> Result<Vec<T>, QqlError> {
    if jobs.is_empty() {
        return Ok(Vec::new());
    }

    let mut out: Vec<Option<T>> = Vec::new();
    out.resize_with(jobs.len(), || None);
    for group in group_positions_by_model(jobs) {
        let model = jobs[group[0]].0.as_str();
        let texts: Vec<String> = group.iter().map(|&i| jobs[i].1.clone()).collect();
        let vecs = call.call(embedder, &texts, model).await?;
        check_len(vecs.len(), group.len(), model)?;
        for (pos, vec) in group.into_iter().zip(vecs) {
            out[pos] = Some(vec);
        }
    }

    drain_batch_out(out, code, kind)
}

fn sparse_len_error(got: usize, expected: usize, model: &str) -> Result<(), QqlError> {
    if got != expected {
        return Err(QqlError::execution(
            "QQL-EMBEDDING-SPARSE",
            format!(
                "embed_sparse_query_batch returned {got} vectors for {expected} texts (model={model})"
            ),
            None,
        ));
    }
    Ok(())
}

fn multi_len_error(got: usize, expected: usize, model: &str) -> Result<(), QqlError> {
    if got != expected {
        return Err(QqlError::execution(
            "QQL-EMBEDDING-MULTI",
            format!("embed_multi_batch returned {got} bags for {expected} texts (model={model})"),
            None,
        ));
    }
    Ok(())
}

fn image_len_error(got: usize, expected: usize, model: &str) -> Result<(), QqlError> {
    if got != expected {
        return Err(QqlError::execution(
            "QQL-EMBEDDING-IMAGE",
            format!(
                "embed_image_batch returned {got} vectors for {expected} sources (model={model})"
            ),
            None,
        ));
    }
    Ok(())
}

/// Batch every collected job: one RPC per (modality, model), results aligned
/// 1:1 with `jobs` by index for the cursor-driven apply pass.
async fn batch_query_jobs(
    embedder: &dyn Embedder,
    jobs: &[QueryJob],
) -> Result<Vec<JobOutput>, QqlError> {
    let mut out: Vec<Option<JobOutput>> = Vec::new();
    out.resize_with(jobs.len(), || None);
    for modality in [
        JobModality::Dense,
        JobModality::Sparse,
        JobModality::Multi,
        JobModality::Image,
    ] {
        let positions: Vec<usize> = jobs
            .iter()
            .enumerate()
            .filter(|(_, job)| job.modality == modality)
            .map(|(i, _)| i)
            .collect();
        if positions.is_empty() {
            continue;
        }
        let sub: Vec<(String, String)> = positions
            .iter()
            .map(|&i| (jobs[i].model.clone(), jobs[i].text.clone()))
            .collect();
        match modality {
            JobModality::Dense => {
                let vecs = batch_by_model(
                    embedder,
                    &sub,
                    "QQL-EMBEDDING",
                    "dense",
                    ensure_batch_len,
                    DenseBatchCall,
                )
                .await?;
                for (pos, vec) in positions.into_iter().zip(vecs) {
                    out[pos] = Some(JobOutput::Dense(vec));
                }
            }
            // Backends overriding only the batch entry point serve every
            // sparse leg (including Hybrid) through this single call site.
            JobModality::Sparse => {
                let vecs = batch_by_model(
                    embedder,
                    &sub,
                    "QQL-EMBEDDING-SPARSE",
                    "sparse",
                    sparse_len_error,
                    SparseBatchCall,
                )
                .await?;
                for (pos, vec) in positions.into_iter().zip(vecs) {
                    out[pos] = Some(JobOutput::Sparse(vec));
                }
            }
            JobModality::Multi => {
                let bags = batch_by_model(
                    embedder,
                    &sub,
                    "QQL-EMBEDDING-MULTI",
                    "multi",
                    multi_len_error,
                    MultiBatchCall,
                )
                .await?;
                for (pos, rows) in positions.into_iter().zip(bags) {
                    out[pos] = Some(JobOutput::Multi(rows));
                }
            }
            JobModality::Image => {
                let vecs = batch_by_model(
                    embedder,
                    &sub,
                    "QQL-EMBEDDING-IMAGE",
                    "image",
                    image_len_error,
                    ImageBatchCall,
                )
                .await?;
                for (pos, vec) in positions.into_iter().zip(vecs) {
                    out[pos] = Some(JobOutput::Image(vec));
                }
            }
        }
    }
    out.into_iter()
        .enumerate()
        .map(|(i, slot)| {
            slot.ok_or_else(|| {
                QqlError::execution(
                    "QQL-EMBEDDING",
                    format!("internal error: missing query embedding at job index {i}"),
                    None,
                )
            })
        })
        .collect()
}

// ── Apply batched vectors (in collect order) ──────────────────────────
//
// Pure AST rewrite: every embedding comes from the job-aligned outputs above,
// so the query path performs no per-input embedder RPCs here.

/// Cursor consuming [`QueryJob`]s and their aligned outputs in walk order.
///
/// Each step checks the job list, so collect/apply drift fails closed instead
/// of silently mis-binding vectors.
struct ApplyCursor<'j> {
    jobs: &'j [QueryJob],
    values: std::vec::IntoIter<JobOutput>,
    pos: usize,
}

impl<'j> ApplyCursor<'j> {
    fn new(jobs: &'j [QueryJob], outputs: Vec<JobOutput>) -> Self {
        debug_assert_eq!(jobs.len(), outputs.len());
        Self {
            jobs,
            values: outputs.into_iter(),
            pos: 0,
        }
    }

    /// Take the next output, verifying the job list expects `modality` here.
    /// `place` extends the run-out message (`""`, or `" for HYBRID"`).
    fn take(&mut self, modality: JobModality, place: &'static str) -> Result<JobOutput, QqlError> {
        let (code, noun) = modality_code(modality);
        let job = self.jobs.get(self.pos).ok_or_else(|| {
            QqlError::execution(
                code,
                format!("internal error: ran out of {noun} embeddings{place}"),
                None,
            )
        })?;
        if job.modality != modality {
            return Err(QqlError::execution(
                code,
                format!(
                    "internal error: embedding job mismatch at index {} (collect walked {} leg, apply walked {noun} leg)",
                    self.pos,
                    modality_code(job.modality).1,
                ),
                None,
            ));
        }
        self.pos += 1;
        self.values.next().ok_or_else(|| {
            QqlError::execution(
                code,
                format!("internal error: ran out of {noun} embeddings{place}"),
                None,
            )
        })
    }

    fn next_dense(&mut self, place: &'static str) -> Result<Vec<f32>, QqlError> {
        match self.take(JobModality::Dense, place)? {
            JobOutput::Dense(vec) => Ok(vec),
            _ => Err(output_mismatch(JobModality::Dense)),
        }
    }

    fn next_sparse(&mut self, place: &'static str) -> Result<SparseVector, QqlError> {
        match self.take(JobModality::Sparse, place)? {
            JobOutput::Sparse(vec) => Ok(vec),
            _ => Err(output_mismatch(JobModality::Sparse)),
        }
    }

    fn next_multi(&mut self, place: &'static str) -> Result<Vec<Vec<f32>>, QqlError> {
        match self.take(JobModality::Multi, place)? {
            JobOutput::Multi(rows) => Ok(rows),
            _ => Err(output_mismatch(JobModality::Multi)),
        }
    }

    fn next_image(&mut self) -> Result<Vec<f32>, QqlError> {
        match self.take(JobModality::Image, "")? {
            JobOutput::Image(vec) => Ok(vec),
            _ => Err(output_mismatch(JobModality::Image)),
        }
    }

    /// Fail closed on unconsumed jobs (collect/apply drift).
    fn finish(self) -> Result<(), QqlError> {
        match self.jobs.get(self.pos) {
            None => Ok(()),
            Some(job) => {
                let (code, noun) = modality_code(job.modality);
                Err(QqlError::execution(
                    code,
                    format!("internal error: unused {noun} embeddings after apply"),
                    None,
                ))
            }
        }
    }
}

fn output_mismatch(modality: JobModality) -> QqlError {
    let (code, noun) = modality_code(modality);
    QqlError::execution(
        code,
        format!("internal error: {noun} job produced a non-{noun} output"),
        None,
    )
}

fn apply_query_embeddings<'a>(
    query: &'a mut QueryStmt,
    cursor: &'a mut ApplyCursor<'_>,
) -> BoxFut<'a, Result<(), QqlError>> {
    Box::pin(async move {
        for cte in &mut query.ctes {
            apply_expr_embeddings(&mut cte.query.expression, cursor).await?;
        }
        apply_expr_embeddings(&mut query.expression, cursor).await?;
        Ok(())
    })
}

fn apply_prefetches_embeddings<'a>(
    prefetches: &'a mut [Prefetch],
    cursor: &'a mut ApplyCursor<'_>,
) -> BoxFut<'a, Result<(), QqlError>> {
    Box::pin(async move {
        for pref in prefetches {
            if let PrefetchSource::Query(sub) = &mut pref.source {
                apply_query_embeddings(sub, cursor).await?;
            }
        }
        Ok(())
    })
}

fn apply_expr_embeddings<'a>(
    expr: &'a mut QueryExpr,
    cursor: &'a mut ApplyCursor<'_>,
) -> BoxFut<'a, Result<(), QqlError>> {
    Box::pin(async move {
        match expr {
            QueryExpr::Nearest {
                input,
                using,
                prefetch,
                ..
            } => {
                apply_input(input, require_embed_target(using)?, cursor)?;
                apply_prefetches_embeddings(prefetch, cursor).await?;
            }
            QueryExpr::Recommend {
                positive,
                negative,
                using,
                prefetch,
                ..
            } => {
                let target = require_embed_target(using)?;
                for input in positive.iter_mut().chain(negative.iter_mut()) {
                    apply_input(input, target, cursor)?;
                }
                apply_prefetches_embeddings(prefetch, cursor).await?;
            }
            QueryExpr::Context {
                pairs,
                using,
                prefetch,
                ..
            } => {
                let target = require_embed_target(using)?;
                for pair in pairs {
                    apply_input(&mut pair.positive, target, cursor)?;
                    apply_input(&mut pair.negative, target, cursor)?;
                }
                apply_prefetches_embeddings(prefetch, cursor).await?;
            }
            QueryExpr::Discover {
                target,
                context,
                using,
                prefetch,
                ..
            } => {
                let emb = require_embed_target(using)?;
                apply_input(target, emb, cursor)?;
                for pair in context {
                    apply_input(&mut pair.positive, emb, cursor)?;
                    apply_input(&mut pair.negative, emb, cursor)?;
                }
                apply_prefetches_embeddings(prefetch, cursor).await?;
            }
            QueryExpr::Fusion { prefetch, .. } | QueryExpr::Formula { prefetch, .. } => {
                apply_prefetches_embeddings(prefetch, cursor).await?;
            }
            QueryExpr::RelevanceFeedback {
                target,
                feedback,
                using,
                prefetch,
                ..
            } => {
                let emb = require_embed_target(using)?;
                apply_input(target, emb, cursor)?;
                for fb in feedback {
                    apply_input(&mut fb.example, emb, cursor)?;
                }
                apply_prefetches_embeddings(prefetch, cursor).await?;
            }
            QueryExpr::Hybrid {
                dense_vector,
                sparse_vector,
                fusion,
                ..
            } => {
                let d_vec = cursor.next_dense(" for HYBRID")?;
                // The sparse leg shares `Hybrid.model` with the batched dense
                // leg above (there is no dedicated sparse-model field).
                let s_vec = cursor.next_sparse(" for HYBRID")?;
                let d_vec_name = dense_vector.as_deref().unwrap_or(DENSE_VECTOR_NAME);
                let s_vec_name = sparse_vector.as_deref().unwrap_or(SPARSE_VECTOR_NAME);

                let dense_sub = QueryStmt {
                    ctes: Vec::new(),
                    collection: qql_core::ast::QueryCollection::Inherited,
                    collection_span: None,
                    expression: QueryExpr::Nearest {
                        input: QueryInput::Vector(VectorValue::Dense(d_vec)),
                        using: Some(VectorTarget {
                            name: d_vec_name.to_string(),
                            kind: Some(VectorKind::Dense),
                            multi: false,
                        }),
                        prefetch: Vec::new(),
                        mmr: None,
                    },
                    filter: None,
                    params: None,
                    score_threshold: None,
                    group: None,
                    output: qql_core::ast::QueryOutput::default(),
                    page: qql_core::ast::PageSpec::default(),
                    shard_key: None,
                };
                let sparse_sub = QueryStmt {
                    ctes: Vec::new(),
                    collection: qql_core::ast::QueryCollection::Inherited,
                    collection_span: None,
                    expression: QueryExpr::Nearest {
                        input: QueryInput::Vector(VectorValue::Sparse {
                            indices: s_vec.indices,
                            values: s_vec.values,
                        }),
                        using: Some(VectorTarget {
                            name: s_vec_name.to_string(),
                            kind: Some(VectorKind::Sparse),
                            multi: false,
                        }),
                        prefetch: Vec::new(),
                        mmr: None,
                    },
                    filter: None,
                    params: None,
                    score_threshold: None,
                    group: None,
                    output: qql_core::ast::QueryOutput::default(),
                    page: qql_core::ast::PageSpec::default(),
                    shard_key: None,
                };

                *expr = QueryExpr::Fusion {
                    method: *fusion,
                    prefetch: vec![
                        Prefetch {
                            source: PrefetchSource::Query(Box::new(dense_sub)),
                            filter: None,
                            score_threshold: None,
                            lookup: None,
                        },
                        Prefetch {
                            source: PrefetchSource::Query(Box::new(sparse_sub)),
                            filter: None,
                            score_threshold: None,
                            lookup: None,
                        },
                    ],
                };
            }
            QueryExpr::Rerank {
                input,
                using,
                prefetch,
                ..
            } => {
                let emb = require_embed_target(using)?;
                apply_input(input, emb, cursor)?;
                apply_prefetches_embeddings(prefetch, cursor).await?;
            }
            QueryExpr::CrossRerank { prefetch, .. } => {
                apply_prefetches_embeddings(prefetch, cursor).await?;
            }
            _ => {}
        }
        Ok(())
    })
}

/// Shared IMAGE target validation for collect and apply: images always
/// produce single-vector dense.
fn check_image_target(target: EmbedTarget) -> Result<(), QqlError> {
    // Images always produce single-vector dense (CLIP vision, etc.).
    if target.kind == VectorKind::Sparse {
        return Err(QqlError::execution(
            "QQL-VECTOR-KIND",
            "IMAGE input requires a dense target, not sparse",
            None,
        ));
    }
    if target.multi {
        return Err(QqlError::execution(
            "QQL-VECTOR-KIND",
            "IMAGE input produces single-vector dense, not multivector; use TEXT with AS MULTI for ColBERT",
            None,
        ));
    }
    Ok(())
}

fn apply_input(
    input: &mut QueryInput,
    target: EmbedTarget,
    cursor: &mut ApplyCursor<'_>,
) -> Result<(), QqlError> {
    match input {
        QueryInput::Image { .. } => {
            check_image_target(target)?;
            let vec = cursor.next_image()?;
            if vec.is_empty() {
                return Err(QqlError::execution(
                    "QQL-EMBEDDING-IMAGE",
                    "embed_image returned an empty vector",
                    None,
                ));
            }
            *input = QueryInput::Vector(VectorValue::Dense(vec));
            Ok(())
        }
        QueryInput::Text { .. } => {
            if target.kind == VectorKind::Sparse {
                let s_vec = cursor.next_sparse("")?;
                *input = QueryInput::Vector(VectorValue::Sparse {
                    indices: s_vec.indices,
                    values: s_vec.values,
                });
                return Ok(());
            }
            if target.multi {
                let rows = cursor.next_multi("")?;
                if rows.is_empty() {
                    return Err(QqlError::execution(
                        "QQL-EMBEDDING-MULTI",
                        "embed_multi returned an empty multivector",
                        None,
                    ));
                }
                *input = QueryInput::Vector(VectorValue::MultiDense(rows));
                return Ok(());
            }
            let vec = cursor.next_dense("")?;
            if vec.is_empty() {
                return Err(QqlError::execution(
                    "QQL-EMBEDDING",
                    "embed_dense_batch returned an empty vector",
                    None,
                ));
            }
            *input = QueryInput::Vector(VectorValue::Dense(vec));
            Ok(())
        }
        QueryInput::Vector(_)
        | QueryInput::Point(_)
        | QueryInput::Param(..)
        | QueryInput::PositionalParam(..) => Ok(()),
        // Custom inference objects have no client-side embedder method; the
        // backend inference service resolves them (options ride along).
        QueryInput::Object { .. } => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolve::resolve_embeddings;
    use std::sync::{Arc, Mutex};

    /// Embedder that records batch calls per modality and singles separately.
    /// Deterministic outputs encode input length so walk-order restoration is
    /// checkable from the resolved AST.
    type BatchLog = Arc<Mutex<Vec<(String, Vec<String>)>>>;

    #[derive(Default)]
    struct BatchCountingEmbedder {
        dense_batches: BatchLog,
        sparse_batches: BatchLog,
        multi_batches: BatchLog,
        image_batches: BatchLog,
        singles: Arc<Mutex<Vec<&'static str>>>,
    }

    #[async_trait::async_trait]
    impl Embedder for BatchCountingEmbedder {
        async fn embed_dense(&self, _text: &str, _model: &str) -> Result<Vec<f32>, QqlError> {
            self.singles.lock().unwrap().push("dense");
            Ok(vec![0.0])
        }

        async fn embed_sparse_query(
            &self,
            _text: &str,
            _model: &str,
        ) -> Result<SparseVector, QqlError> {
            self.singles.lock().unwrap().push("sparse");
            Ok(SparseVector {
                indices: vec![0],
                values: vec![1.0],
            })
        }

        async fn embed_multi(&self, _text: &str, _model: &str) -> Result<Vec<Vec<f32>>, QqlError> {
            self.singles.lock().unwrap().push("multi");
            Ok(vec![vec![0.0]])
        }

        async fn embed_image(&self, _source: &str, _model: &str) -> Result<Vec<f32>, QqlError> {
            self.singles.lock().unwrap().push("image");
            Ok(vec![0.0])
        }

        async fn embed_dense_batch(
            &self,
            texts: &[String],
            model: &str,
        ) -> Result<Vec<Vec<f32>>, QqlError> {
            self.dense_batches
                .lock()
                .unwrap()
                .push((model.to_string(), texts.to_vec()));
            Ok(texts.iter().map(|t| vec![t.len() as f32]).collect())
        }

        async fn embed_sparse_query_batch(
            &self,
            texts: &[String],
            model: &str,
        ) -> Result<Vec<SparseVector>, QqlError> {
            self.sparse_batches
                .lock()
                .unwrap()
                .push((model.to_string(), texts.to_vec()));
            Ok(texts
                .iter()
                .map(|t| SparseVector {
                    indices: vec![t.len() as u32],
                    values: vec![1.0],
                })
                .collect())
        }

        async fn embed_multi_batch(
            &self,
            texts: &[String],
            model: &str,
        ) -> Result<Vec<Vec<Vec<f32>>>, QqlError> {
            self.multi_batches
                .lock()
                .unwrap()
                .push((model.to_string(), texts.to_vec()));
            Ok(texts.iter().map(|t| vec![vec![t.len() as f32]]).collect())
        }

        async fn embed_image_batch(
            &self,
            sources: &[String],
            model: &str,
        ) -> Result<Vec<Vec<f32>>, QqlError> {
            self.image_batches
                .lock()
                .unwrap()
                .push((model.to_string(), sources.to_vec()));
            Ok(sources.iter().map(|s| vec![s.len() as f32]).collect())
        }
    }

    fn cte_sparse_indices(query: &qql_core::ast::QueryStmt, cte: usize) -> Vec<u32> {
        let qql_core::ast::QueryExpr::Nearest { input, .. } = &query.ctes[cte].query.expression
        else {
            panic!("expected Nearest");
        };
        let qql_core::ast::QueryInput::Vector(qql_core::ast::VectorValue::Sparse {
            indices, ..
        }) = input
        else {
            panic!("expected sparse vector, got {input:?}");
        };
        indices.clone()
    }

    #[test]
    fn model_groups_sort_and_fast_path() {
        let jobs = vec![
            ("m1".to_string(), "a".to_string()),
            ("m2".to_string(), "b".to_string()),
            ("m1".to_string(), "c".to_string()),
        ];
        let groups = group_positions_by_model(&jobs);
        assert_eq!(groups.len(), 2);
        // Same-model positions keep walk order inside the group.
        assert!(groups.contains(&vec![0, 2]));
        assert!(groups.contains(&vec![1]));
        // Empty input needs no RPC.
        assert!(group_positions_by_model(&[]).is_empty());
    }

    #[tokio::test]
    async fn sparse_query_legs_batch_once_in_walk_order() {
        let mut stmt = qql_core::parser::Parser::parse(
            "WITH a AS (QUERY TEXT 'alpha' USING s AS SPARSE LIMIT 5), \
             b AS (QUERY TEXT 'beta' USING s AS SPARSE PREFETCH (a) LIMIT 5) \
             QUERY TEXT 'gamma' FROM docs USING s AS SPARSE PREFETCH (b) LIMIT 5;",
        )
        .unwrap();
        let mock = BatchCountingEmbedder::default();
        resolve_embeddings(&mut stmt, &mock).await.unwrap();

        // One sparse RPC for all three legs (was: one await per QueryInput).
        let sparse = mock.sparse_batches.lock().unwrap();
        assert_eq!(sparse.len(), 1, "expected one sparse batch, got {sparse:?}");
        assert_eq!(sparse[0].0, "default");
        assert_eq!(
            sparse[0].1,
            vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()]
        );
        // No dense jobs here, so no dense RPC at all.
        assert!(mock.dense_batches.lock().unwrap().is_empty());
        // Query path never falls back to single-call embeddings.
        assert!(mock.singles.lock().unwrap().is_empty());

        // Walk order survives the batch round-trip (lengths 5, 4, 5).
        let qql_core::ast::Stmt::Query(query) = &stmt else {
            panic!("expected Query");
        };
        assert_eq!(cte_sparse_indices(query, 0), vec![5]);
        assert_eq!(cte_sparse_indices(query, 1), vec![4]);
    }

    #[tokio::test]
    async fn multi_query_legs_batch_once_per_model() {
        let mut stmt = qql_core::parser::Parser::parse(
            "WITH a AS (QUERY TEXT 'alpha' MODEL 'm1' USING c AS MULTI LIMIT 5), \
             b AS (QUERY TEXT 'beta' MODEL 'm2' USING c AS MULTI PREFETCH (a) LIMIT 5) \
             QUERY TEXT 'qq' MODEL 'm1' FROM docs USING c AS MULTI PREFETCH (b) LIMIT 5;",
        )
        .unwrap();
        let mock = BatchCountingEmbedder::default();
        resolve_embeddings(&mut stmt, &mock).await.unwrap();

        let multi = mock.multi_batches.lock().unwrap();
        assert_eq!(
            multi.len(),
            2,
            "expected one batch per model, got {multi:?}"
        );
        // Walk order within a model group is preserved (alpha before qq).
        let m1 = multi.iter().find(|(m, _)| m == "m1").expect("m1 batch");
        assert_eq!(m1.1, vec!["alpha".to_string(), "qq".to_string()]);
        let m2 = multi.iter().find(|(m, _)| m == "m2").expect("m2 batch");
        assert_eq!(m2.1, vec!["beta".to_string()]);
        assert!(mock.singles.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn image_query_legs_batch_once() {
        let mut stmt = qql_core::parser::Parser::parse(
            "WITH a AS (QUERY IMAGE '/a.jpg' MODEL 'm' USING i AS DENSE LIMIT 5), \
             b AS (QUERY IMAGE '/bb.jpg' MODEL 'm' USING i AS DENSE PREFETCH (a) LIMIT 5) \
             QUERY TEXT 'q' FROM docs USING d AS DENSE PREFETCH (b) LIMIT 5;",
        )
        .unwrap();
        let mock = BatchCountingEmbedder::default();
        resolve_embeddings(&mut stmt, &mock).await.unwrap();

        let images = mock.image_batches.lock().unwrap();
        assert_eq!(images.len(), 1, "expected one image batch, got {images:?}");
        assert_eq!(
            images[0].1,
            vec!["/a.jpg".to_string(), "/bb.jpg".to_string()]
        );
        // The TEXT leg still batches through the dense entry point.
        assert_eq!(mock.dense_batches.lock().unwrap().len(), 1);
        assert!(mock.singles.lock().unwrap().is_empty());
    }
}
