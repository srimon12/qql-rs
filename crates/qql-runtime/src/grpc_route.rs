use crate::qdrant_grpc::qdrant;
use qql_core::error::QqlError;
use qql_plan::types::{
    FilterClause, FilterCompound, FilterExpression, MatchValue, PayloadSelectorReq,
    VectorSelectorReq, WithLookupValue,
};
use qql_plan::{PlanPointId, PlanPointVectors, PlanQueryInput, PlanVectorValue};

fn shard_key_selector(key: &Option<String>) -> Option<qdrant::ShardKeySelector> {
    key.as_ref().map(|k| qdrant::ShardKeySelector {
        shard_keys: vec![qdrant::ShardKey {
            key: Some(qdrant::shard_key::Key::Keyword(k.clone())),
        }],
        ..Default::default()
    })
}

fn json_u64(value: &serde_json::Value, key: &str) -> Option<u64> {
    value.get(key).and_then(serde_json::Value::as_u64)
}

fn json_bool(value: &serde_json::Value, key: &str) -> Option<bool> {
    value.get(key).and_then(serde_json::Value::as_bool)
}

// ── Typed plan-to-gRPC converters (no JSON intermediate) ─────────────

pub(crate) fn hnsw_config_from_plan(cfg: &qql_plan::HnswConfig) -> qdrant::HnswConfigDiff {
    qdrant::HnswConfigDiff {
        m: cfg.m,
        ef_construct: cfg.ef_construct,
        full_scan_threshold: cfg.full_scan_threshold,
        max_indexing_threads: cfg.max_indexing_threads,
        on_disk: cfg.on_disk,
        payload_m: cfg.payload_m,
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

fn scalar_quantization(value: &serde_json::Value) -> qdrant::ScalarQuantization {
    qdrant::ScalarQuantization {
        r#type: qdrant::QuantizationType::Int8 as i32,
        quantile: value
            .get("quantile")
            .and_then(serde_json::Value::as_f64)
            .map(|value| value as f32),
        always_ram: json_bool(value, "always_ram"),
    }
}

fn product_quantization(value: &serde_json::Value) -> qdrant::ProductQuantization {
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
    }
}

fn binary_quantization(value: &serde_json::Value) -> qdrant::BinaryQuantization {
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
    }
}

fn turbo_quantization(value: &serde_json::Value) -> qdrant::TurboQuantization {
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

fn quantization_config_diff(value: &serde_json::Value) -> Option<qdrant::QuantizationConfigDiff> {
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

fn distance(value: &serde_json::Value) -> i32 {
    match value
        .get("distance")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("Cosine")
        .to_ascii_lowercase()
        .as_str()
    {
        "euclid" => qdrant::Distance::Euclid as i32,
        "dot" => qdrant::Distance::Dot as i32,
        "manhattan" => qdrant::Distance::Manhattan as i32,
        _ => qdrant::Distance::Cosine as i32,
    }
}

/// Map OpenAPI / JSON datatype strings onto the protobuf `Datatype` enum.
fn datatype_from_json(value: &serde_json::Value) -> Option<i32> {
    value
        .get("datatype")
        .and_then(serde_json::Value::as_str)
        .map(|dt| match dt.to_ascii_lowercase().as_str() {
            "float32" | "f32" => qdrant::Datatype::Float32 as i32,
            "uint8" | "u8" => qdrant::Datatype::Uint8 as i32,
            "float16" | "f16" => qdrant::Datatype::Float16 as i32,
            _ => qdrant::Datatype::Default as i32,
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
        multivector_config: value
            .get("multivector_config")
            .map(|_| qdrant::MultiVectorConfig {
                comparator: qdrant::MultiVectorComparator::MaxSim as i32,
            }),
    }
}

#[allow(dead_code)]
fn vector_params_diff(value: &serde_json::Value) -> qdrant::VectorParamsDiff {
    qdrant::VectorParamsDiff {
        hnsw_config: value.get("hnsw_config").map(hnsw_config),
        quantization_config: value
            .get("quantization_config")
            .and_then(quantization_config_diff),
        on_disk: json_bool(value, "on_disk"),
    }
}

#[allow(dead_code)]
fn vectors_config_diff(value: &serde_json::Value) -> Option<qdrant::VectorsConfigDiff> {
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
    }
}

fn option_bool(options: &serde_json::Map<String, serde_json::Value>, key: &str) -> Option<bool> {
    options.get(key).and_then(serde_json::Value::as_bool)
}

fn option_u64(options: &serde_json::Map<String, serde_json::Value>, key: &str) -> Option<u64> {
    options.get(key).and_then(serde_json::Value::as_u64)
}

fn option_string(
    options: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Option<String> {
    options
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
}

fn payload_index_params(
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
        }),
        "integer" => IndexParams::IntegerIndexParams(qdrant::IntegerIndexParams {
            lookup: option_bool(options, "lookup"),
            range: option_bool(options, "range"),
            is_principal: option_bool(options, "is_principal"),
            on_disk: option_bool(options, "on_disk"),
            enable_hnsw: option_bool(options, "enable_hnsw"),
        }),
        "float" => IndexParams::FloatIndexParams(qdrant::FloatIndexParams {
            on_disk: option_bool(options, "on_disk"),
            is_principal: option_bool(options, "is_principal"),
            enable_hnsw: option_bool(options, "enable_hnsw"),
        }),
        "geo" => IndexParams::GeoIndexParams(qdrant::GeoIndexParams {
            on_disk: option_bool(options, "on_disk"),
            enable_hnsw: option_bool(options, "enable_hnsw"),
        }),
        "bool" => IndexParams::BoolIndexParams(qdrant::BoolIndexParams {
            on_disk: option_bool(options, "on_disk"),
            enable_hnsw: option_bool(options, "enable_hnsw"),
        }),
        "datetime" => IndexParams::DatetimeIndexParams(qdrant::DatetimeIndexParams {
            on_disk: option_bool(options, "on_disk"),
            is_principal: option_bool(options, "is_principal"),
            enable_hnsw: option_bool(options, "enable_hnsw"),
        }),
        "uuid" => IndexParams::UuidIndexParams(qdrant::UuidIndexParams {
            is_tenant: option_bool(options, "is_tenant"),
            on_disk: option_bool(options, "on_disk"),
            enable_hnsw: option_bool(options, "enable_hnsw"),
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

fn text_index_params(
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
    })
}

/// Build a REST-shaped mutation envelope from a gRPC `PointsOperationResponse`.
fn mutation_response_from(resp: qdrant::PointsOperationResponse) -> serde_json::Value {
    let result = resp
        .result
        .map(update_result_to_json)
        .unwrap_or_else(|| serde_json::json!({ "status": "completed" }));
    serde_json::json!({
        "result": result,
        "status": "ok",
        "time": resp.time,
    })
}

/// REST-shaped envelope for collection-level mutations (create/update/drop).
fn collection_mutation_response(resp: qdrant::CollectionOperationResponse) -> serde_json::Value {
    serde_json::json!({
        "result": resp.result,
        "status": "ok",
        "time": resp.time,
    })
}

/// Fallback when the gRPC response type carries no timing (shard-key ops).
fn mutation_response_ok() -> serde_json::Value {
    serde_json::json!({
        "result": { "status": "completed" },
        "status": "ok",
        "time": 0.0_f64,
    })
}

/// Dispatch a [`PlannedOperation`] directly to gRPC — **no Route, no JSON**.
///
/// This is the fast path for gRPC backends.  It matches each
/// `PlannedOperation` variant and builds the corresponding tonic
/// request from the already-typed fields.  There is no intermediate
/// REST `Route` projection and no JSON serialisation/deserialisation.
pub async fn execute_planned_grpc(
    client: &crate::grpc::GrpcQdrant,
    op: &qql_plan::PlannedOperation,
) -> Result<serde_json::Value, QqlError> {
    use qql_plan::PlannedOperation;
    match op {
        PlannedOperation::Query {
            collection,
            request,
        } => {
            let grpc_req = to_query_points(request, collection)?;
            let resp = client
                .query(grpc_req)
                .await
                .map_err(|e| QqlError::backend("QQL-GRPC", format!("query: {e}"), None))?;
            Ok(serde_json::json!({
                "result": {
                    "points": resp.result.into_iter().map(scored_point_to_json).collect::<Vec<_>>()
                },
                "status": "ok",
                "time": resp.time,
            }))
        }
        PlannedOperation::QueryGroups {
            collection,
            request,
        } => {
            let grpc_req = to_query_groups(request, collection)?;
            let resp = client
                .query_groups(grpc_req)
                .await
                .map_err(|e| QqlError::backend("QQL-GRPC", format!("query_groups: {e}"), None))?;
            Ok(serde_json::json!({
                "result": groups_result_to_json(resp.result.ok_or_else(|| QqlError::backend(
                    "QQL-GRPC", "missing groups result", None,
                ))?),
                "status": "ok",
                "time": resp.time,
            }))
        }
        PlannedOperation::GetPoints {
            collection,
            request,
        } => {
            let grpc_req = qdrant::GetPoints {
                collection_name: collection.clone(),
                ids: request.ids.iter().map(to_point_id).collect(),
                with_payload: request.with_payload.as_ref().map(to_payload_selector),
                with_vectors: request.with_vector.as_ref().map(to_vectors_selector),
                shard_key_selector: shard_key_selector(&request.shard_key),
                ..Default::default()
            };
            let resp = client
                .get_points(grpc_req)
                .await
                .map_err(|e| QqlError::backend("QQL-GRPC", format!("get_points: {e}"), None))?;
            Ok(get_points_envelope(
                resp.result
                    .into_iter()
                    .map(retrieved_point_to_json)
                    .collect(),
                resp.time,
            ))
        }
        PlannedOperation::Scroll {
            collection,
            request,
        } => {
            let grpc_req = qdrant::ScrollPoints {
                collection_name: collection.clone(),
                filter: to_filter_opt(request.filter.as_ref())?,
                offset: request.offset.as_ref().map(to_point_id),
                limit: request.limit.map(|l| l as u32),
                with_payload: request.with_payload.as_ref().map(to_payload_selector),
                with_vectors: request.with_vector.as_ref().map(to_vectors_selector),
                shard_key_selector: shard_key_selector(&request.shard_key),
                ..Default::default()
            };
            let resp = client
                .scroll(grpc_req)
                .await
                .map_err(|e| QqlError::backend("QQL-GRPC", format!("scroll: {e}"), None))?;
            let mut obj = serde_json::Map::new();
            obj.insert("status".into(), serde_json::json!("ok"));
            obj.insert("time".into(), serde_json::json!(resp.time));
            obj.insert(
                "result".into(),
                serde_json::json!({
                    "points": resp.result.into_iter().map(retrieved_point_to_json).collect::<Vec<_>>()
                }),
            );
            if let Some(offset) = resp.next_page_offset {
                obj.insert("next_page_offset".into(), point_id_to_json(&offset));
            }
            Ok(serde_json::Value::Object(obj))
        }
        PlannedOperation::Count {
            collection,
            request,
        } => {
            let grpc_req = qdrant::CountPoints {
                collection_name: collection.clone(),
                filter: to_filter_opt(request.filter.as_ref())?,
                exact: Some(true),
                shard_key_selector: shard_key_selector(&request.shard_key),
                ..Default::default()
            };
            let resp = client
                .count_points(grpc_req)
                .await
                .map_err(|e| QqlError::backend("QQL-GRPC", format!("count: {e}"), None))?;
            Ok(serde_json::json!({
                "result": { "count": resp.result.unwrap_or_default().count },
                "status": "ok",
                "time": resp.time,
            }))
        }
        PlannedOperation::Upsert {
            collection,
            request,
            wait: _,
        } => {
            let points: Vec<qdrant::PointStruct> = request
                .points
                .iter()
                .map(|p| {
                    let id = to_point_id(&p.id);
                    let vectors = p.vector.as_ref().and_then(to_vectors);
                    let payload = p
                        .payload
                        .as_ref()
                        .map(|pl| {
                            pl.iter()
                                .map(|(k, v)| (k.clone(), to_qdrant_value(v.clone())))
                                .collect()
                        })
                        .unwrap_or_default();
                    qdrant::PointStruct {
                        id: Some(id),
                        vectors,
                        payload,
                    }
                })
                .collect();
            let grpc_req = qdrant::UpsertPoints {
                collection_name: collection.clone(),
                wait: Some(true),
                points,
                shard_key_selector: shard_key_selector(&request.shard_key),
                ..Default::default()
            };
            let resp = client
                .upsert_points(grpc_req)
                .await
                .map_err(|e| QqlError::backend("QQL-GRPC", format!("upsert: {e}"), None))?;
            Ok(mutation_response_from(resp))
        }
        PlannedOperation::Delete {
            collection,
            request,
        } => {
            let selector =
                points_and_filter_selector(request.points.as_ref(), request.filter.as_ref())?;
            let grpc_req = qdrant::DeletePoints {
                collection_name: collection.clone(),
                wait: Some(true),
                points: selector,
                shard_key_selector: shard_key_selector(&request.shard_key),
                ..Default::default()
            };
            let resp = client
                .delete_points(grpc_req)
                .await
                .map_err(|e| QqlError::backend("QQL-GRPC", format!("delete: {e}"), None))?;
            Ok(mutation_response_from(resp))
        }
        PlannedOperation::ClearPayload {
            collection,
            request,
        } => {
            let selector =
                points_and_filter_selector(request.points.as_ref(), request.filter.as_ref())?;
            let grpc_req = qdrant::ClearPayloadPoints {
                collection_name: collection.clone(),
                wait: Some(true),
                points: selector,
                shard_key_selector: shard_key_selector(&request.shard_key),
                ..Default::default()
            };
            let resp = client
                .clear_payload(grpc_req)
                .await
                .map_err(|e| QqlError::backend("QQL-GRPC", format!("clear_payload: {e}"), None))?;
            Ok(mutation_response_from(resp))
        }
        PlannedOperation::DeletePayload {
            collection,
            request,
        } => {
            let selector =
                points_and_filter_selector(request.points.as_ref(), request.filter.as_ref())?;
            let grpc_req = qdrant::DeletePayloadPoints {
                collection_name: collection.clone(),
                wait: Some(true),
                keys: request.keys.clone(),
                points_selector: selector,
                shard_key_selector: shard_key_selector(&request.shard_key),
                ..Default::default()
            };
            let resp = client
                .delete_payload(grpc_req)
                .await
                .map_err(|e| QqlError::backend("QQL-GRPC", format!("delete_payload: {e}"), None))?;
            Ok(mutation_response_from(resp))
        }
        PlannedOperation::DeleteVectors {
            collection,
            request,
        } => {
            let selector =
                points_and_filter_selector(request.points.as_ref(), request.filter.as_ref())?;
            let grpc_req = qdrant::DeletePointVectors {
                collection_name: collection.clone(),
                wait: Some(true),
                points_selector: selector,
                vectors: Some(qdrant::VectorsSelector {
                    names: request.vector.clone(),
                }),
                shard_key_selector: shard_key_selector(&request.shard_key),
                ..Default::default()
            };
            let resp = client
                .delete_vectors(grpc_req)
                .await
                .map_err(|e| QqlError::backend("QQL-GRPC", format!("delete_vectors: {e}"), None))?;
            Ok(mutation_response_from(resp))
        }
        PlannedOperation::UpdateVectors {
            collection,
            request,
        } => {
            let points: Vec<qdrant::PointVectors> = request
                .points
                .iter()
                .map(|p| qdrant::PointVectors {
                    id: Some(to_point_id(&p.id)),
                    vectors: to_vectors(&p.vector),
                })
                .collect();
            let grpc_req = qdrant::UpdatePointVectors {
                collection_name: collection.clone(),
                wait: Some(true),
                points,
                shard_key_selector: shard_key_selector(&request.shard_key),
                ..Default::default()
            };
            let resp = client
                .update_vectors(grpc_req)
                .await
                .map_err(|e| QqlError::backend("QQL-GRPC", format!("update_vectors: {e}"), None))?;
            Ok(mutation_response_from(resp))
        }
        PlannedOperation::UpdatePayload {
            collection,
            request,
        } => {
            let selector =
                points_and_filter_selector(request.points.as_ref(), request.filter.as_ref())?;
            let payload_map: std::collections::HashMap<String, qdrant::Value> = request
                .payload
                .iter()
                .map(|(k, v)| (k.clone(), to_qdrant_value(v.clone())))
                .collect();
            let grpc_req = qdrant::SetPayloadPoints {
                collection_name: collection.clone(),
                wait: Some(true),
                payload: payload_map,
                points_selector: selector,
                shard_key_selector: shard_key_selector(&request.shard_key),
                ..Default::default()
            };
            let resp = client
                .set_payload(grpc_req)
                .await
                .map_err(|e| QqlError::backend("QQL-GRPC", format!("set_payload: {e}"), None))?;
            Ok(mutation_response_from(resp))
        }
        PlannedOperation::CreateCollection {
            collection,
            request,
        } => {
            let deferred_params =
                request
                    .params
                    .as_ref()
                    .map(collection_params_diff)
                    .filter(|params| {
                        params.read_fan_out_factor.is_some()
                            || params.read_fan_out_delay_ms.is_some()
                    });
            let grpc_req = qdrant::CreateCollection {
                collection_name: collection.clone(),
                vectors_config: request.vectors.as_ref().map(|v| {
                    let map = v
                        .iter()
                        .map(|(name, cfg)| (name.clone(), vector_params(cfg)))
                        .collect();
                    qdrant::VectorsConfig {
                        config: Some(qdrant::vectors_config::Config::ParamsMap(
                            qdrant::VectorParamsMap { map },
                        )),
                    }
                }),
                sparse_vectors_config: request.sparse_vectors.as_ref().map(|sv| {
                    let map = sv
                        .iter()
                        .map(|(name, cfg)| (name.clone(), sparse_vector_params(cfg)))
                        .collect();
                    qdrant::SparseVectorConfig { map }
                }),
                hnsw_config: request.hnsw_config.as_ref().map(hnsw_config_from_plan),
                optimizers_config: request
                    .optimizers_config
                    .as_ref()
                    .map(optimizers_config_from_plan),
                shard_number: request
                    .shard_number
                    .or_else(|| {
                        request
                            .params
                            .as_ref()
                            .and_then(|p| p.get("shard_number"))
                            .and_then(|v| v.as_u64())
                    })
                    .map(|n| n as u32),
                replication_factor: request
                    .params
                    .as_ref()
                    .and_then(|p| p.get("replication_factor"))
                    .and_then(|v| v.as_u64())
                    .map(|n| n as u32),
                on_disk_payload: request
                    .params
                    .as_ref()
                    .and_then(|p| p.get("on_disk_payload"))
                    .and_then(|v| v.as_bool()),
                write_consistency_factor: request
                    .params
                    .as_ref()
                    .and_then(|p| p.get("write_consistency_factor"))
                    .and_then(serde_json::Value::as_u64)
                    .map(|n| n as u32),
                quantization_config: request
                    .quantization_config
                    .as_ref()
                    .and_then(quantization_config_from_plan),
                sharding_method: request.sharding_method.as_ref().map(|method| {
                    match method.to_ascii_lowercase().as_str() {
                        "custom" => qdrant::ShardingMethod::Custom as i32,
                        _ => qdrant::ShardingMethod::Auto as i32,
                    }
                }),
                ..Default::default()
            };
            let resp = client.create_collection_raw(grpc_req).await.map_err(|e| {
                QqlError::backend("QQL-GRPC", format!("create_collection: {e}"), None)
            })?;
            if let Some(params) = deferred_params {
                client
                    .update_collection_raw(qdrant::UpdateCollection {
                        collection_name: collection.clone(),
                        params: Some(params),
                        ..Default::default()
                    })
                    .await
                    .map_err(|e| {
                        QqlError::backend(
                            "QQL-GRPC",
                            format!("update_collection_params: {e}"),
                            None,
                        )
                    })?;
            }
            if let Some(shard_keys) = &request.shard_keys {
                for shard_key in shard_keys {
                    client
                        .create_shard_key(qdrant::CreateShardKeyRequest {
                            collection_name: collection.clone(),
                            request: Some(qdrant::CreateShardKey {
                                shard_key: Some(qdrant::ShardKey {
                                    key: Some(qdrant::shard_key::Key::Keyword(shard_key.clone())),
                                }),
                                ..Default::default()
                            }),
                            ..Default::default()
                        })
                        .await
                        .map_err(|e| {
                            QqlError::backend(
                                "QQL-GRPC",
                                format!("create_shard_key {shard_key}: {e}"),
                                None,
                            )
                        })?;
                }
            }
            Ok(collection_mutation_response(resp))
        }
        PlannedOperation::UpdateCollection {
            collection,
            request,
        } => {
            let grpc_req = qdrant::UpdateCollection {
                collection_name: collection.clone(),
                optimizers_config: request
                    .optimizers_config
                    .as_ref()
                    .map(optimizers_config_from_plan),
                params: request.params.as_ref().map(collection_params_diff),
                hnsw_config: request.hnsw_config.as_ref().map(hnsw_config_from_plan),
                quantization_config: request
                    .quantization_config
                    .as_ref()
                    .and_then(quantization_config_diff),
                ..Default::default()
            };
            let resp = client.update_collection_raw(grpc_req).await.map_err(|e| {
                QqlError::backend("QQL-GRPC", format!("update_collection: {e}"), None)
            })?;
            Ok(collection_mutation_response(resp))
        }
        PlannedOperation::DropCollection { collection } => {
            let grpc_req = qdrant::DeleteCollection {
                collection_name: collection.clone(),
                ..Default::default()
            };
            let resp = client.delete_collection_raw(grpc_req).await.map_err(|e| {
                QqlError::backend("QQL-GRPC", format!("drop_collection: {e}"), None)
            })?;
            Ok(collection_mutation_response(resp))
        }
        PlannedOperation::CreateIndex {
            collection,
            request,
        } => {
            let field_type = match request.field_schema.as_str() {
                "keyword" => qdrant::FieldType::Keyword as i32,
                "integer" => qdrant::FieldType::Integer as i32,
                "float" => qdrant::FieldType::Float as i32,
                "geo" => qdrant::FieldType::Geo as i32,
                "text" => qdrant::FieldType::Text as i32,
                "bool" => qdrant::FieldType::Bool as i32,
                "datetime" => qdrant::FieldType::Datetime as i32,
                "uuid" => qdrant::FieldType::Uuid as i32,
                _ => qdrant::FieldType::Keyword as i32,
            };
            let grpc_req = qdrant::CreateFieldIndexCollection {
                collection_name: collection.clone(),
                wait: Some(true),
                field_name: request.field_name.clone(),
                field_type: Some(field_type),
                field_index_params: Some(payload_index_params(
                    &request.field_schema,
                    &request.extra,
                )?),
                ..Default::default()
            };
            let resp = client
                .create_field_index(grpc_req)
                .await
                .map_err(|e| QqlError::backend("QQL-GRPC", format!("create_index: {e}"), None))?;
            Ok(mutation_response_from(resp))
        }
        PlannedOperation::DropIndex { collection, field } => {
            let grpc_req = qdrant::DeleteFieldIndexCollection {
                collection_name: collection.clone(),
                field_name: field.clone(),
                ..Default::default()
            };
            let resp = client
                .delete_field_index(grpc_req)
                .await
                .map_err(|e| QqlError::backend("QQL-GRPC", format!("drop_index: {e}"), None))?;
            Ok(mutation_response_from(resp))
        }
        PlannedOperation::CreateShardKey {
            collection,
            request,
        } => {
            let grpc_req = qdrant::CreateShardKeyRequest {
                collection_name: collection.clone(),
                request: Some(qdrant::CreateShardKey {
                    shard_key: Some(qdrant::ShardKey {
                        key: Some(qdrant::shard_key::Key::Keyword(request.shard_key.clone())),
                    }),
                    shards_number: request.shards_number.map(|n| n as u32),
                    replication_factor: request.replication_factor.map(|n| n as u32),
                    ..Default::default()
                }),
                ..Default::default()
            };
            client.create_shard_key(grpc_req).await.map_err(|e| {
                QqlError::backend("QQL-GRPC", format!("create_shard_key: {e}"), None)
            })?;
            Ok(mutation_response_ok())
        }
        PlannedOperation::DropShardKey {
            collection,
            request,
        } => {
            let grpc_req = qdrant::DeleteShardKeyRequest {
                collection_name: collection.clone(),
                request: Some(qdrant::DeleteShardKey {
                    shard_key: Some(qdrant::ShardKey {
                        key: Some(qdrant::shard_key::Key::Keyword(request.shard_key.clone())),
                    }),
                }),
                ..Default::default()
            };
            client
                .delete_shard_key(grpc_req)
                .await
                .map_err(|e| QqlError::backend("QQL-GRPC", format!("drop_shard_key: {e}"), None))?;
            Ok(mutation_response_ok())
        }
        // Read-only operations — call gRPC client directly, no Route needed
        PlannedOperation::ListCollections => {
            let resp = client
                .list_collections_raw()
                .await
                .map_err(|e| QqlError::backend("QQL-GRPC", format!("list: {e}"), None))?;
            Ok(list_collections_response_to_json(resp))
        }
        PlannedOperation::GetCollection { collection } => {
            let resp = client
                .collection_info_raw(collection.clone())
                .await
                .map_err(|e| QqlError::backend("QQL-GRPC", format!("get_collection: {e}"), None))?;
            Ok(collection_info_to_json(resp))
        }
        PlannedOperation::ListShardKeys { collection } => {
            let grpc_req = qdrant::ListShardKeysRequest {
                collection_name: collection.clone(),
            };
            let resp = client.list_shard_keys(grpc_req).await.map_err(|e| {
                QqlError::backend("QQL-GRPC", format!("list_shard_keys: {e}"), None)
            })?;
            let keys: Vec<serde_json::Value> = resp
                .shard_keys
                .into_iter()
                .filter_map(|d| d.key)
                .map(|sk| match sk.key {
                    Some(qdrant::shard_key::Key::Keyword(s)) => serde_json::Value::String(s),
                    Some(qdrant::shard_key::Key::Number(n)) => {
                        serde_json::Value::Number((n).into())
                    }
                    None => serde_json::Value::Null,
                })
                .collect();
            Ok(serde_json::json!({
                "result": { "shard_keys": keys },
                "status": "ok",
                "time": 0.0_f64,
            }))
        }
        PlannedOperation::CrossRerank { .. } => Err(QqlError::execution(
            "QQL-RERANK-CROSS",
            "CROSS RERANK is executed client-side by the Executor, not as a single gRPC route",
            None,
        )),
    }
}

/// Convert a batch of QueryRequests and send them via gRPC `QueryBatch`.
pub async fn execute_query_batch_grpc(
    client: &crate::grpc::GrpcQdrant,
    collection: &str,
    batch: &qql_plan::QueryBatchRequest,
) -> Result<Vec<serde_json::Value>, QqlError> {
    let query_points: Result<Vec<_>, _> = batch
        .searches
        .iter()
        .map(|req| to_query_points(req, collection))
        .collect();
    let query_points = query_points?;

    let grpc_req = qdrant::QueryBatchPoints {
        collection_name: collection.to_string(),
        query_points,
        ..Default::default()
    };

    let resp = client
        .query_batch(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("query_batch: {e}"), None))?;

    Ok(resp.result.into_iter().map(batch_result_to_json).collect())
}

/// Convert a mutation batch and send via gRPC `UpdateBatch`.
pub async fn execute_update_batch_grpc(
    client: &crate::grpc::GrpcQdrant,
    collection: &str,
    batch: &qql_plan::UpdateBatchRequest,
) -> Result<Vec<serde_json::Value>, QqlError> {
    let operations: Vec<qdrant::PointsUpdateOperation> = batch
        .operations
        .iter()
        .map(to_points_update_operation)
        .collect::<Result<Vec<_>, QqlError>>()?;

    let grpc_req = qdrant::UpdateBatchPoints {
        collection_name: collection.to_string(),
        wait: Some(true),
        operations,
        ..Default::default()
    };

    let resp = client
        .update_batch(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("update_batch: {e}"), None))?;

    Ok(resp.result.into_iter().map(update_result_to_json).collect())
}

