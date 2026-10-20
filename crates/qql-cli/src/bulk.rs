//! Bulk-load optimizer handshake shared by `qql dump` and `qql migrate`.
//!
//! Both commands raise `indexing_threshold` before replaying points so HNSW is
//! not built segment-by-segment during ingest, then restore the source value.

use qql::backend::CollectionInfo;

/// Default optimizer `indexing_threshold` (KB) during bulk load (~2 GB).
///
/// Official guidance is "very high" so HNSW is not built concurrently with
/// ingest. 10 GB OOMs small nodes; 2 GB is high enough to suppress indexing
/// on typical migrations without pinning unbounded RAM. Override with
/// `--bulk-threshold-kb`.
pub const DEFAULT_BULK_INDEXING_THRESHOLD: u64 = 2_000_000;
/// Qdrant default `indexing_threshold` (20 MB) restored after bulk load.
pub const DEFAULT_INDEXING_THRESHOLD: u64 = 20_000;

/// Raise `indexing_threshold` so HNSW is not built concurrently with ingest.
/// Returns the original threshold (if any) for later restore.
pub fn suppress_indexing(info: &mut CollectionInfo, threshold_kb: u64) -> Option<u64> {
    let optimizers = info.schema.optimizers.get_or_insert_with(Default::default);
    let original = optimizers.indexing_threshold;
    optimizers.indexing_threshold = Some(threshold_kb);
    original
}

/// `ALTER COLLECTION … WITH OPTIMIZERS (indexing_threshold = N)`.
pub(crate) fn indexing_threshold_statement(collection: &str, threshold_kb: u64) -> String {
    format!(
        "ALTER COLLECTION {} WITH OPTIMIZERS (indexing_threshold = {})",
        crate::dump::format_ident(collection),
        threshold_kb
    )
}

/// `ALTER COLLECTION` restoring the source threshold, or Qdrant's default.
pub fn restore_optimizers_statement(collection: &str, original: Option<u64>) -> String {
    indexing_threshold_statement(collection, original.unwrap_or(DEFAULT_INDEXING_THRESHOLD))
}
