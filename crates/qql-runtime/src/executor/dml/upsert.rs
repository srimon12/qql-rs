use crate::backend::{CollectionInfo, VectorSpec};
use crate::client::CreateCollectionReq;
#[cfg(feature = "rest")]
use crate::embedder::HttpEmbedder;
use crate::executor::Executor;
use qql_core::ast::{EmbeddingSpec, PointVectors, UpsertStmt, Value, VectorValue};
use qql_core::error::QqlError;

impl Executor {
    /// Infer implicit text embedding from the existing collection topology.
    /// New collections retain the historical hybrid default, while existing
    /// dense-only and sparse-only collections receive only compatible vectors.
    pub(crate) async fn configure_upsert_embeddings(
        &self,
        upsert: &mut UpsertStmt,
    ) -> Result<Option<CollectionInfo>, QqlError> {
        let needs_implicit = upsert.embedding.is_none()
            && upsert.embed.is_empty()
            && upsert.points.iter().any(|point| {
                point.vectors.is_none()
                    && point.payload.iter().any(|(key, value)| {
                        matches!(value, Value::Str(text) if !text.is_empty())
                            && (key.eq_ignore_ascii_case("text")
                                || key.eq_ignore_ascii_case("body")
                                || key.eq_ignore_ascii_case("content"))
                    })
            });

        let has_unnamed_vectors = upsert
            .points
            .iter()
            .any(|p| matches!(p.vectors, Some(PointVectors::Unnamed(_))));
        let has_embedding = upsert.embedding.is_some() || !upsert.embed.is_empty();
        if !needs_implicit && !has_embedding {
            if has_unnamed_vectors
                && self
                    .client
                    .collection_exists(&upsert.collection)
                    .await
                    .unwrap_or(false)
            {
                if let Ok(info) = self.client.get_collection_info(&upsert.collection).await {
                    let dense = dense_targets(&info);
                    if dense.len() == 1 && !dense[0].is_empty() {
                        let dense_name = &dense[0];
                        for point in &mut upsert.points {
                            if let Some(PointVectors::Unnamed(vv)) = &point.vectors {
                                point.vectors = Some(PointVectors::Named(vec![(
                                    dense_name.clone(),
                                    vv.clone(),
                                )]));
                            }
                        }
                    }
                    return Ok(Some(info));
                }
            }
            return Ok(None);
        }
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

        let info = self.client.get_collection_info(&upsert.collection).await?;
        if needs_implicit {
            let dense = dense_targets(&info);
            let sparse: Vec<String> = info
                .schema
                .sparse_vectors
                .iter()
                .map(|vector| vector.name.clone())
                .collect();
            upsert.embedding = Some(match (dense.as_slice(), sparse.as_slice()) {
                ([dense], []) => EmbeddingSpec::Dense {
                    model: None,
                    vector: Some(dense.clone()),
                    field: None,
                },
                ([], [sparse]) => EmbeddingSpec::Sparse {
                    model: None,
                    vector: Some(sparse.clone()),
                    field: None,
                },
                ([dense], [sparse]) => EmbeddingSpec::Hybrid {
                    dense_model: None,
                    dense_vector: Some(dense.clone()),
                    dense_field: None,
                    sparse_model: None,
                    sparse_vector: Some(sparse.clone()),
                    sparse_field: None,
                },
                _ => {
                    return Err(QqlError::execution(
                        "QQL-EMBEDDING-TOPOLOGY",
                        format!(
                            "cannot infer text embedding targets for collection '{}': {} dense and {} sparse vectors. Add USING DENSE/SPARSE/HYBRID or explicit EMBED directives",
                            upsert.collection,
                            dense.len(),
                            sparse.len()
                        ),
                        None,
                    ));
                }
            });
        }
        resolve_explicit_embedding_targets(upsert, &info)?;
        Ok(Some(info))
    }

