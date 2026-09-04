//! Collection / index DDL converters: typed plan IR and legacy JSON → proto.
//!
//! Covers HNSW, optimizers, quantization (scalar / product / binary / turbo),
//! vector params (dense + sparse), collection params, and payload index params.

#![allow(deprecated)]

use qql_core::error::QqlError;

use crate::grpc::memory::{json_memory, memory_from_str, memory_to_proto};
use crate::qdrant_grpc::qdrant;

use super::common::{
    datatype_from_json, distance, json_bool, json_u64, option_bool, option_memory, option_string,
    option_u64,
};

pub(crate) fn hnsw_config_from_plan(cfg: &qql_plan::HnswConfig) -> qdrant::HnswConfigDiff {
    qdrant::HnswConfigDiff {
        m: cfg.m,
        ef_construct: cfg.ef_construct,
        full_scan_threshold: cfg.full_scan_threshold,
        max_indexing_threads: cfg.max_indexing_threads,
        on_disk: cfg.on_disk,
        payload_m: cfg.payload_m,
        memory: cfg.memory.map(memory_to_proto),
        ..Default::default()
    }
}

pub(crate) fn optimizers_config_from_plan(
    cfg: &qql_plan::OptimizersConfig,
) -> qdrant::OptimizersConfigDiff {
    let max_optimization_threads = cfg
        .max_optimization_threads
        .as_ref()
        .and_then(|v| v.as_u64())
        .map(|value| qdrant::MaxOptimizationThreads {
            variant: Some(qdrant::max_optimization_threads::Variant::Value(value)),
        })
        .or_else(|| {
            cfg.max_optimization_threads
                .as_ref()
                .and_then(|v| v.as_str())
                .filter(|s| s.eq_ignore_ascii_case("auto"))
                .map(|_| qdrant::MaxOptimizationThreads {
                    variant: Some(qdrant::max_optimization_threads::Variant::Setting(
                        qdrant::max_optimization_threads::Setting::Auto as i32,
                    )),
                })
        });

    qdrant::OptimizersConfigDiff {
        deleted_threshold: cfg.deleted_threshold,
        vacuum_min_vector_number: cfg.vacuum_min_vector_number,
        default_segment_number: cfg.default_segment_number,
        max_segment_size: cfg.max_segment_size,
        memmap_threshold: cfg.memmap_threshold,
        indexing_threshold: cfg.indexing_threshold,
        flush_interval_sec: cfg.flush_interval_sec,
        max_optimization_threads,
        prevent_unoptimized: cfg.prevent_unoptimized,
        ..Default::default()
    }
}

