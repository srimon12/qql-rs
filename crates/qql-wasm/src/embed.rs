//! `qql-embed::Embedder` adapter so shared embedding resolution runs in-browser.

use async_trait::async_trait;
use qql_core::error::QqlError;
use qql_embed::{Embedder, SparseVector};

use super::client::Client;

// ── WASM dense embed collect/apply (mirrors runtime batching) ─────

#[cfg(all(feature = "client", target_arch = "wasm32"))]
// ── qql-embed::Embedder adapter (shared resolve path) ─────────────
#[cfg(all(feature = "client", target_arch = "wasm32"))]
#[async_trait(?Send)]
impl Embedder for Client {
    async fn embed_dense(&self, text: &str, model: &str) -> Result<Vec<f32>, QqlError> {
        if !model.is_empty() && !model.eq_ignore_ascii_case("default") {
            return Err(qql_embed::dense_model_unsupported_error(model));
        }
        let batch = self
            .embed_texts(vec![text.to_string()])
            .await
            .map_err(|e| {
                QqlError::execution(
                    "QQL-EMBEDDING",
                    e.as_string().unwrap_or_else(|| "embed failed".into()),
                    None,
                )
            })?;
        batch.into_iter().next().ok_or_else(|| {
            QqlError::execution("QQL-EMBEDDING", "dense embedding response was empty", None)
        })
    }

    async fn embed_dense_batch(
        &self,
        texts: &[String],
        model: &str,
    ) -> Result<Vec<Vec<f32>>, QqlError> {
        if !model.is_empty() && !model.eq_ignore_ascii_case("default") {
            return Err(qql_embed::dense_model_unsupported_error(model));
        }
        self.embed_texts(texts.to_vec()).await.map_err(|e| {
            QqlError::execution(
                "QQL-EMBEDDING",
                e.as_string().unwrap_or_else(|| "embed batch failed".into()),
                None,
            )
        })
    }

    async fn embed_sparse_query(&self, text: &str, model: &str) -> Result<SparseVector, QqlError> {
        if !model.is_empty() && !model.eq_ignore_ascii_case("default") {
            return Err(qql_embed::sparse_model_unsupported_error(model));
        }
        Ok(qql_embed::sparse::embed_query(text))
    }

    fn bm25_params(&self) -> qql_embed::Bm25Params {
        self.bm25
    }

    async fn embed_sparse_document(
        &self,
        text: &str,
        model: &str,
    ) -> Result<SparseVector, QqlError> {
        if !model.is_empty() && !model.eq_ignore_ascii_case("default") {
            return Err(qql_embed::sparse_model_unsupported_error(model));
        }
        Ok(qql_embed::sparse::embed_document_with_params(
            text,
            self.bm25_params(),
        ))
    }

    async fn embed_multi(&self, text: &str, model: &str) -> Result<Vec<Vec<f32>>, QqlError> {
        let bags = self
            .embed_multi_texts(vec![text.to_string()], model)
            .await
            .map_err(|e| {
                let msg = e.as_string().unwrap_or_else(|| "multi embed failed".into());
                if msg.contains("not available") {
                    qql_embed::multi_unsupported_error(model)
                } else {
                    QqlError::execution("QQL-EMBEDDING-MULTI", msg, None)
                }
            })?;
        bags.into_iter().next().ok_or_else(|| {
            QqlError::execution(
                "QQL-EMBEDDING-MULTI",
                "multi embedding response was empty",
                None,
            )
        })
    }

    async fn embed_multi_batch(
        &self,
        texts: &[String],
        model: &str,
    ) -> Result<Vec<Vec<Vec<f32>>>, QqlError> {
        self.embed_multi_texts(texts.to_vec(), model)
            .await
            .map_err(|e| {
                let msg = e.as_string().unwrap_or_else(|| "multi embed failed".into());
                if msg.contains("not available") {
                    qql_embed::multi_unsupported_error(model)
                } else {
                    QqlError::execution("QQL-EMBEDDING-MULTI", msg, None)
                }
            })
    }

    async fn embed_image(&self, source: &str, model: &str) -> Result<Vec<f32>, QqlError> {
        let batch = self
            .embed_image_sources(vec![source.to_string()], model)
            .await
            .map_err(|e| {
                let msg = e.as_string().unwrap_or_else(|| "image embed failed".into());
                if msg.contains("not available") {
                    qql_embed::image_unsupported_error(model)
                } else {
                    QqlError::execution("QQL-EMBEDDING-IMAGE", msg, None)
                }
            })?;
        batch.into_iter().next().ok_or_else(|| {
            QqlError::execution(
                "QQL-EMBEDDING-IMAGE",
                "image embedding response was empty",
                None,
            )
        })
    }

    async fn embed_image_batch(
        &self,
        sources: &[String],
        model: &str,
    ) -> Result<Vec<Vec<f32>>, QqlError> {
        self.embed_image_sources(sources.to_vec(), model)
            .await
            .map_err(|e| {
                let msg = e.as_string().unwrap_or_else(|| "image embed failed".into());
                if msg.contains("not available") {
                    qql_embed::image_unsupported_error(model)
                } else {
                    QqlError::execution("QQL-EMBEDDING-IMAGE", msg, None)
                }
            })
    }

    async fn rerank_pairs(
        &self,
        query: &str,
        documents: &[String],
        model: &str,
    ) -> Result<Vec<f32>, QqlError> {
        self.rerank_pair_scores(query, documents, model)
            .await
            .map_err(|e| {
                let msg = e.as_string().unwrap_or_else(|| "rerank failed".into());
                if msg.contains("not available") {
                    qql_embed::cross_rerank_unsupported_error(model)
                } else {
                    QqlError::execution("QQL-RERANK-CROSS", msg, None)
                }
            })
    }
}