fn to_points_update_operation(
    op: &qql_plan::UpdateOperation,
) -> Result<qdrant::PointsUpdateOperation, QqlError> {
    use qdrant::points_update_operation::{self, Operation};
    use qql_plan::UpdateOperation;

    let operation = match op {
        UpdateOperation::Upsert { upsert } => {
            let points: Vec<qdrant::PointStruct> = upsert
                .points
                .iter()
                .map(|p| {
                    let payload = p
                        .payload
                        .as_ref()
                        .map(|pl| {
                            pl.iter()
                                .map(|(k, v)| (k.clone(), to_qdrant_value(v.clone())))
                                .collect()
                        })
                        .unwrap_or_default();
                    qdrant::PointStruct {
                        id: Some(to_point_id(&p.id)),
                        vectors: p.vector.as_ref().and_then(to_vectors),
                        payload,
                    }
                })
                .collect();
            let shard_key_selector = upsert.shard_key.as_ref().map(|k| qdrant::ShardKeySelector {
                shard_keys: vec![qdrant::ShardKey {
                    key: Some(qdrant::shard_key::Key::Keyword(k.clone())),
                }],
                ..Default::default()
            });
            Operation::Upsert(points_update_operation::PointStructList {
                points,
                shard_key_selector,
                update_filter: None,
                update_mode: None,
            })
        }
        UpdateOperation::Delete { delete } => {
            let points =
                points_and_filter_selector(delete.points.as_ref(), delete.filter.as_ref())?;
            let shard_key_selector = delete.shard_key.as_ref().map(|k| qdrant::ShardKeySelector {
                shard_keys: vec![qdrant::ShardKey {
                    key: Some(qdrant::shard_key::Key::Keyword(k.clone())),
                }],
                ..Default::default()
            });
            Operation::DeletePoints(points_update_operation::DeletePoints {
                points,
                shard_key_selector,
            })
        }
        UpdateOperation::SetPayload { set_payload } => {
            let payload_map: std::collections::HashMap<String, qdrant::Value> = set_payload
                .payload
                .iter()
                .map(|(k, v)| (k.clone(), to_qdrant_value(v.clone())))
                .collect();
            Operation::SetPayload(points_update_operation::SetPayload {
                payload: payload_map,
                points_selector: points_and_filter_selector(
                    set_payload.points.as_ref(),
                    set_payload.filter.as_ref(),
                )?,
                shard_key_selector: shard_key_selector(&set_payload.shard_key),
                key: None,
            })
        }
        UpdateOperation::ClearPayload { clear_payload } => {
            Operation::ClearPayload(points_update_operation::ClearPayload {
                points: points_and_filter_selector(
                    clear_payload.points.as_ref(),
                    clear_payload.filter.as_ref(),
                )?,
                shard_key_selector: shard_key_selector(&clear_payload.shard_key),
            })
        }
        UpdateOperation::UpdateVectors { update_vectors } => {
            let points: Vec<qdrant::PointVectors> = update_vectors
                .points
                .iter()
                .map(|p| qdrant::PointVectors {
                    id: Some(to_point_id(&p.id)),
                    vectors: to_vectors(&p.vector),
                })
                .collect();
            Operation::UpdateVectors(points_update_operation::UpdateVectors {
                points,
                shard_key_selector: shard_key_selector(&update_vectors.shard_key),
                update_filter: None,
            })
        }
        UpdateOperation::DeleteVectors { delete_vectors } => {
            Operation::DeleteVectors(points_update_operation::DeleteVectors {
                points_selector: points_and_filter_selector(
                    delete_vectors.points.as_ref(),
                    delete_vectors.filter.as_ref(),
                )?,
                vectors: Some(qdrant::VectorsSelector {
                    names: delete_vectors.vector.clone(),
                }),
                shard_key_selector: shard_key_selector(&delete_vectors.shard_key),
            })
        }
    };

    Ok(qdrant::PointsUpdateOperation {
        operation: Some(operation),
    })
}

