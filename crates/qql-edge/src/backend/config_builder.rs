//! Edge collection configuration builder passing through dense/sparse vector configs, HNSW, quantization, and optimizers.

use qdrant_edge::EdgeConfigBuilder;

use qql_core::ast::VectorDistance;
use qql_core::error::QqlError;
use qql_plan::{
    CreateCollectionRequest, DenseVectorParams, DenseVectorsConfig, HnswConfig, OptimizersConfig,
};

use super::unsupported::EdgeUnsupported;

pub(crate) fn build_edge_config(
    req: &CreateCollectionRequest,
    on_disk_payload: bool,
) -> Result<qdrant_edge::EdgeConfig, QqlError> {
    // A create-time `WITH PARAMS (on_disk_payload = …)` overrides the
    // executor-level default for this collection; every other param key is
    // rejected before we get here (see `unsupported::reject_collection_params`).
    let on_disk_payload = req
        .params
        .as_ref()
        .and_then(|params| params.on_disk_payload)
        .unwrap_or(on_disk_payload);
    let mut builder = EdgeConfigBuilder::new().on_disk_payload(on_disk_payload);

    match &req.vectors {
        Some(DenseVectorsConfig::Single(params)) => {
            builder = builder.vector(String::new(), edge_vector_params("<default>", params)?);
        }
        Some(DenseVectorsConfig::Named(map)) => {
            for (name, params) in map {
                builder = builder.vector(name.clone(), edge_vector_params(name, params)?);
            }
        }
        None => {}
    }

    if let Some(ref sparse) = req.sparse_vectors {
        for (name, params) in sparse {
            builder = builder.sparse_vector(
                name.clone(),
                qdrant_edge::EdgeSparseVectorParams {
                    full_scan_threshold: params
                        .index
                        .as_ref()
                        .and_then(|index| index.full_scan_threshold)
                        .map(|n| n as usize),
                    on_disk: params.index.as_ref().and_then(|index| index.on_disk),
                    modifier: Some(match params.modifier {
                        qql_plan::SparseModifier::Idf => qdrant_edge::Modifier::Idf,
                        qql_plan::SparseModifier::None => qdrant_edge::Modifier::None,
                    }),
                    // qdrant-edge stores sparse weights as f32 regardless of the
                    // requested datatype; leaving this unset matches its default.
                    datatype: None,
                },
            );
        }
    }

    if let Some(ref hc) = req.hnsw_config {
        builder = builder.hnsw_config(edge_hnsw_config(hc)?);
    }

    if let Some(ref qc) = req.quantization_config {
        let val = serde_json::to_value(qc).map_err(|e| edge_config_error(e.to_string()))?;
        let quant =
            serde_json::from_value::<qdrant_edge::QuantizationConfig>(val).map_err(|error| {
                edge_config_error(format!("invalid quantization configuration: {error}"))
            })?;
        builder = builder.quantization_config(quant);
    }

    if let Some(ref oc) = req.optimizers_config {
        builder = builder.optimizers(edge_optimizers_config(oc)?);
    }

    Ok(builder.build())
}

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
    let mut merged = serde_json::to_value(qdrant_edge::HnswIndexConfig::default())
        .map_err(|e| edge_config_error(e.to_string()))?;
    let provided = serde_json::to_value(config).map_err(|e| edge_config_error(e.to_string()))?;
    if let (Some(base), serde_json::Value::Object(provided)) = (merged.as_object_mut(), provided) {
        base.extend(provided);
    }
    serde_json::from_value(merged)
        .map_err(|error| edge_config_error(format!("invalid HNSW configuration: {error}")))
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
    let val = serde_json::to_value(config).map_err(|e| edge_config_error(e.to_string()))?;
    serde_json::from_value::<qdrant_edge::EdgeOptimizersConfig>(val)
        .map_err(|error| edge_config_error(format!("invalid optimizer configuration: {error}")))
}

