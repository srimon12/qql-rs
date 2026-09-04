use async_trait::async_trait;
use qql_core::error::QqlError;

use crate::sparse::{self, SparseVector};

#[cfg(not(target_arch = "wasm32"))]
/// Send/Sync bound helper for `Embedder` implementations on native targets.
pub trait EmbedderBound: Send + Sync {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Send + Sync> EmbedderBound for T {}

#[cfg(target_arch = "wasm32")]
/// Single-threaded bound helper for `Embedder` implementations on wasm32.
pub trait EmbedderBound {}
#[cfg(target_arch = "wasm32")]
impl<T> EmbedderBound for T {}

/// Host-agnostic embedding backend.
///
/// Dense calls should batch when possible (`embed_dense_batch` → one HTTP
/// request or one ONNX batch). Sparse is role-split: [`Self::embed_sparse_query`]
/// (unit weights) for search text and [`Self::embed_sparse_document`]
/// (BM25 tf saturation) for ingestion text, both defaulting to local
/// wire-compatible BM25. Multivector (ColBERT-style) uses
/// [`Self::embed_multi`] → `Vec<Vec<f32>>`.
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait Embedder: EmbedderBound {
    /// Embed one text into a dense vector; `model` may be empty or `"default"`.
    async fn embed_dense(&self, text: &str, model: &str) -> Result<Vec<f32>, QqlError>;

    /// Sparse embedding for **query** text: unique terms with unit weights,
    /// matching Qdrant's `qdrant/bm25` query embedding.
    ///
    /// Default implementation uses local wire-compatible BM25 when `model` is
    /// empty or `"default"`. Non-default sparse models are rejected — override
    /// this method to provide model-aware sparse inference.
    async fn embed_sparse_query(&self, text: &str, model: &str) -> Result<SparseVector, QqlError> {
        if !model.is_empty() && !model.eq_ignore_ascii_case("default") {
            return Err(sparse_model_unsupported_error(model));
        }
        Ok(sparse::embed_query(text))
    }

    /// Sparse embedding for **document** text at ingestion: BM25
    /// term-frequency saturation, matching Qdrant's `qdrant/bm25` document
    /// embedding.
    ///
    /// Default implementation uses local wire-compatible BM25 when `model` is
    /// empty or `"default"`. Non-default sparse models are rejected — override
    /// this method to provide model-aware sparse inference.
    async fn embed_sparse_document(
        &self,
        text: &str,
        model: &str,
    ) -> Result<SparseVector, QqlError> {
        if !model.is_empty() && !model.eq_ignore_ascii_case("default") {
            return Err(sparse_model_unsupported_error(model));
        }
        Ok(sparse::embed_document(text))
    }

    /// Batch document-side sparse embedding. Default loops
    /// [`Self::embed_sparse_document`]; override for real batching.
    async fn embed_sparse_document_batch(
        &self,
        texts: &[String],
        model: &str,
    ) -> Result<Vec<SparseVector>, QqlError> {
        let mut results = Vec::with_capacity(texts.len());
        for text in texts {
            results.push(self.embed_sparse_document(text, model).await?);
        }
        Ok(results)
    }

    /// Single-pass joint multi-modal / BGE-M3 embedding (dense + sparse + multi-vectors).
    ///
    /// Default implementation delegates to separate dense, sparse, and multi calls.
    /// Override this to make a single inference pass (e.g. `Bgem3Embedding::embed`).
    /// The default propagates the first error and does **not** suppress failures.
    async fn embed_joint(&self, text: &str, model: &str) -> Result<JointEmbeddingOutput, QqlError> {
        let dense = self.embed_dense(text, model).await?;
        let sparse = self.embed_sparse_document(text, model).await?;
        let multi = self.embed_multi(text, model).await?;
        Ok(JointEmbeddingOutput {
            dense: Some(dense),
            sparse: Some(sparse),
            multi: Some(multi),
        })
    }

    /// Batch joint embedding. Default loops [`Self::embed_joint`].
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

    /// Batch multivector embedding. Default loops [`Self::embed_multi`].
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

    /// Batch image embedding. Default loops [`Self::embed_image`].
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

/// Error when a sparse model is requested that this embedder cannot satisfy.
pub fn sparse_model_unsupported_error(model: &str) -> QqlError {
    QqlError::execution(
        "QQL-EMBEDDING-SPARSE",
        format!(
            "sparse model '{model}' is not available on this embedder. \
             Omit the MODEL clause (or use MODEL 'default') for local \
             wire-compatible BM25. To use model-aware sparse embedding \
             (SPLADE / BGE-M3), configure a sparse embedding backend."
        ),
        None,
    )
}

/// Output container for single-pass joint multi-modal / BGE-M3 embedding.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct JointEmbeddingOutput {
    /// Dense vector, when the model provides one.
    pub dense: Option<Vec<f32>>,
    /// Sparse (BM25 / SPLADE) vector, when the model provides one.
    pub sparse: Option<SparseVector>,
    /// Multivector token vectors (ColBERT), when the model provides them.
    pub multi: Option<Vec<Vec<f32>>>,
}

/// Local sparse-only helper (no dense model).
pub struct SparseEmbedder;

impl SparseEmbedder {
    /// Embed query text with local wire-compatible BM25 (unit term weights).
    pub fn embed_query(text: &str) -> SparseVector {
        sparse::embed_query(text)
    }

    /// Embed document text with local wire-compatible BM25 (tf saturation).
    pub fn embed_document(text: &str) -> SparseVector {
        sparse::embed_document(text)
    }
}