fn points_and_filter_selector(
    points: Option<&Vec<PlanPointId>>,
    filter: Option<&FilterExpression>,
) -> Result<Option<qdrant::PointsSelector>, QqlError> {
    if let Some(points) = points {
        Ok(Some(qdrant::PointsSelector {
            points_selector_one_of: Some(qdrant::points_selector::PointsSelectorOneOf::Points(
                qdrant::PointsIdsList {
                    ids: points.iter().map(to_point_id).collect(),
                },
            )),
        }))
    } else if let Some(f) = filter {
        Ok(Some(qdrant::PointsSelector {
            points_selector_one_of: Some(qdrant::points_selector::PointsSelectorOneOf::Filter(
                to_filter(f)?,
            )),
        }))
    } else {
        Ok(None)
    }
}

fn update_result_to_json(r: qdrant::UpdateResult) -> serde_json::Value {
    let status = match r.status() {
        qdrant::UpdateStatus::Acknowledged => "acknowledged",
        qdrant::UpdateStatus::Completed => "completed",
        qdrant::UpdateStatus::ClockRejected => "clock_rejected",
        qdrant::UpdateStatus::WaitTimeout => "wait_timeout",
        qdrant::UpdateStatus::UnknownUpdateStatus => "unknown",
    };
    serde_json::json!({
        "operation_id": r.operation_id,
        "status": status,
    })
}

/// REST-compatible envelope for `GetPoints`: hit extraction reads
/// `result.points`, so a bare `result` array would silently drop every hit
/// (B-3 regression).
fn get_points_envelope(points: Vec<serde_json::Value>, time: f64) -> serde_json::Value {
    serde_json::json!({
        "result": { "points": points },
        "status": "ok",
        "time": time,
    })
}

pub(crate) fn to_query_points(
    req: &qql_plan::types::QueryRequest,
    collection: &str,
) -> Result<qdrant::QueryPoints, QqlError> {
    Ok(qdrant::QueryPoints {
        collection_name: collection.into(),
        prefetch: req
            .prefetch
            .iter()
            .map(to_prefetch)
            .collect::<Result<Vec<_>, _>>()?,
        query: Some(to_query_variant(&req.query)?),
        using: req.using.clone(),
        filter: to_filter_opt(req.filter.as_ref())?,
        params: req.params.as_ref().map(to_search_params),
        score_threshold: req.score_threshold.map(|s| s as f32),
        limit: req.limit,
        offset: req.offset,
        with_payload: req.with_payload.as_ref().map(to_payload_selector),
        with_vectors: req.with_vector.as_ref().map(to_vectors_selector),
        shard_key_selector: shard_key_selector(&req.shard_key),
        // Proto QueryPoints.timeout / read_consistency (not SearchParams body).
        timeout: req.timeout,
        read_consistency: req.consistency.as_ref().map(to_read_consistency),
        ..Default::default()
    })
}

fn to_query_groups(
    req: &qql_plan::types::QueryGroupsRequest,
    collection: &str,
) -> Result<qdrant::QueryPointGroups, QqlError> {
    Ok(qdrant::QueryPointGroups {
        collection_name: collection.into(),
        prefetch: req
            .prefetch
            .iter()
            .map(to_prefetch)
            .collect::<Result<Vec<_>, _>>()?,
        query: Some(to_query_variant(&req.query)?),
        using: req.using.clone(),
        filter: to_filter_opt(req.filter.as_ref())?,
        params: req.params.as_ref().map(to_search_params),
        score_threshold: req.score_threshold.map(|s| s as f32),
        with_payload: req.with_payload.as_ref().map(to_payload_selector),
        with_vectors: req.with_vector.as_ref().map(to_vectors_selector),
        group_by: req.group_by.clone(),
        group_size: Some(req.group_size),
        limit: Some(req.limit),
        with_lookup: req.with_lookup.as_ref().map(|wv| match wv {
            WithLookupValue::Collection(c) => qdrant::WithLookup {
                collection: c.clone(),
                ..Default::default()
            },
            WithLookupValue::Full(wl) => qdrant::WithLookup {
                collection: wl.collection.clone(),
                with_payload: wl.with_payload.as_ref().map(to_payload_selector),
                with_vectors: wl.with_vectors.as_ref().map(to_vectors_selector),
            },
        }),
        shard_key_selector: shard_key_selector(&req.shard_key),
        timeout: req.timeout,
        read_consistency: req.consistency.as_ref().map(to_read_consistency),
        ..Default::default()
    })
}

fn to_read_consistency(c: &qql_plan::types::ReadConsistencyParam) -> qdrant::ReadConsistency {
    use qdrant::read_consistency::Value as RcValue;
    use qql_plan::types::ReadConsistencyParam;
    let value = match c {
        ReadConsistencyParam::Factor(n) => RcValue::Factor(*n),
        ReadConsistencyParam::Majority => {
            RcValue::Type(qdrant::ReadConsistencyType::Majority as i32)
        }
        ReadConsistencyParam::Quorum => RcValue::Type(qdrant::ReadConsistencyType::Quorum as i32),
        ReadConsistencyParam::All => RcValue::Type(qdrant::ReadConsistencyType::All as i32),
    };
    qdrant::ReadConsistency { value: Some(value) }
}

fn to_prefetch(pf: &qql_plan::types::PrefetchRequest) -> Result<qdrant::PrefetchQuery, QqlError> {
    Ok(qdrant::PrefetchQuery {
        prefetch: match pf.prefetch.as_ref() {
            Some(pfs) => pfs
                .iter()
                .map(to_prefetch)
                .collect::<Result<Vec<_>, QqlError>>()?,
            None => Vec::new(),
        },
        query: pf.query.as_ref().map(to_query_variant).transpose()?,
        using: pf.using.clone(),
        filter: to_filter_opt(pf.filter.as_ref())?,
        params: pf.params.as_ref().map(to_search_params),
        score_threshold: pf.score_threshold.map(|s| s as f32),
        limit: pf.limit,
        lookup_from: pf.lookup_from.as_ref().map(|l| qdrant::LookupLocation {
            collection_name: l.collection.clone(),
            vector_name: l.vector.clone(),
            ..Default::default()
        }),
    })
}

/// Convert plan RRF parameters to the gRPC `Rrf` message.
///
/// The pinned proto stores `k` as `uint32` and `weights` as `repeated float`.
/// Values that cannot be represented exactly (`k > u32::MAX`, an `f64` weight
/// that does not round-trip through `f32`) return a structured error instead
/// of being silently dropped or silently losing precision.
fn to_grpc_rrf(rrf: &qql_plan::types::RrfQuery) -> Result<qdrant::Rrf, QqlError> {
    let k = match rrf.rrf.k {
        Some(k) => {
            let k32 = u32::try_from(k).map_err(|_| {
                QqlError::validation(
                    "QQL-GRPC-RRF-K",
                    format!(
                        "rrf_k = {k} cannot be represented on the gRPC path: the bundled Qdrant proto stores k as uint32 (max {})",
                        u32::MAX
                    ),
                    None,
                )
            })?;
            Some(k32)
        }
        None => None,
    };
    let weights = match rrf.rrf.weights.as_ref() {
        Some(weights) => {
            let mut out = Vec::with_capacity(weights.len());
            for w in weights {
                let w32 = *w as f32;
                if (w32 as f64) != *w {
                    return Err(QqlError::validation(
                        "QQL-GRPC-RRF-WEIGHT",
                        format!(
                            "rrf_weights value {w} cannot be represented exactly on the gRPC path: the bundled Qdrant proto stores weights as f32"
                        ),
                        None,
                    ));
                }
                out.push(w32);
            }
            out
        }
        None => Vec::new(),
    };
    Ok(qdrant::Rrf { k, weights })
}

fn to_query_variant(qv: &qql_plan::types::QueryVariant) -> Result<qdrant::Query, QqlError> {
    use qdrant::query::Variant;
    use qql_plan::types::QueryVariant;

    let variant = match qv {
        QueryVariant::Nearest(nq) => Variant::Nearest(to_vector_input(&nq.nearest)),
        QueryVariant::Recommend { recommend } => Variant::Recommend(qdrant::RecommendInput {
            positive: recommend.positive.iter().map(to_vector_input).collect(),
            negative: recommend.negative.iter().map(to_vector_input).collect(),
            strategy: recommend.strategy.as_deref().map(|s| match s {
                "average_vector" => qdrant::RecommendStrategy::AverageVector as i32,
                "best_score" => qdrant::RecommendStrategy::BestScore as i32,
                "sum_scores" => qdrant::RecommendStrategy::SumScores as i32,
                _ => qdrant::RecommendStrategy::AverageVector as i32,
            }),
        }),
        QueryVariant::Context { context } => Variant::Context(qdrant::ContextInput {
            pairs: context
                .iter()
                .map(|p| qdrant::ContextInputPair {
                    positive: Some(to_vector_input(&p.positive)),
                    negative: Some(to_vector_input(&p.negative)),
                })
                .collect(),
        }),
        QueryVariant::Discover { discover } => Variant::Discover(qdrant::DiscoverInput {
            target: Some(to_vector_input(&discover.target)),
            context: Some(qdrant::ContextInput {
                pairs: discover
                    .context
                    .iter()
                    .map(|p| qdrant::ContextInputPair {
                        positive: Some(to_vector_input(&p.positive)),
                        negative: Some(to_vector_input(&p.negative)),
                    })
                    .collect(),
            }),
        }),
        QueryVariant::OrderBy { order_by } => {
            let dir = order_by.direction.as_deref().map(|d| match d {
                "asc" => qdrant::Direction::Asc as i32,
                "desc" => qdrant::Direction::Desc as i32,
                _ => qdrant::Direction::Asc as i32,
            });
            Variant::OrderBy(qdrant::OrderBy {
                key: order_by.key.clone(),
                direction: dir,
                ..Default::default()
            })
        }
        QueryVariant::Sample { .. } => Variant::Sample(0),
        QueryVariant::Fusion { fusion } => {
            let val = match fusion.as_str() {
                "rrf" => qdrant::Fusion::Rrf as i32,
                "dbsf" => qdrant::Fusion::Dbsf as i32,
                other => {
                    return Err(QqlError::validation(
                        "QQL-GRPC-FUSION",
                        format!("unsupported fusion method '{other}' on the gRPC path"),
                        None,
                    ));
                }
            };
            Variant::Fusion(val)
        }
        QueryVariant::Rrf(rrf) => Variant::Rrf(to_grpc_rrf(rrf)?),
        QueryVariant::Formula(fq) => Variant::Formula(qdrant::Formula {
            expression: ast_formula_to_grpc(&fq.formula.0),
            defaults: fq
                .defaults
                .clone()
                .unwrap_or_default()
                .into_iter()
                .map(|(k, v)| (k, to_qdrant_value(v)))
                .collect(),
        }),
        QueryVariant::RelevanceFeedback { relevance_feedback } => {
            let feedback = relevance_feedback
                .feedback
                .iter()
                .map(|item| qdrant::FeedbackItem {
                    example: Some(to_vector_input(&item.example)),
                    score: item.score as f32,
                })
                .collect();
            let strategy = Some(qdrant::FeedbackStrategy {
                variant: Some(qdrant::feedback_strategy::Variant::Naive(
                    qdrant::NaiveFeedbackStrategy {
                        a: relevance_feedback.strategy.naive.a as f32,
                        b: relevance_feedback.strategy.naive.b as f32,
                        c: relevance_feedback.strategy.naive.c as f32,
                    },
                )),
            });
            Variant::RelevanceFeedback(qdrant::RelevanceFeedbackInput {
                target: Some(to_vector_input(&relevance_feedback.target)),
                feedback,
                strategy,
            })
        }
    };
    Ok(qdrant::Query {
        variant: Some(variant),
    })
}

pub(crate) fn to_vector_input(input: &PlanQueryInput) -> qdrant::VectorInput {
    use qdrant::vector_input::Variant;
    match input {
        PlanQueryInput::Point(id) => qdrant::VectorInput {
            variant: Some(Variant::Id(to_point_id(id))),
        },
        PlanQueryInput::Vector(PlanVectorValue::Dense(data)) => qdrant::VectorInput {
            variant: Some(Variant::Dense(qdrant::DenseVector { data: data.clone() })),
        },
        PlanQueryInput::Vector(PlanVectorValue::Sparse { indices, values }) => {
            qdrant::VectorInput {
                variant: Some(Variant::Sparse(qdrant::SparseVector {
                    indices: indices.clone(),
                    values: values.clone(),
                })),
            }
        }
        PlanQueryInput::Vector(PlanVectorValue::MultiDense(rows)) => qdrant::VectorInput {
            variant: Some(Variant::MultiDense(qdrant::MultiDenseVector {
                vectors: rows
                    .iter()
                    .map(|row| qdrant::DenseVector { data: row.clone() })
                    .collect(),
            })),
        },
        PlanQueryInput::Document { text, model } => qdrant::VectorInput {
            variant: Some(Variant::Document(qdrant::Document {
                text: text.clone(),
                model: model.clone().unwrap_or_default(),
                ..Default::default()
            })),
        },
        PlanQueryInput::Image { image, model } => qdrant::VectorInput {
            variant: Some(Variant::Image(qdrant::Image {
                image: Some(qdrant::Value {
                    kind: Some(qdrant::value::Kind::StringValue(image.clone())),
                }),
                model: model.clone().unwrap_or_default(),
                ..Default::default()
            })),
        },
    }
}

fn to_filter(fe: &FilterExpression) -> Result<qdrant::Filter, QqlError> {
    match fe {
        FilterExpression::Compound(fc) => compound_to_filter(fc),
        FilterExpression::Single(fc) => Ok(qdrant::Filter {
            must: vec![to_condition(fc)?],
            ..Default::default()
        }),
    }
}

fn to_filter_opt(fe: Option<&FilterExpression>) -> Result<Option<qdrant::Filter>, QqlError> {
    fe.map(to_filter).transpose()
}

fn compound_to_filter(fc: &FilterCompound) -> Result<qdrant::Filter, QqlError> {
    let must = fc
        .must
        .iter()
        .map(to_condition)
        .collect::<Result<Vec<_>, _>>()?;
    let must_not = fc
        .must_not
        .iter()
        .map(to_condition)
        .collect::<Result<Vec<_>, _>>()?;
    let should = fc
        .should
        .iter()
        .map(to_condition)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(qdrant::Filter {
        must,
        must_not,
        should,
        ..Default::default()
    })
}

