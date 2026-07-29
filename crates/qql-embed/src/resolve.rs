use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use qql_core::ast::{
    EmbedKind, EmbeddingSpec, PointVectors, Prefetch, PrefetchSource, QueryExpr, QueryInput,
    QueryStmt, Stmt, UpsertPoint, UpsertStmt, VectorKind, VectorTarget, VectorValue,
};
use qql_core::error::QqlError;

use crate::embedder::Embedder;

/// Default named dense vector for auto-embedding.
pub const DENSE_VECTOR_NAME: &str = "dense";
/// Default named sparse vector for auto-embedding.
pub const SPARSE_VECTOR_NAME: &str = "sparse";

#[cfg(not(target_arch = "wasm32"))]
type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
#[cfg(target_arch = "wasm32")]
type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// Dense vector iterator passed through recursive apply.
#[cfg(not(target_arch = "wasm32"))]
type DenseIter<'a> = &'a mut (dyn Iterator<Item = Vec<f32>> + Send);
#[cfg(target_arch = "wasm32")]
type DenseIter<'a> = &'a mut dyn Iterator<Item = Vec<f32>>;

/// Resolve text → vectors on a statement before routing/execution.
///
/// Dense jobs are collected and sent through `embed_dense_batch` (grouped by
/// model). Sparse stays local BM25 via the embedder.
pub async fn resolve_embeddings(stmt: &mut Stmt, embedder: &dyn Embedder) -> Result<(), QqlError> {
    match stmt {
        Stmt::Query(query) => resolve_query_embeddings(query, embedder).await?,
        Stmt::Upsert(upsert) => resolve_upsert_embeddings(upsert, embedder).await?,
        _ => {}
    }
    Ok(())
}

async fn resolve_query_embeddings(
    query: &mut QueryStmt,
    embedder: &dyn Embedder,
) -> Result<(), QqlError> {
    let mut dense_jobs: Vec<(String, String)> = Vec::new();
    collect_query_dense_jobs(query, &mut dense_jobs)?;

    let dense_vecs = batch_dense_by_model(embedder, &dense_jobs).await?;
    let mut dense_iter = dense_vecs.into_iter();
    apply_query_embeddings(query, embedder, &mut dense_iter).await?;

    if dense_iter.next().is_some() {
        return Err(QqlError::execution(
            "QQL-EMBEDDING",
            "internal error: unused dense embeddings after apply",
            None,
        ));
    }
    Ok(())
}

