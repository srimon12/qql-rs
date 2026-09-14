//! Qdrant 1.19 `memory` placement conversions (single source of truth).
//!
//! Both the schema extractor ([`super::schema`]) and the plan-to-gRPC
//! converters (`grpc_route`) translate between [`MemoryPlacement`] and the
//! proto `Memory` enum through these helpers.

#![allow(deprecated)]

use qql_plan::types::MemoryPlacement;

use crate::qdrant_grpc::qdrant;

/// Convert typed `MemoryPlacement` to the proto `Memory` enum discriminant.
pub(crate) fn memory_to_proto(value: MemoryPlacement) -> i32 {
    match value {
        MemoryPlacement::Cold => qdrant::Memory::Cold as i32,
        MemoryPlacement::Cached => qdrant::Memory::Cached as i32,
        MemoryPlacement::Pinned => qdrant::Memory::Pinned as i32,
    }
}

/// Convert a proto `Memory` enum value to its QQL/OpenAPI lowercase form.
/// `Unknown` is omitted rather than inventing a placement.
pub(crate) fn memory_to_str(value: i32) -> Option<&'static str> {
    match qdrant::Memory::try_from(value).ok()? {
        qdrant::Memory::Cold => Some("cold"),
        qdrant::Memory::Cached => Some("cached"),
        qdrant::Memory::Pinned => Some("pinned"),
        qdrant::Memory::Unknown => None,
    }
}
