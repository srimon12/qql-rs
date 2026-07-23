use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use qql_core::ast::{
    EmbedKind, EmbeddingSpec, PointVectors, Prefetch, PrefetchSource, QueryExpr, QueryInput,
    QueryStmt, Stmt, UpsertPoint, UpsertStmt, VectorValue,
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
    collect_query_dense_jobs(query, &mut dense_jobs);

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
            let (indices, texts): (Vec<usize>, Vec<String>) = targets.into_iter().unzip();
            let dense_vecs = embedder.embed_dense_batch(&texts, "default").await?;
            ensure_batch_len(dense_vecs.len(), indices.len(), "default")?;
            for ((idx, text), d_vec) in indices.into_iter().zip(texts).zip(dense_vecs) {
                let point = &mut upsert.points[idx];
                add_point_vector(point, DENSE_VECTOR_NAME, VectorValue::Dense(d_vec))?;
                let sparse_vec = embedder.embed_sparse(&text).await?;
                add_point_vector(
                    point,
                    SPARSE_VECTOR_NAME,
                    VectorValue::Sparse {
                        indices: sparse_vec.indices,
                        values: sparse_vec.values,
                    },
                )?;
            }
        }
    }

    if let Some(ref spec) = upsert.embedding {
        match spec {
            EmbeddingSpec::Dense { model, vector } => {
                let model_name = model.as_deref().unwrap_or("default");
                let vector_name = vector.as_deref().unwrap_or(DENSE_VECTOR_NAME);

                let mut targets = Vec::new();
                for (idx, point) in upsert.points.iter().enumerate() {
                    if let Some((_, qql_core::ast::Value::Str(text))) = point
                        .payload
                        .iter()
                        .find(|(k, _)| k.eq_ignore_ascii_case("text"))
                    {
                        if !text.is_empty() {
                            targets.push((idx, text.clone()));
                        }
                    }
                }

                if !targets.is_empty() {
                    let (indices, texts): (Vec<usize>, Vec<String>) = targets.into_iter().unzip();
                    let vecs = embedder.embed_dense_batch(&texts, model_name).await?;
                    ensure_batch_len(vecs.len(), indices.len(), model_name)?;
                    for (idx, vec) in indices.into_iter().zip(vecs) {
                        let point = &mut upsert.points[idx];
                        add_point_vector(point, vector_name, VectorValue::Dense(vec))?;
                    }
                }
            }
            EmbeddingSpec::Hybrid {
                dense_model,
                dense_vector,
                sparse_vector,
                sparse_model,
            } => {
                if sparse_model.is_some() {
                    return Err(QqlError::execution(
                            "QQL-EMBEDDING",
                            "sparse model selection is not supported by the local BM25 sparse embedder; omit SPARSE MODEL",
                            None,
                        ));
                }
                let d_model = dense_model.as_deref().unwrap_or("default");
                let d_vec_name = dense_vector.as_deref().unwrap_or(DENSE_VECTOR_NAME);
                let s_vec_name = sparse_vector.as_deref().unwrap_or(SPARSE_VECTOR_NAME);

                let mut targets = Vec::new();
                for (idx, point) in upsert.points.iter().enumerate() {
                    if let Some((_, qql_core::ast::Value::Str(text))) = point
                        .payload
                        .iter()
                        .find(|(k, _)| k.eq_ignore_ascii_case("text"))
                    {
                        if !text.is_empty() {
                            targets.push((idx, text.clone()));
                        }
                    }
                }

                if !targets.is_empty() {
                    let (indices, texts): (Vec<usize>, Vec<String>) = targets.into_iter().unzip();
                    let dense_vecs = embedder.embed_dense_batch(&texts, d_model).await?;
                    ensure_batch_len(dense_vecs.len(), indices.len(), d_model)?;
                    for ((idx, text), d_vec) in indices.into_iter().zip(texts).zip(dense_vecs) {
                        let sparse_vec = embedder.embed_sparse(&text).await?;
                        let point = &mut upsert.points[idx];
                        add_point_vector(point, d_vec_name, VectorValue::Dense(d_vec))?;
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
            }
        }
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
                    if model.is_some() {
                        return Err(QqlError::execution(
                                "QQL-EMBEDDING",
                                "sparse model selection is not supported by the local BM25 sparse embedder; omit MODEL on EMBED SPARSE",
                                None,
                            ));
                    }
                    for (idx, text) in targets {
                        let s_vec = embedder.embed_sparse(&text).await?;
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
            }
        }
    }

    Ok(())
}

// ── Collect dense text jobs (model, text) in walk order ─────────────

fn collect_query_dense_jobs(query: &QueryStmt, jobs: &mut Vec<(String, String)>) {
    for cte in &query.ctes {
        collect_expr_dense_jobs(&cte.query.expression, jobs);
    }
    collect_expr_dense_jobs(&query.expression, jobs);
}

fn collect_prefetches_dense_jobs(prefetches: &[Prefetch], jobs: &mut Vec<(String, String)>) {
    for pref in prefetches {
        if let PrefetchSource::Query(sub) = &pref.source {
            collect_query_dense_jobs(sub, jobs);
        }
    }
}

