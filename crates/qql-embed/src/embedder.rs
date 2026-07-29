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

    /// Sparse embedding (BM25 or Splade / BGE-M3 sparse model).
    /// Default implementation uses local BM25 hashing.
    async fn embed_sparse(&self, text: &str, _model: &str) -> Result<SparseVector, QqlError> {
        Ok(sparse::build_query_default(text))
    }

    /// Batch sparse embedding. Default loops [`embed_sparse`].
    async fn embed_sparse_batch(
        &self,
        texts: &[String],
        model: &str,
    ) -> Result<Vec<SparseVector>, QqlError> {
        let mut results = Vec::with_capacity(texts.len());
        for text in texts {
            results.push(self.embed_sparse(text, model).await?);
        }
        Ok(results)
    }

    /// Single-pass joint multi-modal / BGE-M3 embedding (dense + sparse + multi-vectors).
    /// Default implementation delegates to separate dense, sparse, and multi calls.
    async fn embed_joint(&self, text: &str, model: &str) -> Result<JointEmbeddingOutput, QqlError> {
        let dense = self.embed_dense(text, model).await.ok();
        let sparse = self.embed_sparse(text, model).await.ok();
        let multi = self.embed_multi(text, model).await.ok();
        Ok(JointEmbeddingOutput {
            dense,
            sparse,
            multi,
        })
    }

    /// Batch joint embedding. Default loops [`embed_joint`].
    async fn embed_joint_batch(
        &self,
        texts: &[String],
        model: &str,
    ) -> Result<Vec<JointEmbeddingOutput>, QqlError> {
        let mut results = Vec::with_capacity(texts.len());
        for text in texts {
            results.push(self.embed_joint(text, model).await?);
        }
        Ok(results)
    }

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

    /// Image / CLIP vision embedding. `source` is a filesystem path or URL.
    ///
    /// Returns a single dense vector in the same space as the paired text
    /// encoder (e.g. CLIP). Default rejects until the host opts in.
    async fn embed_image(&self, source: &str, model: &str) -> Result<Vec<f32>, QqlError> {
        let _ = source;
        Err(image_unsupported_error(model))
    }

    /// Batch image embedding. Default loops [`embed_image`].
    async fn embed_image_batch(
        &self,
        sources: &[String],
        model: &str,
    ) -> Result<Vec<Vec<f32>>, QqlError> {
        let mut results = Vec::with_capacity(sources.len());
        for source in sources {
            results.push(self.embed_image(source, model).await?);
        }
        Ok(results)
    }

    /// Cross-encoder pair scores: `(query, documents[i]) → score`.
    ///
    /// Returns one score per document **in the same order** as `documents`
    /// (not sorted). Hosts that return ranked results must unpermute.
    /// Default rejects until the host opts in (edge `TextRerank`, HTTP rerank API).
    async fn rerank_pairs(
        &self,
        query: &str,
        documents: &[String],
        model: &str,
    ) -> Result<Vec<f32>, QqlError> {
        let _ = (query, documents);
        Err(cross_rerank_unsupported_error(model))
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

/// Error when image embedding is requested but the host has no image path.
pub fn image_unsupported_error(model: &str) -> QqlError {
    let model_note = if model.is_empty() || model.eq_ignore_ascii_case("default") {
        "no model specified".to_string()
    } else {
        format!("model='{model}'")
    };
    QqlError::execution(
        "QQL-EMBEDDING-IMAGE",
        format!(
            "image embedding is not available ({model_note}). \
             Configure an image/CLIP vision embedder (image_embedding_model / edge image_model, \
             or image_embedding_endpoint), pass a precomputed VECTOR [...], \
             or use UPSERT USING IMAGE ON FIELD <path_field>."
        ),
        None,
    )
}

/// Error when cross-encoder pair rerank is requested without a scorer host.
pub fn cross_rerank_unsupported_error(model: &str) -> QqlError {
    let model_note = if model.is_empty() || model.eq_ignore_ascii_case("default") {
        "no model specified".to_string()
    } else {
        format!("model='{model}'")
    };
    QqlError::execution(
        "QQL-RERANK-CROSS",
        format!(
            "cross-encoder pair scoring is not available ({model_note}). \
             Configure a rerank host (rerank_endpoint / rerank_model, or edge \
             reranker_model for offline TextRerank / bge-reranker)."
        ),
        None,
    )
}

/// Output container for single-pass joint multi-modal / BGE-M3 embedding.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct JointEmbeddingOutput {
    pub dense: Option<Vec<f32>>,
    pub sparse: Option<SparseVector>,
    pub multi: Option<Vec<Vec<f32>>>,
}

/// Local sparse-only helper (no dense model).
pub struct SparseEmbedder;

impl SparseEmbedder {
    /// Generate a local BM25-style sparse vector representation for `text`.
    pub fn embed_sparse(text: &str) -> SparseVector {
        sparse::build_query_default(text)
    }
}