async fn resolve_upsert_embeddings(
    upsert: &mut UpsertStmt,
    embedder: &dyn Embedder,
) -> Result<(), QqlError> {
    if upsert.embedding.is_none() && upsert.embed.is_empty() {
        let mut targets = Vec::new();
        for (idx, point) in upsert.points.iter().enumerate() {
            if point.vectors.is_none() {
                if let Some((_, qql_core::ast::Value::Str(text))) =
                    point.payload.iter().find(|(k, _)| {
                        k.eq_ignore_ascii_case("text")
                            || k.eq_ignore_ascii_case("body")
                            || k.eq_ignore_ascii_case("content")
                    })
                {
                    if !text.is_empty() {
                        targets.push((idx, text.clone()));
                    }
                }
            }
        }
        if !targets.is_empty() {
            // Topology-unaware fallback: dense only. Hybrid/sparse targets must
            // be set by the executor (configure_upsert_embeddings) or explicit
            // USING / EMBED directives before calling resolve_embeddings — so
            // dense-only collections never receive orphan sparse vectors.
            let (indices, texts): (Vec<usize>, Vec<String>) = targets.into_iter().unzip();
            let dense_vecs = embedder.embed_dense_batch(&texts, "default").await?;
            ensure_batch_len(dense_vecs.len(), indices.len(), "default")?;
            for (idx, d_vec) in indices.into_iter().zip(dense_vecs) {
                let point = &mut upsert.points[idx];
                add_point_vector(point, DENSE_VECTOR_NAME, VectorValue::Dense(d_vec))?;
            }
        }
    }

    if let Some(spec) = upsert.embedding.clone() {
        let mut seen_vectors = std::collections::HashSet::new();
        resolve_single_embedding_spec(upsert, &spec, embedder, &mut seen_vectors).await?;
    }

    for directive in &upsert.embed {
        let field_name = &directive.source_field;
        let target_vec_name = &directive.target_vector;
        let mut targets = Vec::new();
        for (idx, point) in upsert.points.iter().enumerate() {
            if let Some((_, qql_core::ast::Value::Str(text))) = point
                .payload
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(field_name))
            {
                if !text.is_empty() {
                    targets.push((idx, text.clone()));
                }
            }
        }

        if !targets.is_empty() {
            match &directive.kind {
                EmbedKind::Dense { model } => {
                    let m_name = model.as_deref().unwrap_or("default");
                    let (indices, texts): (Vec<usize>, Vec<String>) = targets.into_iter().unzip();
                    let vecs = embedder.embed_dense_batch(&texts, m_name).await?;
                    ensure_batch_len(vecs.len(), indices.len(), m_name)?;
                    for (idx, vec) in indices.into_iter().zip(vecs) {
                        let point = &mut upsert.points[idx];
                        add_point_vector(point, target_vec_name, VectorValue::Dense(vec))?;
                    }
                }
                EmbedKind::Sparse { model } => {
                    let m = model.as_deref().unwrap_or("default");
                    for (idx, text) in targets {
                        let s_vec = embedder.embed_sparse(&text, m).await?;
                        let point = &mut upsert.points[idx];
                        add_point_vector(
                            point,
                            target_vec_name,
                            VectorValue::Sparse {
                                indices: s_vec.indices,
                                values: s_vec.values,
                            },
                        )?;
                    }
                }
                EmbedKind::Multi { model } => {
                    let m_name = model.as_deref().unwrap_or("default");
                    let (indices, texts): (Vec<usize>, Vec<String>) = targets.into_iter().unzip();
                    let bags = embedder.embed_multi_batch(&texts, m_name).await?;
                    if bags.len() != indices.len() {
                        return Err(QqlError::execution(
                            "QQL-EMBEDDING-MULTI",
                            format!(
                                "embed_multi_batch returned {} bags for {} texts (model={m_name})",
                                bags.len(),
                                indices.len()
                            ),
                            None,
                        ));
                    }
                    for (idx, rows) in indices.into_iter().zip(bags) {
                        if rows.is_empty() {
                            return Err(QqlError::execution(
                                "QQL-EMBEDDING-MULTI",
                                "embed_multi returned an empty multivector",
                                None,
                            ));
                        }
                        let point = &mut upsert.points[idx];
                        add_point_vector(point, target_vec_name, VectorValue::MultiDense(rows))?;
                    }
                }
                EmbedKind::Image { model } => {
                    let m_name = model.as_deref().unwrap_or("default");
                    let (indices, sources): (Vec<usize>, Vec<String>) = targets.into_iter().unzip();
                    let vecs = embedder.embed_image_batch(&sources, m_name).await?;
                    if vecs.len() != indices.len() {
                        return Err(QqlError::execution(
                            "QQL-EMBEDDING-IMAGE",
                            format!(
                                "embed_image_batch returned {} vectors for {} sources (model={m_name})",
                                vecs.len(),
                                indices.len()
                            ),
                            None,
                        ));
                    }
                    for (idx, vec) in indices.into_iter().zip(vecs) {
                        let point = &mut upsert.points[idx];
                        add_point_vector(point, target_vec_name, VectorValue::Dense(vec))?;
                    }
                }
            }
        }
    }

    Ok(())
}

// ── Collect dense text jobs (model, text) in walk order ─────────────

