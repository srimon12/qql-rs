//! Embedding adapters for the runtime.
//!
//! The shared `Embedder` trait and AST resolve live in `qql-embed`.
//! This module re-exports them alongside the `HttpEmbedder` reqwest adapter
//! (see `super::embedder_http` for the client implementation).

// Re-export shared API so existing `qql::embedder::Embedder` paths keep working.
pub use qql_embed::SparseVector;
pub use qql_embed::bm25_lang::Language;
pub use qql_embed::bm25_text::{Bm25Pipeline, Bm25TextConfig, Stemmer, Stopwords, Tokenizer};
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
    /// BM25 text-processing language (Qdrant name or alias, e.g. `"spanish"`,
    /// `"es"`); `None` keeps English. Drives default stopwords/stemmer, like
    /// Qdrant's per-vector `language` option. Unknown names fail closed.
    pub bm25_language: Option<String>,
    /// BM25 tokenizer (`"word"`, `"whitespace"`, `"prefix"`); `None` keeps
    /// `"word"`. `"multilingual"` parses but fails closed at embed time until
    /// script-aware segmentation is compiled in.
    pub bm25_tokenizer: Option<String>,
    /// Lowercase before matching; `None` keeps `true` (Qdrant default).
    pub bm25_lowercase: Option<bool>,
    /// Lucene ASCII folding before lowercasing; `None` keeps `false`.
    pub bm25_ascii_folding: Option<bool>,
    /// Custom stopwords **replacing** the language default (`None` keeps it;
    /// `Some(vec![])` disables filtering). Compared post-normalization.
    pub bm25_stopwords: Option<Vec<String>>,
    /// Stemmer override (`None` = language default; `Some("none")` disables;
    /// `Some("<language>")` overrides with that language's Snowball stemmer).
    pub bm25_stemmer: Option<String>,
    /// Drop tokens shorter than this (chars); `None` keeps no minimum.
    pub bm25_min_token_len: Option<usize>,
    /// Drop over-long tokens on the document path (chars); `None` keeps no
    /// maximum. The prefix query path truncates instead of dropping.
    pub bm25_max_token_len: Option<usize>,
}

#[cfg(feature = "rest")]
impl HttpEmbedderOptions {
    /// Resolve the numeric BM25 hyperparameters (`k1`/`b`/`avg_len`).
    pub fn bm25_params(&self) -> Result<Bm25Params, qql_core::error::QqlError> {
        Bm25Params::resolve(self.bm25_k1, self.bm25_b, self.bm25_avg_len)
    }

    /// Resolve the full BM25 text configuration (single choke point: every
    /// text knob flows through [`qql_embed::Bm25TextConfig::resolve`]).
    pub fn bm25_text_config(&self) -> Result<Bm25TextConfig, qql_core::error::QqlError> {
        Bm25TextConfig::resolve(
            self.bm25_k1,
            self.bm25_b,
            self.bm25_avg_len,
            self.bm25_language.as_deref(),
            self.bm25_tokenizer.as_deref(),
            self.bm25_lowercase,
            self.bm25_ascii_folding,
            self.bm25_stopwords.clone(),
            self.bm25_stemmer.as_deref(),
            self.bm25_min_token_len,
            self.bm25_max_token_len,
        )
    }
}
