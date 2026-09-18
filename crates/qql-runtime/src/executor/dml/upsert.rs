use crate::backend::{CollectionInfo, VectorSpec};
#[cfg(feature = "rest")]
use crate::embedder::HttpEmbedder;
use crate::executor::Executor;
use qql_core::ast::{EmbeddingSpec, PointEntry, PointVectors, UpsertStmt, Value, VectorValue};
use qql_core::error::QqlError;
use std::collections::HashMap;
use std::sync::Arc;

impl Executor {
    /// Infer implicit text embedding from the existing collection topology.
    /// New collections retain the historical hybrid default, while existing
    /// dense-only and sparse-only collections receive only compatible vectors.
    pub(crate) async fn configure_upsert_embeddings(
        &self,
        upsert: &mut UpsertStmt,
    ) -> Result<Option<Arc<CollectionInfo>>, QqlError> {
        let needs_implicit = upsert.embedding.is_none()
            && upsert.embed.is_empty()
            && upsert.points.iter().any(|point| {
                let PointEntry::Inline(inline) = point else {
                    return false;
                };
                inline.vectors.is_none()
                    && inline.payload.iter().any(|(key, value)| {
                        matches!(value, Value::Str(text) if !text.is_empty())
                            && (key.eq_ignore_ascii_case("text")
                                || key.eq_ignore_ascii_case("body")
                                || key.eq_ignore_ascii_case("content"))
                    })
            });

        let has_unnamed_vectors = upsert.points.iter().any(|p| {
            matches!(
                p,
                PointEntry::Inline(inline)
                    if matches!(inline.vectors, Some(PointVectors::Unnamed(_)))
            )
        });
        let has_embedding = upsert.embedding.is_some() || !upsert.embed.is_empty();
        if !needs_implicit && !has_embedding {
            // Cache-first unnamed mapping: a hit costs no round trip; a miss
            // probes existence and fetches only when the collection exists,
            // so a missing collection falls through without a schema fetch.
            if has_unnamed_vectors {
                if let Some(info) = self.peek_cached_collection_info(&upsert.collection).await {
                    map_unnamed_to_single_dense(upsert, &info);
                    return Ok(Some(info));
                }
                if self
                    .client
                    .collection_exists(&upsert.collection)
                    .await
                    .unwrap_or(false)
                    && let Ok(info) = self.get_cached_collection_info(&upsert.collection).await
                {
                    map_unnamed_to_single_dense(upsert, &info);
                    return Ok(Some(info));
                }
            }
            return Ok(None);
        }
        // Cache-first with the same error contract as before: the existence
        // probe still decides missing (historical implicit defaults) versus
        // transient (propagated); the schema fetch runs only for existing
        // collections, and a cache hit skips both round trips.
        let info = match self.peek_cached_collection_info(&upsert.collection).await {
            Some(info) => info,
            None => {
                if !self.client.collection_exists(&upsert.collection).await? {
                    if needs_implicit {
                        upsert.embedding = Some(EmbeddingSpec::Hybrid {
                            dense_model: None,
                            dense_vector: Some(crate::executor::DENSE_VECTOR_NAME.to_string()),
                            dense_field: None,
                            sparse_model: None,
                            sparse_vector: Some(crate::executor::SPARSE_VECTOR_NAME.to_string()),
                            sparse_field: None,
                        });
                    }
                    return Ok(None);
                }
                self.get_cached_collection_info(&upsert.collection).await?
            }
        };
        // One schema pass for every name list below (infer + explicit
        // resolution previously scanned the schema three times).
        let targets = schema_targets(&info);
        if needs_implicit {
            // Single-vector dense slots exclude multivector names for auto-embed.
            let dense: Vec<String> = targets
                .dense
                .iter()
                .filter(|name| !targets.multi.contains(name))
                .cloned()
                .collect();
            upsert.embedding = Some(infer_embedding_spec(
                &upsert.collection,
                &dense,
                &targets.sparse,
                &targets.multi,
            )?);
        }
        resolve_explicit_embedding_targets(upsert, &targets)?;
        Ok(Some(info))
    }

