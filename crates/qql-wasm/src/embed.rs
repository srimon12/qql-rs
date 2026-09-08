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
    async fn embed_dense(&self, text: &str, _model: &str) -> Result<Vec<f32>, QqlError> {
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
        Ok(batch.into_iter().next().unwrap_or_default())
    }

    async fn embed_dense_batch(
        &self,
        texts: &[String],
        _model: &str,
    ) -> Result<Vec<Vec<f32>>, QqlError> {
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

    async fn embed_sparse_document(
        &self,
        text: &str,
        model: &str,
    ) -> Result<SparseVector, QqlError> {
        if !model.is_empty() && !model.eq_ignore_ascii_case("default") {
            return Err(qql_embed::sparse_model_unsupported_error(model));
        }
        Ok(qql_embed::sparse::embed_document(text))
    }
}
