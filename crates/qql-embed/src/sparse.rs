//! Client-side BM25 sparse embeddings, wire-compatible with Qdrant's
//! `qdrant/bm25` model defaults.
//!
//! Token IDs are murmur3-32 (seed 0, `|i32|` made positive) — identical to the
//! Qdrant server, Qdrant Edge, and FastEmbed's `Qdrant/bm25`. The text pipeline
//! mirrors the server defaults: word tokenizer (split on non-alphanumeric),
//! Unicode lowercasing, English stopword removal, and English snowball
//! stemming. Queries embed with unit term weights; documents with BM25
//! term-frequency saturation (k1=1.2, b=0.75, avg_len=256, or explicit
//! [`Bm25Params`]). IDF is applied server-side via the sparse vector
//! `modifier: idf`.
//!
//! Because token IDs and formulas match the server, vectors produced here can
//! be mixed with server-side `qdrant/bm25` inference on the same collection.
//!
//! ### Tuning BM25 (`k1`, `b`, `avg_len`)
//!
//! [`Bm25Params`] is a **client-side, write-path-only** setting: it shapes how
//! *documents* are encoded (tf saturation via `k1`, length normalization via
//! `b`, and the expected average document length via `avg_len`). It is not a
//! collection or wire setting, does not affect query-side weights (always unit
//! weights), and does not change server-side `qdrant/bm25` inference. A wrong
//! `avg_len` silently misjudges every document, so tune it to the corpus being
//! written; documents embedded before the change keep their vectors — re-ingest
//! to apply.
//!
//! ### FastEmbed Query Weighting Parity Note
//! FastEmbed's Python `Qdrant/bm25` emits a uniform scaling factor (~1.665) on query
//! term weights, whereas Qdrant's server-side inference and QQL use unit weights (1.0).
//! Because this factor is uniform across all terms in a query, ranking order is
//! mathematically identical, but raw score magnitudes will scale by ~1.665x.

use murmur3_32::Murmur3;
use qql_core::error::QqlError;

use super::bm25_text::default_pipeline;

/// Sparse embedding (indices + values). Transport-neutral — not a protobuf type.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SparseVector {
    /// Sorted token IDs (murmur3-32, wire-compatible with Qdrant `qdrant/bm25`).
    pub indices: Vec<u32>,
    /// Per-token weights aligned with `indices` (unit for queries, tf for docs).
    pub values: Vec<f32>,
}

/// BM25 term-frequency saturation, matching Qdrant's `qdrant/bm25` default.
pub const DEFAULT_K1: f64 = 1.2;
/// BM25 document-length normalization, matching Qdrant's `qdrant/bm25` default.
pub const DEFAULT_B: f64 = 0.75;
/// BM25 expected average document length in tokens, matching Qdrant's
/// `qdrant/bm25` default.
pub const DEFAULT_AVGDL: f64 = 256.0;

/// Validated BM25 hyperparameters for **document-side** local encoding.
///
/// Only documents are affected: query text always embeds with unit term
/// weights, and IDF is applied by the backend from the sparse vector's
/// `modifier: idf`. This is a client-side, write-path-only knob — it is not a
/// collection/wire setting, and it does not change server-side `qdrant/bm25`
/// inference. Vectors written before a change stay as written; re-ingest to
/// apply new parameters.
///
/// Construct via [`Bm25Params::new`] (or [`Bm25Params::resolve`] for optional
/// overrides); invalid values fail closed with `QQL-VALIDATION-CONFIG`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bm25Params {
    k1: f64,
    b: f64,
    avg_len: f64,
}

impl Default for Bm25Params {
    /// Qdrant's `qdrant/bm25` defaults: `k1 = 1.2`, `b = 0.75`,
    /// `avg_len = 256`. Unset configuration keeps this exact behavior.
    fn default() -> Self {
        Self {
            k1: DEFAULT_K1,
            b: DEFAULT_B,
            avg_len: DEFAULT_AVGDL,
        }
    }
}

impl Bm25Params {
    /// Validate and build explicit BM25 parameters.
    ///
    /// `k1` must be finite and `>= 0` (like Qdrant's validator; `0` gives
    /// binary weighting), `b` finite and within `[0, 1]`, and `avg_len`
    /// finite and `> 0`. NaN and ±Inf are rejected for all three.
    pub fn new(k1: f64, b: f64, avg_len: f64) -> Result<Self, QqlError> {
        if !k1.is_finite() || k1 < 0.0 {
            return Err(config_error(
                "bm25 k1 must be a finite number greater than or equal to zero".to_string(),
            ));
        }
        if !b.is_finite() || !(0.0..=1.0).contains(&b) {
            return Err(config_error(
                "bm25 b must be a finite number in [0, 1]".to_string(),
            ));
        }
        if !avg_len.is_finite() || avg_len <= 0.0 {
            return Err(config_error(
                "bm25 avg_len must be a finite number greater than zero".to_string(),
            ));
        }
        Ok(Self { k1, b, avg_len })
    }