    pub(crate) fn validate_embedded_upsert(
        &self,
        upsert: &UpsertStmt,
        info: &CollectionInfo,
    ) -> Result<(), QqlError> {
        let dense_specs = &info.schema.vectors;
        // Index once: the per-point loop below ran a linear `find` per
        // vector, i.e. O(points × vectors × specs). First-wins insertion
        // preserves `find` semantics for duplicate names.
        let mut by_name: HashMap<&str, &VectorSpec> = HashMap::new();
        for spec in dense_specs {
            if let Some(name) = spec.name.as_deref() {
                by_name.entry(name).or_insert(spec);
            }
        }
        let default_spec = dense_specs.iter().find(|spec| spec.name.is_none());
        // The historical `unwrap_or("")` comparison also matched an
        // explicitly `""`-named vector against the unnamed spec.
        if let Some(spec) = default_spec {
            by_name.entry("").or_insert(spec);
        }
        for point in &upsert.points {
            let PointEntry::Inline(point) = point else {
                continue;
            };
            let Some(vectors) = &point.vectors else {
                continue;
            };
            match vectors {
                PointVectors::Unnamed(value) => {
                    if let Some(spec) = default_spec {
                        validate_vector_value(&upsert.collection, "<default>", value, spec)?;
                    }
                }
                PointVectors::Named(values) => {
                    for (name, value) in values {
                        if let VectorValue::Dense(_) | VectorValue::MultiDense(_) = value
                            && let Some(spec) = by_name.get(name.as_str())
                        {
                            validate_vector_value(&upsert.collection, name, value, spec)?;
                        }
                    }
                }
                PointVectors::Param(..) | PointVectors::PositionalParam(..) => {}
            }
        }
        Ok(())
    }

    pub(crate) async fn ensure_collection_for_upsert(
        &self,
        collection: &str,
        model: Option<&str>,
        requested_dense: bool,
        requested_sparse: bool,
        explicit_dense: Option<&str>,
        explicit_sparse: Option<&str>,
    ) -> Result<bool, QqlError> {
        let exists = self.client.collection_exists(collection).await?;
        if exists {
            return Ok(false);
        }

        use qql_plan::{PlannedOperation, types::CreateCollectionRequest};
        let mut req = CreateCollectionRequest::default();
        if requested_dense {
            let dense_size = self.resolve_dense_vector_size(model).await?;
            let dense_name = explicit_dense.unwrap_or(crate::executor::DENSE_VECTOR_NAME);
            let mut vectors = std::collections::BTreeMap::new();
            vectors.insert(
                dense_name.to_string(),
                qql_plan::DenseVectorParams {
                    size: dense_size as u64,
                    distance: qql_core::ast::VectorDistance::Cosine,
                    hnsw_config: None,
                    quantization_config: None,
                    on_disk: None,
                    memory: None,
                    datatype: None,
                    multivector_config: None,
                },
            );
            req.vectors = Some(qql_plan::DenseVectorsConfig::Named(vectors));
        }
        if requested_sparse {
            let sparse_name = explicit_sparse.unwrap_or(crate::executor::SPARSE_VECTOR_NAME);
            let mut sparse = std::collections::BTreeMap::new();
            sparse.insert(
                sparse_name.to_string(),
                qql_plan::SparseVectorParams {
                    index: None,
                    modifier: qql_plan::SparseModifier::Idf,
                },
            );
            req.sparse_vectors = Some(sparse);
        }

        let op = PlannedOperation::CreateCollection {
            collection: collection.to_string(),
            request: req,
        };
        self.client.execute_planned(&op).await?;
        Ok(true)
    }

    pub(crate) async fn resolve_dense_vector_size(
        &self,
        model: Option<&str>,
    ) -> Result<usize, QqlError> {
        if let Some(dimension) = self
            .embedder
            .as_deref()
            .and_then(crate::embedder::Embedder::dimension)
        {
            return Ok(dimension);
        }

        if self.uses_local_embeddings() {
            if let Some(ref cfg) = self.config
                && cfg.embedding_dimension > 0
            {
                return Ok(cfg.embedding_dimension);
            }
            return match self.config.as_ref() {
                #[cfg(feature = "rest")]
                Some(cfg)
                    if !cfg.embedding_endpoint.as_deref().unwrap_or("").is_empty()
                        && !cfg.embedding_model.as_deref().unwrap_or("").is_empty() =>
                {
                    let embedder = HttpEmbedder::new(
                        cfg.embedding_endpoint.clone().unwrap_or_default(),
                        cfg.embedding_api_key.clone().unwrap_or_default(),
                        cfg.embedding_model.clone().unwrap_or_default(),
                        1,
                    )?;
                    let dim = embedder.probe_dimension("probe").await?;
                    Ok(dim)
                }
                _ if model.is_none() => Ok(crate::executor::DENSE_VECTOR_SIZE as usize),
                _ => Err(QqlError::execution(
                    "QQL-EMBEDDING-DIM",
                    "embedding_dimension must be configured when creating collections with USING MODEL in local inference mode",
                    None,
                )),
            };
        }

        if let Some(ref cfg) = self.config
            && cfg.embedding_dimension > 0
        {
            return Ok(cfg.embedding_dimension);
        }

        if model.is_some_and(|m| !m.is_empty())
            && self
                .config
                .as_ref()
                .map(|c| c.embedding_dimension == 0)
                .unwrap_or(true)
        {
            return Err(QqlError::execution(
                "QQL-EMBEDDING-DIM",
                "embedding_dimension must be configured when creating collections with USING MODEL",
                None,
            ));
        }

        Ok(crate::executor::DENSE_VECTOR_SIZE as usize)
    }
}