fn to_condition(clause: &FilterClause) -> Result<qdrant::Condition, QqlError> {
    use qdrant::condition::ConditionOneOf;
    let condition_one_of = match clause {
        FilterClause::Field(fc) => {
            let mut field = qdrant::FieldCondition {
                key: fc.key.clone(),
                ..Default::default()
            };
            if let Some(mv) = &fc.r#match {
                field.r#match = Some(to_match(mv)?);
            }
            if let Some(r) = &fc.range {
                field.range = Some(qdrant::Range {
                    gt: r.gt.as_ref().and_then(|v| v.as_f64()),
                    gte: r.gte.as_ref().and_then(|v| v.as_f64()),
                    lt: r.lt.as_ref().and_then(|v| v.as_f64()),
                    lte: r.lte.as_ref().and_then(|v| v.as_f64()),
                });
            }
            if let Some(b) = &fc.geo_bounding_box {
                field.geo_bounding_box = Some(qdrant::GeoBoundingBox {
                    top_left: Some(qdrant::GeoPoint {
                        lat: b.top_left.lat,
                        lon: b.top_left.lon,
                    }),
                    bottom_right: Some(qdrant::GeoPoint {
                        lat: b.bottom_right.lat,
                        lon: b.bottom_right.lon,
                    }),
                });
            }
            if let Some(r) = &fc.geo_radius {
                field.geo_radius = Some(qdrant::GeoRadius {
                    center: Some(qdrant::GeoPoint {
                        lat: r.center.lat,
                        lon: r.center.lon,
                    }),
                    radius: r.radius as f32,
                });
            }
            if let Some(vc) = &fc.values_count {
                field.values_count = Some(qdrant::ValuesCount {
                    gt: vc.gt,
                    gte: vc.gte,
                    lt: vc.lt,
                    lte: vc.lte,
                });
            }
            ConditionOneOf::Field(field)
        }
        FilterClause::IsNull(n) => ConditionOneOf::IsNull(qdrant::IsNullCondition {
            key: n.is_null.key.clone(),
        }),
        FilterClause::IsEmpty(e) => ConditionOneOf::IsEmpty(qdrant::IsEmptyCondition {
            key: e.is_empty.key.clone(),
        }),
        FilterClause::HasId(h) => ConditionOneOf::HasId(qdrant::HasIdCondition {
            has_id: h.has_id.iter().map(to_point_id).collect(),
        }),
        FilterClause::HasVector(v) => ConditionOneOf::HasVector(qdrant::HasVectorCondition {
            has_vector: v.has_vector.clone(),
        }),
        FilterClause::Nested(n) => ConditionOneOf::Nested(qdrant::NestedCondition {
            key: n.nested.key.clone(),
            filter: Some(to_filter(&n.nested.filter)?),
        }),
        FilterClause::Filter(f) => ConditionOneOf::Filter(compound_to_filter(f)?),
    };
    Ok(qdrant::Condition {
        condition_one_of: Some(condition_one_of),
    })
}

/// Convert an `IN`/`NOT IN` value list to the gRPC `Match`.
///
/// The pinned proto's `Match` carries either a repeated string list
/// (`keywords` / `except_keywords`) or a repeated int64 list (`integers` /
/// `except_integers`); there is no repeated bool or double list. Every entry
/// must therefore be the same kind and representable as the target type.
/// Mixed lists, non-integral floats, and `u64` values above `i64::MAX`
/// produce a structured error instead of being silently dropped or wrapped
/// into negative integers.
fn exact_list_match(values: &[serde_json::Value], any: bool) -> Result<qdrant::Match, QqlError> {
    use qdrant::r#match::MatchValue as Mv;

    let any_string = values.iter().any(|v| v.is_string());
    let all_strings = values.iter().all(|v| v.is_string());
    if any_string && !all_strings {
        let offending = values
            .iter()
            .find(|v| !v.is_string())
            .map(|v| v.to_string())
            .unwrap_or_else(|| "non-string entry".to_string());
        return Err(QqlError::validation(
            "QQL-GRPC-LIST-TYPE",
            format!(
                "IN/NOT IN list mixes strings with non-string entry {offending}: the bundled Qdrant proto only supports homogeneous string or int64 lists"
            ),
            None,
        ));
    }
    if all_strings {
        let strings: Vec<String> = values
            .iter()
            .map(|v| String::from(v.as_str().expect("guarded by all_strings")))
            .collect();
        return Ok(qdrant::Match {
            match_value: Some(if any {
                Mv::Keywords(qdrant::RepeatedStrings { strings })
            } else {
                Mv::ExceptKeywords(qdrant::RepeatedStrings { strings })
            }),
        });
    }

    // No strings: every entry must fit the proto's repeated int64 list.
    // Integral floats map to integer matches (mirroring single-value
    // `to_match`); anything else is a structured error.
    let integers = values
        .iter()
        .map(list_integer)
        .collect::<Result<Vec<i64>, QqlError>>()?;
    Ok(qdrant::Match {
        match_value: Some(if any {
            Mv::Integers(qdrant::RepeatedIntegers { integers })
        } else {
            Mv::ExceptIntegers(qdrant::RepeatedIntegers { integers })
        }),
    })
}

/// Convert one `IN`/`NOT IN` entry to the int64 wire type, rejecting values
/// the bundled proto cannot carry.
fn list_integer(value: &serde_json::Value) -> Result<i64, QqlError> {
    if let Some(n) = value.as_i64() {
        return Ok(n);
    }
    if let Some(n) = value.as_u64() {
        return i64::try_from(n).map_err(|_| {
            QqlError::validation(
                "QQL-GRPC-LIST-INT",
                format!(
                    "integer {n} in an IN/NOT IN list cannot be represented on the gRPC path: the bundled Qdrant proto stores match lists as int64 (max {})",
                    i64::MAX
                ),
                None,
            )
        });
    }
    if let Some(f) = value.as_f64() {
        // Mirror single-value `to_match`: an integral float is carried as an
        // integer match (Qdrant compares payload numbers numerically).
        let integral = f.is_finite()
            && f.fract() == 0.0
            && f >= i64::MIN as f64
            && f <= i64::MAX as f64
            && (f as i64) as f64 == f;
        if integral {
            return Ok(f as i64);
        }
        return Err(QqlError::validation(
            "QQL-GRPC-LIST-TYPE",
            format!(
                "IN/NOT IN list entry {value} cannot be represented on the gRPC path: the bundled Qdrant proto only supports homogeneous string or int64 lists; use an exact integer value or a RANGE filter"
            ),
            None,
        ));
    }
    Err(QqlError::validation(
        "QQL-GRPC-LIST-TYPE",
        format!(
            "IN/NOT IN list entry {value} cannot be represented on the gRPC path: the bundled Qdrant proto only supports homogeneous string or int64 lists"
        ),
        None,
    ))
}

fn to_match(mv: &MatchValue) -> Result<qdrant::Match, QqlError> {
    use qdrant::r#match::MatchValue as Mv;
    match mv {
        MatchValue::Value { value } => {
            if let Some(s) = value.as_str() {
                Ok(qdrant::Match {
                    match_value: Some(Mv::Keyword(s.into())),
                })
            } else if let Some(b) = value.as_bool() {
                Ok(qdrant::Match {
                    match_value: Some(Mv::Boolean(b)),
                })
            } else if let Some(n) = value.as_i64() {
                Ok(qdrant::Match {
                    match_value: Some(Mv::Integer(n)),
                })
            } else if let Some(f) = value.as_f64() {
                // The pinned proto's `Match` has no `double_value` field, so a
                // float equality filter (`WHERE x = 1.5`) has no exact wire
                // representation. An integral value can be carried as an
                // integer match (Qdrant compares payload numbers numerically);
                // anything else returns a structured error instead of emitting
                // an empty `Match` and silently matching nothing.
                let integral = f.is_finite()
                    && f.fract() == 0.0
                    && f >= i64::MIN as f64
                    && f <= i64::MAX as f64
                    && (f as i64) as f64 == f;
                if integral {
                    Ok(qdrant::Match {
                        match_value: Some(Mv::Integer(f as i64)),
                    })
                } else {
                    Err(QqlError::validation(
                        "QQL-GRPC-FLOAT-MATCH",
                        format!(
                            "float equality value {f} cannot be represented on the gRPC path: the bundled Qdrant proto's Match has no double field; use a RANGE filter or an exact integer value"
                        ),
                        None,
                    ))
                }
            } else {
                Err(QqlError::validation(
                    "QQL-GRPC-MATCH-VALUE",
                    format!(
                        "match value {} cannot be represented on the gRPC path",
                        value
                    ),
                    None,
                ))
            }
        }
        MatchValue::Text { text } => Ok(qdrant::Match {
            match_value: Some(Mv::Text(text.clone())),
        }),
        MatchValue::TextAny { text } => Ok(qdrant::Match {
            match_value: Some(Mv::TextAny(text.clone())),
        }),
        MatchValue::Any { any } => exact_list_match(any, true),
        MatchValue::Except { except } => exact_list_match(except, false),
        MatchValue::Phrase { phrase } => Ok(qdrant::Match {
            match_value: Some(Mv::Phrase(phrase.clone())),
        }),
    }
}

fn to_point_id(id: &PlanPointId) -> qdrant::PointId {
    match id {
        PlanPointId::Number(n) => qdrant::PointId {
            point_id_options: Some(qdrant::point_id::PointIdOptions::Num(*n)),
        },
        PlanPointId::String(s) => qdrant::PointId {
            point_id_options: Some(qdrant::point_id::PointIdOptions::Uuid(s.clone())),
        },
    }
}

fn to_payload_selector(ps: &PayloadSelectorReq) -> qdrant::WithPayloadSelector {
    match ps {
        PayloadSelectorReq::All(b) => qdrant::WithPayloadSelector {
            selector_options: Some(qdrant::with_payload_selector::SelectorOptions::Enable(*b)),
        },
        PayloadSelectorReq::Include { include } => qdrant::WithPayloadSelector {
            selector_options: Some(qdrant::with_payload_selector::SelectorOptions::Include(
                qdrant::PayloadIncludeSelector {
                    fields: include.clone(),
                },
            )),
        },
        PayloadSelectorReq::Exclude { exclude } => qdrant::WithPayloadSelector {
            selector_options: Some(qdrant::with_payload_selector::SelectorOptions::Exclude(
                qdrant::PayloadExcludeSelector {
                    fields: exclude.clone(),
                },
            )),
        },
    }
}

fn to_vectors_selector(vs: &VectorSelectorReq) -> qdrant::WithVectorsSelector {
    match vs {
        VectorSelectorReq::All(b) => qdrant::WithVectorsSelector {
            selector_options: Some(qdrant::with_vectors_selector::SelectorOptions::Enable(*b)),
        },
        VectorSelectorReq::Names(names) => qdrant::WithVectorsSelector {
            selector_options: Some(qdrant::with_vectors_selector::SelectorOptions::Include(
                qdrant::VectorsSelector {
                    names: names.clone(),
                },
            )),
        },
    }
}

fn to_search_params(params: &qql_plan::types::SearchParamsRequest) -> qdrant::SearchParams {
    qdrant::SearchParams {
        hnsw_ef: params.hnsw_ef,
        exact: params.exact,
        indexed_only: params.indexed_only,
        quantization: params
            .quantization
            .as_ref()
            .map(|q| qdrant::QuantizationSearchParams {
                ignore: q.ignore,
                rescore: q.rescore,
                oversampling: q.oversampling,
            }),
        acorn: params.acorn.as_ref().map(|a| qdrant::AcornSearchParams {
            enable: Some(a.enable),
            max_selectivity: a.max_selectivity,
        }),
    }
}

fn plan_vector_to_proto(v: &PlanVectorValue) -> qdrant::Vector {
    match v {
        PlanVectorValue::Dense(data) => qdrant::Vector {
            vector: Some(qdrant::vector::Vector::Dense(qdrant::DenseVector {
                data: data.clone(),
            })),
            ..Default::default()
        },
        PlanVectorValue::Sparse { indices, values } => qdrant::Vector {
            vector: Some(qdrant::vector::Vector::Sparse(qdrant::SparseVector {
                indices: indices.clone(),
                values: values.clone(),
            })),
            ..Default::default()
        },
        PlanVectorValue::MultiDense(rows) => qdrant::Vector {
            vector: Some(qdrant::vector::Vector::MultiDense(
                qdrant::MultiDenseVector {
                    vectors: rows
                        .iter()
                        .map(|row| qdrant::DenseVector { data: row.clone() })
                        .collect(),
                },
            )),
            ..Default::default()
        },
    }
}

fn to_vectors(vectors: &PlanPointVectors) -> Option<qdrant::Vectors> {
    match vectors {
        PlanPointVectors::Unnamed(v) => Some(qdrant::Vectors {
            vectors_options: Some(qdrant::vectors::VectorsOptions::Vector(
                plan_vector_to_proto(v),
            )),
        }),
        PlanPointVectors::Named(entries) => {
            let mut map = std::collections::HashMap::new();
            for (name, v) in entries {
                map.insert(name.clone(), plan_vector_to_proto(v));
            }
            Some(qdrant::Vectors {
                vectors_options: Some(qdrant::vectors::VectorsOptions::Vectors(
                    qdrant::NamedVectors { vectors: map },
                )),
            })
        }
    }
}

