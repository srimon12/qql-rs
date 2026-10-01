//! Shared REST-boundary surface: closed response IR, telemetry, the strict
//! OpenAPI response parser, collection-schema reading, and response
//! normalization.
//!
//! Nothing here performs I/O. `qql-runtime` uses it as its REST response
//! boundary and re-exports it to keep the executor API stable; the WASM host
//! consumes the same parser and normalizer, so both hosts cannot drift on
//! response shapes.

/// Pure `BackendResponse` → `ExecResponse` shaping shared by all hosts.
pub mod normalize;
/// The closed response IR: `ExecData` family, scores, telemetry envelope.
pub mod response;
/// Strict OpenAPI REST response parsing, one shape per operation.
pub mod rest;
/// Collection metadata types and the REST schema reader.
pub mod schema;
/// Server telemetry and client phase timings.
pub mod telemetry;

pub use normalize::normalize_planned;
pub use response::*;
pub use rest::*;
pub use schema::*;
pub use telemetry::*;