pub(crate) fn quantization_config_from_plan(
    cfg: &qql_plan::QuantizationConfig,
) -> Option<qdrant::QuantizationConfig> {
    match cfg {
        qql_plan::QuantizationConfig::Scalar { scalar } => Some(qdrant::QuantizationConfig {
            quantization: Some(qdrant::quantization_config::Quantization::Scalar(
                qdrant::ScalarQuantization {
                    r#type: qdrant::QuantizationType::Int8 as i32,
                    quantile: scalar.quantile.map(|v| v as f32),
                    always_ram: scalar.always_ram,
                    memory: scalar.memory.map(memory_to_proto),
                },
            )),
        }),
        qql_plan::QuantizationConfig::Product { product } => {
            let compression = match product.compression.to_ascii_lowercase().as_str() {
                "x8" => qdrant::CompressionRatio::X8,
                "x16" => qdrant::CompressionRatio::X16,
                "x32" => qdrant::CompressionRatio::X32,
                "x64" => qdrant::CompressionRatio::X64,
                _ => qdrant::CompressionRatio::X4,
            };
            Some(qdrant::QuantizationConfig {
                quantization: Some(qdrant::quantization_config::Quantization::Product(
                    qdrant::ProductQuantization {
                        compression: compression as i32,
                        always_ram: product.always_ram,
                        memory: product.memory.map(memory_to_proto),
                    },
                )),
            })
        }
        qql_plan::QuantizationConfig::Binary { binary } => {
            let encoding = binary.encoding.as_deref().map(|e| {
                let key = e.to_ascii_lowercase();
                match key.as_str() {
                    "two_bits" | "2" => qdrant::BinaryQuantizationEncoding::TwoBits as i32,
                    "one_and_half_bits" | "1.5" => {
                        qdrant::BinaryQuantizationEncoding::OneAndHalfBits as i32
                    }
                    _ => qdrant::BinaryQuantizationEncoding::OneBit as i32,
                }
            });
            let query_encoding = binary.query_encoding.as_deref().map(|_qe| {
                qdrant::BinaryQuantizationQueryEncoding {
                    variant: Some(
                        qdrant::binary_quantization_query_encoding::Variant::Setting(
                            qdrant::binary_quantization_query_encoding::Setting::Default as i32,
                        ),
                    ),
                }
            });
            Some(qdrant::QuantizationConfig {
                quantization: Some(qdrant::quantization_config::Quantization::Binary(
                    qdrant::BinaryQuantization {
                        always_ram: binary.always_ram,
                        encoding,
                        query_encoding,
                        memory: binary.memory.map(memory_to_proto),
                    },
                )),
            })
        }
        qql_plan::QuantizationConfig::Turbo { turbo } => {
            let bits =
                turbo
                    .bits
                    .as_deref()
                    .map(|label| match label.to_ascii_lowercase().as_str() {
                        "bits1_5" | "1.5" => qdrant::TurboQuantBitSize::Bits15 as i32,
                        "bits2" | "2" => qdrant::TurboQuantBitSize::Bits2 as i32,
                        "bits4" | "4" => qdrant::TurboQuantBitSize::Bits4 as i32,
                        _ => qdrant::TurboQuantBitSize::Bits1 as i32,
                    });
            Some(qdrant::QuantizationConfig {
                quantization: Some(qdrant::quantization_config::Quantization::Turboquant(
                    qdrant::TurboQuantization {
                        always_ram: turbo.always_ram,
                        bits,
                        memory: turbo.memory.map(memory_to_proto),
                    },
                )),
            })
        }
    }
}

// ── Legacy JSON-to-gRPC converters (still used for PATCH quantization,
//     sparse vector params, and backward compat) ───────────────────────

pub(crate) fn hnsw_config(value: &serde_json::Value) -> qdrant::HnswConfigDiff {
    qdrant::HnswConfigDiff {
        m: json_u64(value, "m"),
        ef_construct: json_u64(value, "ef_construct"),
        full_scan_threshold: json_u64(value, "full_scan_threshold"),
        max_indexing_threads: json_u64(value, "max_indexing_threads"),
        on_disk: json_bool(value, "on_disk"),
        payload_m: json_u64(value, "payload_m"),
        inline_storage: json_bool(value, "inline_storage"),
        memory: json_memory(value, "memory"),
    }
}

#[allow(dead_code)]
pub(crate) fn optimizers_config(value: &serde_json::Value) -> qdrant::OptimizersConfigDiff {
    let max_optimization_threads = value
        .get("max_optimization_threads")
        .and_then(|threads| {
            threads
                .as_u64()
                .map(|value| qdrant::MaxOptimizationThreads {
                    variant: Some(qdrant::max_optimization_threads::Variant::Value(value)),
                })
        })
        .or_else(|| {
            value
                .get("max_optimization_threads")
                .and_then(serde_json::Value::as_str)
                .filter(|value| value.eq_ignore_ascii_case("auto"))
                .map(|_| qdrant::MaxOptimizationThreads {
                    variant: Some(qdrant::max_optimization_threads::Variant::Setting(
                        qdrant::max_optimization_threads::Setting::Auto as i32,
                    )),
                })
        });

    qdrant::OptimizersConfigDiff {
        deleted_threshold: value
            .get("deleted_threshold")
            .and_then(serde_json::Value::as_f64),
        vacuum_min_vector_number: json_u64(value, "vacuum_min_vector_number"),
        default_segment_number: json_u64(value, "default_segment_number"),
        max_segment_size: json_u64(value, "max_segment_size"),
        memmap_threshold: json_u64(value, "memmap_threshold"),
        indexing_threshold: json_u64(value, "indexing_threshold"),
        flush_interval_sec: json_u64(value, "flush_interval_sec"),
        deprecated_max_optimization_threads: json_u64(value, "deprecated_max_optimization_threads"),
        max_optimization_threads,
        prevent_unoptimized: json_bool(value, "prevent_unoptimized"),
    }
}