fn ast_formula_to_grpc(expr: &qql_core::ast::FormulaExpr) -> Option<qdrant::Expression> {
    use qdrant::expression::Variant;
    match expr {
        qql_core::ast::FormulaExpr::Constant { value } => Some(qdrant::Expression {
            variant: Some(Variant::Constant(*value as f32)),
        }),
        qql_core::ast::FormulaExpr::Variable { name } => Some(qdrant::Expression {
            variant: Some(Variant::Variable(if name == "score" {
                "$score".to_string()
            } else {
                name.clone()
            })),
        }),
        qql_core::ast::FormulaExpr::Sum { left, right } => {
            let l = ast_formula_to_grpc(left)?;
            let r = ast_formula_to_grpc(right)?;
            Some(qdrant::Expression {
                variant: Some(Variant::Sum(qdrant::SumExpression { sum: vec![l, r] })),
            })
        }
        qql_core::ast::FormulaExpr::Sub { left, right } => {
            let l = ast_formula_to_grpc(left)?;
            let r = ast_formula_to_grpc(right)?;
            let neg_r = qdrant::Expression {
                variant: Some(Variant::Neg(Box::new(r))),
            };
            Some(qdrant::Expression {
                variant: Some(Variant::Sum(qdrant::SumExpression {
                    sum: vec![l, neg_r],
                })),
            })
        }
        qql_core::ast::FormulaExpr::Mul { left, right } => {
            let l = ast_formula_to_grpc(left)?;
            let r = ast_formula_to_grpc(right)?;
            Some(qdrant::Expression {
                variant: Some(Variant::Mult(qdrant::MultExpression { mult: vec![l, r] })),
            })
        }
        qql_core::ast::FormulaExpr::Div {
            left,
            right,
            by_zero_default,
        } => {
            let l = ast_formula_to_grpc(left)?;
            let r = ast_formula_to_grpc(right)?;
            Some(qdrant::Expression {
                variant: Some(Variant::Div(Box::new(qdrant::DivExpression {
                    left: Some(Box::new(l)),
                    right: Some(Box::new(r)),
                    by_zero_default: by_zero_default.map(|f| f as f32),
                }))),
            })
        }
        qql_core::ast::FormulaExpr::Neg { operand } => {
            let inner = ast_formula_to_grpc(operand)?;
            Some(qdrant::Expression {
                variant: Some(Variant::Neg(Box::new(inner))),
            })
        }
        qql_core::ast::FormulaExpr::Abs { x } => {
            let inner = ast_formula_to_grpc(x)?;
            Some(qdrant::Expression {
                variant: Some(Variant::Abs(Box::new(inner))),
            })
        }
        qql_core::ast::FormulaExpr::Sqrt { x } => {
            let inner = ast_formula_to_grpc(x)?;
            Some(qdrant::Expression {
                variant: Some(Variant::Sqrt(Box::new(inner))),
            })
        }
        qql_core::ast::FormulaExpr::Log { x } => {
            let inner = ast_formula_to_grpc(x)?;
            Some(qdrant::Expression {
                variant: Some(Variant::Log10(Box::new(inner))),
            })
        }
        qql_core::ast::FormulaExpr::Ln { x } => {
            let inner = ast_formula_to_grpc(x)?;
            Some(qdrant::Expression {
                variant: Some(Variant::Ln(Box::new(inner))),
            })
        }
        qql_core::ast::FormulaExpr::Exp { x } => {
            let inner = ast_formula_to_grpc(x)?;
            Some(qdrant::Expression {
                variant: Some(Variant::Exp(Box::new(inner))),
            })
        }
        qql_core::ast::FormulaExpr::Pow { base, exponent } => {
            let b = ast_formula_to_grpc(base)?;
            let e = ast_formula_to_grpc(exponent)?;
            Some(qdrant::Expression {
                variant: Some(Variant::Pow(Box::new(qdrant::PowExpression {
                    base: Some(Box::new(b)),
                    exponent: Some(Box::new(e)),
                }))),
            })
        }
        qql_core::ast::FormulaExpr::GeoDistance { lat, lon, field } => Some(qdrant::Expression {
            variant: Some(Variant::GeoDistance(qdrant::GeoDistance {
                origin: Some(qdrant::GeoPoint {
                    lat: *lat,
                    lon: *lon,
                }),
                to: field.clone(),
            })),
        }),
        qql_core::ast::FormulaExpr::Decay {
            kind,
            x,
            target,
            scale,
            midpoint,
        } => {
            let x_expr = ast_formula_to_grpc(x)?;
            let target_expr = match target {
                Some(t) => Some(Box::new(ast_formula_to_grpc(t)?)),
                None => None,
            };
            let decay = Box::new(qdrant::DecayParamsExpression {
                x: Some(Box::new(x_expr)),
                target: target_expr,
                scale: scale.map(|f| f as f32),
                midpoint: midpoint.map(|f| f as f32),
            });
            let variant = match kind.to_ascii_lowercase().as_str() {
                "exp" | "exp_decay" => Variant::ExpDecay(decay),
                "lin" | "lin_decay" => Variant::LinDecay(decay),
                _ => Variant::GaussDecay(decay),
            };
            Some(qdrant::Expression {
                variant: Some(variant),
            })
        }
        _ => to_formula_expression(&qql_plan::query::lower_formula_expr(expr)),
    }
}

fn to_formula_expression(val: &serde_json::Value) -> Option<qdrant::Expression> {
    use qdrant::expression::Variant;
    // OpenAPI Expression: bare number / bare string / one-key objects with snake_case keys.
    match val {
        serde_json::Value::Number(n) => n.as_f64().map(|f| qdrant::Expression {
            variant: Some(Variant::Constant(f as f32)),
        }),
        serde_json::Value::String(s) => Some(qdrant::Expression {
            variant: Some(Variant::Variable(s.clone())),
        }),
        serde_json::Value::Object(obj) if obj.len() == 1 => {
            let (key, val) = obj.iter().next()?;
            match key.as_str() {
                // REST dialect (qql-plan output) + legacy PascalCase keys
                "Constant" | "constant" => val.as_f64().map(|f| qdrant::Expression {
                    variant: Some(Variant::Constant(f as f32)),
                }),
                "Variable" | "variable" => val.as_str().map(|s| qdrant::Expression {
                    variant: Some(Variant::Variable(s.to_string())),
                }),
                "sum" | "Add" => {
                    let terms: Vec<qdrant::Expression> = val
                        .as_array()?
                        .iter()
                        .filter_map(to_formula_expression)
                        .collect();
                    Some(qdrant::Expression {
                        variant: Some(Variant::Sum(qdrant::SumExpression { sum: terms })),
                    })
                }
                "mult" | "Multiply" => {
                    let terms: Vec<qdrant::Expression> = val
                        .as_array()?
                        .iter()
                        .filter_map(to_formula_expression)
                        .collect();
                    Some(qdrant::Expression {
                        variant: Some(Variant::Mult(qdrant::MultExpression { mult: terms })),
                    })
                }
                "div" | "Divide" => {
                    let obj = val.as_object()?;
                    let left = to_formula_expression(obj.get("left")?)?;
                    let right = to_formula_expression(obj.get("right")?)?;
                    let by_zero_default = obj
                        .get("by_zero_default")
                        .and_then(|v| v.as_f64())
                        .map(|f| f as f32);
                    Some(qdrant::Expression {
                        variant: Some(Variant::Div(Box::new(qdrant::DivExpression {
                            left: Some(Box::new(left)),
                            right: Some(Box::new(right)),
                            by_zero_default,
                        }))),
                    })
                }
                "neg" | "Negate" => {
                    let inner = to_formula_expression(val)?;
                    Some(qdrant::Expression {
                        variant: Some(Variant::Neg(Box::new(inner))),
                    })
                }
                "abs" | "Abs" => {
                    let inner = to_formula_expression(val)?;
                    Some(qdrant::Expression {
                        variant: Some(Variant::Abs(Box::new(inner))),
                    })
                }
                "sqrt" | "Sqrt" => {
                    let inner = to_formula_expression(val)?;
                    Some(qdrant::Expression {
                        variant: Some(Variant::Sqrt(Box::new(inner))),
                    })
                }
                "log10" | "Log10" => {
                    let inner = to_formula_expression(val)?;
                    Some(qdrant::Expression {
                        variant: Some(Variant::Log10(Box::new(inner))),
                    })
                }
                "ln" | "NaturalLog" => {
                    let inner = to_formula_expression(val)?;
                    Some(qdrant::Expression {
                        variant: Some(Variant::Ln(Box::new(inner))),
                    })
                }
                "exp" | "Exp" => {
                    let inner = to_formula_expression(val)?;
                    Some(qdrant::Expression {
                        variant: Some(Variant::Exp(Box::new(inner))),
                    })
                }
                "pow" | "Pow" => {
                    if let Some(arr) = val.as_array() {
                        Some(qdrant::Expression {
                            variant: Some(Variant::Pow(Box::new(qdrant::PowExpression {
                                base: Some(Box::new(to_formula_expression(&arr[0])?)),
                                exponent: Some(Box::new(to_formula_expression(&arr[1])?)),
                            }))),
                        })
                    } else {
                        let obj = val.as_object()?;
                        Some(qdrant::Expression {
                            variant: Some(Variant::Pow(Box::new(qdrant::PowExpression {
                                base: Some(Box::new(to_formula_expression(obj.get("base")?)?)),
                                exponent: Some(Box::new(to_formula_expression(
                                    obj.get("exponent")?,
                                )?)),
                            }))),
                        })
                    }
                }
                "geo_distance" | "GeoDistance" => {
                    let obj = val.as_object()?;
                    let origin = obj.get("origin")?;
                    let lat = origin
                        .get("lat")
                        .or_else(|| origin.get("latitude"))
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0);
                    let lon = origin
                        .get("lon")
                        .or_else(|| origin.get("longitude"))
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0);
                    let to = obj
                        .get("to")
                        .or_else(|| obj.get("field"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    Some(qdrant::Expression {
                        variant: Some(Variant::GeoDistance(qdrant::GeoDistance {
                            origin: Some(qdrant::GeoPoint { lat, lon }),
                            to,
                        })),
                    })
                }
                "exp_decay" | "gauss_decay" | "lin_decay" => {
                    let obj = val.as_object()?;
                    let x = to_formula_expression(obj.get("x")?)?;
                    let target = obj.get("target").and_then(to_formula_expression);
                    let scale = obj.get("scale").and_then(|v| v.as_f64()).map(|f| f as f32);
                    let midpoint = obj
                        .get("midpoint")
                        .and_then(|v| v.as_f64())
                        .map(|f| f as f32);
                    let decay = Box::new(qdrant::DecayParamsExpression {
                        x: Some(Box::new(x)),
                        target: target.map(Box::new),
                        scale,
                        midpoint,
                    });
                    let variant = match key.as_str() {
                        "exp_decay" => Variant::ExpDecay(decay),
                        "lin_decay" => Variant::LinDecay(decay),
                        _ => Variant::GaussDecay(decay),
                    };
                    Some(qdrant::Expression {
                        variant: Some(variant),
                    })
                }
                "datetime" | "DateTime" => val.as_str().map(|s| qdrant::Expression {
                    variant: Some(Variant::Datetime(s.to_string())),
                }),
                "datetime_key" | "DateTimeField" => val.as_str().map(|s| qdrant::Expression {
                    variant: Some(Variant::DatetimeKey(s.to_string())),
                }),
                // Field condition used as boolean Expression (key + match/range/...)
                "key" => {
                    // Reconstruct single-key object for condition parser
                    to_condition_from_json(&serde_json::Value::Object(obj.clone())).map(|c| {
                        qdrant::Expression {
                            variant: Some(Variant::Condition(c)),
                        }
                    })
                }
                _ => {
                    // Try as a filter condition object (must/should/must_not or field clause).
                    to_condition_from_json(val).map(|c| qdrant::Expression {
                        variant: Some(Variant::Condition(c)),
                    })
                }
            }
        }
        // Multi-key objects: likely a field condition {key, match}
        serde_json::Value::Object(obj) => {
            to_condition_from_json(&serde_json::Value::Object(obj.clone())).map(|c| {
                qdrant::Expression {
                    variant: Some(Variant::Condition(c)),
                }
            })
        }
        _ => None,
    }
}

