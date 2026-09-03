//! gRPC transport for Qdrant (`GrpcQdrant`).
//!
//! Dual-read of Qdrant 1.19-deprecated on-disk / always-ram fields while
//! schema extraction also reports the new `memory` placement. See grpc_route.
//!
//! Layout:
//!
//! - [`client`] — connection handle, auth metadata, client factories
//! - [`points`] / [`collections`] — thin typed wrappers (qdrant-client shape)
//! - [`ops`] — [`crate::client::QdrantOps`] implementation
//! - [`schema`] — proto `CollectionInfo` → typed [`crate::backend::CollectionSchema`]
//! - [`error`] / [`memory`] — shared status mapping and `memory` placement helpers
#![allow(deprecated)]

mod client;
mod collections;
mod error;
pub(crate) mod memory;
mod ops;
mod points;
mod schema;

pub use client::{GrpcQdrant, ROUTE_AFFINITY_METADATA};