/// Map unnamed point vectors onto the single dense target, if the collection
/// has exactly one (`dense_targets`).
///
/// Pure: no I/O. Shared by [`configure_upsert_embeddings`](Executor::configure_upsert_embeddings)
/// and the prepared point-splice fast path, so both resolve identically.
pub(crate) fn map_unnamed_to_single_dense(upsert: &mut UpsertStmt, info: &CollectionInfo) {
    let dense = dense_targets(info);
    if dense.len() == 1 && !dense[0].is_empty() {
        let dense_name = &dense[0];
        for point in &mut upsert.points {
            let PointEntry::Inline(inline) = point else {
                continue;
            };
            // Move (never clone) the vectors out: only `Unnamed` is
            // replaced, `Named` is restored untouched. A borrow-then-clone
            // here would deep-copy every dense vector once per point.
            match inline.vectors.take() {
                Some(PointVectors::Unnamed(vv)) => {
                    inline.vectors = Some(PointVectors::Named(vec![(dense_name.clone(), vv)]));
                }
                taken => inline.vectors = taken,
            }
        }
    }
}

fn dense_targets(info: &CollectionInfo) -> Vec<String> {
    schema_targets(info).dense
}

/// Named-vector inventory of a collection schema in a single pass.
///
/// `dense` keeps the historical fallback semantics (legacy `dense_vectors`,
/// then `[""]` for schemaless single-vector collections); `sparse`/`multi`
/// are straight name lists. One pass replaces the three separate scans
/// (`dense_targets` + `multivector_targets` + sparse names) the configure
/// path used to perform.
struct SchemaTargets {
    dense: Vec<String>,
    sparse: Vec<String>,
    multi: Vec<String>,
}

fn schema_targets(info: &CollectionInfo) -> SchemaTargets {
    let mut dense = if info.schema.vectors.is_empty() {
        info.schema.dense_vectors.clone()
    } else {
        Vec::with_capacity(info.schema.vectors.len())
    };
    let mut multi = Vec::new();
    for vector in &info.schema.vectors {
        let name = vector.name.clone().unwrap_or_default();
        if vector.multivector.is_some() {
            multi.push(name.clone());
        }
        dense.push(name);
    }
    if dense.is_empty() && info.schema.sparse_vectors.is_empty() {
        dense.push(String::new());
    }
    SchemaTargets {
        dense,
        sparse: info
            .schema
            .sparse_vectors
            .iter()
            .map(|vector| vector.name.clone())
            .collect(),
        multi,
    }
}

