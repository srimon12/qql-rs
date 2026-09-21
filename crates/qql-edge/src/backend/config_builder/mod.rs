//! Edge collection configuration builder passing through dense/sparse vector configs, HNSW, quantization, and optimizers.
//!
//! Split by size hygiene: `vectors` (collection builder + dense/sparse
//! params), `hnsw` (HNSW/optimizer lowering), `quantization`, `shared`
//! (placement mapping, error constructor). Stable paths are re-exported here
//! so callers (`backend`, `ops`, `index_schema`) keep their imports.

mod hnsw;
mod quantization;
mod shared;
mod vectors;

#[cfg(test)]
mod tests;

pub(crate) use hnsw::{edge_hnsw_config_over, overlay_optimizers};
pub(crate) use shared::edge_memory;
pub(crate) use vectors::build_edge_config;
