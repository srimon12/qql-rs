use crate::executor::{
    DENSE_VECTOR_NAME, Executor, RERANK_VECTOR_NAME, RERANK_VECTOR_SIZE, SPARSE_VECTOR_NAME,
};
use qql_core::ast;
use qql_core::error::QqlError;

impl Executor {
    /// Prepare a CREATE COLLECTION statement with inferred vectors and dimensions.
    pub(crate) async fn prepare_create_collection(
        &self,
        create: &mut ast::CreateCollectionStmt,
    ) -> Result<(), QqlError> {
        if let ast::CollectionMode::Dense { model: Some(model) } = &create.mode
            && let Some(embedder) = self.embedder.as_deref()
            && !embedder.accepts_model(model)
        {
            return Err(QqlError::execution(
                "QQL-EMBEDDING-MODEL",
                format!("embedding model '{model}' is not available from the configured embedder"),
                None,
            ));
        }
        if !create.vectors.is_empty() {
            if let ast::CollectionMode::Dense { model: Some(model) } = &create.mode {
                let expected = self.resolve_dense_vector_size(Some(model)).await? as u64;
                if create.vectors.len() == 1 && create.vectors[0].size != expected {
                    return Err(QqlError::execution(
                        "QQL-EMBEDDING-DIM",
                        format!(
                            "collection vector dimension {} does not match embedding model '{model}' dimension {expected}",
                            create.vectors[0].size
                        ),
                        None,
                    ));
                }
            }
            return Ok(());
        }
        if !create.sparse_vectors.is_empty()
            && matches!(create.mode, ast::CollectionMode::Dense { model: None })
        {
            // An explicit sparse definition is a valid sparse-only collection;
            // do not silently add the default dense vector to it.
            return Ok(());
        }

        let (model, dense_name, sparse_name, with_colbert) = match &create.mode {
            ast::CollectionMode::Dense { model } => {
                (model.as_deref(), DENSE_VECTOR_NAME, None, false)
            }
            ast::CollectionMode::Hybrid {
                dense_vector,
                sparse_vector,
            } => (
                None,
                dense_vector.as_deref().unwrap_or(DENSE_VECTOR_NAME),
                Some(sparse_vector.as_deref().unwrap_or(SPARSE_VECTOR_NAME)),
                false,
            ),
            // Conventional dense + sparse + ColBERT multivector topology.
            ast::CollectionMode::Rerank => {
                (None, DENSE_VECTOR_NAME, Some(SPARSE_VECTOR_NAME), true)
            }
        };
        let dense_size = self.resolve_dense_vector_size(model).await? as u64;
        create.vectors.push(ast::VectorDef {
            name: dense_name.to_string(),
            size: dense_size,
            distance: ast::VectorDistance::Cosine,
            hnsw: None,
            quantization: None,
            multivector: None,
            vectors: None,
        });
        if let Some(sparse_name) = sparse_name {
            create.sparse_vectors.push(ast::SparseVectorDef {
                name: sparse_name.to_string(),
                index: None,
                modifier: None,
            });
        }
        if with_colbert {
            let multi_size = self
                .embedder
                .as_deref()
                .and_then(crate::embedder::Embedder::multi_dimension)
                .or_else(|| {
                    self.config.as_ref().and_then(|c| {
                        (c.multi_embedding_dimension > 0).then_some(c.multi_embedding_dimension)
                    })
                })
                .unwrap_or(RERANK_VECTOR_SIZE as usize) as u64;
            create.vectors.push(ast::VectorDef {
                name: RERANK_VECTOR_NAME.to_string(),
                size: multi_size,
                distance: ast::VectorDistance::Cosine,
                hnsw: None,
                quantization: None,
                multivector: Some(ast::MultivectorConfig {
                    comparator: ast::MultivectorComparator::MaxSim,
                }),
                vectors: None,
            });
        }

        Ok(())
    }
}