fn to_condition_from_json(val: &serde_json::Value) -> Option<qdrant::Condition> {
    match val {
        serde_json::Value::Object(obj) if obj.len() == 1 => {
            let (key, inner) = obj.iter().next()?;
            match key.as_str() {
                "And" => {
                    let conditions: Vec<qdrant::Condition> = inner
                        .as_array()?
                        .iter()
                        .filter_map(to_condition_from_json)
                        .collect();
                    Some(qdrant::Condition {
                        condition_one_of: Some(qdrant::condition::ConditionOneOf::Filter(
                            qdrant::Filter {
                                must: conditions,
                                ..Default::default()
                            },
                        )),
                    })
                }
                "Or" => {
                    let conditions: Vec<qdrant::Condition> = inner
                        .as_array()?
                        .iter()
                        .filter_map(to_condition_from_json)
                        .collect();
                    Some(qdrant::Condition {
                        condition_one_of: Some(qdrant::condition::ConditionOneOf::Filter(
                            qdrant::Filter {
                                should: conditions,
                                ..Default::default()
                            },
                        )),
                    })
                }
                "Not" => {
                    let inner_cond = to_condition_from_json(inner)?;
                    Some(qdrant::Condition {
                        condition_one_of: Some(qdrant::condition::ConditionOneOf::Filter(
                            qdrant::Filter {
                                must_not: vec![inner_cond],
                                ..Default::default()
                            },
                        )),
                    })
                }
                "Compare" => {
                    let obj = inner.as_object()?;
                    let field = obj.get("field")?.as_str()?;
                    let op = obj.get("op")?.as_str()?;
                    let value = obj.get("value")?;
                    let range = match op {
                        "Eq" => qdrant::Range {
                            gte: value.as_f64(),
                            lte: value.as_f64(),
                            ..Default::default()
                        },
                        "Gt" => qdrant::Range {
                            gt: value.as_f64(),
                            ..Default::default()
                        },
                        "Gte" => qdrant::Range {
                            gte: value.as_f64(),
                            ..Default::default()
                        },
                        "Lt" => qdrant::Range {
                            lt: value.as_f64(),
                            ..Default::default()
                        },
                        "Lte" => qdrant::Range {
                            lte: value.as_f64(),
                            ..Default::default()
                        },
                        _ => return None,
                    };
                    Some(qdrant::Condition {
                        condition_one_of: Some(qdrant::condition::ConditionOneOf::Field(
                            qdrant::FieldCondition {
                                key: field.to_string(),
                                range: Some(range),
                                ..Default::default()
                            },
                        )),
                    })
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn to_qdrant_value(val: serde_json::Value) -> qdrant::Value {
    use qdrant::value::Kind;
    match val {
        serde_json::Value::Null => qdrant::Value { kind: None },
        serde_json::Value::Bool(b) => qdrant::Value {
            kind: Some(Kind::BoolValue(b)),
        },
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                qdrant::Value {
                    kind: Some(Kind::IntegerValue(i)),
                }
            } else {
                qdrant::Value {
                    kind: Some(Kind::DoubleValue(n.as_f64().unwrap_or(0.0))),
                }
            }
        }
        serde_json::Value::String(s) => qdrant::Value {
            kind: Some(Kind::StringValue(s)),
        },
        serde_json::Value::Array(arr) => qdrant::Value {
            kind: Some(Kind::ListValue(qdrant::ListValue {
                values: arr.into_iter().map(to_qdrant_value).collect(),
            })),
        },
        serde_json::Value::Object(obj) => {
            let fields = obj
                .into_iter()
                .map(|(k, v)| (k, to_qdrant_value(v)))
                .collect();
            qdrant::Value {
                kind: Some(Kind::StructValue(qdrant::Struct { fields })),
            }
        }
    }
}

// ── Proto response → JSON conversion ─────────────────────────────

fn point_id_to_json(id: &qdrant::PointId) -> serde_json::Value {
    match &id.point_id_options {
        Some(qdrant::point_id::PointIdOptions::Num(n)) => serde_json::json!(*n),
        Some(qdrant::point_id::PointIdOptions::Uuid(s)) => serde_json::json!(s),
        None => serde_json::Value::Null,
    }
}

fn group_id_to_json(id: &qdrant::GroupId) -> serde_json::Value {
    match &id.kind {
        Some(qdrant::group_id::Kind::UnsignedValue(n)) => serde_json::json!(*n),
        Some(qdrant::group_id::Kind::IntegerValue(i)) => serde_json::json!(*i),
        Some(qdrant::group_id::Kind::StringValue(s)) => serde_json::json!(s),
        None => serde_json::Value::Null,
    }
}

fn qdrant_value_to_json(v: &qdrant::Value) -> serde_json::Value {
    use qdrant::value::Kind;
    match &v.kind {
        None | Some(Kind::NullValue(_)) => serde_json::Value::Null,
        Some(Kind::DoubleValue(d)) => serde_json::json!(*d),
        Some(Kind::IntegerValue(i)) => serde_json::json!(*i),
        Some(Kind::StringValue(s)) => serde_json::json!(s),
        Some(Kind::BoolValue(b)) => serde_json::json!(*b),
        Some(Kind::ListValue(l)) => {
            serde_json::Value::Array(l.values.iter().map(qdrant_value_to_json).collect())
        }
        Some(Kind::StructValue(s)) => serde_json::Value::Object(
            s.fields
                .iter()
                .map(|(k, v)| (k.clone(), qdrant_value_to_json(v)))
                .collect(),
        ),
    }
}

fn vector_output_to_json(vo: &qdrant::VectorOutput) -> serde_json::Value {
    use qdrant::vector_output;
    match &vo.vector {
        Some(vector_output::Vector::Dense(d)) => {
            serde_json::Value::Array(d.data.iter().map(|f| serde_json::json!(*f)).collect())
        }
        Some(vector_output::Vector::Sparse(s)) => serde_json::json!({
            "indices": s.indices,
            "values": s.values,
        }),
        Some(vector_output::Vector::MultiDense(m)) => serde_json::Value::Array(
            m.vectors
                .iter()
                .map(|d| {
                    serde_json::Value::Array(d.data.iter().map(|f| serde_json::json!(*f)).collect())
                })
                .collect(),
        ),
        None => serde_json::Value::Null,
    }
}

fn vectors_output_to_json(v: &qdrant::VectorsOutput) -> serde_json::Value {
    use qdrant::vectors_output::VectorsOptions;
    match &v.vectors_options {
        Some(VectorsOptions::Vector(vo)) => vector_output_to_json(vo),
        Some(VectorsOptions::Vectors(named)) => {
            let mut map = serde_json::Map::new();
            for (name, vec) in &named.vectors {
                map.insert(name.clone(), vector_output_to_json(vec));
            }
            serde_json::Value::Object(map)
        }
        None => serde_json::Value::Null,
    }
}

fn scored_point_to_json(p: qdrant::ScoredPoint) -> serde_json::Value {
    let id =
        p.id.as_ref()
            .map_or(serde_json::Value::Null, point_id_to_json);
    let payload = serde_json::Value::Object(
        p.payload
            .into_iter()
            .map(|(k, v)| (k, qdrant_value_to_json(&v)))
            .collect(),
    );
    let mut obj = serde_json::Map::new();
    obj.insert("id".into(), id);
    obj.insert("score".into(), serde_json::json!(p.score));
    obj.insert("payload".into(), payload);
    if p.version != 0 {
        obj.insert("version".into(), serde_json::json!(p.version));
    }
    if let Some(vectors) = &p.vectors {
        obj.insert("vector".into(), vectors_output_to_json(vectors));
    }
    serde_json::Value::Object(obj)
}

fn retrieved_point_to_json(p: qdrant::RetrievedPoint) -> serde_json::Value {
    let id =
        p.id.as_ref()
            .map_or(serde_json::Value::Null, point_id_to_json);
    let payload = serde_json::Value::Object(
        p.payload
            .into_iter()
            .map(|(k, v)| (k, qdrant_value_to_json(&v)))
            .collect(),
    );
    let mut obj = serde_json::Map::new();
    obj.insert("id".into(), id);
    obj.insert("payload".into(), payload);
    if let Some(vectors) = &p.vectors {
        obj.insert("vector".into(), vectors_output_to_json(vectors));
    }
    serde_json::Value::Object(obj)
}

fn groups_result_to_json(r: qdrant::GroupsResult) -> serde_json::Value {
    serde_json::json!({
        "groups": r.groups.into_iter().map(point_group_to_json).collect::<Vec<_>>(),
    })
}

fn batch_result_to_json(r: qdrant::BatchResult) -> serde_json::Value {
    let points: Vec<_> = r.result.into_iter().map(scored_point_to_json).collect();
    serde_json::json!({ "result": { "points": points } })
}

fn point_group_to_json(g: qdrant::PointGroup) -> serde_json::Value {
    let hits: Vec<_> = g.hits.into_iter().map(scored_point_to_json).collect();
    let id =
        g.id.as_ref()
            .map_or(serde_json::Value::Null, group_id_to_json);
    let mut obj = serde_json::Map::new();
    obj.insert("id".into(), id);
    obj.insert("hits".into(), serde_json::json!(hits));
    if let Some(lookup) = g.lookup {
        obj.insert("lookup".into(), retrieved_point_to_json(lookup));
    }
    serde_json::Value::Object(obj)
}

fn list_collections_response_to_json(resp: qdrant::ListCollectionsResponse) -> serde_json::Value {
    serde_json::json!({
        "result": {
            "collections": resp.collections.into_iter()
                .map(|c| serde_json::json!({"name": c.name}))
                .collect::<Vec<_>>(),
        },
        "status": "ok",
        "time": resp.time,
    })
}

fn collection_info_to_json(resp: qdrant::GetCollectionInfoResponse) -> serde_json::Value {
    let info = resp.result.map(|info| {
        let mut obj = serde_json::Map::new();
        obj.insert("status".into(), serde_json::json!(info.status));
        if let Some(os) = info.optimizer_status {
            obj.insert("optimizer_status".into(), serde_json::json!(os.ok));
        }
        obj.insert(
            "segments_count".into(),
            serde_json::json!(info.segments_count),
        );
        if let Some(pc) = info.points_count {
            obj.insert("points_count".into(), serde_json::json!(pc));
        }
        if let Some(ivc) = info.indexed_vectors_count {
            obj.insert("indexed_vectors_count".into(), serde_json::json!(ivc));
        }
        if let Some(cfg) = info.config {
            obj.insert("config".into(), collection_config_to_json(&cfg));
        }
        if !info.payload_schema.is_empty() {
            let schema: serde_json::Map<_, _> = info
                .payload_schema
                .iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        serde_json::json!({
                            "data_type": v.data_type,
                            "points": v.points,
                        }),
                    )
                })
                .collect();
            obj.insert("payload_schema".into(), serde_json::Value::Object(schema));
        }
        serde_json::Value::Object(obj)
    });
    serde_json::json!({
        "result": info,
        "status": "ok",
        "time": resp.time,
    })
}

fn collection_config_to_json(c: &qdrant::CollectionConfig) -> serde_json::Value {
    let mut obj = serde_json::Map::new();
    if let Some(params) = &c.params {
        let mut p = serde_json::Map::new();
        p.insert(
            "shard_number".into(),
            serde_json::json!(params.shard_number),
        );
        p.insert(
            "on_disk_payload".into(),
            serde_json::json!(params.on_disk_payload),
        );
        if let Some(vc) = &params.vectors_config {
            p.insert("vectors".into(), vectors_config_to_json(vc));
        }
        if let Some(rf) = params.replication_factor {
            p.insert("replication_factor".into(), serde_json::json!(rf));
        }
        if let Some(wcf) = params.write_consistency_factor {
            p.insert("write_consistency_factor".into(), serde_json::json!(wcf));
        }
        if let Some(rff) = params.read_fan_out_factor {
            p.insert("read_fan_out_factor".into(), serde_json::json!(rff));
        }
        if let Some(svc) = &params.sparse_vectors_config {
            let map: serde_json::Map<_, _> = svc
                .map
                .iter()
                .map(|(k, v)| {
                    let mut entry = serde_json::Map::new();
                    if let Some(sidx) = &v.index {
                        entry.insert(
                            "index".into(),
                            serde_json::json!({
                                "on_disk": sidx.on_disk,
                            }),
                        );
                    }
                    (k.clone(), serde_json::Value::Object(entry))
                })
                .collect();
            p.insert("sparse_vectors".into(), serde_json::Value::Object(map));
        }
        obj.insert("params".into(), serde_json::Value::Object(p));
    }
    if let Some(hnsw) = &c.hnsw_config {
        obj.insert(
            "hnsw_config".into(),
            serde_json::json!({
                "m": hnsw.m,
                "ef_construct": hnsw.ef_construct,
                "full_scan_threshold": hnsw.full_scan_threshold,
                "max_indexing_threads": hnsw.max_indexing_threads,
                "on_disk": hnsw.on_disk,
                "payload_m": hnsw.payload_m,
            }),
        );
    }
    if let Some(opt) = &c.optimizer_config {
        let max_threads = opt
            .max_optimization_threads
            .as_ref()
            .map(|m| match &m.variant {
                Some(qdrant::max_optimization_threads::Variant::Value(n)) => {
                    serde_json::json!(*n)
                }
                Some(qdrant::max_optimization_threads::Variant::Setting(_)) => {
                    serde_json::json!("auto")
                }
                None => serde_json::Value::Null,
            });
        obj.insert(
            "optimizer_config".into(),
            serde_json::json!({
                "deleted_threshold": opt.deleted_threshold,
                "vacuum_min_vector_number": opt.vacuum_min_vector_number,
                "default_segment_number": opt.default_segment_number,
                "max_segment_size": opt.max_segment_size,
                "memmap_threshold": opt.memmap_threshold,
                "indexing_threshold": opt.indexing_threshold,
                "flush_interval_sec": opt.flush_interval_sec,
                "max_optimization_threads": max_threads,
            }),
        );
    }
    if let Some(wal) = &c.wal_config {
        obj.insert(
            "wal_config".into(),
            serde_json::json!({
                "wal_capacity_mb": wal.wal_capacity_mb,
                "wal_segments_ahead": wal.wal_segments_ahead,
            }),
        );
    }
    if let Some(qc) = &c.quantization_config {
        obj.insert(
            "quantization_config".into(),
            quantization_config_to_json(qc),
        );
    }
    if let Some(sm) = &c.strict_mode_config {
        obj.insert(
            "strict_mode_config".into(),
            serde_json::json!({
                "enabled": sm.enabled,
                "max_collection_vector_size_bytes": sm.max_collection_vector_size_bytes,
                "read_rate_limit": sm.read_rate_limit,
                "write_rate_limit": sm.write_rate_limit,
                "max_query_limit": sm.max_query_limit,
            }),
        );
    }
    serde_json::Value::Object(obj)
}

fn quantization_config_to_json(qc: &qdrant::QuantizationConfig) -> serde_json::Value {
    use qdrant::quantization_config::Quantization;
    let mut obj = serde_json::Map::new();
    match &qc.quantization {
        Some(Quantization::Scalar(s)) => {
            obj.insert(
                "scalar".into(),
                serde_json::json!({
                    "r#type": s.r#type,
                    "quantile": s.quantile,
                    "always_ram": s.always_ram,
                }),
            );
        }
        Some(Quantization::Product(p)) => {
            obj.insert(
                "product".into(),
                serde_json::json!({
                    "compression": p.compression,
                    "always_ram": p.always_ram,
                }),
            );
        }
        Some(Quantization::Binary(b)) => {
            obj.insert(
                "binary".into(),
                serde_json::json!({
                    "always_ram": b.always_ram,
                }),
            );
        }
        Some(Quantization::Turboquant(_)) => {}
        None => {}
    }
    serde_json::Value::Object(obj)
}

fn vectors_config_to_json(vc: &qdrant::VectorsConfig) -> serde_json::Value {
    use qdrant::vectors_config::Config;
    match &vc.config {
        Some(Config::Params(p)) => vector_params_to_json(p),
        Some(Config::ParamsMap(pm)) => {
            let map: serde_json::Map<_, _> = pm
                .map
                .iter()
                .map(|(k, v)| (k.clone(), vector_params_to_json(v)))
                .collect();
            serde_json::Value::Object(map)
        }
        None => serde_json::json!({}),
    }
}

fn vector_params_to_json(vp: &qdrant::VectorParams) -> serde_json::Value {
    let mut obj = serde_json::Map::new();
    obj.insert("size".into(), serde_json::json!(vp.size));
    obj.insert(
        "distance".into(),
        serde_json::json!(distance_to_str(vp.distance)),
    );
    if let Some(od) = vp.on_disk {
        obj.insert("on_disk".into(), serde_json::json!(od));
    }
    if let Some(hnsw) = &vp.hnsw_config {
        obj.insert(
            "hnsw_config".into(),
            serde_json::json!({
                "m": hnsw.m,
                "ef_construct": hnsw.ef_construct,
                "full_scan_threshold": hnsw.full_scan_threshold,
                "max_indexing_threads": hnsw.max_indexing_threads,
                "on_disk": hnsw.on_disk,
                "payload_m": hnsw.payload_m,
            }),
        );
    }
    if let Some(qc) = &vp.quantization_config {
        obj.insert(
            "quantization_config".into(),
            quantization_config_to_json(qc),
        );
    }
    if let Some(mv) = &vp.multivector_config {
        obj.insert(
            "multivector_config".into(),
            serde_json::json!({
                "comparator": multivec_comp_to_str(mv.comparator),
            }),
        );
    }
    serde_json::Value::Object(obj)
}

fn distance_to_str(d: i32) -> &'static str {
    match d {
        1 => "Cosine",
        2 => "Euclid",
        3 => "Dot",
        4 => "Manhattan",
        _ => "UnknownDistance",
    }
}

fn multivec_comp_to_str(c: i32) -> &'static str {
    match c {
        0 => "MaxSim",
        _ => "MaxSim",
    }
}

/// Test-only re-exports for REST/gRPC parity contract tests.
#[cfg(test)]
pub(crate) mod test_api {
    pub(crate) use super::{to_query_points, to_vector_input};
}

#[cfg(test)]
pub(crate) mod test_api_ddl {
    pub(crate) use super::vector_params;
}

#[cfg(test)]
mod tests {
    use super::*;
    use qql_core::parser::Parser;

    #[test]
    fn dense_vector_params_propagates_datatype() {
        let params = vector_params(&serde_json::json!({
            "size": 128,
            "distance": "Cosine",
            "datatype": "uint8",
        }));
        assert_eq!(params.datatype, Some(qdrant::Datatype::Uint8 as i32));

        let f16 = vector_params(&serde_json::json!({
            "size": 64,
            "distance": "Dot",
            "datatype": "float16",
        }));
        assert_eq!(f16.datatype, Some(qdrant::Datatype::Float16 as i32));

        let none = vector_params(&serde_json::json!({
            "size": 32,
            "distance": "Cosine",
        }));
        assert_eq!(none.datatype, None);
    }

