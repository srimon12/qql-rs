use crate::client::{CreateCollectionReq, VectorTopology};
#[cfg(feature = "rest")]
use crate::embedder::HttpEmbedder;
use crate::executor::Executor;
use qql_core::error::QqlError;

impl Executor {
    pub(crate) async fn ensure_collection_for_upsert(
        &self,
        collection: &str,
        model: Option<&str>,
        requested_hybrid: bool,
        explicit_dense: Option<&str>,
        explicit_sparse: Option<&str>,
    ) -> Result<bool, QqlError> {
        let exists = self.client.collection_exists(collection).await?;
        if exists {
            return Ok(false);
        }

        let dense_size = self.resolve_dense_vector_size(model).await?;
        let dense_name = explicit_dense.unwrap_or(crate::executor::DENSE_VECTOR_NAME);

        let mut create_req = CreateCollectionReq::new(collection.to_string());
        create_req.vectors_config = Some(serde_json::json!({
            dense_name: {
                "size": dense_size,
                "distance": "Cosine"
            }
        }));

        if requested_hybrid {
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

        for vname in &info.schema.sparse_vectors {
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
