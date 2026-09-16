//! Query-side embedding resolution (`AstLowerer`-free AST rewrite).
//!
//! Split from `resolve.rs` (size hygiene): the `QUERY` collect → batch →
//! apply pipeline (`resolve_query_embeddings`, the `QueryJobs` walkers, the
//! per-modality batchers, and the apply pass) plus its tests. The statement
//! dispatcher, the shared target/length helpers, and the upsert half stay in
//! `resolve`. Behavior is unchanged.

use std::future::Future;
use std::pin::Pin;

use qql_core::ast::{
    Prefetch, PrefetchSource, QueryExpr, QueryInput, QueryStmt, VectorKind, VectorTarget,
    VectorValue,
};
use qql_core::error::QqlError;

use super::resolve::{
    DENSE_VECTOR_NAME, EmbedTarget, SPARSE_VECTOR_NAME, ensure_batch_len, require_embed_target,
};
use crate::embedder::Embedder;
use crate::sparse::SparseVector;

#[cfg(not(target_arch = "wasm32"))]
type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
#[cfg(target_arch = "wasm32")]
type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// Dense vector iterator passed through recursive apply.
#[cfg(not(target_arch = "wasm32"))]
type DenseIter<'a> = &'a mut (dyn Iterator<Item = Vec<f32>> + Send);
#[cfg(target_arch = "wasm32")]
type DenseIter<'a> = &'a mut dyn Iterator<Item = Vec<f32>>;

/// Query-side sparse vector iterator passed through recursive apply.
#[cfg(not(target_arch = "wasm32"))]
type SparseIter<'a> = &'a mut (dyn Iterator<Item = SparseVector> + Send);
#[cfg(target_arch = "wasm32")]
type SparseIter<'a> = &'a mut dyn Iterator<Item = SparseVector>;

/// Multivector bag iterator passed through recursive apply.
#[cfg(not(target_arch = "wasm32"))]
type MultiIter<'a> = &'a mut (dyn Iterator<Item = Vec<Vec<f32>>> + Send);
#[cfg(target_arch = "wasm32")]
type MultiIter<'a> = &'a mut dyn Iterator<Item = Vec<Vec<f32>>>;

/// Image dense-vector iterator passed through recursive apply.
#[cfg(not(target_arch = "wasm32"))]
type ImageIter<'a> = &'a mut (dyn Iterator<Item = Vec<f32>> + Send);
#[cfg(target_arch = "wasm32")]
type ImageIter<'a> = &'a mut dyn Iterator<Item = Vec<f32>>;

pub(crate) async fn resolve_query_embeddings(
    query: &mut QueryStmt,
    embedder: &dyn Embedder,
) -> Result<(), QqlError> {
    let mut jobs = QueryJobs::default();
    collect_query_jobs(query, &mut jobs)?;

    let dense_vecs = batch_dense_by_model(embedder, &jobs.dense).await?;
    let sparse_vecs = batch_sparse_by_model(embedder, &jobs.sparse).await?;
    let multi_vecs = batch_multi_by_model(embedder, &jobs.multi).await?;
    let image_vecs = batch_image_by_model(embedder, &jobs.image).await?;

    let mut dense_iter = dense_vecs.into_iter();
    let mut sparse_iter = sparse_vecs.into_iter();
    let mut multi_iter = multi_vecs.into_iter();
    let mut image_iter = image_vecs.into_iter();
    apply_query_embeddings(
        query,
        &mut dense_iter,
        &mut sparse_iter,
        &mut multi_iter,
        &mut image_iter,
    )
    .await?;

    if dense_iter.next().is_some() {
        return Err(QqlError::execution(
            "QQL-EMBEDDING",
            "internal error: unused dense embeddings after apply",
            None,
        ));
    }
    if sparse_iter.next().is_some() {
        return Err(QqlError::execution(
            "QQL-EMBEDDING-SPARSE",
            "internal error: unused sparse embeddings after apply",
            None,
        ));
    }
    if multi_iter.next().is_some() {
        return Err(QqlError::execution(
            "QQL-EMBEDDING-MULTI",
            "internal error: unused multi embeddings after apply",
            None,
        ));
    }
    if image_iter.next().is_some() {
        return Err(QqlError::execution(
            "QQL-EMBEDDING-IMAGE",
            "internal error: unused image embeddings after apply",
            None,
        ));
    }
    Ok(())
}

