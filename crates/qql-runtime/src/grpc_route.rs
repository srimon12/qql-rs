//! Plan → Qdrant gRPC translation and fast-path execution.
//!
//! Dual-read/write of Qdrant 1.19-deprecated `on_disk` / `always_ram` /
//! `on_disk_payload` fields alongside the new `memory` placement enum, until
//! upstream removes them in 1.21. Prefer `memory` for new code paths.
//!
//! Layout:
//!
//! - [`common`] — JSON / shard-key / enum helpers
//! - [`ddl`] — collection & index converters (HNSW, optimizers, quantization,
//!   vector params, payload index params)
//! - [`query`] — query / point converters (`QueryPoints`, selectors, vectors)
//! - [`filter`] — filter expression converters
//! - [`formula`] — formula expression converters
//! - [`values`] — payload value conversions (JSON ↔ proto)
//! - [`responses`] — proto responses → REST-shaped JSON envelopes
//! - [`execute`] — fast-path dispatch ([`execute_planned_grpc`])
//! - [`execute_read`] / [`execute_write`] / [`execute_ddl`] — dispatch helpers
#![allow(deprecated)]

mod common;
mod ddl;
mod execute;
mod execute_ddl;
mod execute_read;
mod execute_write;
mod filter;
mod formula;
mod query;
mod responses;
#[cfg(test)]
mod tests;
mod values;

pub use execute::execute_planned_grpc;
pub use execute_read::execute_query_batch_grpc;
pub use execute_write::execute_update_batch_grpc;

/// Test-only re-exports for REST/gRPC parity contract tests.
#[cfg(test)]
pub(crate) mod test_api {
    pub(crate) use super::query::{to_query_points, to_vector_input};
}

#[cfg(test)]
pub(crate) mod test_api_ddl {
    pub(crate) use super::ddl::vector_params;
}
