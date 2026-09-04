//! Client-side BM25 sparse embeddings, wire-compatible with Qdrant's
//! `qdrant/bm25` model defaults.
//!
//! Token IDs are murmur3-32 (seed 0, `|i32|` made positive) — identical to the
//! Qdrant server, Qdrant Edge, and FastEmbed's `Qdrant/bm25`. The text pipeline
//! mirrors the server defaults: word tokenizer (split on non-alphanumeric),
//! Unicode lowercasing, English stopword removal, and English snowball
//! stemming. Queries embed with unit term weights; documents with BM25
//! term-frequency saturation (k1=1.2, b=0.75, avg_len=256). IDF is applied
//! server-side via the sparse vector `modifier: idf`.
//!
//! Because token IDs and formulas match the server, vectors produced here can
//! be mixed with server-side `qdrant/bm25` inference on the same collection.

use std::sync::LazyLock;

use murmur3_32::Murmur3;
use phf::phf_set;
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

/// Token → `u32` ID. Wire-compatible with Qdrant's BM25 sparse vectors:
/// murmur3 32-bit (seed 0), then `|i32|` to make it positive.
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

/// Embed document text with explicit BM25 parameters.
///
/// `avgdl <= 0` or non-finite falls back to [`DEFAULT_AVGDL`] (it is a
/// divisor). Frequencies are counted per token ID: on the rare murmur3
/// collision two terms merge into one dimension with summed counts, which
/// keeps output deterministic across runs (the server's own per-string
/// counting is randomized there, so collided IDs carry no cross-implementation
/// contract).
pub fn embed_document_with(text: &str, k1: f64, b: f64, avgdl: f64) -> SparseVector {
    let mut token_ids: Vec<u32> = Vec::with_capacity(text.len() / 6 + 1);
    for_each_token_id(text, |id| {
        token_ids.push(id);
    });

    if token_ids.is_empty() {
        return SparseVector::default();
    }

    let doc_len = token_ids.len() as f64;
    let safe_avgdl = if avgdl.is_finite() && avgdl > 0.0 {
        avgdl
    } else {
        DEFAULT_AVGDL
    };
    let denom_scale = k1 * (1.0 - b + b * doc_len / safe_avgdl);
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