fn collect_query_dense_jobs(
    query: &QueryStmt,
    jobs: &mut Vec<(String, String)>,
) -> Result<(), QqlError> {
    for cte in &query.ctes {
        collect_expr_dense_jobs(&cte.query.expression, jobs)?;
    }
    collect_expr_dense_jobs(&query.expression, jobs)
}

fn collect_prefetches_dense_jobs(
    prefetches: &[Prefetch],
    jobs: &mut Vec<(String, String)>,
) -> Result<(), QqlError> {
    for pref in prefetches {
        if let PrefetchSource::Query(sub) = &pref.source {
            collect_query_dense_jobs(sub, jobs)?;
        }
    }
    Ok(())
}

fn collect_expr_dense_jobs(
    expr: &QueryExpr,
    jobs: &mut Vec<(String, String)>,
) -> Result<(), QqlError> {
    match expr {
        QueryExpr::Nearest {
            input,
            using,
            prefetch,
            ..
        } => {
            collect_input_dense_job(input, require_embed_target(using)?, "default", jobs);
            collect_prefetches_dense_jobs(prefetch, jobs)?;
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
                collect_input_dense_job(input, target, "default", jobs);
            }
            collect_prefetches_dense_jobs(prefetch, jobs)?;
        }
        QueryExpr::Context {
            pairs,
            using,
            prefetch,
            ..
        } => {
            let target = require_embed_target(using)?;
            for pair in pairs {
                collect_input_dense_job(&pair.positive, target, "default", jobs);
                collect_input_dense_job(&pair.negative, target, "default", jobs);
            }
            collect_prefetches_dense_jobs(prefetch, jobs)?;
        }
        QueryExpr::Discover {
            target,
            context,
            using,
            prefetch,
            ..
        } => {
            let emb = require_embed_target(using)?;
            collect_input_dense_job(target, emb, "default", jobs);
            for pair in context {
                collect_input_dense_job(&pair.positive, emb, "default", jobs);
                collect_input_dense_job(&pair.negative, emb, "default", jobs);
            }
            collect_prefetches_dense_jobs(prefetch, jobs)?;
        }
        QueryExpr::Fusion { prefetch, .. } | QueryExpr::Formula { prefetch, .. } => {
            collect_prefetches_dense_jobs(prefetch, jobs)?;
        }
        QueryExpr::RelevanceFeedback {
            target,
            feedback,
            using,
            prefetch,
            ..
        } => {
            let emb = require_embed_target(using)?;
            collect_input_dense_job(target, emb, "default", jobs);
            for fb in feedback {
                collect_input_dense_job(&fb.example, emb, "default", jobs);
            }
            collect_prefetches_dense_jobs(prefetch, jobs)?;
        }
        QueryExpr::Hybrid { text, model, .. } => {
            let m = model.as_deref().unwrap_or("default").to_string();
            jobs.push((m, text.clone()));
        }
        QueryExpr::CrossRerank { prefetch, .. } => {
            // Query string is scored by the pair model, not embedded.
            collect_prefetches_dense_jobs(prefetch, jobs)?;
        }
        QueryExpr::Rerank {
            input,
            model,
            using,
            prefetch,
            ..
        } => {
            // RERANK uses the MODEL string for dense/multi, not "default".
            let mut emb = require_embed_target(using)?;
            // RERANK is always dense-family; multi comes from USING / schema.
            if emb.kind == VectorKind::Sparse {
                return Err(QqlError::execution(
                    "QQL-VECTOR-KIND",
                    "RERANK requires a dense (or multivector) target, not sparse",
                    None,
                ));
            }
            emb.kind = VectorKind::Dense;
            collect_input_dense_job(input, emb, model.as_str(), jobs);
            collect_prefetches_dense_jobs(prefetch, jobs)?;
        }
        _ => {}
    }
    Ok(())
}