    #[test]
    fn test_grpc_route_conversion_all_statements() {
        let statements = [
            "QUERY TEXT 'search' MODEL 'test-model' FROM docs USING dense LIMIT 10;",
            "QUERY POINTS (1, 2, 'uuid-str') FROM docs WITH PAYLOAD INCLUDE ('title');",
            "SCROLL FROM docs WHERE status = 'active' LIMIT 50;",
            "UPSERT INTO docs VALUES {id: 1, text: 'hello', category: 'tech'} USING DENSE MODEL 'm';",
            "DELETE FROM docs WHERE category = 'old';",
            "UPDATE docs SET VECTOR dense = [0.1, 0.2] WHERE id = 1;",
            "UPDATE docs SET PAYLOAD = {status: 'ok'} WHERE id = 1;",
            "CREATE COLLECTION docs (dense VECTOR(384, COSINE), sparse SPARSE);",
            "ALTER COLLECTION docs WITH HNSW (m = 16);",
            "DROP COLLECTION docs;",
            "CREATE INDEX ON COLLECTION docs FOR title TYPE text;",
            "SHOW COLLECTIONS;",
            "SHOW COLLECTION docs;",
        ];

        for stmt_str in statements {
            let stmt = Parser::parse(stmt_str)
                .unwrap_or_else(|e| panic!("parse failed for {stmt_str}: {e}"));
            let op = qql_plan::plan(&stmt).unwrap();
            match &op {
                qql_plan::PlannedOperation::Query {
                    collection,
                    request,
                } => {
                    let grpc_req = to_query_points(request, collection);
                    assert!(
                        grpc_req.is_ok(),
                        "to_query_points failed for {stmt_str}: {:?}",
                        grpc_req.err()
                    );
                }
                qql_plan::PlannedOperation::QueryGroups {
                    collection,
                    request,
                } => {
                    let grpc_req = to_query_groups(request, collection);
                    assert!(
                        grpc_req.is_ok(),
                        "to_query_groups failed for {stmt_str}: {:?}",
                        grpc_req.err()
                    );
                }
                qql_plan::PlannedOperation::GetPoints { request, .. } => {
                    assert_eq!(request.ids.len(), 3);
                }
                qql_plan::PlannedOperation::Scroll { request, .. } => {
                    assert!(request.filter.is_some());
                }
                qql_plan::PlannedOperation::Upsert { request, .. } => {
                    assert_eq!(request.points.len(), 1);
                }
                qql_plan::PlannedOperation::Delete { request, .. } => {
                    assert!(request.filter.is_some());
                }
                qql_plan::PlannedOperation::UpdateVectors { .. } => {}
                qql_plan::PlannedOperation::UpdatePayload { .. } => {}
                qql_plan::PlannedOperation::CreateCollection { request, .. } => {
                    assert!(request.vectors.is_some() || request.hnsw_config.is_some());
                }
                qql_plan::PlannedOperation::UpdateCollection { .. } => {}
                qql_plan::PlannedOperation::CreateIndex { request, .. } => {
                    assert_eq!(request.field_name, "title");
                }
                qql_plan::PlannedOperation::ClearPayload { .. } => {}
                qql_plan::PlannedOperation::DeleteVectors { .. } => {}
                qql_plan::PlannedOperation::Count { .. } => {}
                qql_plan::PlannedOperation::CreateShardKey { .. } => {}
                qql_plan::PlannedOperation::DropShardKey { .. } => {}
                _ => {}
            }
        }
    }

    #[test]
    fn converts_collection_quantization_and_vector_update() {
        let scalar = qql_plan::QuantizationConfig::Scalar {
            scalar: qql_plan::ScalarQuantization {
                qtype: "int8".into(),
                quantile: Some(0.95),
                always_ram: Some(true),
            },
        };
        let q_proto = quantization_config_from_plan(&scalar);
        assert!(q_proto.is_some());

        let stmt =
            Parser::parse("UPDATE docs SET VECTOR dense = [0.1, 0.2] WHERE id = 1;").unwrap();
        let op = qql_plan::plan(&stmt).unwrap();
        if let qql_plan::PlannedOperation::UpdateVectors { request, .. } = op {
            assert_eq!(request.points.len(), 1);
        } else {
            panic!("expected UpdateVectors");
        }
    }

    /// Query → gRPC with limit, offset, using, score_threshold
    #[test]
    fn query_points_field_level_basics() {
        let stmt = Parser::parse(
            "QUERY TEXT 'search' MODEL 'test-model' FROM my_coll USING dense SCORE THRESHOLD 0.7 LIMIT 10 OFFSET 5;",
        )
        .unwrap();
        let op = qql_plan::plan(&stmt).unwrap();
        let (collection, req) = match &op {
            qql_plan::PlannedOperation::Query {
                collection,
                request,
            } => (collection, request),
            other => panic!("expected Query, got {:?}", other),
        };
        let qp = to_query_points(req, collection).unwrap();

        assert_eq!(qp.collection_name, "my_coll");
        assert_eq!(qp.limit, Some(10));
        assert_eq!(qp.offset, Some(5));
        assert_eq!(qp.using, Some("dense".into()));
        assert_eq!(qp.score_threshold, Some(0.7f32));
        // query variant should be Nearest with vector input
        let query = qp.query.expect("query should be set");
        assert!(matches!(
            query.variant,
            Some(qdrant::query::Variant::Nearest(_))
        ));
    }

    /// Query with WITH PAYLOAD INCLUDE + WITH VECTOR → gRPC selectors
    #[test]
    fn query_points_with_payload_and_vectors() {
        let stmt = Parser::parse(
            "QUERY TEXT 'x' MODEL 'test-model' FROM docs WITH PAYLOAD INCLUDE ('title', 'url') WITH VECTOR (dense) LIMIT 5;",
        )
        .unwrap();
        let op = qql_plan::plan(&stmt).unwrap();
        let (collection, req) = match &op {
            qql_plan::PlannedOperation::Query {
                collection,
                request,
            } => (collection, request),
            other => panic!("expected Query, got {:?}", other),
        };
        let qp = to_query_points(req, collection).unwrap();

        // with_payload → selector_options.Include
        let wp = qp.with_payload.expect("with_payload should be set");
        match wp.selector_options.expect("selector_options should be set") {
            qdrant::with_payload_selector::SelectorOptions::Include(inc) => {
                assert!(inc.fields.contains(&"title".to_string()));
                assert!(inc.fields.contains(&"url".to_string()));
            }
            other => panic!("expected Include, got {:?}", other),
        }
        // with_vectors → selector_options.Include
        let wv = qp.with_vectors.expect("with_vectors should be set");
        match wv.selector_options.expect("selector_options should be set") {
            qdrant::with_vectors_selector::SelectorOptions::Include(inc) => {
                assert_eq!(inc.names, vec!["dense"]);
            }
            other => panic!("expected Include, got {:?}", other),
        }
    }

    /// Query with SHARD KEY → gRPC shard_key_selector
    #[test]
    fn query_points_shard_key() {
        let stmt = Parser::parse(
            "QUERY TEXT 'x' MODEL 'test-model' FROM docs USING dense SHARD 'tenant-42' LIMIT 5;",
        )
        .unwrap();
        let op = qql_plan::plan(&stmt).unwrap();
        let (collection, req) = match &op {
            qql_plan::PlannedOperation::Query {
                collection,
                request,
            } => (collection, request),
            other => panic!("expected Query, got {:?}", other),
        };
        let qp = to_query_points(req, collection).unwrap();

        let sks = qp
            .shard_key_selector
            .expect("shard_key_selector should be set");
        assert_eq!(sks.shard_keys.len(), 1);
        match sks.shard_keys[0].key.as_ref().unwrap() {
            qdrant::shard_key::Key::Keyword(k) => assert_eq!(k, "tenant-42"),
            other => panic!("expected Keyword, got {:?}", other),
        }
    }

    /// Query with WHERE → gRPC filter present with must conditions
    #[test]
    fn query_points_filter_equality() {
        let stmt = Parser::parse(
            "QUERY TEXT 'x' MODEL 'test-model' FROM docs WHERE status = 'active' LIMIT 5;",
        )
        .unwrap();
        let op = qql_plan::plan(&stmt).unwrap();
        let (collection, req) = match &op {
            qql_plan::PlannedOperation::Query {
                collection,
                request,
            } => (collection, request),
            other => panic!("expected Query, got {:?}", other),
        };
        let qp = to_query_points(req, collection).unwrap();

        let filter = qp.filter.expect("filter should be set");
        // Single condition wraps into must
        let must = &filter.must;
        assert_eq!(must.len(), 1, "expected 1 condition in must");
        let cond = &must[0];
        // condition_one_of should be Field with key="status"
        match cond.condition_one_of.as_ref().unwrap() {
            qdrant::condition::ConditionOneOf::Field(fc) => {
                assert_eq!(fc.key, "status");
                // match → Keyword("active")
                let mv = fc.r#match.as_ref().expect("match should be set");
                match mv.match_value.as_ref().unwrap() {
                    qdrant::r#match::MatchValue::Keyword(kw) => assert_eq!(kw, "active"),
                    _ => panic!("expected Keyword match"),
                }
            }
            other => panic!("expected Field condition, got {:?}", other),
        }
    }

    /// Query with AND → gRPC filter with 2 must conditions
    #[test]
    fn query_points_filter_range_compound() {
        let stmt = Parser::parse(
            "QUERY TEXT 'x' MODEL 'test-model' FROM docs WHERE age >= 18 AND age < 65 LIMIT 5;",
        )
        .unwrap();
        let op = qql_plan::plan(&stmt).unwrap();
        let (collection, req) = match &op {
            qql_plan::PlannedOperation::Query {
                collection,
                request,
            } => (collection, request),
            other => panic!("expected Query, got {:?}", other),
        };
        let qp = to_query_points(req, collection).unwrap();
        let filter = qp.filter.expect("filter should be set");
        let must = &filter.must;
        assert_eq!(must.len(), 2, "expected 2 conditions for AND (>= and <)");
        // Both should be Field conditions on key "age" with Range
        for c in must {
            match c.condition_one_of.as_ref().unwrap() {
                qdrant::condition::ConditionOneOf::Field(fc) => {
                    assert_eq!(fc.key, "age");
                    assert!(fc.range.is_some(), "expected Range for age comparison");
                }
                _ => panic!("expected Field condition"),
            }
        }
    }

    /// Group-by query → gRPC QueryPointGroups with lookup
    #[test]
    fn query_points_group_by_with_lookup() {
        let stmt = Parser::parse(
            "QUERY TEXT 'x' MODEL 'test-model' FROM docs GROUP BY category SIZE 3 LOOKUP FROM categories LIMIT 10;",
        )
        .unwrap();
        let op = qql_plan::plan(&stmt).unwrap();
        let (collection, req) = match &op {
            qql_plan::PlannedOperation::QueryGroups {
                collection,
                request,
            } => (collection, request),
            other => panic!("expected QueryGroups, got {:?}", other),
        };
        let qg = to_query_groups(req, collection).unwrap();

        assert_eq!(qg.group_by, "category");
        assert_eq!(qg.group_size, Some(3));
        assert_eq!(qg.limit, Some(10));
        let lookup = qg.with_lookup.expect("with_lookup should be set");
        assert_eq!(lookup.collection, "categories");
    }

    /// CreateCollection → gRPC vectors config + HNSW validation
    #[test]
    fn create_collection_vectors_and_hnsw() {
        let stmt = Parser::parse(
            "CREATE COLLECTION docs (dense VECTOR(384, COSINE)) WITH HNSW (m = 32, ef_construct = 100);",
        )
        .unwrap();
        let op = qql_plan::plan(&stmt).unwrap();
        let req = match &op {
            qql_plan::PlannedOperation::CreateCollection { request, .. } => request,
            other => panic!("expected CreateCollection, got {:?}", other),
        };

        // vectors should contain dense → size 384, distance Cosine
        let vectors = req.vectors.as_ref().expect("vectors should be set");
        let dense = vectors.get("dense").expect("dense vector config missing");
        assert_eq!(dense["size"], 384);
        assert_eq!(dense["distance"], "Cosine");

        // HNSW → gRPC conversion with m + ef_construct
        let hnsw_json = req.hnsw_config.as_ref().expect("hnsw_config should be set");
        let hnsw = hnsw_config_from_plan(hnsw_json);
        assert_eq!(hnsw.m, Some(32));
        assert_eq!(hnsw.ef_construct, Some(100));
    }

    /// Upsert → gRPC point count + shard key
    #[test]
    fn upsert_points_field_level() {
        let stmt = Parser::parse(
            "UPSERT INTO docs VALUES {id: 1, text: 'hello'}, {id: 2, text: 'world'};",
        )
        .unwrap();
        let op = qql_plan::plan(&stmt).unwrap();
        let req = match &op {
            qql_plan::PlannedOperation::Upsert { request, .. } => request,
            other => panic!("expected Upsert, got {:?}", other),
        };

        assert_eq!(req.points.len(), 2);
        assert!(req.shard_key.is_none());
    }

    /// DELETE with compound filter → gRPC must conditions present
    #[test]
    fn delete_with_compound_filter() {
        let stmt = Parser::parse("DELETE FROM docs WHERE category = 'archived' AND priority < 3;")
            .unwrap();
        let op = qql_plan::plan(&stmt).unwrap();
        let req = match &op {
            qql_plan::PlannedOperation::Delete { request, .. } => request,
            other => panic!("expected Delete, got {:?}", other),
        };

        let filter = req.filter.as_ref().expect("delete filter should be set");
        match filter {
            FilterExpression::Compound(fc) => {
                assert_eq!(
                    fc.must.len(),
                    2,
                    "expected 2 conditions in compound AND filter"
                );
            }
            other => panic!(
                "expected Compound filter, got {:?}",
                std::mem::discriminant(other)
            ),
        }
    }

    /// Order-by query → gRPC OrderBy variant + filter
    #[test]
    fn query_order_by_direction() {
        let stmt = Parser::parse(
            "QUERY ORDER BY created_at DESC FROM docs WHERE status = 'active' LIMIT 20;",
        )
        .unwrap();
        let op = qql_plan::plan(&stmt).unwrap();
        let (collection, req) = match &op {
            qql_plan::PlannedOperation::Query {
                collection,
                request,
            } => (collection, request),
            other => panic!("expected Query, got {:?}", other),
        };
        let qp = to_query_points(req, collection).unwrap();

        let query = qp.query.expect("query should be set");
        match query.variant.expect("variant should be set") {
            qdrant::query::Variant::OrderBy(ob) => {
                assert_eq!(ob.key, "created_at");
                assert_eq!(ob.direction, Some(qdrant::Direction::Desc as i32));
            }
            other => panic!("expected OrderBy variant, got {:?}", other),
        }
        // Filter should be present
        assert!(qp.filter.is_some(), "filter should be set for WHERE clause");
    }

    /// RRF `k` + `weights` survive the gRPC conversion (regression: they were
    /// silently dropped into a bare `Fusion(1)`).
    #[test]
    fn grpc_rrf_params_are_transmitted() {
        let stmt = Parser::parse(
            "QUERY HYBRID TEXT 'search' MODEL 'bge' DENSE dense SPARSE sparse FUSION RRF \
             FROM docs PARAMS (rrf_k = 5, rrf_weights = [1.0, 0.5]) LIMIT 10;",
        )
        .unwrap();
        let op = qql_plan::plan(&stmt).unwrap();
        let (collection, req) = match &op {
            qql_plan::PlannedOperation::Query {
                collection,
                request,
            } => (collection, request),
            other => panic!("expected Query, got {:?}", other),
        };
        let qp = to_query_points(req, collection).unwrap();
        use qdrant::query::Variant as Qv;
        match qp.query.as_ref().and_then(|q| q.variant.as_ref()) {
            Some(Qv::Rrf(rrf)) => {
                assert_eq!(rrf.k, Some(5), "rrf k must be transmitted");
                assert_eq!(rrf.weights, vec![1.0, 0.5]);
            }
            other => panic!("expected Rrf variant, got {other:?}"),
        }
    }

    /// RRF values that the pinned proto cannot represent exactly must produce
    /// structured errors, never silent loss.
    #[test]
    fn grpc_rrf_unrepresentable_values_error() {
        // k beyond uint32 range.
        let stmt = Parser::parse(
            "QUERY HYBRID TEXT 'search' MODEL 'bge' DENSE dense SPARSE sparse FUSION RRF \
             FROM docs PARAMS (rrf_k = 4294967296) LIMIT 10;",
        )
        .unwrap();
        let op = qql_plan::plan(&stmt).unwrap();
        let (collection, req) = match &op {
            qql_plan::PlannedOperation::Query {
                collection,
                request,
            } => (collection, request),
            other => panic!("expected Query, got {:?}", other),
        };
        let err = to_query_points(req, collection).unwrap_err();
        assert_eq!(err.kind, qql_core::error::ErrorKind::Validation);
        assert_eq!(err.code, "QQL-GRPC-RRF-K");
        assert!(err.message.contains("rrf_k"));

        // Weight that does not round-trip f64 → f32 exactly (0.1 in f32 is
        // 0.10000000149011612…).
        let stmt = Parser::parse(
            "QUERY HYBRID TEXT 'search' MODEL 'bge' DENSE dense SPARSE sparse FUSION RRF \
             FROM docs PARAMS (rrf_weights = [1.0, 0.1]) LIMIT 10;",
        )
        .unwrap();
        let op = qql_plan::plan(&stmt).unwrap();
        let (collection, req) = match &op {
            qql_plan::PlannedOperation::Query {
                collection,
                request,
            } => (collection, request),
            other => panic!("expected Query, got {:?}", other),
        };
        let err = to_query_points(req, collection).unwrap_err();
        assert_eq!(err.code, "QQL-GRPC-RRF-WEIGHT");
        assert!(err.message.contains("0.1"));
    }