    /// Resolve optional overrides on top of [`Bm25Params::default`]; `None`
    /// keeps the corresponding Qdrant `qdrant/bm25` default.
    pub fn resolve(
        k1: Option<f64>,
        b: Option<f64>,
        avg_len: Option<f64>,
    ) -> Result<Self, QqlError> {
        let defaults = Self::default();
        Self::new(
            k1.unwrap_or(defaults.k1),
            b.unwrap_or(defaults.b),
            avg_len.unwrap_or(defaults.avg_len),
        )
    }

    /// Term-frequency saturation (`k1`).
    pub fn k1(&self) -> f64 {
        self.k1
    }

    /// Document-length normalization factor (`b`), `0` = none, `1` = full.
    pub fn b(&self) -> f64 {
        self.b
    }

    /// Expected average document length in tokens (`avg_len`).
    pub fn avg_len(&self) -> f64 {
        self.avg_len
    }
}

fn config_error(message: String) -> QqlError {
    QqlError::validation("QQL-VALIDATION-CONFIG", message, None)
}

/// Token → `u32` ID. Wire-compatible with Qdrant's BM25 sparse vectors:
/// murmur3 32-bit (seed 0), then `|i32|` to make it positive.
///
/// Hashes bytes **as given**: the embedding pipeline lowercases (and stems)
/// before calling, so callers must pass already-normalized text — hashing
/// `"Hello"` and `"hello"` yields different IDs by design (like the server's
/// own `token_id` layer).
pub fn token_id(token: &str) -> u32 {
    (Murmur3::hash(0, token.as_bytes()) as i32).unsigned_abs()
}

/// Tokenize and iterate over processed tokens (default English pipeline: word
/// tokenizer, lowercase, English stopwords, English stemming).
///
/// Compatibility shim over [`crate::bm25_text::Bm25Pipeline`]: allocates one
/// `Vec` per call (the old stack-buffered zero-alloc form is gone — hot
/// paths should use the pipeline directly). For other languages and options
/// see [`crate::bm25_text::Bm25Pipeline`].
#[inline]
pub fn for_each_token<F>(text: &str, mut f: F)
where
    F: FnMut(&str),
{
    // The default pipeline only runs the word tokenizer: infallible.
    if let Ok(tokens) = default_pipeline().doc_tokens(text) {
        for token in &tokens {
            f(token);
        }
    }
}

/// Tokenize and iterate directly over `u32` token IDs without intermediate allocations.
#[inline]
pub fn for_each_token_id<F>(text: &str, mut f: F)
where
    F: FnMut(u32),
{
    for_each_token(text, |token| {
        f(token_id(token));
    });
}

/// Server-default text pipeline: word tokenizer (split on non-alphanumeric),
/// Unicode lowercase, English stopword removal, English snowball stemming.
///
/// Matches `WordTokenizer` + default `TokensProcessor` on the Qdrant server —
/// the same pipeline Qdrant Edge's `EdgeBm25` runs. For other languages and
/// options see [`crate::bm25_text::Bm25Pipeline`].
pub fn tokenize(text: &str) -> Vec<String> {
    default_pipeline().doc_tokens(text).unwrap_or_default()
}

/// Embed query text: unique token IDs (sorted) with unit weights — identical
/// to Qdrant's `qdrant/bm25` query embedding.
pub fn embed_query(text: &str) -> SparseVector {
    default_pipeline().embed_query(text).unwrap_or_default()
}

/// Embed document text with BM25 term-frequency saturation using Qdrant's
/// default parameters (`k1=1.2`, `b=0.75`, `avg_len=256`).
pub fn embed_document(text: &str) -> SparseVector {
    embed_document_with(text, DEFAULT_K1, DEFAULT_B, DEFAULT_AVGDL)
}

/// Embed document text with validated [`Bm25Params`].
///
/// Prefer this over [`embed_document_with`] on configurable paths: the
/// parameters are validated once at construction instead of sanitized per call.
pub fn embed_document_with_params(text: &str, params: &Bm25Params) -> SparseVector {
    super::bm25_text::Bm25Pipeline::with_params(params)
        .embed_document(text)
        .unwrap_or_default()
}

/// Embed document text with explicit BM25 parameters.
///
/// `avgdl <= 0` or non-finite falls back to [`DEFAULT_AVGDL`] (it is a
/// divisor). `k1` and `b` are used as given — including non-finite values,
/// which propagate as `NaN` weights — so prefer
/// [`embed_document_with_params`] for fail-closed validation. Frequencies are
/// counted per token ID: on the rare murmur3 collision two terms merge into
/// one dimension with summed counts, which keeps output deterministic across
/// runs (the server's own per-string counting is randomized there, so collided
/// IDs carry no cross-implementation contract).
pub fn embed_document_with(text: &str, k1: f64, b: f64, avgdl: f64) -> SparseVector {
    let safe_avgdl = if avgdl.is_finite() && avgdl > 0.0 {
        avgdl
    } else {
        DEFAULT_AVGDL
    };
    default_pipeline()
        .embed_document_with(text, k1, b, safe_avgdl)
        .unwrap_or_default()
}
