//! Shared telemetry types, re-exported from [`qql_protocol::telemetry`].
//!
//! Phase timings and the server `time`/`usage` envelope live in
//! `qql-protocol` so the runtime and WASM hosts share one shape.

pub use qql_protocol::telemetry::*;
