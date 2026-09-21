//! Runtime config lowering: AST HNSW/optimizer/quantization to plan IR.
//!
//! Pure move from `ddl.rs` (size hygiene split).

use crate::types::*;
use alloc::format;
use alloc::string::ToString;

/// Lower an AST HNSW config into the typed plan `HnswConfig` IR.
pub fn lower_hnsw_config(config: &qql_core::ast::HnswRuntimeConfig) -> HnswConfig {
    HnswConfig {
        m: config.m,
        ef_construct: config.ef_construct,
        full_scan_threshold: config.full_scan_threshold,
        max_indexing_threads: config.max_indexing_threads,
        on_disk: config.on_disk,
        payload_m: config.payload_m,
        inline_storage: config.inline_storage,
        memory: config.memory,
    }
}

/// Lower an AST optimizer config into the typed plan `OptimizersConfig` IR.
pub fn lower_optimizers_config(
    config: &qql_core::ast::OptimizersRuntimeConfig,
) -> OptimizersConfig {
    OptimizersConfig {
        deleted_threshold: config.deleted_threshold,
        vacuum_min_vector_number: config.vacuum_min_vector_number,
        default_segment_number: config.default_segment_number,
        max_segment_size: config.max_segment_size,
        memmap_threshold: config.memmap_threshold,
        indexing_threshold: config.indexing_threshold,
        flush_interval_sec: config.flush_interval_sec,
        max_optimization_threads: config.max_optimization_threads.as_ref().map(|threads| {
            if threads.auto_ {
                MaxOptimizationThreads::Auto
            } else {
                MaxOptimizationThreads::Threads(threads.value)
            }
        }),
        prevent_unoptimized: config.prevent_unoptimized,
    }
}

/// Lower an AST quantization config into the typed plan `QuantizationConfig`.
///
/// `compression` defaults to `x4` (the OpenAPI enum has no empty string) and is
/// normalized to lowercase, mirroring the REST projection this replaces.
pub fn lower_quantization_config(config: &qql_core::ast::QuantizationConfig) -> QuantizationConfig {
    match config.qtype {
        qql_core::ast::QuantizationType::Scalar => QuantizationConfig::Scalar {
            scalar: ScalarQuantization {
                qtype: "int8".into(),
                quantile: config.quantile,
                always_ram: Some(config.always_ram),
                memory: config.memory,
            },
        },
        qql_core::ast::QuantizationType::Product => QuantizationConfig::Product {
            product: ProductQuantization {
                compression: config
                    .compression
                    .as_deref()
                    .map(|c| c.to_ascii_lowercase())
                    .unwrap_or_else(|| "x4".into()),
                always_ram: Some(config.always_ram),
                memory: config.memory,
            },
        },
        qql_core::ast::QuantizationType::Binary => QuantizationConfig::Binary {
            binary: BinaryQuantization {
                always_ram: Some(config.always_ram),
                encoding: config.encoding.clone(),
                query_encoding: config.query_encoding.clone(),
                memory: config.memory,
            },
        },
        qql_core::ast::QuantizationType::Turbo => QuantizationConfig::Turbo {
            turbo: TurboQuantization {
                bits: turbo_bits_label(config.bits),
                always_ram: Some(config.always_ram),
                memory: config.memory,
            },
        },
    }
}

/// Map numeric turbo bits onto the OpenAPI `TurboQuantBitSize` label.
///
/// Unknown values still emit a best-effort `bits<value>` label so the backend
/// can reject them clearly instead of silently mislabeling as `bits1`.
pub(crate) fn turbo_bits_label(bits: Option<f64>) -> Option<String> {
    let bits = bits?;
    let label = if (bits - 1.5).abs() < f64::EPSILON {
        "bits1_5"
    } else if (bits - 2.0).abs() < f64::EPSILON {
        "bits2"
    } else if (bits - 4.0).abs() < f64::EPSILON {
        "bits4"
    } else if (bits - 1.0).abs() < f64::EPSILON {
        "bits1"
    } else {
        return Some(format!("bits{bits}"));
    };
    Some(label.to_string())
}
