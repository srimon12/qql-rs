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

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::LazyLock;

use murmur3_32::Murmur3;
use rust_stemmers::{Algorithm, Stemmer};

/// Sparse embedding (indices + values). Transport-neutral — not a protobuf type.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SparseVector {
    pub indices: Vec<u32>,
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
const ENGLISH_STOPWORDS: &[&str] = &[
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
];

static STOPWORDS: LazyLock<HashSet<&'static str>> =
    LazyLock::new(|| ENGLISH_STOPWORDS.iter().copied().collect());

static STEMMER: LazyLock<Stemmer> = LazyLock::new(|| Stemmer::create(Algorithm::English));

/// Server-default text pipeline: word tokenizer (split on non-alphanumeric),
/// Unicode lowercase, English stopword removal, English snowball stemming.
///
/// Matches `WordTokenizer` + default `TokensProcessor` on the Qdrant server —
/// the same pipeline Qdrant Edge's `EdgeBm25` runs.
pub fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(|token| token.to_lowercase())
        .filter(|token| !STOPWORDS.contains(token.as_str()))
        .map(|token| STEMMER.stem(&token).into_owned())
        .collect()
}

/// Embed query text: unique token IDs (sorted) with unit weights — identical
/// to Qdrant's `qdrant/bm25` query embedding.
pub fn embed_query(text: &str) -> SparseVector {
    let mut indices: Vec<u32> = tokenize(text).iter().map(|token| token_id(token)).collect();
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
/// divisor). Term frequencies are counted per token string and only then
/// mapped to token IDs, so a rare murmur3 collision merges exactly like the
/// Qdrant server's implementation (last write wins).
pub fn embed_document_with(text: &str, k1: f64, b: f64, avgdl: f64) -> SparseVector {
    let tokens = tokenize(text);
    if tokens.is_empty() {
        return SparseVector::default();
    }

    let doc_len = tokens.len() as f64;
    let safe_avgdl = if avgdl.is_finite() && avgdl > 0.0 {
        avgdl
    } else {
        DEFAULT_AVGDL
    };
    let denom_scale = k1 * (1.0 - b + b * doc_len / safe_avgdl);
    let k1p1 = k1 + 1.0;

    let mut counts: HashMap<&str, u32> = HashMap::with_capacity(tokens.len());
    for token in &tokens {
        *counts.entry(token).or_insert(0) += 1;
    }

    // BTreeMap keeps indices sorted (post-insert invariant, like the server).
    let mut tf_map: BTreeMap<u32, f64> = BTreeMap::new();
    for (token, n) in &counts {
        let tf = (*n as f64) * k1p1 / (denom_scale + *n as f64);
        tf_map.insert(token_id(token), tf);
    }

    let indices: Vec<u32> = tf_map.keys().copied().collect();
    let values: Vec<f32> = tf_map.values().map(|&tf| tf as f32).collect();
    SparseVector { indices, values }
}
