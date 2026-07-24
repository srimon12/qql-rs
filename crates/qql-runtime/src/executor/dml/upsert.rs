use crate::backend::{CollectionInfo, VectorSpec};
use crate::client::{CreateCollectionReq, VectorTopology};
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

        if !needs_implicit {
            return Ok(None);
        }
        if !self.client.collection_exists(&upsert.collection).await? {
            if needs_implicit {
                upsert.embedding = Some(EmbeddingSpec::Hybrid {
                    dense_model: None,
                    dense_vector: Some(crate::executor::DENSE_VECTOR_NAME.to_string()),
                    sparse_model: None,
                    sparse_vector: Some(crate::executor::SPARSE_VECTOR_NAME.to_string()),
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
                },
                ([], [sparse]) => EmbeddingSpec::Sparse {
                    model: None,
                    vector: Some(sparse.clone()),
                },
                ([dense], [sparse]) => EmbeddingSpec::Hybrid {
                    dense_model: None,
                    dense_vector: Some(dense.clone()),
                    sparse_model: None,
                    sparse_vector: Some(sparse.clone()),
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

    #[allow(dead_code)]
    pub(crate) async fn resolve_vector_topology(
        &self,
        collection: &str,
    ) -> Result<VectorTopology, QqlError> {
        let info = self.client.get_collection_info(collection).await?;
        let mut topo = VectorTopology {
            dense_vector: None,
            sparse_vector: None,
            rerank_vector: None,
        };

        for vname in &info.schema.dense_vectors {
            if vname == crate::executor::DENSE_VECTOR_NAME {
                topo.dense_vector = Some(crate::executor::DENSE_VECTOR_NAME.to_string());
            } else if vname == crate::executor::RERANK_VECTOR_NAME {
                topo.rerank_vector = Some(crate::executor::RERANK_VECTOR_NAME.to_string());
            } else if topo.dense_vector.is_none()
                || topo
                    .dense_vector
                    .as_ref()
                    .is_some_and(|name| name.is_empty())
            {
                topo.dense_vector = Some(vname.clone());
            }
        }

        for sv in &info.schema.sparse_vectors {
            let vname = &sv.name;
            if vname == crate::executor::SPARSE_VECTOR_NAME {
                topo.sparse_vector = Some(crate::executor::SPARSE_VECTOR_NAME.to_string());
            } else if topo.sparse_vector.is_none()
                || topo
                    .sparse_vector
                    .as_ref()
                    .is_some_and(|name| name.is_empty())
            {
                topo.sparse_vector = Some(vname.clone());
            }
        }

        Ok(topo)
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
    if info.schema.vectors.is_empty() {
        info.schema.dense_vectors.clone()
    } else {
        info.schema
            .vectors
            .iter()
            .map(|vector| vector.name.clone().unwrap_or_default())
            .collect()
    }
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
