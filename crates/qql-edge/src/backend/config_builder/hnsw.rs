//! HNSW / optimizer lowering: plan runtime configs onto engine types.
//!
//! Pure move from `config_builder.rs` (size hygiene split).

use qql_core::error::QqlError;
use qql_plan::{HnswConfig, OptimizersConfig};

use super::shared::{edge_memory, usize_config};
use crate::backend::unsupported::EdgeUnsupported;

/// Lower a plan HNSW config into the qdrant-edge segment config type.
///
/// Shared by `CREATE COLLECTION` and the `ALTER COLLECTION` HNSW setter so the
/// two cannot drift. The engine's `HnswConfig` has required fields (`m`,
/// `ef_construct`, `full_scan_threshold`) while QQL allows any subset, so the
/// provided keys are overlaid onto the engine's own defaults — a plain serde
/// parse would reject a partial config.
pub(crate) fn edge_hnsw_config(
    config: &HnswConfig,
) -> Result<qdrant_edge::HnswIndexConfig, QqlError> {
    edge_hnsw_config_over(config, &qdrant_edge::HnswIndexConfig::default())
}

/// Overlay a plan optimizer patch onto an existing engine config.
///
/// `set_optimizers_config` replaces the whole blob, so a partial
/// `ALTER … WITH OPTIMIZERS (indexing_threshold = …)` must keep the other
/// keys already persisted on the shard.
pub(crate) fn overlay_optimizers(
    plan: &OptimizersConfig,
    base: &qdrant_edge::EdgeOptimizersConfig,
) -> Result<qdrant_edge::EdgeOptimizersConfig, QqlError> {
    let patch = edge_optimizers_config(plan)?;
    Ok(qdrant_edge::EdgeOptimizersConfig {
        deleted_threshold: patch.deleted_threshold.or(base.deleted_threshold),
        vacuum_min_vector_number: patch
            .vacuum_min_vector_number
            .or(base.vacuum_min_vector_number),
        default_segment_number: patch.default_segment_number.or(base.default_segment_number),
        max_segment_size: patch.max_segment_size.or(base.max_segment_size),
        indexing_threshold: patch.indexing_threshold.or(base.indexing_threshold),
        prevent_unoptimized: patch.prevent_unoptimized.or(base.prevent_unoptimized),
    })
}

/// Field-merge a plan HNSW config over `base`.
///
/// Used for per-vector overrides, `ALTER COLLECTION` (live shard config as
/// base), and plain create-time configs (engine defaults as base). Engine
/// `Memory` is not re-exported, so that one field is decoded from its keyword
/// via serde (`StrDeserializer`), not a `serde_json::Value`.
#[allow(deprecated)]
pub(crate) fn edge_hnsw_config_over(
    config: &HnswConfig,
    base: &qdrant_edge::HnswIndexConfig,
) -> Result<qdrant_edge::HnswIndexConfig, QqlError> {
    let mut merged = *base;
    if let Some(m) = config.m {
        merged.m = usize_config("hnsw.m", m)?;
    }
    if let Some(ef_construct) = config.ef_construct {
        merged.ef_construct = usize_config("hnsw.ef_construct", ef_construct)?;
    }
    if let Some(full_scan_threshold) = config.full_scan_threshold {
        merged.full_scan_threshold = usize_config("hnsw.full_scan_threshold", full_scan_threshold)?;
    }
    if let Some(max_indexing_threads) = config.max_indexing_threads {
        merged.max_indexing_threads =
            usize_config("hnsw.max_indexing_threads", max_indexing_threads)?;
    }
    if let Some(on_disk) = config.on_disk {
        merged.on_disk = Some(on_disk);
    }
    if let Some(payload_m) = config.payload_m {
        merged.payload_m = Some(usize_config("hnsw.payload_m", payload_m)?);
    }
    if let Some(inline_storage) = config.inline_storage {
        merged.inline_storage = Some(inline_storage);
    }
    if let Some(memory) = config.memory {
        merged.memory = Some(edge_memory(memory)?);
    }
    Ok(merged)
}

/// Lower a plan optimizer config into the qdrant-edge optimizer config type.
///
/// Shared by `CREATE COLLECTION` and the `ALTER COLLECTION` optimizer setter so
/// the two cannot drift. qdrant-edge deliberately excludes `memmap_threshold`
/// (deprecated upstream), `flush_interval_sec` (edge does not flush on a
/// timer), and `max_optimization_threads` (optimizations are manual); those
/// keys fail closed instead of being silently dropped.
pub(crate) fn edge_optimizers_config(
    config: &OptimizersConfig,
) -> Result<qdrant_edge::EdgeOptimizersConfig, QqlError> {
    for (key, present) in [
        ("memmap_threshold", config.memmap_threshold.is_some()),
        ("flush_interval_sec", config.flush_interval_sec.is_some()),
        (
            "max_optimization_threads",
            config.max_optimization_threads.is_some(),
        ),
    ] {
        if present {
            return Err(EdgeUnsupported::OptimizerKey
                .error()
                .with_field("config_key", key));
        }
    }
    Ok(qdrant_edge::EdgeOptimizersConfig {
        deleted_threshold: config.deleted_threshold,
        vacuum_min_vector_number: config
            .vacuum_min_vector_number
            .map(|value| usize_config("optimizers.vacuum_min_vector_number", value))
            .transpose()?,
        default_segment_number: config
            .default_segment_number
            .map(|value| usize_config("optimizers.default_segment_number", value))
            .transpose()?,
        max_segment_size: config
            .max_segment_size
            .map(|value| usize_config("optimizers.max_segment_size", value))
            .transpose()?,
        indexing_threshold: config
            .indexing_threshold
            .map(|value| usize_config("optimizers.indexing_threshold", value))
            .transpose()?,
        prevent_unoptimized: config.prevent_unoptimized,
    })
}