/// Only single-vector dense TEXT inputs join the dense batch; sparse and multi
/// are applied one-by-one later.
fn collect_input_dense_job(
    input: &QueryInput,
    target: EmbedTarget,
    default_model: &str,
    jobs: &mut Vec<(String, String)>,
) {
    if let QueryInput::Text { text, model } = input {
        if target.kind == VectorKind::Dense && !target.multi {
            let m = model.as_deref().unwrap_or(default_model).to_string();
            jobs.push((m, text.clone()));
        }
    }
}

/// Group jobs by model, call `embed_dense_batch` once per model, restore walk order.
async fn batch_dense_by_model(
    embedder: &dyn Embedder,
    jobs: &[(String, String)],
) -> Result<Vec<Vec<f32>>, QqlError> {
    if jobs.is_empty() {
        return Ok(Vec::new());
    }

    let mut by_model: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, (model, _)) in jobs.iter().enumerate() {
        by_model.entry(model.clone()).or_default().push(i);
    }

    let mut out: Vec<Option<Vec<f32>>> = vec![None; jobs.len()];
    for (model, indices) in by_model {
        let texts: Vec<String> = indices.iter().map(|&i| jobs[i].1.clone()).collect();
        let vecs = embedder.embed_dense_batch(&texts, &model).await?;
        if vecs.len() != indices.len() {
            return Err(QqlError::execution(
                "QQL-EMBEDDING",
                format!(
                    "embed_dense_batch returned {} vectors for {} texts (model={model})",
                    vecs.len(),
                    indices.len()
                ),
                None,
            ));
        }
        for (idx, vec) in indices.into_iter().zip(vecs) {
            out[idx] = Some(vec);
        }
    }

    out.into_iter()
        .enumerate()
        .map(|(i, v)| {
            v.ok_or_else(|| {
                QqlError::execution(
                    "QQL-EMBEDDING",
                    format!("missing dense embedding at job index {i}"),
                    None,
                )
            })
        })
        .collect()
}

// ── Apply dense vectors (in collect order) + resolve sparse ─────────

fn apply_query_embeddings<'a>(
    query: &'a mut QueryStmt,
    embedder: &'a dyn Embedder,
    dense: DenseIter<'a>,
) -> BoxFut<'a, Result<(), QqlError>> {
    Box::pin(async move {
        for cte in &mut query.ctes {
            apply_expr_embeddings(&mut cte.query.expression, embedder, dense).await?;
        }
        apply_expr_embeddings(&mut query.expression, embedder, dense).await?;
        Ok(())
    })
}

fn apply_prefetches_embeddings<'a>(
    prefetches: &'a mut [Prefetch],
    embedder: &'a dyn Embedder,
    dense: DenseIter<'a>,
) -> BoxFut<'a, Result<(), QqlError>> {
    Box::pin(async move {
        for pref in prefetches {
            if let PrefetchSource::Query(sub) = &mut pref.source {
                apply_query_embeddings(sub, embedder, dense).await?;
            }
        }
        Ok(())
    })
}

