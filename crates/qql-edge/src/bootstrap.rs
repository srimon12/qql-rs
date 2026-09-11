//! Snapshot seeding helpers for the edge backend.
//!
//! Keeps the `qdrant_edge` snapshot calls behind qql-edge so the CLI (and other
//! embedders) never touch engine types directly. The engine's own snapshot API
//! is used verbatim — archives are never unpacked or merged by hand (Qdrant's
//! edge docs call manual merging a failure mode).
//!
//! Follows the documented seed flow: download a remote shard snapshot, unpack
//! it into an empty directory, then load the result. The snapshot carries the
//! source collection's configuration, built HNSW indexes and quantized data, so
//! nothing needs re-indexing on the device.

use std::path::Path;

use qdrant_edge::EdgeShard;
use qql_core::error::QqlError;

use crate::backend::error_map::{EdgeOp, edge_err};

/// Counts collected from a snapshot-seeded shard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShardSummary {
    /// Approximate point count (segments are summed; can exceed distinct ids).
    pub points_count: u64,
    /// Approximate number of vectors that have been added to an index.
    pub indexed_vectors_count: u64,
    /// Number of segments in the shard.
    pub segments_count: u64,
}

/// Unpack a downloaded shard-snapshot archive into `target`.
///
/// `target` must not contain segment data; callers stage into a fresh
/// directory and only move it into place after the snapshot verifies.
pub fn unpack_snapshot(snapshot: &Path, target: &Path) -> Result<(), QqlError> {
    EdgeShard::unpack_snapshot(snapshot, target).map_err(|error| {
        edge_err(EdgeOp::Snapshot, None, error)
            .with_field("snapshot", snapshot.display().to_string())
    })
}

/// Load the shard at `shard_dir`, read its counts, and close it again.
///
/// Used to verify a freshly unpacked snapshot before it is moved into place.
/// The shard is flushed on drop; the directory is free for renaming afterwards.
pub fn inspect_shard(shard_dir: &Path) -> Result<ShardSummary, QqlError> {
    let shard = EdgeShard::load(shard_dir, None).map_err(|error| {
        edge_err(EdgeOp::Snapshot, None, error).with_field("shard", shard_dir.display().to_string())
    })?;
    let info = shard.info().map_err(|error| {
        edge_err(EdgeOp::Snapshot, None, error).with_field("shard", shard_dir.display().to_string())
    })?;
    drop(shard);
    Ok(ShardSummary {
        points_count: info.points_count as u64,
        indexed_vectors_count: info.indexed_vectors_count as u64,
        segments_count: info.segments_count as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inspect_missing_shard_fails_without_panicking() {
        let dir = std::env::temp_dir().join(format!("qql-edge-inspect-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create dir");
        let error = inspect_shard(&dir).expect_err("empty dir is not a shard");
        assert!(error.code.starts_with("QQL-EDGE-"), "{}", error.code);
        assert_eq!(error.field("operation"), Some("snapshot"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
