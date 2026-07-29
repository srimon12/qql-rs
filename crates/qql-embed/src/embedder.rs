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
/// Multivector (ColBERT-style) uses [`embed_multi`] → `Vec<Vec<f32>>`.
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait Embedder: EmbedderBound {
    async fn embed_dense(&self, text: &str, model: &str) -> Result<Vec<f32>, QqlError>;
    async fn embed_sparse(&self, text: &str) -> Result<SparseVector, QqlError>;

    /// Dense output dimension when it is known without running inference.
    /// Custom and remote embedders may return `None`.
    fn dimension(&self) -> Option<usize> {
        None
    }

    /// Multivector (ColBERT) per-token dimension when known without inference.
    fn multi_dimension(&self) -> Option<usize> {
        None
    }

    /// Whether this embedder can satisfy a requested model identifier.
    /// Dynamic providers may return `true` for every model.
    fn accepts_model(&self, _model: &str) -> bool {
        true
    }

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

    /// Multivector embedding (ColBERT-style late interaction).
    ///
    /// Returns one dense vector per token/segment. Default rejects so hosts
    /// that only support single-vector dense must opt in explicitly.
    async fn embed_multi(&self, text: &str, model: &str) -> Result<Vec<Vec<f32>>, QqlError> {
        let _ = text;
        Err(multi_unsupported_error(model))
    }

    /// Batch multivector embedding. Default loops [`embed_multi`].
    async fn embed_multi_batch(
        &self,
        texts: &[String],
        model: &str,
    ) -> Result<Vec<Vec<Vec<f32>>>, QqlError> {
        let mut results = Vec::with_capacity(texts.len());
        for text in texts {
            results.push(self.embed_multi(text, model).await?);
        }
        Ok(results)
    }
}

/// Error when multi-vector embedding is requested but the host has no multi path.
pub fn multi_unsupported_error(model: &str) -> QqlError {
    let model_note = if model.is_empty() || model.eq_ignore_ascii_case("default") {
        "no model specified".to_string()
    } else {
        format!("model='{model}'")
    };
    QqlError::execution(
        "QQL-EMBEDDING-MULTI",
        format!(
            "multi-vector embedding is not available ({model_note}). \
             Configure a multi embedder (multi_embedding_endpoint / multi_embedding_model, \
             or edge multi_model for offline BGE-M3), pass precomputed VECTOR [[...], ...], \
             or use UPSERT with explicit multivector bags."
        ),
        None,
    )
}

/// Local sparse-only helper (no dense model).
pub struct SparseEmbedder;

impl SparseEmbedder {
    pub fn embed_sparse(text: &str) -> SparseVector {
        sparse::build_query_default(text)
    }
}