fn edge_vector_params(
    name: &str,
    params: &DenseVectorParams,
) -> Result<qdrant_edge::EdgeVectorParams, QqlError> {
    let size = usize::try_from(params.size).map_err(|error| {
        edge_config_error(format!("vector '{name}' size is too large: {error}"))
    })?;
    let distance = match params.distance {
        VectorDistance::Cosine => qdrant_edge::Distance::Cosine,
        VectorDistance::Dot => qdrant_edge::Distance::Dot,
        VectorDistance::Euclid => qdrant_edge::Distance::Euclid,
        VectorDistance::Manhattan => qdrant_edge::Distance::Manhattan,
    };
    let multivector_config = params
        .multivector_config
        .map(|_| qdrant_edge::MultiVectorConfig {
            comparator: qdrant_edge::MultiVectorComparator::MaxSim,
        });
    Ok(qdrant_edge::EdgeVectorParams {
        size,
        distance,
        multivector_config,
        // qql-edge currently passes only the vector identity; qdrant-edge's
        // `EdgeVectorParams` also carries on_disk / datatype / per-vector
        // hnsw_config / quantization_config, which the planner can express —
        // see the capability audit in the crate report.
        datatype: None,
        hnsw_config: None,
        quantization_config: None,
        on_disk: None,
    })
}

fn edge_config_error(message: impl Into<String>) -> QqlError {
    QqlError::execution("QQL-EDGE-CONFIG", message.into(), None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn partial_hnsw(m: Option<u64>) -> HnswConfig {
        HnswConfig {
            m,
            ef_construct: None,
            full_scan_threshold: None,
            max_indexing_threads: None,
            on_disk: None,
            payload_m: None,
            inline_storage: None,
            memory: None,
        }
    }

    fn empty_optimizers() -> OptimizersConfig {
        OptimizersConfig {
            deleted_threshold: None,
            vacuum_min_vector_number: None,
            default_segment_number: None,
            max_segment_size: None,
            memmap_threshold: None,
            indexing_threshold: None,
            flush_interval_sec: None,
            max_optimization_threads: None,
            prevent_unoptimized: None,
        }
    }

    /// A partial `WITH HNSW (m = …)` must merge over engine defaults instead of
    /// failing on the engine's required fields.
    #[test]
    fn partial_hnsw_config_merges_engine_defaults() {
        let edge = edge_hnsw_config(&partial_hnsw(Some(32))).expect("partial hnsw");
        assert_eq!(edge.m, 32);
        assert_eq!(
            edge.ef_construct,
            qdrant_edge::HnswIndexConfig::default().ef_construct
        );
    }

    #[test]
    fn optimizer_partial_config_converts() {
        let mut config = empty_optimizers();
        config.indexing_threshold = Some(500);
        let edge = edge_optimizers_config(&config).expect("optimizer");
        assert_eq!(edge.indexing_threshold, Some(500));
    }

    /// Keys qdrant-edge deliberately excludes fail closed rather than being
    /// silently dropped.
    #[test]
    fn optimizer_excluded_keys_fail_closed() {
        let mut config = empty_optimizers();
        config.flush_interval_sec = Some(5);
        let error = edge_optimizers_config(&config).expect_err("flush interval");
        assert_eq!(error.code, "QQL-EDGE-UNSUPPORTED-OPTIMIZER-KEY");
        assert_eq!(error.field("config_key"), Some("flush_interval_sec"));

        let mut config = empty_optimizers();
        config.memmap_threshold = Some(100);
        assert_eq!(
            edge_optimizers_config(&config).unwrap_err().code,
            "QQL-EDGE-UNSUPPORTED-OPTIMIZER-KEY"
        );

        let mut config = empty_optimizers();
        config.max_optimization_threads = Some(qql_plan::MaxOptimizationThreads::Threads(4));
        assert_eq!(
            edge_optimizers_config(&config).unwrap_err().code,
            "QQL-EDGE-UNSUPPORTED-OPTIMIZER-KEY"
        );
    }
}