// ── Collect embed jobs (model, text) in walk order ────────────────────
//
// One AST walk fills every modality at once. Apply consumes each modality
// iterator in this same walk order, so collect and apply must visit inputs
// in identical sequence (CTEs, then expression; prefetch members in order;
// positive before negative; Hybrid dense leg before its sparse leg).

/// Batched query jobs in walk order, split by modality so each modality
/// batches 1 RPC per model.
#[derive(Debug, Default)]
struct QueryJobs {
    dense: Vec<(String, String)>,
    sparse: Vec<(String, String)>,
    multi: Vec<(String, String)>,
    image: Vec<(String, String)>,
}

fn collect_query_jobs(query: &QueryStmt, jobs: &mut QueryJobs) -> Result<(), QqlError> {
    for cte in &query.ctes {
        collect_expr_jobs(&cte.query.expression, jobs)?;
    }
    collect_expr_jobs(&query.expression, jobs)
}

fn collect_prefetches_jobs(prefetches: &[Prefetch], jobs: &mut QueryJobs) -> Result<(), QqlError> {
    for pref in prefetches {
        if let PrefetchSource::Query(sub) = &pref.source {
            collect_query_jobs(sub, jobs)?;
        }
    }
    Ok(())
}

fn collect_expr_jobs(expr: &QueryExpr, jobs: &mut QueryJobs) -> Result<(), QqlError> {
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
            jobs.dense.push((m.clone(), text.clone()));
            jobs.sparse.push((m, text.clone()));
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
    jobs: &mut QueryJobs,
) -> Result<(), QqlError> {
    match input {
        QueryInput::Text { text, model, .. } => {
            let m = model.as_deref().unwrap_or(default_model).to_string();
            if target.kind == VectorKind::Sparse {
                jobs.sparse.push((m, text.clone()));
            } else if target.multi {
                jobs.multi.push((m, text.clone()));
            } else {
                jobs.dense.push((m, text.clone()));
            }
        }
        QueryInput::Image { source, model, .. } => {
            check_image_target(target)?;
            let m = model.as_deref().unwrap_or(default_model).to_string();
            jobs.image.push((m, source.clone()));
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

/// Group jobs by model, call `embed_dense_batch` once per model, restore walk order.
async fn batch_dense_by_model(
    embedder: &dyn Embedder,
    jobs: &[(String, String)],
) -> Result<Vec<Vec<f32>>, QqlError> {
    if jobs.is_empty() {
        return Ok(Vec::new());
    }

    let mut out: Vec<Option<Vec<f32>>> = vec![None; jobs.len()];
    for group in group_positions_by_model(jobs) {
        let model = jobs[group[0]].0.as_str();
        let texts: Vec<String> = group.iter().map(|&i| jobs[i].1.clone()).collect();
        let vecs = embedder.embed_dense_batch(&texts, model).await?;
        ensure_batch_len(vecs.len(), group.len(), model)?;
        for (pos, vec) in group.into_iter().zip(vecs) {
            out[pos] = Some(vec);
        }
    }

    drain_batch_out(out, "QQL-EMBEDDING", "dense")
}

/// Group jobs by model, call `embed_sparse_query_batch` once per model,
/// restore walk order. Backends overriding only the batch entry point serve
/// every sparse leg (including Hybrid) through this single call site.
async fn batch_sparse_by_model(
    embedder: &dyn Embedder,
    jobs: &[(String, String)],
) -> Result<Vec<SparseVector>, QqlError> {
    if jobs.is_empty() {
        return Ok(Vec::new());
    }

    let mut out: Vec<Option<SparseVector>> = vec![None; jobs.len()];
    for group in group_positions_by_model(jobs) {
        let model = jobs[group[0]].0.as_str();
        let texts: Vec<String> = group.iter().map(|&i| jobs[i].1.clone()).collect();
        let vecs = embedder.embed_sparse_query_batch(&texts, model).await?;
        if vecs.len() != group.len() {
            return Err(QqlError::execution(
                "QQL-EMBEDDING-SPARSE",
                format!(
                    "embed_sparse_query_batch returned {} vectors for {} texts (model={model})",
                    vecs.len(),
                    group.len()
                ),
                None,
            ));
        }
        for (pos, vec) in group.into_iter().zip(vecs) {
            out[pos] = Some(vec);
        }
    }

    drain_batch_out(out, "QQL-EMBEDDING-SPARSE", "sparse")
}

/// Group jobs by model, call `embed_multi_batch` once per model, restore walk order.
async fn batch_multi_by_model(
    embedder: &dyn Embedder,
    jobs: &[(String, String)],
) -> Result<Vec<Vec<Vec<f32>>>, QqlError> {
    if jobs.is_empty() {
        return Ok(Vec::new());
    }

    let mut out: Vec<Option<Vec<Vec<f32>>>> = vec![None; jobs.len()];
    for group in group_positions_by_model(jobs) {
        let model = jobs[group[0]].0.as_str();
        let texts: Vec<String> = group.iter().map(|&i| jobs[i].1.clone()).collect();
        let bags = embedder.embed_multi_batch(&texts, model).await?;
        if bags.len() != group.len() {
            return Err(QqlError::execution(
                "QQL-EMBEDDING-MULTI",
                format!(
                    "embed_multi_batch returned {} bags for {} texts (model={model})",
                    bags.len(),
                    group.len()
                ),
                None,
            ));
        }
        for (pos, rows) in group.into_iter().zip(bags) {
            out[pos] = Some(rows);
        }
    }

    drain_batch_out(out, "QQL-EMBEDDING-MULTI", "multi")
}

/// Group jobs by model, call `embed_image_batch` once per model, restore walk order.
async fn batch_image_by_model(
    embedder: &dyn Embedder,
    jobs: &[(String, String)],
) -> Result<Vec<Vec<f32>>, QqlError> {
    if jobs.is_empty() {
        return Ok(Vec::new());
    }

    let mut out: Vec<Option<Vec<f32>>> = vec![None; jobs.len()];
    for group in group_positions_by_model(jobs) {
        let model = jobs[group[0]].0.as_str();
        let sources: Vec<String> = group.iter().map(|&i| jobs[i].1.clone()).collect();
        let vecs = embedder.embed_image_batch(&sources, model).await?;
        if vecs.len() != group.len() {
            return Err(QqlError::execution(
                "QQL-EMBEDDING-IMAGE",
                format!(
                    "embed_image_batch returned {} vectors for {} sources (model={model})",
                    vecs.len(),
                    group.len()
                ),
                None,
            ));
        }
        for (pos, vec) in group.into_iter().zip(vecs) {
            out[pos] = Some(vec);
        }
    }

    drain_batch_out(out, "QQL-EMBEDDING-IMAGE", "image")
}

// ── Apply batched vectors (in collect order) ──────────────────────────
//
// Pure AST rewrite: every embedding comes from the per-modality iterators
// filled by the batched calls above, so the query path performs no
// per-input embedder RPCs here.

fn apply_query_embeddings<'a>(
    query: &'a mut QueryStmt,
    dense: DenseIter<'a>,
    sparse: SparseIter<'a>,
    multi: MultiIter<'a>,
    image: ImageIter<'a>,
) -> BoxFut<'a, Result<(), QqlError>> {
    Box::pin(async move {
        for cte in &mut query.ctes {
            apply_expr_embeddings(&mut cte.query.expression, dense, sparse, multi, image).await?;
        }
        apply_expr_embeddings(&mut query.expression, dense, sparse, multi, image).await?;
        Ok(())
    })
}

fn apply_prefetches_embeddings<'a>(
    prefetches: &'a mut [Prefetch],
    dense: DenseIter<'a>,
    sparse: SparseIter<'a>,
    multi: MultiIter<'a>,
    image: ImageIter<'a>,
) -> BoxFut<'a, Result<(), QqlError>> {
    Box::pin(async move {
        for pref in prefetches {
            if let PrefetchSource::Query(sub) = &mut pref.source {
                apply_query_embeddings(sub, dense, sparse, multi, image).await?;
            }
        }
        Ok(())
    })
}

