use qql_core::ast::{
    EmbedKind, EmbeddingSpec, PointEntry, PointVectors, Stmt, UpsertStmt, VectorValue,
};
use qql_core::error::QqlError;

use crate::embedder::Embedder;

pub(crate) use super::resolve_query::ensure_batch_len;
use super::resolve_query::resolve_query_embeddings;
pub use super::resolve_query::{DENSE_VECTOR_NAME, SPARSE_VECTOR_NAME};

/// Resolve text → vectors on a statement before routing/execution.
///
/// Every modality batches alike: dense, query-side sparse, multi, and image
/// jobs are each collected in walk order and sent through the matching
/// `*_batch` entry point grouped by model (one RPC per model). Sparse stays
/// role-split via the embedder: queries embed with unit weights, documents
/// with BM25 tf saturation (both wire-compatible with Qdrant's
/// `qdrant/bm25`).
pub async fn resolve_embeddings(stmt: &mut Stmt, embedder: &dyn Embedder) -> Result<(), QqlError> {
    match stmt {
        Stmt::Query(query) => resolve_query_embeddings(query, embedder).await?,
        Stmt::Upsert(upsert) => resolve_upsert_embeddings(upsert, embedder).await?,
        _ => {}
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
            let PointEntry::Inline(inline) = point else {
                continue;
            };
            if inline.vectors.is_none()
                && let Some((_, qql_core::ast::Value::Str(text))) =
                    inline.payload.iter().find(|(k, _)| {
                        eq_lowered(k, "text") || eq_lowered(k, "body") || eq_lowered(k, "content")
                    })
                && !text.is_empty()
            {
                targets.push((idx, text.clone()));
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
        // Duplicate-target tracking stays a small `Vec`: a handful of names
        // at most, so linear scan beats a per-op `HashSet` (no hashing).
        let mut seen_vectors = Vec::new();
        resolve_single_embedding_spec(upsert, &spec, embedder, &mut seen_vectors).await?;
    }

    for directive in &upsert.embed {
        let field_name = &directive.source_field;
        let target_vec_name = &directive.target_vector;
        let mut targets = Vec::new();
        for (idx, point) in upsert.points.iter().enumerate() {
            let PointEntry::Inline(inline) = point else {
                continue;
            };
            if let Some((_, qql_core::ast::Value::Str(text))) = inline
                .payload
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(field_name))
                && !text.is_empty()
            {
                targets.push((idx, text.clone()));
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
                    let (indices, texts): (Vec<usize>, Vec<String>) = targets.into_iter().unzip();
                    let vecs = embedder.embed_sparse_document_batch(&texts, m).await?;
                    ensure_batch_len(vecs.len(), indices.len(), m)?;
                    for (idx, s_vec) in indices.into_iter().zip(vecs) {
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

async fn resolve_single_embedding_spec(
    upsert: &mut UpsertStmt,
    spec: &EmbeddingSpec,
    embedder: &dyn Embedder,
    seen_vectors: &mut Vec<String>,
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

            let (indices, texts): (Vec<usize>, Vec<String>) = targets.into_iter().unzip();
            let vecs = embedder
                .embed_sparse_document_batch(&texts, model_name)
                .await?;
            ensure_batch_len(vecs.len(), indices.len(), model_name)?;
            for (idx, sparse_vec) in indices.into_iter().zip(vecs) {
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

            let (sparse_indices, sparse_texts): (Vec<usize>, Vec<String>) =
                sparse_targets.into_iter().unzip();
            let sparse_vecs = embedder
                .embed_sparse_document_batch(&sparse_texts, s_model)
                .await?;
            ensure_batch_len(sparse_vecs.len(), sparse_indices.len(), s_model)?;
            for (idx, sparse_vec) in sparse_indices.into_iter().zip(sparse_vecs) {
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
            .and_then(|p| match p {
                PointEntry::Inline(inline) => Some(inline),
                PointEntry::Param(..) | PointEntry::PositionalParam(..) => None,
            })
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
    seen_vectors: &mut Vec<String>,
    vector_name: &str,
) -> Result<(), QqlError> {
    if seen_vectors.iter().any(|name| name == vector_name) {
        return Err(QqlError::execution(
            "QQL-EMBEDDING",
            format!("duplicate target vector '{vector_name}' in multi-spec embedding clause"),
            None,
        ));
    }
    seen_vectors.push(vector_name.to_string());
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

/// ASCII case-insensitive equality against an already-lowercased needle.
///
/// Callers lowercase the needle once outside the per-point loop, so each
/// comparison folds only the haystack side. The exact-match fast path covers
/// the common already-lowercase payload keys (`text`, `title`, …) with a
/// single `memcmp`.
fn eq_lowered(haystack: &str, needle_lower: &str) -> bool {
    if haystack == needle_lower {
        return true;
    }
    // ASCII lowercasing never changes byte length, so the length check is
    // exact and short-circuits mismatched keys before folding.
    haystack.len() == needle_lower.len()
        && haystack
            .as_bytes()
            .iter()
            .zip(needle_lower.as_bytes())
            .all(|(a, b)| a.to_ascii_lowercase() == *b)
}

/// Collect image path/URL payload fields for IMAGE embedding specs.
fn collect_image_targets(
    points: &[PointEntry],
    field_override: Option<&str>,
) -> Vec<(usize, String)> {
    if let Some(target_field) = field_override {
        let target_lower = target_field.to_ascii_lowercase();
        points
            .iter()
            .enumerate()
            .filter_map(|(idx, point)| {
                let PointEntry::Inline(inline) = point else {
                    return None;
                };
                inline.payload.iter().find_map(|(key, value)| {
                    if eq_lowered(key, &target_lower)
                        && let qql_core::ast::Value::Str(source) = value
                        && !source.is_empty()
                    {
                        return Some((idx, source.clone()));
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
                let PointEntry::Inline(inline) = point else {
                    return None;
                };
                for &candidate in DEFAULT_IMAGE_FIELDS_ORDERED {
                    if let Some((_, qql_core::ast::Value::Str(source))) = inline
                        .payload
                        .iter()
                        .find(|(key, _)| eq_lowered(key, candidate))
                        && !source.is_empty()
                    {
                        return Some((idx, source.clone()));
                    }
                }
                None
            })
            .collect()
    }
}

fn collect_text_targets(
    points: &[PointEntry],
    field_override: Option<&str>,
) -> Vec<(usize, String)> {
    if let Some(target_field) = field_override {
        let target_lower = target_field.to_ascii_lowercase();
        points
            .iter()
            .enumerate()
            .filter_map(|(idx, point)| {
                let PointEntry::Inline(inline) = point else {
                    return None;
                };
                inline.payload.iter().find_map(|(key, value)| {
                    if eq_lowered(key, &target_lower)
                        && let qql_core::ast::Value::Str(text) = value
                        && !text.is_empty()
                    {
                        return Some((idx, text.clone()));
                    }
                    None
                })
            })
            .collect()
    } else {
        collect_default_text_targets(points)
    }
}

fn collect_default_text_targets(points: &[PointEntry]) -> Vec<(usize, String)> {
    points
        .iter()
        .enumerate()
        .filter_map(|(idx, point)| {
            let PointEntry::Inline(inline) = point else {
                return None;
            };
            for &candidate in DEFAULT_TEXT_FIELDS_ORDERED {
                if let Some((_, qql_core::ast::Value::Str(text))) = inline
                    .payload
                    .iter()
                    .find(|(key, _)| eq_lowered(key, candidate))
                    && !text.is_empty()
                {
                    return Some((idx, text.clone()));
                }
            }
            None
        })
        .collect()
}

fn add_point_vector(
    point: &mut PointEntry,
    name: &str,
    vector: VectorValue,
) -> Result<(), QqlError> {
    let point = match point {
        PointEntry::Inline(inline) => inline,
        // Unreachable via collect_*_targets (they skip placeholders), but
        // embedding into unknown payload must never silently drop vectors.
        PointEntry::Param(name, span) => {
            return Err(QqlError::execution(
                "QQL-EMBEDDING",
                format!("cannot embed into unbound point parameter ':{name}'"),
                span.as_deref().copied(),
            ));
        }
        PointEntry::PositionalParam(idx, span) => {
            return Err(QqlError::execution(
                "QQL-EMBEDDING",
                format!("cannot embed into unbound point parameter '?{}'", *idx + 1),
                span.as_deref().copied(),
            ));
        }
    };
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
            Some(PointVectors::Param(..)) | Some(PointVectors::PositionalParam(..)) => {
                point.vectors = Some(PointVectors::Unnamed(vector));
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
        Some(PointVectors::Unnamed(_))
        | Some(PointVectors::Param(..))
        | Some(PointVectors::PositionalParam(..)) => Err(QqlError::execution(
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