pub(crate) fn scalar_quantization(value: &serde_json::Value) -> qdrant::ScalarQuantization {
    qdrant::ScalarQuantization {
        r#type: qdrant::QuantizationType::Int8 as i32,
        quantile: value
            .get("quantile")
            .and_then(serde_json::Value::as_f64)
            .map(|value| value as f32),
        always_ram: json_bool(value, "always_ram"),
        memory: json_memory(value, "memory"),
    }
}

pub(crate) fn product_quantization(value: &serde_json::Value) -> qdrant::ProductQuantization {
    let compression = match value
        .get("compression")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("x4")
        .to_ascii_lowercase()
        .as_str()
    {
        "x8" => qdrant::CompressionRatio::X8,
        "x16" => qdrant::CompressionRatio::X16,
        "x32" => qdrant::CompressionRatio::X32,
        "x64" => qdrant::CompressionRatio::X64,
        _ => qdrant::CompressionRatio::X4,
    };
    qdrant::ProductQuantization {
        compression: compression as i32,
        always_ram: json_bool(value, "always_ram"),
        memory: json_memory(value, "memory"),
    }
}

pub(crate) fn binary_quantization(value: &serde_json::Value) -> qdrant::BinaryQuantization {
    let encoding = value.get("encoding").and_then(|enc| {
        // Accept string aliases and numeric shorthand (1 / 2 / 1.5).
        let key = if let Some(s) = enc.as_str() {
            s.to_ascii_lowercase()
        } else {
            let n = enc.as_f64()?;
            if (n - 1.5).abs() < f64::EPSILON {
                "1.5".into()
            } else if (n - 2.0).abs() < f64::EPSILON {
                "2".into()
            } else {
                "1".into()
            }
        };
        Some(match key.as_str() {
            "twobits" | "two_bits" | "2" | "twobitsencoding" => {
                qdrant::BinaryQuantizationEncoding::TwoBits as i32
            }
            "oneandhalfbits" | "one_and_half_bits" | "1.5" => {
                qdrant::BinaryQuantizationEncoding::OneAndHalfBits as i32
            }
            _ => qdrant::BinaryQuantizationEncoding::OneBit as i32,
        })
    });
    let query_encoding = value
        .get("query_encoding")
        .and_then(serde_json::Value::as_str)
        .map(|q| {
            let setting = match q.to_ascii_lowercase().as_str() {
                "binary" => qdrant::binary_quantization_query_encoding::Setting::Binary,
                "scalar4bits" | "scalar4" => {
                    qdrant::binary_quantization_query_encoding::Setting::Scalar4Bits
                }
                "scalar8bits" | "scalar8" => {
                    qdrant::binary_quantization_query_encoding::Setting::Scalar8Bits
                }
                _ => qdrant::binary_quantization_query_encoding::Setting::Default,
            };
            qdrant::BinaryQuantizationQueryEncoding {
                variant: Some(
                    qdrant::binary_quantization_query_encoding::Variant::Setting(setting as i32),
                ),
            }
        });
    qdrant::BinaryQuantization {
        always_ram: json_bool(value, "always_ram"),
        encoding,
        query_encoding,
        memory: json_memory(value, "memory"),
    }
}

