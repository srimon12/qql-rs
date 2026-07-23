use async_trait::async_trait;
use qql_core::error::QqlError;

use crate::sparse::{self, SparseVector};

#[cfg(not(target_arch = "wasm32"))]
pub trait EmbedderBound: Send + Sync {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Send + Sync> EmbedderBound for T {}

#[cfg(target_arch = "wasm32")]
pub trait EmbedderBound {}
#[cfg(target_arch = "wasm32")]
impl<T> EmbedderBound for T {}

/// Host-agnostic embedding backend.
///
/// Dense calls should batch when possible (`embed_dense_batch` → one HTTP
/// request or one ONNX batch). Sparse defaults to local BM25-style hashing.
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait Embedder: EmbedderBound {
    async fn embed_dense(&self, text: &str, model: &str) -> Result<Vec<f32>, QqlError>;
    async fn embed_sparse(&self, text: &str) -> Result<SparseVector, QqlError>;

    /// Embed many texts in one shot. Default loops `embed_dense`; override for
    /// real batching (OpenAI-compatible `input: [...]`, fastembed batch, etc.).
    async fn embed_dense_batch(
        &self,
        texts: &[String],
        model: &str,
    ) -> Result<Vec<Vec<f32>>, QqlError> {
        let mut results = Vec::with_capacity(texts.len());
        for text in texts {
            results.push(self.embed_dense(text, model).await?);
        }
        Ok(results)
    }
}

/// Local sparse-only helper (no dense model).
pub struct SparseEmbedder;

impl SparseEmbedder {
    pub fn embed_sparse(text: &str) -> SparseVector {
        sparse::build_query_default(text)
    }
}