    pub(crate) fn validate_embedded_upsert(
        &self,
        upsert: &UpsertStmt,
        info: &CollectionInfo,
    ) -> Result<(), QqlError> {
        let dense_specs = &info.schema.vectors;
        for point in &upsert.points {
            let Some(vectors) = &point.vectors else {
                continue;
            };
            match vectors {
                PointVectors::Unnamed(value) => {
                    if let Some(spec) = dense_specs.iter().find(|spec| spec.name.is_none()) {
                        validate_vector_value(&upsert.collection, "<default>", value, spec)?;
                    }
                }
                PointVectors::Named(values) => {
                    for (name, value) in values {
                        if let VectorValue::Dense(_) | VectorValue::MultiDense(_) = value {
                            if let Some(spec) = dense_specs
                                .iter()
                                .find(|spec| spec.name.as_deref().unwrap_or("") == name)
                            {
                                validate_vector_value(&upsert.collection, name, value, spec)?;
                            }
                        }
                    }
                }
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

        let mut create_req = CreateCollectionReq::new(collection.to_string());
        if requested_dense {
            let dense_size = self.resolve_dense_vector_size(model).await?;
            let dense_name = explicit_dense.unwrap_or(crate::executor::DENSE_VECTOR_NAME);
            create_req.vectors_config = Some(serde_json::json!({
                dense_name: {
                    "size": dense_size,
                    "distance": "Cosine"
                }
            }));
        }

        if requested_sparse {
            let sparse_name = explicit_sparse.unwrap_or(crate::executor::SPARSE_VECTOR_NAME);
            create_req.sparse_vectors_config = Some(serde_json::json!({
                sparse_name: {
                    "modifier": "idf"
                }
            }));
        }

        self.client.create_collection(create_req).await?;
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
            if let Some(ref cfg) = self.config {
                if cfg.embedding_dimension > 0 {
                    return Ok(cfg.embedding_dimension);
                }
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

        if let Some(ref cfg) = self.config {
            if cfg.embedding_dimension > 0 {
                return Ok(cfg.embedding_dimension);
            }
        }

        if model.is_some()
            && model.unwrap() != ""
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

fn dense_targets(info: &CollectionInfo) -> Vec<String> {
    let targets = if info.schema.vectors.is_empty() {
        info.schema.dense_vectors.clone()
    } else {
        info.schema
            .vectors
            .iter()
            .map(|vector| vector.name.clone().unwrap_or_default())
            .collect()
    };
    if targets.is_empty() && info.schema.sparse_vectors.is_empty() {
        vec![String::new()]
    } else {
        targets
    }
}

fn resolve_explicit_embedding_targets(
    upsert: &mut UpsertStmt,
    info: &CollectionInfo,
) -> Result<(), QqlError> {
    let dense = dense_targets(info);
    let sparse = info
        .schema
        .sparse_vectors
        .iter()
        .map(|vector| vector.name.clone())
        .collect::<Vec<_>>();

    if let Some(spec) = &mut upsert.embedding {
        fn resolve_spec(
            collection: &str,
            spec: &mut EmbeddingSpec,
            dense: &[String],
            sparse: &[String],
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
                EmbeddingSpec::Multi(specs) => {
                    for s in specs {
                        resolve_spec(collection, s, dense, sparse)?;
                    }
                }
            }
            Ok(())
        }
        resolve_spec(&upsert.collection, spec, &dense, &sparse)?;
    }

    for directive in &upsert.embed {
        let (available, kind) = match directive.kind {
            qql_core::ast::EmbedKind::Dense { .. } => (&dense, "dense"),
            qql_core::ast::EmbedKind::Sparse { .. } => (&sparse, "sparse"),
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
        VectorValue::Sparse { .. } => None,
    };
    if let Some(got) = dimensions {
        if got != spec.size as usize {
            return Err(QqlError::execution(
                "QQL-EMBEDDING-DIM",
                format!(
                    "embedding dimension mismatch for collection '{collection}' vector '{name}': model produced {got}, collection expects {}",
                    spec.size
                ),
                None,
            ));
        }
    }
    Ok(())
}
