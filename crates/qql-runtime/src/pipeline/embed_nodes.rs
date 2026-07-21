use async_trait::async_trait;
use qql_core::error::QqlError;
use std::vec::Vec;

use super::{ExecutionNode, PrefetchQuery, QueryState, QueryVariant, VectorInput};

pub struct DenseEmbedNode {
    pub model: String,
    pub vector_name: String,
    pub limit: u64,
    pub as_prefetch: bool,
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl ExecutionNode for DenseEmbedNode {
    async fn execute(&self, state: &mut QueryState) -> Result<(), QqlError> {
        let query: QueryVariant;
        let mmr_nearest: Option<VectorInput>;

        if state.local_embed {
            let embedder = state.embedder.as_ref().ok_or_else(|| {
                QqlError::execution("QQL-EXECUTION", "local embedding requested but no Embedder provided", None)
            })?;
            let dense_vector = embedder
                .embed_dense(&state.query_text, &self.model)
                .await
                .map_err(|e| {
                    QqlError::execution("QQL-EXECUTION", format!("failed to embed dense search query: {}", e), None)
                })?;
            query = QueryVariant::Nearest(dense_vector.clone());
            mmr_nearest = if state.has_mmr {
                Some(VectorInput::Dense(dense_vector))
            } else {
                None
            };
        } else {
            let doc = QueryVariant::Document {
                text: state.query_text.clone(),
                model: self.model.clone(),
                options: state.get_doc_options(),
            };
            query = doc.clone();
            mmr_nearest = if state.has_mmr {
                Some(match &doc {
                    QueryVariant::Document {
                        text,
                        model,
                        options,
                    } => VectorInput::Document {
                        text: text.clone(),
                        model: model.clone(),
                        options: options.clone(),
                    },
                    _ => unreachable!(),
                })
            } else {
                None
            };
        }

        let final_query = if state.has_mmr {
            if let Some(input) = mmr_nearest {
                QueryVariant::MMR {
                    input: Box::new(QueryVariant::Nearest(match &input {
                        VectorInput::Dense(v) => v.clone(),
                        _ => return Err(QqlError::execution("QQL-EXECUTION", "MMR requires dense vector input", None)),
                    })),
                    diversity: state.mmr_diversity,
                    candidates: state.mmr_candidates,
                }
            } else {
                query
            }
        } else {
            query
        };

        if self.as_prefetch {
            state.prefetches.push(PrefetchQuery {
                prefetches: Vec::new(),
                query: Some(final_query),
                using: Some(self.vector_name.clone()),
                limit: Some(self.limit),
                params: state.params.clone(),
                filter: None,
                score_threshold: None,
                lookup_from: None,
            });
        } else {
            state.target_query = Some(final_query);
        }

        Ok(())
    }
}

pub struct RawVectorNode {
    pub vector: Vec<f64>,
    pub vector_name: String,
    pub as_prefetch: bool,
    pub limit: u64,
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl ExecutionNode for RawVectorNode {
    async fn execute(&self, _state: &mut QueryState) -> Result<(), QqlError> {
        let raw: Vec<f32> = self.vector.iter().map(|v| *v as f32).collect();
        let query = QueryVariant::Nearest(raw);

        if self.as_prefetch {
            _state.prefetches.push(PrefetchQuery {
                prefetches: Vec::new(),
                query: Some(query),
                using: Some(self.vector_name.clone()),
                limit: Some(self.limit),
                params: _state.params.clone(),
                filter: None,
                score_threshold: None,
                lookup_from: None,
            });
        } else {
            _state.target_query = Some(query);
            if !self.vector_name.is_empty() {
                _state.vector_name = self.vector_name.clone();
            }
        }

        Ok(())
    }
}

pub struct SparseEmbedNode {
    pub model: String,
    pub vector_name: String,
    pub limit: u64,
    pub as_prefetch: bool,
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl ExecutionNode for SparseEmbedNode {
    async fn execute(&self, state: &mut QueryState) -> Result<(), QqlError> {
        if state.has_mmr && !self.as_prefetch {
            return Err(QqlError::execution("QQL-EXECUTION", 
                "MMR is supported only for standard NEAREST queries, not sparse-only queries", None));
        }

        let query: QueryVariant;

        if state.local_embed {
            let embedder = state.embedder.as_ref().ok_or_else(|| {
                QqlError::execution("QQL-EXECUTION", "local embedding requested but no Embedder provided", None)
            })?;
            let sv = embedder
                .embed_sparse(&state.query_text)
                .await
                .map_err(|e| {
                    QqlError::execution("QQL-EXECUTION", format!("failed to embed sparse search query: {}", e), None)
                })?;
            query = QueryVariant::Sparse(sv.indices, sv.values);
        } else {
            query = QueryVariant::Document {
                text: state.query_text.clone(),
                model: self.model.clone(),
                options: state.get_doc_options(),
            };
        }

        if self.as_prefetch {
            state.prefetches.push(PrefetchQuery {
                prefetches: Vec::new(),
                query: Some(query),
                using: Some(self.vector_name.clone()),
                limit: Some(self.limit),
                params: state.params.clone(),
                filter: None,
                score_threshold: None,
                lookup_from: None,
            });
        } else {
            state.target_query = Some(query);
        }

        Ok(())
    }
}
