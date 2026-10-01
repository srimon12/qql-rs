//! Transport-neutral collection metadata, re-exported from
//! [`qql_plan::surface::schema`].
//!
//! Types in this module are the boundary between QQL compilation and a Qdrant
//! transport adapter. They live in `qql-plan::surface` so the runtime, the
//! WASM host, and the CLI share one schema reader. Query hits live on
//! [`crate::executor::SearchHit`].
//!
//! # Deliberate JSON
//!
//! Collection-level [`CollectionSchema::hnsw`] / [`CollectionSchema::optimizers`]
//! / [`CollectionSchema::quantization`] are typed plan configs. What stays
//! `serde_json` is intentional:
//!
//! - [`VectorSpec::hnsw`] / [`VectorSpec::quantization`] /
//!   [`VectorSpec::multivector`] and [`SparseVectorSpec::index`]: per-vector
//!   config fragments Qdrant owns and dump re-emits verbatim.
//! - [`PayloadIndexSpec::params`]: Qdrant's payload-index parameter union.
//! - Formula `DEFAULTS`: the expression engine takes JSON values directly
//!   (`qql-edge`'s formula lowering converts the typed plan tree at the
//!   boundary).
//! - REST request/response bodies (`qql_plan::Route`, `crate::surface::rest`):
//!   the wire format itself.

pub use qql_plan::surface::schema::*;
