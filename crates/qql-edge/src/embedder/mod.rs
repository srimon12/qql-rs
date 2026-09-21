//! Local fastembed embedder: dense ONNX model plus optional sparse, multi
//! (ColBERT), image, and cross-encoder slots, with wire-compatible BM25 fallback.
//!
//! Split by size hygiene: `construct` (option resolution + cached init),
//! `accessors` (introspection + acceptance), `catalog` (model list/resolve),
//! `inference` (the `Embedder` trait dispatch), `bm25` (fallback helpers).
//! Stable paths are re-exported here so `lib.rs` and downstream crates keep
//! their imports.

mod accessors;
mod bm25;
mod catalog;
mod construct;
mod inference;

#[cfg(test)]
mod tests;

pub use catalog::{
    list_embedding_models, resolve_embedding_model, resolve_image_model, resolve_multi_model,
};

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::{Arc, Mutex};

use fastembed::{Bgem3Embedding, ImageEmbedding, SparseTextEmbedding, TextEmbedding, TextRerank};
use qdrant_edge::bm25_embed::EdgeBm25;
use qql_core::error::QqlError;
use qql_embed::{Bm25Params, Bm25TextConfig};

pub(crate) fn err(msg: impl Into<std::borrow::Cow<'static, str>>) -> QqlError {
    QqlError::execution("QQL-EDGE-EMBED", msg, None)
}

/// Public description of a local ONNX embedding model.
#[derive(Debug, Clone)]
pub struct EmbeddingModelInfo {
    /// Stable enum-style name, e.g. `"BGESmallENV15"`.
    pub name: String,
    /// HuggingFace / Xenova model code, e.g. `"Xenova/bge-small-en-v1.5"`.
    pub model_code: String,
    /// Output dimension of dense vectors.
    pub dim: usize,
    /// Short human description from fastembed.
    pub description: String,
    /// True when this entry is a multivector / ColBERT-capable offline model.
    pub multi: bool,
    /// True when this entry is an image / CLIP vision model.
    pub image: bool,
}

/// Options for constructing a [`FastEmbedder`].
#[derive(Debug, Clone, Default)]
pub struct FastEmbedderOptions {
    /// Dense model name. Accepts enum names (`BGESmallENV15`), HF codes
    /// (`Xenova/bge-small-en-v1.5`), or short aliases (`bge-small-en-v1.5`).
    /// For CLIP text use `ClipVitB32` / `Qdrant/clip-ViT-B-32-text`.
    /// `None` → default `BGESmallENV15` (384-d).
    pub model: Option<String>,
    /// Offline sparse model (SPLADE or BGE-M3 via `SparseTextEmbedding`).
    /// Accepts `splade`, `SPLADEPPV1`, `Qdrant/Splade_PP_en_v1`, `bge-m3`,
    /// `BGEM3`, `BAAI/bge-m3`. When set, sparse embedding uses real ONNX
    /// inference. `None` → local wire-compatible BM25 (Qdrant
    /// `qdrant/bm25`-identical token IDs) for sparse requests.
    pub sparse_model: Option<String>,
    /// Offline multivector model. Accepts `bge-m3`, `BGEM3Q`,
    /// `gpahal/bge-m3-onnx-int8`. When set, `embed_multi` runs via BGE-M3 ColBERT.
    pub multi_model: Option<String>,
    /// Offline image / CLIP vision model. Accepts `ClipVitB32`,
    /// `Qdrant/clip-ViT-B-32-vision`, `clip-vision`. Pairs with dense CLIP text.
    pub image_model: Option<String>,
    /// Offline cross-encoder reranker (`bge-reranker-base`, `BGERerankerBase`, …).
    pub reranker_model: Option<String>,
    /// Override model cache directory. `None` → fastembed default
    /// (`FASTEMBED_CACHE_DIR` / `HF_HOME` / `./.fastembed_cache`).
    pub cache_dir: Option<PathBuf>,
    /// Show HuggingFace download progress (default: `false` for bindings —
    /// progress bars on a Node/Python stderr are noise).
    pub show_download_progress: bool,
    /// Client-side BM25 `k1` for the local wire-compatible fallback used when
    /// no fastembed sparse model is configured. `None` → Qdrant
    /// `qdrant/bm25` default (`1.2`). **Write-path only**: shapes document tf
    /// saturation; query weights stay unit and server-side inference is
    /// untouched. Invalid values fail closed with `QQL-VALIDATION-CONFIG`.
    pub bm25_k1: Option<f64>,
    /// Client-side BM25 `b` (length normalization, `[0, 1]`); `None` → `0.75`.
    pub bm25_b: Option<f64>,
    /// Client-side BM25 expected average document length in tokens; `None` →
    /// `256`.
    pub bm25_avg_len: Option<f64>,
    /// BM25 text-processing language (Qdrant name/alias); `None` keeps
    /// English. Forwards to the engine (drives its stopwords/stemmer).
    pub bm25_language: Option<String>,
    /// BM25 tokenizer (`"word"`, `"whitespace"`, `"prefix"`,
    /// `"multilingual"` — the engine ships the segmentation stack, so all
    /// four work here); `None` keeps `"word"`. Note: `multilingual` writes
    /// succeed, but the core `avg_len` estimator cannot measure multilingual
    /// pipelines (no segmentation stack there) and fails closed on them.
    pub bm25_tokenizer: Option<String>,
    /// Lowercase before matching; `None` keeps `true`.
    pub bm25_lowercase: Option<bool>,
    /// Lucene ASCII folding before lowercasing; `None` keeps `false`.
    pub bm25_ascii_folding: Option<bool>,
    /// Drop tokens shorter than this (chars); `None` keeps no minimum.
    pub bm25_min_token_len: Option<usize>,
    /// Drop over-long tokens on the document path (chars); `None` keeps no
    /// maximum.
    pub bm25_max_token_len: Option<usize>,
    /// Custom stopwords **replacing** the language default (`None` keeps it;
    /// `Some(vec![])` disables filtering). Compared post-normalization.
    pub bm25_stopwords: Option<Vec<String>>,
    /// Stemmer override (`None` = language default; `Some("none")` disables;
    /// `Some("<language>")` overrides with that language's Snowball stemmer).
    pub bm25_stemmer: Option<String>,
    /// Additional language stopword lists merged with `bm25_stopwords`.
    pub bm25_stopwords_languages: Option<Vec<String>>,
}

