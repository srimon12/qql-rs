//! Shared embedding layer for QQL.
//!
//! - [`Embedder`] — host-agnostic dense/sparse API (batch dense by default when overridden)
//! - [`resolve_embeddings`] — rewrite `QUERY` / `UPSERT` text into vectors on the AST
//! - [`sparse`] — local wire-compatible BM25 sparse vectors (no network)
//!
//! Used by `qql` (runtime HttpEmbedder), `qql-edge` (FastEmbedder), and `qql-wasm`
//! (fetch / JS adapters). No Qdrant I/O and no HTTP client live here.

/// ASCII folding table (Lucene mapping, as Qdrant ships it).
mod bm25_fold;
/// BM25 processing languages (Qdrant `Language` names + aliases).
pub mod bm25_lang;
/// Vendored per-language stopword lists (Qdrant segment crate, Apache-2.0).
mod bm25_stopwords;
/// BM25 text-processing pipeline: languages, tokenizers, folding, and the
/// `avg_len` estimator — option-compatible with Qdrant's `Bm25Config`.
pub mod bm25_text;
/// Host-agnostic `Embedder` trait, joint embedding output, and error helpers.
pub mod embedder;
/// AST rewriting: resolve `TEXT` inputs to vectors via the configured embedder.
pub mod resolve;
/// Query-side embedding resolution (collect → batch → apply), split from [`resolve`].
mod resolve_query;
/// Local wire-compatible BM25 sparse vectors (no network).
pub mod sparse;
/// `USING` vector-kind resolution from collection topology (dense/sparse/multi).
pub mod topology;

#[cfg(test)]
mod bm25_text_test;
#[cfg(test)]
mod resolve_test;
#[cfg(test)]
mod sparse_test;
#[cfg(test)]
mod topology_test;

pub use bm25_lang::Language;
pub use bm25_text::{
    AvgLenEstimate, Bm25Pipeline, Bm25TextConfig, Stemmer, Stopwords, Tokenizer, estimate_avg_len,
};
pub use embedder::{
    Embedder, EmbedderBound, JointEmbeddingOutput, SparseEmbedder, cross_rerank_unsupported_error,
    dense_model_unsupported_error, image_unsupported_error, multi_unsupported_error,
    sparse_model_unsupported_error,
};
pub use resolve::{DENSE_VECTOR_NAME, SPARSE_VECTOR_NAME, resolve_embeddings};
pub use sparse::{Bm25Params, SparseVector};
pub use topology::{
    TopologyNames, query_needs_kind_resolution, resolve_query_vector_kinds,
    resolve_query_vector_kinds_simple,
};
