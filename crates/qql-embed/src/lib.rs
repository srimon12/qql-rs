//! Shared embedding layer for QQL.
//!
//! - [`Embedder`] — host-agnostic dense/sparse API (batch dense by default when overridden)
//! - [`resolve_embeddings`] — rewrite `QUERY` / `UPSERT` text into vectors on the AST
//! - [`sparse`] — local wire-compatible BM25 sparse vectors (no network)
//!
//! Used by `qql` (runtime HttpEmbedder), `qql-edge` (FastEmbedder), and `qql-wasm`
//! (fetch / JS adapters). No Qdrant I/O and no HTTP client live here.

/// Host-agnostic `Embedder` trait, joint embedding output, and error helpers.
pub mod embedder;
/// AST rewriting: resolve `TEXT` inputs to vectors via the configured embedder.
pub mod resolve;
pub mod sparse;
pub mod topology;

#[cfg(test)]
mod resolve_test;
#[cfg(test)]
mod sparse_test;

pub use embedder::{
    Embedder, EmbedderBound, JointEmbeddingOutput, SparseEmbedder, cross_rerank_unsupported_error,
    image_unsupported_error, multi_unsupported_error, sparse_model_unsupported_error,
};
pub use resolve::{DENSE_VECTOR_NAME, SPARSE_VECTOR_NAME, resolve_embeddings};
pub use sparse::SparseVector;
pub use topology::{
    TopologyNames, query_needs_kind_resolution, resolve_query_vector_kinds,
    resolve_query_vector_kinds_simple,
};
