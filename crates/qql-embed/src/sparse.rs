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

use std::sync::LazyLock;

use murmur3_32::Murmur3;
use phf::phf_set;
use qql_core::error::QqlError;
use rust_stemmers::{Algorithm, Stemmer};

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
    /// `k1` must be finite and `> 0`, `b` finite and within `[0, 1]`, and
    /// `avg_len` finite and `> 0`. NaN and ±Inf are rejected for all three.
    pub fn new(k1: f64, b: f64, avg_len: f64) -> Result<Self, QqlError> {
        if !k1.is_finite() || k1 <= 0.0 {
            return Err(config_error(
                "bm25 k1 must be a finite number greater than zero".to_string(),
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

/// English stopwords, identical to the Qdrant server set
/// (`lib/segment/src/index/field_index/full_text_index/stop_words/english.rs`).
static STOPWORDS: phf::Set<&'static str> = phf_set! {
    "i",
    "me",
    "my",
    "myself",
    "we",
    "our",
    "ours",
    "ourselves",
    "you",
    "you're",
    "you've",
    "you'll",
    "you'd",
    "your",
    "yours",
    "yourself",
    "yourselves",
    "he",
    "him",
    "his",
    "himself",
    "she",
    "she's",
    "her",
    "hers",
    "herself",
    "it",
    "it's",
    "its",
    "itself",
    "they",
    "them",
    "their",
    "theirs",
    "themselves",
    "what",
    "which",
    "who",
    "whom",
    "this",
    "that",
    "that'll",
    "these",
    "those",
    "am",
    "is",
    "are",
    "was",
    "were",
    "be",
    "been",
    "being",
    "have",
    "has",
    "had",
    "having",
    "do",
    "does",
    "did",
    "doing",
    "a",
    "an",
    "the",
    "and",
    "but",
    "if",
    "or",
    "because",
    "as",
    "until",
    "while",
    "of",
    "at",
    "by",
    "for",
    "with",
    "about",
    "against",
    "between",
    "into",
    "through",
    "during",
    "before",
    "after",
    "above",
    "below",
    "to",
    "from",
    "up",
    "down",
    "in",
    "out",
    "on",
    "off",
    "over",
    "under",
    "again",
    "further",
    "then",
    "once",
    "here",
    "there",
    "when",
    "where",
    "why",
    "how",
    "all",
    "any",
    "both",
    "each",
    "few",
    "more",
    "most",
    "other",
    "some",
    "such",
    "no",
    "nor",
    "not",
    "only",
    "own",
    "same",
    "so",
    "than",
    "too",
    "very",
    "s",
    "t",
    "can",
    "will",
    "just",
    "don",
    "don't",
    "should",
    "should've",
    "now",
    "d",
    "ll",
    "m",
    "o",
    "re",
    "ve",
    "y",
    "ain",
    "aren",
    "aren't",
    "couldn",
    "couldn't",
    "didn",
    "didn't",
    "doesn",
    "doesn't",
    "hadn",
    "hadn't",
    "hasn",
    "hasn't",
    "haven",
    "haven't",
    "isn",
    "isn't",
    "ma",
    "mightn",
    "mightn't",
    "mustn",
    "mustn't",
    "needn",
    "needn't",
    "shan",
    "shan't",
    "shouldn",
    "shouldn't",
    "wasn",
    "wasn't",
    "weren",
    "weren't",
    "won",
    "won't",
    "wouldn",
    "wouldn't",
};

static STEMMER: LazyLock<Stemmer> = LazyLock::new(|| Stemmer::create(Algorithm::English));

#[inline]
fn process_token<F>(raw: &str, buf: &mut [u8; 64], f: &mut F)
where
    F: FnMut(&str),
{
    let bytes = raw.as_bytes();
    let len = bytes.len();
    if len <= buf.len() && raw.is_ascii() {
        for (j, &b) in bytes.iter().enumerate() {
            buf[j] = b.to_ascii_lowercase();
        }
        // Safe: `raw.is_ascii()` guarantees `bytes` is ASCII, and
        // `to_ascii_lowercase()` maps ASCII to ASCII, so `buf[..len]`
        // is valid UTF-8. Use the checked conversion so a logic error
        // fails loudly instead of invoking undefined behavior.
        let lower =
            std::str::from_utf8(&buf[..len]).expect("ascii lowercasing preserves valid UTF-8");
        if !STOPWORDS.contains(lower) {
            let stemmed = STEMMER.stem(lower);
            f(&stemmed);
        }
    } else {
        let lower = raw.to_lowercase();
        if !STOPWORDS.contains(lower.as_str()) {
            let stemmed = STEMMER.stem(&lower);
            f(&stemmed);
        }
    }
}

/// Tokenize and iterate over stemmed tokens without intermediate heap allocations.
#[inline]
pub fn for_each_token<F>(text: &str, mut f: F)
where
    F: FnMut(&str),
{
    let mut buf = [0u8; 64];

    if text.is_ascii() {
        let bytes = text.as_bytes();
        let mut start = None;
        for (i, &b) in bytes.iter().enumerate() {
            if b.is_ascii_alphanumeric() {
                if start.is_none() {
                    start = Some(i);
                }
            } else if let Some(s) = start {
                process_token(&text[s..i], &mut buf, &mut f);
                start = None;
            }
        }
        if let Some(s) = start {
            process_token(&text[s..], &mut buf, &mut f);
        }
    } else {
        let mut start = None;
        for (i, c) in text.char_indices() {
            if c.is_alphanumeric() {
                if start.is_none() {
                    start = Some(i);
                }
            } else if let Some(s) = start {
                process_token(&text[s..i], &mut buf, &mut f);
                start = None;
            }
        }
        if let Some(s) = start {
            process_token(&text[s..], &mut buf, &mut f);
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
/// the same pipeline Qdrant Edge's `EdgeBm25` runs.
pub fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    for_each_token(text, |token| {
        tokens.push(token.to_string());
    });
    tokens
}

/// Embed query text: unique token IDs (sorted) with unit weights — identical
/// to Qdrant's `qdrant/bm25` query embedding.
pub fn embed_query(text: &str) -> SparseVector {
    let mut indices: Vec<u32> = Vec::with_capacity(text.len() / 6 + 1);
    for_each_token_id(text, |id| {
        indices.push(id);
    });

    if indices.is_empty() {
        return SparseVector::default();
    }

    indices.sort_unstable();
    indices.dedup();

    let values = vec![1.0; indices.len()];
    SparseVector { indices, values }
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
    embed_document_impl(text, params.k1, params.b, params.avg_len)
}

/// Embed document text with explicit BM25 parameters.
///
/// `avgdl <= 0` or non-finite falls back to [`DEFAULT_AVGDL`] (it is a
/// divisor). `k1` and `b` are used as given — prefer
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
    embed_document_impl(text, k1, b, safe_avgdl)
}

fn embed_document_impl(text: &str, k1: f64, b: f64, avgdl: f64) -> SparseVector {
    let mut token_ids: Vec<u32> = Vec::with_capacity(text.len() / 6 + 1);
    for_each_token_id(text, |id| {
        token_ids.push(id);
    });

    if token_ids.is_empty() {
        return SparseVector::default();
    }

    let doc_len = token_ids.len() as f64;
    let denom_scale = k1 * (1.0 - b + b * doc_len / avgdl);
    let k1p1 = k1 + 1.0;

    token_ids.sort_unstable();

    let mut indices = Vec::with_capacity(token_ids.len());
    let mut values = Vec::with_capacity(token_ids.len());

    let mut i = 0;
    while i < token_ids.len() {
        let id = token_ids[i];
        let mut count = 1u32;
        while i + 1 < token_ids.len() && token_ids[i + 1] == id {
            count += 1;
            i += 1;
        }
        indices.push(id);
        let n = count as f64;
        values.push((n * k1p1 / (denom_scale + n)) as f32);
        i += 1;
    }

    SparseVector { indices, values }
}
