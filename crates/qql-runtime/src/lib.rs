//! QQL executor and Qdrant transport adapters (published as the `qql` crate).
//!
//! One executor API over three transports:
//!
//! ```text
//! source text
//!   → qql-core: parse + semantic validation
//!   → prepare: schema USING resolution + embedding inference (qql-embed)
//!   → qql-plan: plan() → PlannedOperation (transport-neutral IR)
//!   → batch classification / dispatch
//!   → REST (rest) | gRPC (grpc) | in-process edge (qql-edge)
//! ```
//!
//! Backends implement the [`client::QdrantOps`] contract (11 methods); the
//! [`executor::Executor`] drives it. DDL flows through the same planner as
//! DML. The gRPC path converts typed plan structs directly to protobuf —
//! query vectors and point IDs never take a JSON detour.
//!
//! # Features
//!
//! - `rest` (default) — HTTP transport adapter ([`rest::RestQdrant`])
//! - `grpc` (default) — tonic-based gRPC transport ([`grpc::GrpcQdrant`])
//! - With both disabled, custom [`client::QdrantOps`] implementations still
//!   compile against the executor with no transport dependency.
//!
//! # Errors
//!
//! Execution failures are structured `QqlError` values from `qql-core` with
//! stable codes (`QQL-EXECUTION-*`, `QQL-TRANSPORT-*`, `QQL-BACKEND`, …).

pub mod backend;
/// The `QdrantOps` backend contract plus REST/gRPC client plumbing shared by
/// the transport adapters.
pub mod client;
/// Endpoint, API key, and timeout configuration ([`QqlConfig`](config::QqlConfig)).
pub mod config;
/// Remote embedding-server adapter ([`HttpEmbedder`](embedder::HttpEmbedder))
/// implementing the `qql-embed` [`Embedder`](embedder::Embedder) trait.
pub mod embedder;
/// The executor: parse → prepare → plan → batch → dispatch.
pub mod executor;
/// Tonic channel client and typed protobuf conversions for the gRPC transport.
#[cfg(feature = "grpc")]
pub mod grpc;
/// gRPC route execution: typed plan structs converted directly to protobuf.
#[cfg(feature = "grpc")]
mod grpc_route;
/// Typify-generated REST wire types from `openapi.json` (build-time output;
/// do not edit).
pub mod qdrant;
/// Tonic-generated protobuf types from `proto/` (build-time output; do not
/// edit).
#[cfg(feature = "grpc")]
pub mod qdrant_grpc;
/// REST transport adapter ([`RestQdrant`](rest::RestQdrant)) implementing
/// `QdrantOps` over JSON HTTP.
#[cfg(feature = "rest")]
pub mod rest;
/// Sparse vector helpers re-exported from `qql-embed` (wire-compatible BM25).
pub mod sparse;

// Sparse unit tests live in `qql-embed` (shared implementation).

#[cfg(test)]
mod contract_test;
#[cfg(test)]
mod executor_test;