pub(crate) fn turbo_quantization(value: &serde_json::Value) -> qdrant::TurboQuantization {
    let bits = value
        .get("turbo_bits")
        .or_else(|| value.get("bits"))
        .and_then(|v| {
            if let Some(bits) = v.as_f64() {
                return Some(if (bits - 1.5).abs() < f64::EPSILON {
                    qdrant::TurboQuantBitSize::Bits15 as i32
                } else if (bits - 2.0).abs() < f64::EPSILON {
                    qdrant::TurboQuantBitSize::Bits2 as i32
                } else if (bits - 4.0).abs() < f64::EPSILON {
                    qdrant::TurboQuantBitSize::Bits4 as i32
                } else {
                    qdrant::TurboQuantBitSize::Bits1 as i32
                });
            }
            // OpenAPI string enum: bits1 | bits1_5 | bits2 | bits4
            v.as_str()
                .map(|label| match label.to_ascii_lowercase().as_str() {
                    "bits1_5" | "1.5" => qdrant::TurboQuantBitSize::Bits15 as i32,
                    "bits2" | "2" => qdrant::TurboQuantBitSize::Bits2 as i32,
                    "bits4" | "4" => qdrant::TurboQuantBitSize::Bits4 as i32,
                    _ => qdrant::TurboQuantBitSize::Bits1 as i32,
                })
        });
    qdrant::TurboQuantization {
        always_ram: json_bool(value, "always_ram"),
        bits,
        memory: json_memory(value, "memory"),
    }
}

pub(crate) fn quantization_config(value: &serde_json::Value) -> Option<qdrant::QuantizationConfig> {
    let value = value.get("quantization_config").unwrap_or(value);
    // Accept nested OpenAPI `{ "scalar": {…} }` and flat IR `{ "type": "scalar", … }`.
    let (kind, payload) = {
        let obj = value.as_object()?;
        if let Some(inner) = obj.get("scalar") {
            ("scalar", inner)
        } else if let Some(inner) = obj.get("product") {
            ("product", inner)
        } else if let Some(inner) = obj.get("binary") {
            ("binary", inner)
        } else if let Some(inner) = obj.get("turbo").or_else(|| obj.get("turboquant")) {
            ("turbo", inner)
        } else {
            let kind = obj.get("type").and_then(|v| v.as_str()).unwrap_or("");
            (kind, value)
        }
    };
    let quantization = match kind.to_ascii_lowercase().as_str() {
        // Nested OpenAPI uses ScalarType `int8`; flat IR uses `scalar`.
        "scalar" | "int8" => {
            qdrant::quantization_config::Quantization::Scalar(scalar_quantization(payload))
        }
        "product" => {
            qdrant::quantization_config::Quantization::Product(product_quantization(payload))
        }
        "binary" => qdrant::quantization_config::Quantization::Binary(binary_quantization(payload)),
        "turbo" | "turboquant" => {
            qdrant::quantization_config::Quantization::Turboquant(turbo_quantization(payload))
        }
        _ => return None,
    };
    Some(qdrant::QuantizationConfig {
        quantization: Some(quantization),
    })
}

pub(crate) fn quantization_config_diff(
    value: &serde_json::Value,
) -> Option<qdrant::QuantizationConfigDiff> {
    if value.as_str() == Some("Disabled")
        || value.get("disabled").and_then(serde_json::Value::as_bool) == Some(true)
    {
        return Some(qdrant::QuantizationConfigDiff {
            quantization: Some(qdrant::quantization_config_diff::Quantization::Disabled(
                qdrant::Disabled {},
            )),
        });
    }
    let config = quantization_config(value)?;
    let quantization = match config.quantization? {
        qdrant::quantization_config::Quantization::Scalar(value) => {
            qdrant::quantization_config_diff::Quantization::Scalar(value)
        }
        qdrant::quantization_config::Quantization::Product(value) => {
            qdrant::quantization_config_diff::Quantization::Product(value)
        }
        qdrant::quantization_config::Quantization::Binary(value) => {
            qdrant::quantization_config_diff::Quantization::Binary(value)
        }
        qdrant::quantization_config::Quantization::Turboquant(value) => {
            qdrant::quantization_config_diff::Quantization::Turboquant(value)
        }
    };
    Some(qdrant::QuantizationConfigDiff {
        quantization: Some(quantization),
    })
}