fn apply_expr_embeddings<'a>(
    expr: &'a mut QueryExpr,
    embedder: &'a dyn Embedder,
    dense: DenseIter<'a>,
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
                    "default",
                    embedder,
                    dense,
                )
                .await?;
                apply_prefetches_embeddings(prefetch, embedder, dense).await?;
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
                    apply_input(input, target, "default", embedder, dense).await?;
                }
                apply_prefetches_embeddings(prefetch, embedder, dense).await?;
            }
            QueryExpr::Context {
                pairs,
                using,
                prefetch,
                ..
            } => {
                let target = require_embed_target(using)?;
                for pair in pairs {
                    apply_input(&mut pair.positive, target, "default", embedder, dense).await?;
                    apply_input(&mut pair.negative, target, "default", embedder, dense).await?;
                }
                apply_prefetches_embeddings(prefetch, embedder, dense).await?;
            }
            QueryExpr::Discover {
                target,
                context,
                using,
                prefetch,
                ..
            } => {
                let emb = require_embed_target(using)?;
                apply_input(target, emb, "default", embedder, dense).await?;
                for pair in context {
                    apply_input(&mut pair.positive, emb, "default", embedder, dense).await?;
                    apply_input(&mut pair.negative, emb, "default", embedder, dense).await?;
                }
                apply_prefetches_embeddings(prefetch, embedder, dense).await?;
            }
            QueryExpr::Fusion { prefetch, .. } | QueryExpr::Formula { prefetch, .. } => {
                apply_prefetches_embeddings(prefetch, embedder, dense).await?;
            }
            QueryExpr::RelevanceFeedback {
                target,
                feedback,
                using,
                prefetch,
                ..
            } => {
                let emb = require_embed_target(using)?;
                apply_input(target, emb, "default", embedder, dense).await?;
                for fb in feedback {
                    apply_input(&mut fb.example, emb, "default", embedder, dense).await?;
                }
                apply_prefetches_embeddings(prefetch, embedder, dense).await?;
            }
            QueryExpr::Hybrid {
                text,
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
                let s_vec = embedder.embed_sparse(text, "default").await?;
                let d_vec_name = dense_vector.as_deref().unwrap_or(DENSE_VECTOR_NAME);
                let s_vec_name = sparse_vector.as_deref().unwrap_or(SPARSE_VECTOR_NAME);

                let dense_sub = QueryStmt {
                    ctes: Vec::new(),
                    collection: qql_core::ast::QueryCollection::Inherited,
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
                model,
                using,
                prefetch,
                ..
            } => {
                let mut emb = require_embed_target(using)?;
                emb.kind = VectorKind::Dense;
                apply_input(input, emb, model.as_str(), embedder, dense).await?;
                apply_prefetches_embeddings(prefetch, embedder, dense).await?;
            }
            QueryExpr::CrossRerank { prefetch, .. } => {
                apply_prefetches_embeddings(prefetch, embedder, dense).await?;
            }
            _ => {}
        }
        Ok(())
    })
}