/// Infer UPSERT embedding targets from collection topology, including multivector slots.
fn infer_embedding_spec(
    collection: &str,
    dense: &[String],
    sparse: &[String],
    multi: &[String],
) -> Result<EmbeddingSpec, QqlError> {
    let mut parts: Vec<EmbeddingSpec> = Vec::new();
    match (dense, sparse) {
        ([d], []) => parts.push(EmbeddingSpec::Dense {
            model: None,
            vector: Some(d.clone()),
            field: None,
        }),
        ([], [s]) => parts.push(EmbeddingSpec::Sparse {
            model: None,
            vector: Some(s.clone()),
            field: None,
        }),
        ([d], [s]) => parts.push(EmbeddingSpec::Hybrid {
            dense_model: None,
            dense_vector: Some(d.clone()),
            dense_field: None,
            sparse_model: None,
            sparse_vector: Some(s.clone()),
            sparse_field: None,
        }),
        ([], []) if !multi.is_empty() => {}
        _ => {
            return Err(QqlError::execution(
                "QQL-EMBEDDING-TOPOLOGY",
                format!(
                    "cannot infer text embedding targets for collection '{collection}': {} dense and {} sparse vectors. Add USING DENSE/SPARSE/HYBRID/MULTI or explicit EMBED directives",
                    dense.len(),
                    sparse.len()
                ),
                None,
            ));
        }
    }
    for m in multi {
        parts.push(EmbeddingSpec::MultiVector {
            model: None,
            vector: Some(m.clone()),
            field: None,
        });
    }
    if parts.is_empty() {
        return Err(QqlError::execution(
            "QQL-EMBEDDING-TOPOLOGY",
            format!(
                "cannot infer text embedding targets for collection '{collection}': no dense, sparse, or multivector slots"
            ),
            None,
        ));
    }
    if parts.len() == 1 {
        Ok(parts.remove(0))
    } else {
        Ok(EmbeddingSpec::Multi(parts))
    }
}

fn resolve_explicit_embedding_targets(
    upsert: &mut UpsertStmt,
    targets: &SchemaTargets,
) -> Result<(), QqlError> {
    let dense: &[String] = &targets.dense;
    let sparse: &[String] = &targets.sparse;
    // Multi targets are still dense names on the schema; allow MULTI VECTOR
    // to resolve against dense when the multivector list is empty (offline).
    let multi: &[String] = if targets.multi.is_empty() {
        dense
    } else {
        &targets.multi
    };

    if let Some(spec) = &mut upsert.embedding {
        fn resolve_spec(
            collection: &str,
            spec: &mut EmbeddingSpec,
            dense: &[String],
            sparse: &[String],
            multi: &[String],
        ) -> Result<(), QqlError> {
            match spec {
                EmbeddingSpec::Dense { vector, .. } => {
                    resolve_embedding_target(collection, vector, dense, "dense")?;
                }
                EmbeddingSpec::Sparse { vector, .. } => {
                    resolve_embedding_target(collection, vector, sparse, "sparse")?;
                }
                EmbeddingSpec::Hybrid {
                    dense_vector,
                    sparse_vector,
                    ..
                } => {
                    resolve_embedding_target(collection, dense_vector, dense, "dense")?;
                    resolve_embedding_target(collection, sparse_vector, sparse, "sparse")?;
                }
                EmbeddingSpec::MultiVector { vector, .. } => {
                    resolve_embedding_target(collection, vector, multi, "multivector")?;
                }
                EmbeddingSpec::Image { vector, .. } => {
                    // Image embeds into a dense named vector (CLIP space).
                    resolve_embedding_target(collection, vector, dense, "dense")?;
                }
                EmbeddingSpec::Multi(specs) => {
                    for s in specs {
                        resolve_spec(collection, s, dense, sparse, multi)?;
                    }
                }
            }
            Ok(())
        }
        resolve_spec(&upsert.collection, spec, dense, sparse, multi)?;
    }

    for directive in &upsert.embed {
        let (available, kind) = match directive.kind {
            qql_core::ast::EmbedKind::Dense { .. } | qql_core::ast::EmbedKind::Image { .. } => {
                (dense, "dense")
            }
            qql_core::ast::EmbedKind::Sparse { .. } => (sparse, "sparse"),
            qql_core::ast::EmbedKind::Multi { .. } => (multi, "multivector"),
        };
        if !available
            .iter()
            .any(|candidate| candidate == &directive.target_vector)
        {
            return Err(embedding_target_error(
                &upsert.collection,
                &directive.target_vector,
                available,
                kind,
            ));
        }
    }
    Ok(())
}

fn resolve_embedding_target(
    collection: &str,
    target: &mut Option<String>,
    available: &[String],
    kind: &str,
) -> Result<(), QqlError> {
    if let Some(name) = target {
        if available.iter().any(|candidate| candidate == name) {
            return Ok(());
        }
        return Err(embedding_target_error(collection, name, available, kind));
    }
    if available.len() != 1 {
        return Err(QqlError::execution(
            "QQL-EMBEDDING-TOPOLOGY",
            format!(
                "cannot infer a {kind} embedding target for collection '{collection}': available {kind} vectors are {}",
                display_vector_names(available)
            ),
            None,
        ));
    }
    *target = Some(available[0].clone());
    Ok(())
}