pub(crate) fn vector_params(value: &serde_json::Value) -> qdrant::VectorParams {
    qdrant::VectorParams {
        size: json_u64(value, "size").unwrap_or(0),
        distance: distance(value),
        hnsw_config: value.get("hnsw_config").map(hnsw_config),
        quantization_config: value
            .get("quantization_config")
            .and_then(quantization_config),
        on_disk: json_bool(value, "on_disk"),
        datatype: datatype_from_json(value),
        memory: json_memory(value, "memory"),
        multivector_config: value
            .get("multivector_config")
            .map(|_| qdrant::MultiVectorConfig {
                comparator: qdrant::MultiVectorComparator::MaxSim as i32,
            }),
    }
}

#[allow(dead_code)]
pub(crate) fn vector_params_diff(value: &serde_json::Value) -> qdrant::VectorParamsDiff {
    qdrant::VectorParamsDiff {
        hnsw_config: value.get("hnsw_config").map(hnsw_config),
        quantization_config: value
            .get("quantization_config")
            .and_then(quantization_config_diff),
        on_disk: json_bool(value, "on_disk"),
        memory: json_memory(value, "memory"),
    }
}

#[allow(dead_code)]
pub(crate) fn vectors_config_diff(value: &serde_json::Value) -> Option<qdrant::VectorsConfigDiff> {
    let object = value.as_object()?;
    let config = if object.contains_key("on_disk")
        || object.contains_key("hnsw_config")
        || object.contains_key("quantization_config")
    {
        qdrant::vectors_config_diff::Config::Params(vector_params_diff(value))
    } else {
        let map = object
            .iter()
            .map(|(name, value)| (name.clone(), vector_params_diff(value)))
            .collect();
        qdrant::vectors_config_diff::Config::ParamsMap(qdrant::VectorParamsDiffMap { map })
    };
    Some(qdrant::VectorsConfigDiff {
        config: Some(config),
    })
}

pub(crate) fn sparse_vector_params(value: &serde_json::Value) -> qdrant::SparseVectorParams {
    let index = value.get("index").map(|idx| qdrant::SparseIndexConfig {
        full_scan_threshold: json_u64(idx, "full_scan_threshold"),
        on_disk: json_bool(idx, "on_disk"),
        datatype: datatype_from_json(idx),
        memory: json_memory(idx, "memory"),
    });
    qdrant::SparseVectorParams {
        index,
        modifier: value
            .get("modifier")
            .and_then(serde_json::Value::as_str)
            .map(|modifier| match modifier.to_ascii_lowercase().as_str() {
                "idf" => qdrant::Modifier::Idf as i32,
                _ => qdrant::Modifier::None as i32,
            }),
    }
}

pub(crate) fn collection_params_diff(value: &serde_json::Value) -> qdrant::CollectionParamsDiff {
    qdrant::CollectionParamsDiff {
        replication_factor: json_u64(value, "replication_factor").map(|n| n as u32),
        write_consistency_factor: json_u64(value, "write_consistency_factor").map(|n| n as u32),
        on_disk_payload: json_bool(value, "on_disk_payload"),
        read_fan_out_factor: json_u64(value, "read_fan_out_factor").map(|n| n as u32),
        read_fan_out_delay_ms: json_u64(value, "read_fan_out_delay_ms"),
        payload: value
            .get("payload")
            .and_then(|p| p.get("memory"))
            .and_then(serde_json::Value::as_str)
            .and_then(memory_from_str)
            .map(|memory| qdrant::PayloadStorageParams {
                memory: Some(memory),
            }),
    }
}

