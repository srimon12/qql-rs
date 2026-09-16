//! Embedding adapters for the runtime.
//!
//! The shared `Embedder` trait and AST resolve live in `qql-embed`.
//! This module re-exports them alongside the `HttpEmbedder` reqwest adapter
//! (see `super::embedder_http` for the client implementation).

// Re-export shared API so existing `qql::embedder::Embedder` paths keep working.
pub use qql_embed::SparseVector;
pub use qql_embed::embedder::{Embedder, EmbedderBound, SparseEmbedder};
pub use qql_embed::sparse::Bm25Params;

#[cfg(feature = "rest")]
pub use super::embedder_http::HttpEmbedder;

/// Options for constructing an [`HttpEmbedder`].
#[cfg(feature = "rest")]
#[derive(Debug, Clone, Default)]
pub struct HttpEmbedderOptions {
    /// OpenAI-compatible embedding endpoint URL (e.g. `https://host/v1/embeddings`).
    pub endpoint: String,
    /// Bearer token sent as `Authorization` when non-empty.
    pub api_key: String,
    /// Default dense model name.
    pub model: String,
    /// Expected dense dimension; must be positive.
    pub dimension: usize,
    /// Optional multi/ColBERT endpoint. Falls back to `endpoint` when empty.
    pub multi_endpoint: Option<String>,
    /// Bearer token for `multi_endpoint`; falls back to `api_key`.
    pub multi_api_key: Option<String>,
    /// Multi/ColBERT model name; falls back to `model` when unset.
    pub multi_model: Option<String>,
    /// Expected per-token dim for multi responses. `0` skips dim checks.
    pub multi_dimension: usize,
    /// Optional image/CLIP vision endpoint. Falls back to `endpoint` when empty.
    pub image_endpoint: Option<String>,
    /// Bearer token for `image_endpoint`; falls back to `api_key`.
    pub image_api_key: Option<String>,
    /// Image/CLIP vision model name; falls back to `model` when unset.
    pub image_model: Option<String>,
    /// Expected dense dim for image responses (CLIP = 512). `0` uses dense dim.
    pub image_dimension: usize,
    /// Cross-encoder pair-rerank endpoint (Cohere-compatible).
    pub rerank_endpoint: Option<String>,
    /// Bearer token for the rerank endpoint; falls back to `api_key`.
    pub rerank_api_key: Option<String>,
    /// Cross-encoder model name (Cohere-style `rerank` API).
    pub rerank_model: Option<String>,
    /// Client-side BM25 `k1` for local **document** sparse embedding. `None`
    /// keeps the Qdrant `qdrant/bm25` default (`1.2`). Write-path only: it does
    /// not affect dense/multi/image/rerank HTTP inference, query-side sparse
    /// weights (always unit), or server-side inference. Documents written
    /// before a change keep their vectors — re-ingest to apply. Invalid values
    /// fail closed with `QQL-VALIDATION-CONFIG`.
    pub bm25_k1: Option<f64>,
    /// Client-side BM25 `b` (length normalization, `[0, 1]`); `None` keeps the
    /// Qdrant default (`0.75`). See [`Self::bm25_k1`].
    pub bm25_b: Option<f64>,
    /// Client-side BM25 expected average document length in tokens; `None`
    /// keeps the Qdrant default (`256`). See [`Self::bm25_k1`].
    pub bm25_avg_len: Option<f64>,
}