async fn apply_input(
    input: &mut QueryInput,
    target: EmbedTarget,
    default_model: &str,
    embedder: &dyn Embedder,
    dense: DenseIter<'_>,
) -> Result<(), QqlError> {
    match input {
        QueryInput::Image { source, model } => {
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
            let model_name = model.as_deref().unwrap_or(default_model);
            let vec = embedder.embed_image(source, model_name).await?;
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
        QueryInput::Text { text, model } => {
            let model_name = model.as_deref().unwrap_or(default_model);
            if target.kind == VectorKind::Sparse {
                let s_vec = embedder.embed_sparse(text, model_name).await?;
                *input = QueryInput::Vector(VectorValue::Sparse {
                    indices: s_vec.indices,
                    values: s_vec.values,
                });
                return Ok(());
            }
            if target.multi {
                let rows = embedder.embed_multi(text, model_name).await?;
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
            *input = QueryInput::Vector(VectorValue::Dense(vec));
            Ok(())
        }
        QueryInput::Vector(_) | QueryInput::Point(_) => Ok(()),
    }
}

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

fn ensure_batch_len(got: usize, expected: usize, model: &str) -> Result<(), QqlError> {
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

async fn resolve_single_embedding_spec(
    upsert: &mut UpsertStmt,
    spec: &EmbeddingSpec,
    embedder: &dyn Embedder,
    seen_vectors: &mut std::collections::HashSet<String>,
) -> Result<(), QqlError> {
    match spec {
        EmbeddingSpec::Multi(specs) => {
            for sub_spec in specs {
                Box::pin(resolve_single_embedding_spec(
                    upsert,
                    sub_spec,
                    embedder,
                    seen_vectors,
                ))
                .await?;
            }
        }
        EmbeddingSpec::Dense {
            model,
            vector,
            field,
        } => {
            let model_name = model.as_deref().unwrap_or("default");
            let vector_name = vector.as_deref().unwrap_or(DENSE_VECTOR_NAME);
            check_and_insert_vector_name(seen_vectors, vector_name)?;

            let targets = collect_text_targets(&upsert.points, field.as_deref());
            validate_non_empty_targets(upsert, &targets, "DENSE", field.as_deref())?;

            let (indices, texts): (Vec<usize>, Vec<String>) = targets.into_iter().unzip();
            let vecs = embedder.embed_dense_batch(&texts, model_name).await?;
            ensure_batch_len(vecs.len(), indices.len(), model_name)?;
            for (idx, vec) in indices.into_iter().zip(vecs) {
                let point = &mut upsert.points[idx];
                add_point_vector(point, vector_name, VectorValue::Dense(vec))?;
            }
        }
        EmbeddingSpec::Sparse {
            model,
            vector,
            field,
        } => {
            let model_name = model.as_deref().unwrap_or("default");
            let vector_name = vector.as_deref().unwrap_or(SPARSE_VECTOR_NAME);
            check_and_insert_vector_name(seen_vectors, vector_name)?;

            let targets = collect_text_targets(&upsert.points, field.as_deref());
            validate_non_empty_targets(upsert, &targets, "SPARSE", field.as_deref())?;

            for (idx, text) in targets {
                let sparse_vec = embedder.embed_sparse(&text, model_name).await?;
                add_point_vector(
                    &mut upsert.points[idx],
                    vector_name,
                    VectorValue::Sparse {
                        indices: sparse_vec.indices,
                        values: sparse_vec.values,
                    },
                )?;
            }
        }
        EmbeddingSpec::Hybrid {
            dense_model,
            dense_vector,
            dense_field,
            sparse_model,
            sparse_vector,
            sparse_field,
        } => {
            let d_model = dense_model.as_deref().unwrap_or("default");
            let s_model = sparse_model.as_deref().unwrap_or("default");
            let d_vec_name = dense_vector.as_deref().unwrap_or(DENSE_VECTOR_NAME);
            let s_vec_name = sparse_vector.as_deref().unwrap_or(SPARSE_VECTOR_NAME);

            check_and_insert_vector_name(seen_vectors, d_vec_name)?;
            check_and_insert_vector_name(seen_vectors, s_vec_name)?;

            let dense_targets = collect_text_targets(&upsert.points, dense_field.as_deref());
            let sparse_targets = collect_text_targets(&upsert.points, sparse_field.as_deref());

            validate_non_empty_targets(upsert, &dense_targets, "DENSE", dense_field.as_deref())?;
            validate_non_empty_targets(upsert, &sparse_targets, "SPARSE", sparse_field.as_deref())?;

            let (indices, texts): (Vec<usize>, Vec<String>) = dense_targets.into_iter().unzip();
            let dense_vecs = embedder.embed_dense_batch(&texts, d_model).await?;
            ensure_batch_len(dense_vecs.len(), indices.len(), d_model)?;
            for (idx, d_vec) in indices.into_iter().zip(dense_vecs) {
                let point = &mut upsert.points[idx];
                add_point_vector(point, d_vec_name, VectorValue::Dense(d_vec))?;
            }

            for (idx, text) in sparse_targets {
                let sparse_vec = embedder.embed_sparse(&text, s_model).await?;
                let point = &mut upsert.points[idx];
                add_point_vector(
                    point,
                    s_vec_name,
                    VectorValue::Sparse {
                        indices: sparse_vec.indices,
                        values: sparse_vec.values,
                    },
                )?;
            }
        }
        EmbeddingSpec::MultiVector {
            model,
            vector,
            field,
        } => {
            let model_name = model.as_deref().unwrap_or("default");
            let vector_name = vector.as_deref().unwrap_or("colbert");
            check_and_insert_vector_name(seen_vectors, vector_name)?;

            let targets = collect_text_targets(&upsert.points, field.as_deref());
            validate_non_empty_targets(upsert, &targets, "MULTI", field.as_deref())?;

            let (indices, texts): (Vec<usize>, Vec<String>) = targets.into_iter().unzip();
            let bags = embedder.embed_multi_batch(&texts, model_name).await?;
            if bags.len() != indices.len() {
                return Err(QqlError::execution(
                    "QQL-EMBEDDING-MULTI",
                    format!(
                        "embed_multi_batch returned {} bags for {} texts (model={model_name})",
                        bags.len(),
                        indices.len()
                    ),
                    None,
                ));
            }
            for (idx, rows) in indices.into_iter().zip(bags) {
                if rows.is_empty() {
                    return Err(QqlError::execution(
                        "QQL-EMBEDDING-MULTI",
                        "embed_multi returned an empty multivector",
                        None,
                    ));
                }
                add_point_vector(
                    &mut upsert.points[idx],
                    vector_name,
                    VectorValue::MultiDense(rows),
                )?;
            }
        }
        EmbeddingSpec::Image {
            model,
            vector,
            field,
        } => {
            let model_name = model.as_deref().unwrap_or("default");
            let vector_name = vector.as_deref().unwrap_or("image");
            check_and_insert_vector_name(seen_vectors, vector_name)?;

            let targets = collect_image_targets(&upsert.points, field.as_deref());
            validate_non_empty_targets(upsert, &targets, "IMAGE", field.as_deref())?;

            let (indices, sources): (Vec<usize>, Vec<String>) = targets.into_iter().unzip();
            let vecs = embedder.embed_image_batch(&sources, model_name).await?;
            if vecs.len() != indices.len() {
                return Err(QqlError::execution(
                    "QQL-EMBEDDING-IMAGE",
                    format!(
                        "embed_image_batch returned {} vectors for {} sources (model={model_name})",
                        vecs.len(),
                        indices.len()
                    ),
                    None,
                ));
            }
            for (idx, vec) in indices.into_iter().zip(vecs) {
                add_point_vector(
                    &mut upsert.points[idx],
                    vector_name,
                    VectorValue::Dense(vec),
                )?;
            }
        }
    }
    Ok(())
}

fn validate_non_empty_targets(
    upsert: &UpsertStmt,
    targets: &[(usize, String)],
    kind: &str,
    field: Option<&str>,
) -> Result<(), QqlError> {
    if targets.is_empty() {
        let actual_fields = upsert
            .points
            .first()
            .map(|p| {
                p.payload
                    .iter()
                    .map(|(k, _)| k.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();

        let err_msg = if let Some(f) = field {
            format!(
                "USING {kind} MODEL specified with ON FIELD '{f}' but no matching text payload field found. Found fields: {actual_fields}"
            )
        } else {
            format!(
                "USING {kind} MODEL specified but no text payload field found. Expected one of: {}. Found fields: {actual_fields}",
                DEFAULT_TEXT_FIELDS_ORDERED.join(", ")
            )
        };

        return Err(QqlError::execution("QQL-EMBEDDING", err_msg, None));
    }
    Ok(())
}

fn check_and_insert_vector_name(
    seen_vectors: &mut std::collections::HashSet<String>,
    vector_name: &str,
) -> Result<(), QqlError> {
    if !seen_vectors.insert(vector_name.to_string()) {
        return Err(QqlError::execution(
            "QQL-EMBEDDING",
            format!("duplicate target vector '{vector_name}' in multi-spec embedding clause"),
            None,
        ));
    }
    Ok(())
}

const DEFAULT_TEXT_FIELDS_ORDERED: &[&str] = &[
    "text",
    "body",
    "content",
    "title",
    "description",
    "name",
    "summary",
    "document",
];

const DEFAULT_IMAGE_FIELDS_ORDERED: &[&str] = &[
    "image",
    "image_path",
    "image_url",
    "photo",
    "picture",
    "img",
    "path",
    "url",
];

/// Collect image path/URL payload fields for IMAGE embedding specs.
fn collect_image_targets(
    points: &[UpsertPoint],
    field_override: Option<&str>,
) -> Vec<(usize, String)> {
    if let Some(target_field) = field_override {
        points
            .iter()
            .enumerate()
            .filter_map(|(idx, point)| {
                point.payload.iter().find_map(|(key, value)| {
                    if key.eq_ignore_ascii_case(target_field) {
                        if let qql_core::ast::Value::Str(source) = value {
                            if !source.is_empty() {
                                return Some((idx, source.clone()));
                            }
                        }
                    }
                    None
                })
            })
            .collect()
    } else {
        points
            .iter()
            .enumerate()
            .filter_map(|(idx, point)| {
                for &candidate in DEFAULT_IMAGE_FIELDS_ORDERED {
                    if let Some((_, qql_core::ast::Value::Str(source))) = point
                        .payload
                        .iter()
                        .find(|(key, _)| key.eq_ignore_ascii_case(candidate))
                    {
                        if !source.is_empty() {
                            return Some((idx, source.clone()));
                        }
                    }
                }
                None
            })
            .collect()
    }
}

fn collect_text_targets(
    points: &[UpsertPoint],
    field_override: Option<&str>,
) -> Vec<(usize, String)> {
    if let Some(target_field) = field_override {
        points
            .iter()
            .enumerate()
            .filter_map(|(idx, point)| {
                point.payload.iter().find_map(|(key, value)| {
                    if key.eq_ignore_ascii_case(target_field) {
                        if let qql_core::ast::Value::Str(text) = value {
                            if !text.is_empty() {
                                return Some((idx, text.clone()));
                            }
                        }
                    }
                    None
                })
            })
            .collect()
    } else {
        collect_default_text_targets(points)
    }
}

fn collect_default_text_targets(points: &[UpsertPoint]) -> Vec<(usize, String)> {
    points
        .iter()
        .enumerate()
        .filter_map(|(idx, point)| {
            for &candidate in DEFAULT_TEXT_FIELDS_ORDERED {
                if let Some((_, qql_core::ast::Value::Str(text))) = point
                    .payload
                    .iter()
                    .find(|(key, _)| key.eq_ignore_ascii_case(candidate))
                {
                    if !text.is_empty() {
                        return Some((idx, text.clone()));
                    }
                }
            }
            None
        })
        .collect()
}

fn add_point_vector(
    point: &mut UpsertPoint,
    name: &str,
    vector: VectorValue,
) -> Result<(), QqlError> {
    if name.is_empty() {
        return match &mut point.vectors {
            Some(PointVectors::Unnamed(existing)) => {
                *existing = vector;
                Ok(())
            }
            Some(PointVectors::Named(list)) => {
                if let Some(existing) = list.iter_mut().find(|(key, _)| key.is_empty()) {
                    existing.1 = vector;
                } else {
                    list.push((String::new(), vector));
                }
                Ok(())
            }
            None => {
                point.vectors = Some(PointVectors::Unnamed(vector));
                Ok(())
            }
        };
    }
    match &mut point.vectors {
        Some(PointVectors::Named(list)) => {
            if let Some(existing) = list.iter_mut().find(|(k, _)| k == name) {
                existing.1 = vector;
            } else {
                list.push((name.to_string(), vector));
            }
            Ok(())
        }
        Some(PointVectors::Unnamed(_)) => Err(QqlError::execution(
            "QQL-EMBEDDING",
            format!(
                "cannot add named vector '{name}' to a point that already has an unnamed vector; \
                 provide an explicit named-vector topology or omit EMBED for this point"
            ),
            None,
        )),
        None => {
            point.vectors = Some(PointVectors::Named(vec![(name.to_string(), vector)]));
            Ok(())
        }
    }
}