    /// Fusion methods must map to the correct proto enum values (RRF=0,
    /// DBSF=1); previously "rrf" was mapped to 1 (DBSF) and "dbsf" to an
    /// undefined 2.
    #[test]
    fn grpc_fusion_method_maps_to_correct_enum() {
        use qdrant::query::Variant as Qv;

        let stmt = Parser::parse(
            "QUERY HYBRID TEXT 'search' MODEL 'bge' DENSE dense SPARSE sparse FUSION RRF FROM docs LIMIT 10;",
        )
        .unwrap();
        let op = qql_plan::plan(&stmt).unwrap();
        let (collection, req) = match &op {
            qql_plan::PlannedOperation::Query {
                collection,
                request,
            } => (collection, request),
            other => panic!("expected Query, got {:?}", other),
        };
        let qp = to_query_points(req, collection).unwrap();
        match qp.query.as_ref().and_then(|q| q.variant.as_ref()) {
            Some(Qv::Fusion(f)) => assert_eq!(*f, qdrant::Fusion::Rrf as i32),
            other => panic!("expected Fusion(Rrf), got {other:?}"),
        }

        let stmt = Parser::parse(
            "QUERY HYBRID TEXT 'search' MODEL 'bge' DENSE dense SPARSE sparse FUSION DBSF FROM docs LIMIT 10;",
        )
        .unwrap();
        let op = qql_plan::plan(&stmt).unwrap();
        let (collection, req) = match &op {
            qql_plan::PlannedOperation::Query {
                collection,
                request,
            } => (collection, request),
            other => panic!("expected Query, got {:?}", other),
        };
        let qp = to_query_points(req, collection).unwrap();
        match qp.query.as_ref().and_then(|q| q.variant.as_ref()) {
            Some(Qv::Fusion(f)) => assert_eq!(*f, qdrant::Fusion::Dbsf as i32),
            other => panic!("expected Fusion(Dbsf), got {other:?}"),
        }
    }

    /// Float equality filters must not silently lower to an empty `Match`:
    /// integral floats map to an integer match, non-integral floats produce a
    /// structured error (the pinned proto's Match has no double field).
    #[test]
    fn grpc_float_equality_filter_is_explicit() {
        // Non-integral float → structured error.
        let stmt = Parser::parse(
            "QUERY TEXT 'x' MODEL 'test-model' FROM docs WHERE rating = 1.5 LIMIT 5;",
        )
        .unwrap();
        let op = qql_plan::plan(&stmt).unwrap();
        let (collection, req) = match &op {
            qql_plan::PlannedOperation::Query {
                collection,
                request,
            } => (collection, request),
            other => panic!("expected Query, got {:?}", other),
        };
        let err = to_query_points(req, collection).unwrap_err();
        assert_eq!(err.kind, qql_core::error::ErrorKind::Validation);
        assert_eq!(err.code, "QQL-GRPC-FLOAT-MATCH");
        assert!(err.message.contains("1.5"));

        // Integral float → integer match (numerically equivalent in Qdrant).
        let stmt = Parser::parse(
            "QUERY TEXT 'x' MODEL 'test-model' FROM docs WHERE rating = 2.0 LIMIT 5;",
        )
        .unwrap();
        let op = qql_plan::plan(&stmt).unwrap();
        let (collection, req) = match &op {
            qql_plan::PlannedOperation::Query {
                collection,
                request,
            } => (collection, request),
            other => panic!("expected Query, got {:?}", other),
        };
        let qp = to_query_points(req, collection).unwrap();
        let filter = qp.filter.expect("filter should be set");
        let mv = filter.must[0]
            .condition_one_of
            .as_ref()
            .and_then(|c| match c {
                qdrant::condition::ConditionOneOf::Field(f) => f.r#match.as_ref(),
                _ => None,
            })
            .expect("field match");
        match mv.match_value.as_ref().unwrap() {
            qdrant::r#match::MatchValue::Integer(n) => assert_eq!(*n, 2),
            other => panic!("expected Integer(2), got {other:?}"),
        }
    }

    /// GetPoints must wrap hits in `result.points` so hit extraction sees
    /// them (B-3 regression); a bare `result` array would report 0 hits.
    #[test]
    fn grpc_get_points_envelope_keeps_hits_extractable() {
        use crate::executor::dml::query::extract_search_hits;

        let envelope = get_points_envelope(
            vec![
                serde_json::json!({"id": 1, "payload": {"title": "a"}}),
                serde_json::json!({"id": 2, "payload": {"title": "b"}}),
            ],
            0.0,
        );
        assert_eq!(envelope["result"]["points"].as_array().unwrap().len(), 2);
        let hits = extract_search_hits(&envelope);
        assert_eq!(hits.len(), 2, "GetPoints hits must survive hit extraction");
        assert_eq!(hits[0].id, "1");
        assert_eq!(hits[1].id, "2");

        // The empty case still yields zero hits, not an error.
        let envelope = get_points_envelope(Vec::new(), 0.0);
        assert_eq!(extract_search_hits(&envelope).len(), 0);
    }

    /// IN/NOT IN lists must be homogeneous and int64-representable: valid
    /// string/integer lists survive, while mixed lists, non-integral floats,
    /// and `u64` values above `i64::MAX` produce structured errors instead of
    /// being silently dropped or wrapped into negative integers.
    #[test]
    fn grpc_exact_list_match_is_homogeneous_and_fallible() {
        // Homogeneous string list → keywords.
        let mv = to_match(&MatchValue::Any {
            any: vec![serde_json::json!("a"), serde_json::json!("b")],
        })
        .unwrap();
        match mv.match_value.unwrap() {
            qdrant::r#match::MatchValue::Keywords(k) => {
                assert_eq!(k.strings, vec!["a", "b"]);
            }
            other => panic!("expected Keywords, got {other:?}"),
        }

        // Homogeneous string list via Except → except_keywords.
        let mv = to_match(&MatchValue::Except {
            except: vec![serde_json::json!("a"), serde_json::json!("b")],
        })
        .unwrap();
        match mv.match_value.unwrap() {
            qdrant::r#match::MatchValue::ExceptKeywords(k) => {
                assert_eq!(k.strings, vec!["a", "b"]);
            }
            other => panic!("expected ExceptKeywords, got {other:?}"),
        }

        // Homogeneous integer list (positive and negative) → integers.
        let mv = to_match(&MatchValue::Any {
            any: vec![
                serde_json::json!(1),
                serde_json::json!(-2),
                serde_json::json!(3),
            ],
        })
        .unwrap();
        match mv.match_value.unwrap() {
            qdrant::r#match::MatchValue::Integers(k) => {
                assert_eq!(k.integers, vec![1, -2, 3]);
            }
            other => panic!("expected Integers, got {other:?}"),
        }

        // Homogeneous integer list via Except → except_integers.
        let mv = to_match(&MatchValue::Except {
            except: vec![serde_json::json!(10), serde_json::json!(20)],
        })
        .unwrap();
        match mv.match_value.unwrap() {
            qdrant::r#match::MatchValue::ExceptIntegers(k) => {
                assert_eq!(k.integers, vec![10, 20]);
            }
            other => panic!("expected ExceptIntegers, got {other:?}"),
        }

        // Integral floats map to integer matches, mirroring single-value
        // `WHERE x = 2.0` → Integer(2).
        let mv = to_match(&MatchValue::Any {
            any: vec![serde_json::json!(2.0), serde_json::json!(3.0)],
        })
        .unwrap();
        match mv.match_value.unwrap() {
            qdrant::r#match::MatchValue::Integers(k) => {
                assert_eq!(k.integers, vec![2, 3]);
            }
            other => panic!("expected Integers, got {other:?}"),
        }
    }

    #[test]
    fn grpc_exact_list_match_rejects_unrepresentable() {
        // Mixed string + integer list → structured error, not a silent drop
        // of the integer entry.
        let err = to_match(&MatchValue::Any {
            any: vec![serde_json::json!("a"), serde_json::json!(1)],
        })
        .unwrap_err();
        assert_eq!(err.kind, qql_core::error::ErrorKind::Validation);
        assert_eq!(err.code, "QQL-GRPC-LIST-TYPE");
        assert!(err.message.contains("mixes strings"), "{err}");

        // Non-integral floats have no list encoding → structured error.
        let err = to_match(&MatchValue::Any {
            any: vec![serde_json::json!(1.5), serde_json::json!(2.5)],
        })
        .unwrap_err();
        assert_eq!(err.kind, qql_core::error::ErrorKind::Validation);
        assert_eq!(err.code, "QQL-GRPC-LIST-TYPE");
        assert!(err.message.contains("1.5"), "{err}");

        // A float mixed into a string list is caught as heterogeneous.
        let err = to_match(&MatchValue::Except {
            except: vec![serde_json::json!("a"), serde_json::json!(1.5)],
        })
        .unwrap_err();
        assert_eq!(err.code, "QQL-GRPC-LIST-TYPE");
        assert!(err.message.contains("mixes strings"), "{err}");

        // Booleans are unrepresentable in either list form.
        let err = to_match(&MatchValue::Any {
            any: vec![serde_json::json!(true), serde_json::json!(false)],
        })
        .unwrap_err();
        assert_eq!(err.code, "QQL-GRPC-LIST-TYPE");
        assert!(err.message.contains("true"), "{err}");

        // u64 above i64::MAX must error, never wrap into a negative integer.
        let oversized = i64::MAX as u64 + 1;
        let err = to_match(&MatchValue::Any {
            any: vec![serde_json::json!(oversized)],
        })
        .unwrap_err();
        assert_eq!(err.kind, qql_core::error::ErrorKind::Validation);
        assert_eq!(err.code, "QQL-GRPC-LIST-INT");
        assert!(err.message.contains("9223372036854775808"), "{err}");
    }

    /// Errors from list conversion must propagate through the full filter
    /// path (`to_filter` → `to_condition` → `to_match`).
    #[test]
    fn grpc_in_list_errors_propagate_through_filter() {
        let stmt = Parser::parse(
            "QUERY TEXT 'x' MODEL 'test-model' FROM docs WHERE status IN ('a', 1) LIMIT 5;",
        )
        .unwrap();
        let op = qql_plan::plan(&stmt).unwrap();
        let (collection, req) = match &op {
            qql_plan::PlannedOperation::Query {
                collection,
                request,
            } => (collection, request),
            other => panic!("expected Query, got {other:?}"),
        };
        let err = to_query_points(req, collection).unwrap_err();
        assert_eq!(err.kind, qql_core::error::ErrorKind::Validation);
        assert_eq!(err.code, "QQL-GRPC-LIST-TYPE");

        // Non-integral floats in IN also propagate.
        let stmt = Parser::parse(
            "QUERY TEXT 'x' MODEL 'test-model' FROM docs WHERE rating IN (1.5, 2.5) LIMIT 5;",
        )
        .unwrap();
        let op = qql_plan::plan(&stmt).unwrap();
        let (collection, req) = match &op {
            qql_plan::PlannedOperation::Query {
                collection,
                request,
            } => (collection, request),
            other => panic!("expected Query, got {other:?}"),
        };
        let err = to_query_points(req, collection).unwrap_err();
        assert_eq!(err.code, "QQL-GRPC-LIST-TYPE");
        assert!(err.message.contains("1.5"), "{err}");
    }

    /// Valid homogeneous lists survive the full parser → plan → gRPC path.
    #[test]
    fn grpc_in_list_homogeneous_lists_survive_full_path() {
        // String list → keywords.
        let stmt = Parser::parse(
            "QUERY TEXT 'x' MODEL 'test-model' FROM docs WHERE status IN ('a', 'b') LIMIT 5;",
        )
        .unwrap();
        let op = qql_plan::plan(&stmt).unwrap();
        let (collection, req) = match &op {
            qql_plan::PlannedOperation::Query {
                collection,
                request,
            } => (collection, request),
            other => panic!("expected Query, got {other:?}"),
        };
        let qp = to_query_points(req, collection).unwrap();
        let filter = qp.filter.expect("filter should be set");
        let mv = filter.must[0]
            .condition_one_of
            .as_ref()
            .and_then(|c| match c {
                qdrant::condition::ConditionOneOf::Field(f) => f.r#match.as_ref(),
                _ => None,
            })
            .expect("field match");
        match mv.match_value.as_ref().unwrap() {
            qdrant::r#match::MatchValue::Keywords(k) => {
                assert_eq!(k.strings, vec!["a", "b"]);
            }
            other => panic!("expected Keywords, got {other:?}"),
        }

        // Integer list → integers.
        let stmt = Parser::parse(
            "QUERY TEXT 'x' MODEL 'test-model' FROM docs WHERE year IN (2024, 2025) LIMIT 5;",
        )
        .unwrap();
        let op = qql_plan::plan(&stmt).unwrap();
        let (collection, req) = match &op {
            qql_plan::PlannedOperation::Query {
                collection,
                request,
            } => (collection, request),
            other => panic!("expected Query, got {other:?}"),
        };
        let qp = to_query_points(req, collection).unwrap();
        let filter = qp.filter.expect("filter should be set");
        let mv = filter.must[0]
            .condition_one_of
            .as_ref()
            .and_then(|c| match c {
                qdrant::condition::ConditionOneOf::Field(f) => f.r#match.as_ref(),
                _ => None,
            })
            .expect("field match");
        match mv.match_value.as_ref().unwrap() {
            qdrant::r#match::MatchValue::Integers(k) => {
                assert_eq!(k.integers, vec![2024, 2025]);
            }
            other => panic!("expected Integers, got {other:?}"),
        }

        // NOT IN lowers to a `must_not` condition that still carries the
        // keyword list.
        let stmt = Parser::parse(
            "QUERY TEXT 'x' MODEL 'test-model' FROM docs WHERE status NOT IN ('deleted', 'archived') LIMIT 5;",
        )
        .unwrap();
        let op = qql_plan::plan(&stmt).unwrap();
        let (collection, req) = match &op {
            qql_plan::PlannedOperation::Query {
                collection,
                request,
            } => (collection, request),
            other => panic!("expected Query, got {other:?}"),
        };
        let qp = to_query_points(req, collection).unwrap();
        let filter = qp.filter.expect("filter should be set");
        assert_eq!(filter.must_not.len(), 1, "NOT IN should lower to must_not");
        let mv = filter.must_not[0]
            .condition_one_of
            .as_ref()
            .and_then(|c| match c {
                qdrant::condition::ConditionOneOf::Field(f) => f.r#match.as_ref(),
                _ => None,
            })
            .expect("field match");
        match mv.match_value.as_ref().unwrap() {
            qdrant::r#match::MatchValue::Keywords(k) => {
                assert_eq!(k.strings, vec!["deleted", "archived"]);
            }
            other => panic!("expected Keywords, got {other:?}"),
        }
    }
}