pub(crate) struct DenseSlot {
    pub(crate) model: Arc<Mutex<TextEmbedding>>,
    pub(crate) model_name: String,
    pub(crate) model_code: String,
    pub(crate) dim: usize,
}

pub(crate) struct MultiSlot {
    pub(crate) model: Arc<Mutex<Bgem3Embedding>>,
    pub(crate) model_name: String,
    pub(crate) model_code: String,
    /// Per-token ColBERT dimension (1024 for BGE-M3).
    pub(crate) dim: usize,
}

pub(crate) struct ImageSlot {
    pub(crate) model: Arc<Mutex<ImageEmbedding>>,
    pub(crate) model_name: String,
    pub(crate) model_code: String,
    pub(crate) dim: usize,
}

pub(crate) struct RerankSlot {
    pub(crate) model: Arc<Mutex<TextRerank>>,
    pub(crate) model_name: String,
    pub(crate) model_code: String,
}

pub(crate) struct SparseSlot {
    pub(crate) model: Arc<Mutex<SparseTextEmbedding>>,
    pub(crate) model_name: String,
    pub(crate) model_code: String,
}

/// Local fastembed embedder: dense ONNX model plus optional sparse, multi
/// (ColBERT), image, and cross-encoder slots, with wire-compatible BM25 fallback.
pub struct FastEmbedder {
    dense: DenseSlot,
    sparse: Option<SparseSlot>,
    multi: Option<MultiSlot>,
    image: Option<ImageSlot>,
    reranker: Option<RerankSlot>,
    /// Wire-compatible BM25 (Qdrant Edge pipeline) used when no fastembed
    /// sparse model is configured. Query/document roles produce the same
    /// token IDs and weights as the Qdrant server's `qdrant/bm25`.
    bm25: EdgeBm25,
    /// Validated parameters the fallback `bm25` was built with (exposed via
    /// [`qql_embed::Embedder::bm25_params`]).
    bm25_params: Bm25Params,
    /// Full text configuration the fallback `bm25` was built with (exposed
    /// via [`qql_embed::Embedder::bm25_text_config`], so estimators measure with the
    /// same language/tokenizer the engine embeds with).
    bm25_text: Bm25TextConfig,
}

pub(crate) type CacheKey = (String, String);
pub(crate) type CachedModel<T> = Arc<Mutex<T>>;
pub(crate) type ModelCache<T> = Mutex<HashMap<CacheKey, CachedModel<T>>>;

static DENSE_CACHE: OnceLock<ModelCache<TextEmbedding>> = OnceLock::new();
static SPARSE_CACHE: OnceLock<ModelCache<SparseTextEmbedding>> = OnceLock::new();
static MULTI_CACHE: OnceLock<ModelCache<Bgem3Embedding>> = OnceLock::new();
static IMAGE_CACHE: OnceLock<ModelCache<ImageEmbedding>> = OnceLock::new();
static RERANK_CACHE: OnceLock<ModelCache<TextRerank>> = OnceLock::new();

pub(crate) fn cache_dir_key(dir: Option<&PathBuf>) -> String {
    dir.map(|p| p.display().to_string()).unwrap_or_default()
}

pub(crate) fn dense_cache() -> &'static ModelCache<TextEmbedding> {
    DENSE_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn sparse_cache() -> &'static ModelCache<SparseTextEmbedding> {
    SPARSE_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn multi_cache() -> &'static ModelCache<Bgem3Embedding> {
    MULTI_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn image_cache() -> &'static ModelCache<ImageEmbedding> {
    IMAGE_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn rerank_cache() -> &'static ModelCache<TextRerank> {
    RERANK_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}