fn embedding_target_error(
    collection: &str,
    target: &str,
    available: &[String],
    kind: &str,
) -> QqlError {
    QqlError::execution(
        "QQL-EMBEDDING-TARGET",
        format!(
            "{kind} vector '{target}' does not exist in collection '{collection}'. Available {kind} vectors: {}",
            display_vector_names(available)
        ),
        None,
    )
}

fn display_vector_names(names: &[String]) -> String {
    if names.is_empty() {
        return "<none>".to_string();
    }
    names
        .iter()
        .map(|name| {
            if name.is_empty() {
                "<default>"
            } else {
                name.as_str()
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn validate_vector_value(
    collection: &str,
    name: &str,
    value: &VectorValue,
    spec: &VectorSpec,
) -> Result<(), QqlError> {
    let dimensions = match value {
        VectorValue::Dense(vector) => Some(vector.len()),
        VectorValue::MultiDense(rows) => rows.first().map(Vec::len),
        VectorValue::Sparse { .. }
        | VectorValue::Document { .. }
        | VectorValue::Image { .. }
        | VectorValue::Object { .. }
        | VectorValue::Param(..)
        | VectorValue::PositionalParam(..) => None,
    };
    if let Some(got) = dimensions
        && got != spec.size as usize
    {
        return Err(QqlError::execution(
            "QQL-EMBEDDING-DIM",
            format!(
                "embedding dimension mismatch for collection '{collection}' vector '{name}': model produced {got}, collection expects {}",
                spec.size
            ),
            None,
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{CollectionInfo, CollectionSchema, VectorSpec};
    use qql_core::ast::{PointEntry, PointVectors};

    fn info_with_dense(names: &[&str]) -> CollectionInfo {
        CollectionInfo {
            schema: CollectionSchema {
                vectors: names
                    .iter()
                    .map(|n| VectorSpec {
                        name: (!n.is_empty()).then(|| n.to_string()),
                        size: 3,
                        distance: "Cosine".into(),
                        hnsw: None,
                        quantization: None,
                        multivector: None,
                        on_disk: None,
                        datatype: None,
                        memory: None,
                    })
                    .collect(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn parse_upsert(sql: &str) -> qql_core::ast::UpsertStmt {
        match qql_core::parser::Parser::parse(sql).unwrap() {
            qql_core::ast::Stmt::Upsert(u) => *u,
            other => panic!("expected upsert, got {other:?}"),
        }
    }

    /// Regression: the unnamed→named mapping must not strip named-vector
    /// points. A blind `vectors.take()` dropped them (the point-splice fast
    /// path then sent points with no vectors and the backend rejected with
    /// "Expected some vectors").
    #[test]
    fn map_unnamed_preserves_named_vectors() {
        let mut upsert = parse_upsert(
            "UPSERT INTO c VALUES {id: 1, vector: [0.1, 0.2, 0.3]}, \
             {id: 2, vector: {dense: [0.1, 0.2, 0.3], bm25: {indices: [1], values: [0.5]}}}",
        );
        let info = info_with_dense(&["dense"]);
        map_unnamed_to_single_dense(&mut upsert, &info);

        let [PointEntry::Inline(first), PointEntry::Inline(second)] = &upsert.points[..] else {
            panic!("expected two inline points");
        };
        let Some(PointVectors::Named(first)) = &first.vectors else {
            panic!("unnamed point must map to the dense target");
        };
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].0, "dense");

        let Some(PointVectors::Named(second)) = &second.vectors else {
            panic!("named-vector point must keep its vectors (take() regression)");
        };
        assert_eq!(
            second.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
            vec!["dense", "bm25"]
        );
    }

    /// Single unnamed-dense topology: named sparse entries survive alongside
    /// the mapped dense target.
    #[test]
    fn map_unnamed_keeps_named_dense_with_sparse() {
        let mut upsert = parse_upsert(
            "UPSERT INTO c VALUES {id: 1, vector: {dense: [0.1, 0.2, 0.3], bm25: {indices: [1], values: [0.5]}}}",
        );
        let info = info_with_dense(&["dense"]);
        map_unnamed_to_single_dense(&mut upsert, &info);
        let Some(PointEntry::Inline(point)) = upsert.points.first() else {
            panic!("expected one inline point");
        };
        let Some(PointVectors::Named(entries)) = &point.vectors else {
            panic!("named vectors must survive the mapping");
        };
        assert_eq!(entries.len(), 2);
    }
}
