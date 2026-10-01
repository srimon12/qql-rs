//! Shared telemetry types, re-exported from [`qql_plan::surface::telemetry`].
//!
//! Phase timings and the server `time`/`usage` envelope live in
//! `qql-plan::surface` so the runtime and WASM hosts share one shape.

pub use qql_plan::surface::telemetry::*;