pub(crate) fn payload_index_params(
    field_schema: &str,
    options: &serde_json::Map<String, serde_json::Value>,
) -> Result<qdrant::PayloadIndexParams, QqlError> {
    use qdrant::payload_index_params::IndexParams;

    let field_schema = field_schema.to_ascii_lowercase();
    let index_params = match field_schema.as_str() {
        "keyword" => IndexParams::KeywordIndexParams(qdrant::KeywordIndexParams {
            is_tenant: option_bool(options, "is_tenant"),
            on_disk: option_bool(options, "on_disk"),
            enable_hnsw: option_bool(options, "enable_hnsw"),
            prefix: option_bool(options, "prefix").map(|_| qdrant::KeywordPrefixParams {}),
            memory: option_memory(options, "memory"),
        }),
        "integer" => IndexParams::IntegerIndexParams(qdrant::IntegerIndexParams {
            lookup: option_bool(options, "lookup"),
            range: option_bool(options, "range"),
            is_principal: option_bool(options, "is_principal"),
            on_disk: option_bool(options, "on_disk"),
            enable_hnsw: option_bool(options, "enable_hnsw"),
            memory: option_memory(options, "memory"),
        }),
        "float" => IndexParams::FloatIndexParams(qdrant::FloatIndexParams {
            on_disk: option_bool(options, "on_disk"),
            is_principal: option_bool(options, "is_principal"),
            enable_hnsw: option_bool(options, "enable_hnsw"),
            memory: option_memory(options, "memory"),
        }),
        "geo" => IndexParams::GeoIndexParams(qdrant::GeoIndexParams {
            on_disk: option_bool(options, "on_disk"),
            enable_hnsw: option_bool(options, "enable_hnsw"),
            memory: option_memory(options, "memory"),
        }),
        "bool" => IndexParams::BoolIndexParams(qdrant::BoolIndexParams {
            on_disk: option_bool(options, "on_disk"),
            enable_hnsw: option_bool(options, "enable_hnsw"),
            memory: option_memory(options, "memory"),
        }),
        "datetime" => IndexParams::DatetimeIndexParams(qdrant::DatetimeIndexParams {
            on_disk: option_bool(options, "on_disk"),
            is_principal: option_bool(options, "is_principal"),
            enable_hnsw: option_bool(options, "enable_hnsw"),
            memory: option_memory(options, "memory"),
        }),
        "uuid" => IndexParams::UuidIndexParams(qdrant::UuidIndexParams {
            is_tenant: option_bool(options, "is_tenant"),
            on_disk: option_bool(options, "on_disk"),
            enable_hnsw: option_bool(options, "enable_hnsw"),
            memory: option_memory(options, "memory"),
        }),
        "text" => IndexParams::TextIndexParams(text_index_params(options)?),
        other => {
            return Err(QqlError::validation(
                "QQL-GRPC-INDEX",
                format!("unsupported field index type: {other}"),
                None,
            ));
        }
    };

    Ok(qdrant::PayloadIndexParams {
        index_params: Some(index_params),
    })
}

pub(crate) fn text_index_params(
    options: &serde_json::Map<String, serde_json::Value>,
) -> Result<qdrant::TextIndexParams, QqlError> {
    let tokenizer = match option_string(options, "tokenizer")
        .unwrap_or_else(|| "word".to_string())
        .to_ascii_lowercase()
        .as_str()
    {
        "prefix" => qdrant::TokenizerType::Prefix,
        "whitespace" => qdrant::TokenizerType::Whitespace,
        "multilingual" => qdrant::TokenizerType::Multilingual,
        "word" => qdrant::TokenizerType::Word,
        other => {
            return Err(QqlError::validation(
                "QQL-GRPC-INDEX",
                format!("unsupported text tokenizer: {other}"),
                None,
            ));
        }
    };
    let stopwords = options
        .get("stopwords")
        .and_then(serde_json::Value::as_array)
        .map(|words| qdrant::StopwordsSet {
            languages: Vec::new(),
            custom: words
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
                .collect(),
        });
    Ok(qdrant::TextIndexParams {
        tokenizer: tokenizer as i32,
        lowercase: option_bool(options, "lowercase"),
        min_token_len: option_u64(options, "min_token_len"),
        max_token_len: option_u64(options, "max_token_len"),
        on_disk: option_bool(options, "on_disk"),
        stopwords,
        phrase_matching: option_bool(options, "phrase_matching"),
        stemmer: None,
        ascii_folding: option_bool(options, "ascii_folding"),
        enable_hnsw: option_bool(options, "enable_hnsw"),
        memory: option_memory(options, "memory"),
    })
}
