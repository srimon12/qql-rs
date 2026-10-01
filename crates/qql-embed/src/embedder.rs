use async_trait::async_trait;
use qql_core::error::QqlError;

use crate::sparse::{Bm25Params, SparseVector};

#[cfg(not(target_arch = "wasm32"))]
/// Send/Sync bound helper for `Embedder` implementations on native targets.
///
/// Kept as a twin of `QdrantOpsBound` in `qql-runtime` (not shared): sharing
/// would couple the crates backwards (`qql-embed` must stay dependency-free
/// of the runtime) or mistype the bound (`QdrantOps: EmbedderBound` reads as
/// an is-a relationship that does not exist). One shim per trait, each
/// documenting its own target split.
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
/// Calls should batch when possible (`*_batch` → one HTTP request or one ONNX
/// batch): `resolve_embeddings` batches dense, sparse-query, multi, and image
/// jobs alike (grouped by model, one `*_batch` call per model), so remote
/// backends must override the batch variants for real batching instead of
/// relying on the sequential single-call defaults. Sparse is role-split:
/// [`Self::embed_sparse_query`] (unit weights) for search text and
/// [`Self::embed_sparse_document`] (BM25 tf saturation) for ingestion text,
/// both defaulting to local wire-compatible BM25. Multivector (ColBERT-style)
/// uses [`Self::embed_multi`] → `Vec<Vec<f32>>`.
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait Embedder: EmbedderBound {
    /// Embed one text into a dense vector; `model` may be empty or `"default"`.
    async fn embed_dense(&self, text: &str, model: &str) -> Result<Vec<f32>, QqlError>;

    /// Sparse embedding for **query** text: unique terms with unit weights,
    /// matching Qdrant's `qdrant/bm25` query embedding.
    ///
    /// Default implementation uses the local pipeline from
    /// [`Self::bm25_text_config`] when `model` is empty, `"default"`, or
    /// `"qdrant/bm25"` (the local pipeline *is* that model — see
    /// [`is_local_bm25_model`]). Non-default sparse models are rejected —
    /// override this method to provide model-aware sparse inference.
    async fn embed_sparse_query(&self, text: &str, model: &str) -> Result<SparseVector, QqlError> {
        if !is_local_bm25_model(model) {
            return Err(sparse_model_unsupported_error(model));
        }
        let config = self.bm25_text_config();
        if config == crate::Bm25TextConfig::default() {
            // Default config: reuse the process-wide pipeline instead of
            // rebuilding the stopword set + stemmer per text.
            return crate::bm25_text::default_pipeline().embed_query(text);
        }
        config.pipeline().embed_query(text)
    }

    /// Sparse embedding for **document** text at ingestion: BM25
    /// term-frequency saturation, matching Qdrant's `qdrant/bm25` document
    /// embedding.
    ///
    /// Default implementation uses the local pipeline from
    /// [`Self::bm25_text_config`] when `model` is empty, `"default"`, or
    /// `"qdrant/bm25"` (the local pipeline *is* that model), honoring its
    /// [`Bm25Params`]. Non-default sparse models are rejected — override this
    /// method to provide model-aware sparse inference.
    async fn embed_sparse_document(
        &self,
        text: &str,
        model: &str,
    ) -> Result<SparseVector, QqlError> {
        if !is_local_bm25_model(model) {
            return Err(sparse_model_unsupported_error(model));
        }
        let config = self.bm25_text_config();
        if config == crate::Bm25TextConfig::default() {
            // Default config: reuse the process-wide pipeline (see the query
            // method above).
            return crate::bm25_text::default_pipeline().embed_document(text);
        }
        config.pipeline().embed_document(text)
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

    /// Batch query-side sparse embedding. Default loops
    /// [`Self::embed_sparse_query`]; override for real batching
    /// (model-backed SPLADE / BGE-M3 sparse inference).
    async fn embed_sparse_query_batch(
        &self,
        texts: &[String],
        model: &str,
    ) -> Result<Vec<SparseVector>, QqlError> {
        let mut results = Vec::with_capacity(texts.len());
        for text in texts {
            results.push(self.embed_sparse_query(text, model).await?);
        }
        Ok(results)
    }

    /// Dense output dimension when it is known without running inference.
    /// Custom and remote embedders may return `None`.
    fn dimension(&self) -> Option<usize> {
        None
    }

    /// Local BM25 hyperparameters used by the default
    /// [`Self::embed_sparse_document`] / [`Self::embed_sparse_document_batch`]
    /// implementations.
    ///
    /// Document-side only: it does not affect [`Self::embed_sparse_query`]
    /// (always unit term weights) or model-backed sparse inference (SPLADE /
    /// BGE-M3), and it is not a collection/wire setting. Defaults to
    /// [`Bm25Params::default`], so hosts that never override it keep Qdrant's
    /// `qdrant/bm25` defaults. Changing it affects documents embedded *after*
    /// the change — re-ingest to apply.
    fn bm25_params(&self) -> Bm25Params {
        Bm25Params::default()
    }

    /// Full local BM25 text configuration (pipeline + hyperparameters) used
    /// by the default [`Self::embed_sparse_document`] /
    /// [`Self::embed_sparse_query`] implementations.
    ///
    /// Default wraps [`Self::bm25_params`] with Qdrant's text defaults (word
    /// tokenizer, English, lowercase on, folding off, language
    /// stopwords/stemmer), so overriding only `bm25_params` keeps working
    /// unchanged. Override this instead to change tokenization, language,
    /// folding, stopwords, stemming, or token length limits.
    fn bm25_text_config(&self) -> crate::Bm25TextConfig {
        crate::Bm25TextConfig {
            params: self.bm25_params(),
            ..crate::Bm25TextConfig::default()
        }
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

/// Whether `model` names the local wire-compatible BM25 pipeline: empty,
/// `"default"`, or Qdrant's `qdrant/bm25` (ASCII case-insensitive).
///
/// The local pipeline is byte-compatible with that model, so hosts can accept
/// the server-side spelling instead of rejecting the name users copy from
/// Qdrant docs.
pub fn is_local_bm25_model(model: &str) -> bool {
    model.is_empty()
        || model.eq_ignore_ascii_case("default")
        || model.eq_ignore_ascii_case("qdrant/bm25")
}

/// Error when a sparse model is requested that this embedder cannot satisfy.
pub fn sparse_model_unsupported_error(model: &str) -> QqlError {
    QqlError::execution(
        "QQL-EMBEDDING-SPARSE",
        format!(
            "sparse model '{model}' is not available on this embedder. \
             Omit the MODEL clause (or use MODEL 'default' / 'qdrant/bm25') \
             for local wire-compatible BM25. To use model-aware sparse \
             embedding (SPLADE / BGE-M3), configure a sparse embedding backend."
        ),
        None,
    )
}

/// Error when a dense model is requested that this embedder cannot satisfy.
///
/// Mirrors [`sparse_model_unsupported_error`]: single-model hosts (WASM client
/// embedder, fixed local models) reject non-default `MODEL` clauses instead
/// of silently returning vectors from the wrong model.
pub fn dense_model_unsupported_error(model: &str) -> QqlError {
    QqlError::execution(
        "QQL-EMBEDDING",
        format!(
            "dense model '{model}' is not available on this embedder. \
             Omit the MODEL clause (or use MODEL 'default') to use the \
             configured dense model. To serve multiple dense models, \
             configure a model-routing dense embedding backend."
        ),
        None,
    )
}