fn collect_expr_dense_jobs(expr: &QueryExpr, jobs: &mut Vec<(String, String)>) {
    match expr {
        QueryExpr::Nearest {
            input,
            using,
            prefetch,
            ..
        } => {
            collect_input_dense_job(
                input,
                using.as_deref().unwrap_or("default"),
                "default",
                jobs,
            );
            collect_prefetches_dense_jobs(prefetch, jobs);
        }
        QueryExpr::Recommend {
            positive,
            negative,
            using,
            prefetch,
            ..
        } => {
            let v = using.as_deref().unwrap_or("default");
            for input in positive.iter().chain(negative.iter()) {
                collect_input_dense_job(input, v, "default", jobs);
            }
            collect_prefetches_dense_jobs(prefetch, jobs);
        }
        QueryExpr::Context {
            pairs,
            using,
            prefetch,
            ..
        } => {
            let v = using.as_deref().unwrap_or("default");
            for pair in pairs {
                collect_input_dense_job(&pair.positive, v, "default", jobs);
                collect_input_dense_job(&pair.negative, v, "default", jobs);
            }
            collect_prefetches_dense_jobs(prefetch, jobs);
        }
        QueryExpr::Discover {
            target,
            context,
            using,
            prefetch,
            ..
        } => {
            let v = using.as_deref().unwrap_or("default");
            collect_input_dense_job(target, v, "default", jobs);
            for pair in context {
                collect_input_dense_job(&pair.positive, v, "default", jobs);
                collect_input_dense_job(&pair.negative, v, "default", jobs);
            }
            collect_prefetches_dense_jobs(prefetch, jobs);
        }
        QueryExpr::Fusion { prefetch, .. } | QueryExpr::Formula { prefetch, .. } => {
            collect_prefetches_dense_jobs(prefetch, jobs);
        }
        QueryExpr::RelevanceFeedback {
            target,
            feedback,
            using,
            prefetch,
            ..
        } => {
            let v = using.as_deref().unwrap_or("default");
            collect_input_dense_job(target, v, "default", jobs);
            for fb in feedback {
                collect_input_dense_job(&fb.example, v, "default", jobs);
            }
            collect_prefetches_dense_jobs(prefetch, jobs);
        }
        QueryExpr::Hybrid { text, model, .. } => {
            let m = model.as_deref().unwrap_or("default").to_string();
            jobs.push((m, text.clone()));
        }
        QueryExpr::Rerank {
            input,
            model,
            using,
            prefetch,
        } => {
            collect_input_dense_job(input, using.as_str(), model.as_str(), jobs);
            collect_prefetches_dense_jobs(prefetch, jobs);
        }
        _ => {}
    }
}

fn collect_input_dense_job(
    input: &QueryInput,
    using: &str,
    default_model: &str,
    jobs: &mut Vec<(String, String)>,
) {
    if let QueryInput::Text { text, model } = input {
        // Sparse vector space uses local sparse embedder, not dense HTTP batch.
        if using != "sparse" {
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
                    using.as_deref().unwrap_or("default"),
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
                let v = using.as_deref().unwrap_or("default");
                for input in positive.iter_mut().chain(negative.iter_mut()) {
                    apply_input(input, v, embedder, dense).await?;
                }
                apply_prefetches_embeddings(prefetch, embedder, dense).await?;
            }
            QueryExpr::Context {
                pairs,
                using,
                prefetch,
                ..
            } => {
                let v = using.as_deref().unwrap_or("default");
                for pair in pairs {
                    apply_input(&mut pair.positive, v, embedder, dense).await?;
                    apply_input(&mut pair.negative, v, embedder, dense).await?;
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
                let v = using.as_deref().unwrap_or("default");
                apply_input(target, v, embedder, dense).await?;
                for pair in context {
                    apply_input(&mut pair.positive, v, embedder, dense).await?;
                    apply_input(&mut pair.negative, v, embedder, dense).await?;
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
                let v = using.as_deref().unwrap_or("default");
                apply_input(target, v, embedder, dense).await?;
                for fb in feedback {
                    apply_input(&mut fb.example, v, embedder, dense).await?;
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
                let s_vec = embedder.embed_sparse(text).await?;
                let d_vec_name = dense_vector.as_deref().unwrap_or(DENSE_VECTOR_NAME);
                let s_vec_name = sparse_vector.as_deref().unwrap_or(SPARSE_VECTOR_NAME);

                let dense_sub = QueryStmt {
                    ctes: Vec::new(),
                    collection: qql_core::ast::QueryCollection::Inherited,
                    expression: QueryExpr::Nearest {
                        input: QueryInput::Vector(VectorValue::Dense(d_vec)),
                        using: Some(d_vec_name.to_string()),
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
                        using: Some(s_vec_name.to_string()),
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
            } => {
                // Consume dense vectors under the rerank model; sparse only when using == "sparse".
                let dense_key = if using == "sparse" {
                    using.as_str()
                } else {
                    model.as_str()
                };
                apply_input(input, dense_key, embedder, dense).await?;
                apply_prefetches_embeddings(prefetch, embedder, dense).await?;
            }
            _ => {}
        }
        Ok(())
    })
}

async fn apply_input(
    input: &mut QueryInput,
    using: &str,
    embedder: &dyn Embedder,
    dense: DenseIter<'_>,
) -> Result<(), QqlError> {
    if let QueryInput::Text { text, .. } = input {
        if using == "sparse" {
            let s_vec = embedder.embed_sparse(text).await?;
            *input = QueryInput::Vector(VectorValue::Sparse {
                indices: s_vec.indices,
                values: s_vec.values,
            });
        } else {
            let vec = dense.next().ok_or_else(|| {
                QqlError::execution(
                    "QQL-EMBEDDING",
                    "internal error: ran out of dense embeddings",
                    None,
                )
            })?;
            *input = QueryInput::Vector(VectorValue::Dense(vec));
        }
    }
    Ok(())
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

fn add_point_vector(
    point: &mut UpsertPoint,
    name: &str,
    vector: VectorValue,
) -> Result<(), QqlError> {
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