fn apply_expr_embeddings<'a>(
    expr: &'a mut QueryExpr,
    dense: DenseIter<'a>,
    sparse: SparseIter<'a>,
    multi: MultiIter<'a>,
    image: ImageIter<'a>,
) -> BoxFut<'a, Result<(), QqlError>> {
    Box::pin(async move {
        match expr {
            QueryExpr::Nearest {
                input,
                using,
                prefetch,
                ..
            } => {
                apply_input(
                    input,
                    require_embed_target(using)?,
                    dense,
                    sparse,
                    multi,
                    image,
                )?;
                apply_prefetches_embeddings(prefetch, dense, sparse, multi, image).await?;
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
                    apply_input(input, target, dense, sparse, multi, image)?;
                }
                apply_prefetches_embeddings(prefetch, dense, sparse, multi, image).await?;
            }
            QueryExpr::Context {
                pairs,
                using,
                prefetch,
                ..
            } => {
                let target = require_embed_target(using)?;
                for pair in pairs {
                    apply_input(&mut pair.positive, target, dense, sparse, multi, image)?;
                    apply_input(&mut pair.negative, target, dense, sparse, multi, image)?;
                }
                apply_prefetches_embeddings(prefetch, dense, sparse, multi, image).await?;
            }
            QueryExpr::Discover {
                target,
                context,
                using,
                prefetch,
                ..
            } => {
                let emb = require_embed_target(using)?;
                apply_input(target, emb, dense, sparse, multi, image)?;
                for pair in context {
                    apply_input(&mut pair.positive, emb, dense, sparse, multi, image)?;
                    apply_input(&mut pair.negative, emb, dense, sparse, multi, image)?;
                }
                apply_prefetches_embeddings(prefetch, dense, sparse, multi, image).await?;
            }
            QueryExpr::Fusion { prefetch, .. } | QueryExpr::Formula { prefetch, .. } => {
                apply_prefetches_embeddings(prefetch, dense, sparse, multi, image).await?;
            }
            QueryExpr::RelevanceFeedback {
                target,
                feedback,
                using,
                prefetch,
                ..
            } => {
                let emb = require_embed_target(using)?;
                apply_input(target, emb, dense, sparse, multi, image)?;
                for fb in feedback {
                    apply_input(&mut fb.example, emb, dense, sparse, multi, image)?;
                }
                apply_prefetches_embeddings(prefetch, dense, sparse, multi, image).await?;
            }
            QueryExpr::Hybrid {
                dense_vector,
                sparse_vector,
                fusion,
                ..
            } => {
                let d_vec = dense.next().ok_or_else(|| {
                    QqlError::execution(
                        "QQL-EMBEDDING",
                        "internal error: ran out of dense embeddings for HYBRID",
                        None,
                    )
                })?;
                // The sparse leg shares `Hybrid.model` with the batched dense
                // leg above (there is no dedicated sparse-model field).
                let s_vec = sparse.next().ok_or_else(|| {
                    QqlError::execution(
                        "QQL-EMBEDDING-SPARSE",
                        "internal error: ran out of sparse embeddings for HYBRID",
                        None,
                    )
                })?;
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
                apply_input(input, emb, dense, sparse, multi, image)?;
                apply_prefetches_embeddings(prefetch, dense, sparse, multi, image).await?;
            }
            QueryExpr::CrossRerank { prefetch, .. } => {
                apply_prefetches_embeddings(prefetch, dense, sparse, multi, image).await?;
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
    dense: DenseIter<'_>,
    sparse: SparseIter<'_>,
    multi: MultiIter<'_>,
    image: ImageIter<'_>,
) -> Result<(), QqlError> {
    match input {
        QueryInput::Image { .. } => {
            check_image_target(target)?;
            let vec = image.next().ok_or_else(|| {
                QqlError::execution(
                    "QQL-EMBEDDING-IMAGE",
                    "internal error: ran out of image embeddings",
                    None,
                )
            })?;
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
                let s_vec = sparse.next().ok_or_else(|| {
                    QqlError::execution(
                        "QQL-EMBEDDING-SPARSE",
                        "internal error: ran out of sparse embeddings",
                        None,
                    )
                })?;
                *input = QueryInput::Vector(VectorValue::Sparse {
                    indices: s_vec.indices,
                    values: s_vec.values,
                });
                return Ok(());
            }
            if target.multi {
                let rows = multi.next().ok_or_else(|| {
                    QqlError::execution(
                        "QQL-EMBEDDING-MULTI",
                        "internal error: ran out of multi embeddings",
                        None,
                    )
                })?;
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
            let vec = dense.next().ok_or_else(|| {
                QqlError::execution(
                    "QQL-EMBEDDING",
                    "internal error: ran out of dense embeddings",
                    None,
                )
            })?;
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
