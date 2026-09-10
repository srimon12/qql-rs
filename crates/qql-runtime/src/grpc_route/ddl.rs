//! Collection / index DDL converters: typed plan IR → proto.
//!
//! Covers HNSW, optimizers, quantization (scalar / product / binary / turbo),
//! vector params (dense + sparse), collection params, and payload index params.
//! Every input is a plan-owned typed struct; no JSON maps are read here.

#![allow(deprecated)]

use qql_core::ast::{VectorDatatype, VectorDistance};
use qql_core::error::QqlError;
use qql_plan::types::{
    CreateIndexRequest, DenseVectorParams, IndexFieldType, IndexOptions, SparseModifier,
    SparseVectorParams,
};

use crate::grpc::memory::memory_to_proto;
use crate::qdrant_grpc::qdrant;

/// Convert a plan `u64` into the bundled proto's `uint32` field, failing
/// closed instead of silently truncating values the proto cannot express.
pub(crate) fn u32_param(value: u64, field: &str) -> Result<u32, QqlError> {
    u32::try_from(value).map_err(|_| {
        QqlError::validation(
            "QQL-GRPC-DDL-RANGE",
            format!(
                "{field} value {value} cannot be represented on the gRPC path: the bundled Qdrant proto stores it as uint32 (max {})",
                u32::MAX
            ),
            None,
        )
    })
}

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
    let max_optimization_threads = cfg.max_optimization_threads.map(|threads| {
        let variant = match threads {
            qql_plan::MaxOptimizationThreads::Threads(value) => {
                qdrant::max_optimization_threads::Variant::Value(value)
            }
            qql_plan::MaxOptimizationThreads::Auto => {
                qdrant::max_optimization_threads::Variant::Setting(
                    qdrant::max_optimization_threads::Setting::Auto as i32,
                )
            }
        };
        qdrant::MaxOptimizationThreads {
            variant: Some(variant),
        }
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
    let quantization = match cfg {
        qql_plan::QuantizationConfig::Scalar { scalar } => {
            qdrant::quantization_config::Quantization::Scalar(scalar_quantization(scalar))
        }
        qql_plan::QuantizationConfig::Product { product } => {
            qdrant::quantization_config::Quantization::Product(product_quantization(product))
        }
        qql_plan::QuantizationConfig::Binary { binary } => {
            qdrant::quantization_config::Quantization::Binary(binary_quantization(binary))
        }
        qql_plan::QuantizationConfig::Turbo { turbo } => {
            qdrant::quantization_config::Quantization::Turboquant(turbo_quantization(turbo))
        }
    };
    Some(qdrant::QuantizationConfig {
        quantization: Some(quantization),
    })
}

pub(crate) fn quantization_config_diff(
    cfg: &qql_plan::QuantizationConfigDiff,
) -> Option<qdrant::QuantizationConfigDiff> {
    use qdrant::quantization_config_diff::Quantization;
    let quantization = match cfg {
        qql_plan::QuantizationConfigDiff::Disabled => Quantization::Disabled(qdrant::Disabled {}),
        qql_plan::QuantizationConfigDiff::Config(config) => match config {
            qql_plan::QuantizationConfig::Scalar { scalar } => {
                Quantization::Scalar(scalar_quantization(scalar))
            }
            qql_plan::QuantizationConfig::Product { product } => {
                Quantization::Product(product_quantization(product))
            }
            qql_plan::QuantizationConfig::Binary { binary } => {
                Quantization::Binary(binary_quantization(binary))
            }
            qql_plan::QuantizationConfig::Turbo { turbo } => {
                Quantization::Turboquant(turbo_quantization(turbo))
            }
        },
    };
    Some(qdrant::QuantizationConfigDiff {
        quantization: Some(quantization),
    })
}

fn scalar_quantization(scalar: &qql_plan::ScalarQuantization) -> qdrant::ScalarQuantization {
    qdrant::ScalarQuantization {
        r#type: qdrant::QuantizationType::Int8 as i32,
        quantile: scalar.quantile.map(|v| v as f32),
        always_ram: scalar.always_ram,
        memory: scalar.memory.map(memory_to_proto),
    }
}

fn product_quantization(product: &qql_plan::ProductQuantization) -> qdrant::ProductQuantization {
    let compression = match product.compression.as_str() {
        "x8" => qdrant::CompressionRatio::X8,
        "x16" => qdrant::CompressionRatio::X16,
        "x32" => qdrant::CompressionRatio::X32,
        "x64" => qdrant::CompressionRatio::X64,
        _ => qdrant::CompressionRatio::X4,
    };
    qdrant::ProductQuantization {
        compression: compression as i32,
        always_ram: product.always_ram,
        memory: product.memory.map(memory_to_proto),
    }
}

fn binary_quantization(binary: &qql_plan::BinaryQuantization) -> qdrant::BinaryQuantization {
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
    let query_encoding = binary.query_encoding.as_deref().map(|query_encoding| {
        let setting = match query_encoding.to_ascii_lowercase().as_str() {
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
        always_ram: binary.always_ram,
        encoding,
        query_encoding,
        memory: binary.memory.map(memory_to_proto),
    }
}

fn turbo_quantization(turbo: &qql_plan::TurboQuantization) -> qdrant::TurboQuantization {
    let bits = turbo
        .bits
        .as_deref()
        .map(|label| match label.to_ascii_lowercase().as_str() {
            "bits1_5" | "1.5" => qdrant::TurboQuantBitSize::Bits15 as i32,
            "bits2" | "2" => qdrant::TurboQuantBitSize::Bits2 as i32,
            "bits4" | "4" => qdrant::TurboQuantBitSize::Bits4 as i32,
            _ => qdrant::TurboQuantBitSize::Bits1 as i32,
        });
    qdrant::TurboQuantization {
        always_ram: turbo.always_ram,
        bits,
        memory: turbo.memory.map(memory_to_proto),
    }
}

pub(crate) fn vector_params(params: &DenseVectorParams) -> qdrant::VectorParams {
    qdrant::VectorParams {
        size: params.size,
        distance: distance_to_proto(params.distance),
        hnsw_config: params.hnsw_config.as_ref().map(hnsw_config_from_plan),
        quantization_config: params
            .quantization_config
            .as_ref()
            .and_then(quantization_config_from_plan),
        on_disk: params.on_disk,
        datatype: params.datatype.map(datatype_to_proto),
        memory: params.memory.map(memory_to_proto),
        multivector_config: params
            .multivector_config
            .map(|_| qdrant::MultiVectorConfig {
                comparator: qdrant::MultiVectorComparator::MaxSim as i32,
            }),
    }
}

pub(crate) fn sparse_vector_params(params: &SparseVectorParams) -> qdrant::SparseVectorParams {
    qdrant::SparseVectorParams {
        index: params
            .index
            .as_ref()
            .map(|index| qdrant::SparseIndexConfig {
                full_scan_threshold: index.full_scan_threshold,
                on_disk: index.on_disk,
                datatype: index.datatype.map(datatype_to_proto),
                memory: index.memory.map(memory_to_proto),
            }),
        modifier: Some(match params.modifier {
            SparseModifier::Idf => qdrant::Modifier::Idf as i32,
            SparseModifier::None => qdrant::Modifier::None as i32,
        }),
    }
}

pub(crate) fn collection_params_diff(
    params: &qql_plan::CollectionParams,
) -> Result<qdrant::CollectionParamsDiff, QqlError> {
    Ok(qdrant::CollectionParamsDiff {
        replication_factor: params
            .replication_factor
            .map(|n| u32_param(n, "replication_factor"))
            .transpose()?,
        write_consistency_factor: params
            .write_consistency_factor
            .map(|n| u32_param(n, "write_consistency_factor"))
            .transpose()?,
        on_disk_payload: params.on_disk_payload,
        read_fan_out_factor: params
            .read_fan_out_factor
            .map(|n| u32_param(n, "read_fan_out_factor"))
            .transpose()?,
        read_fan_out_delay_ms: params.read_fan_out_delay_ms,
        payload: params
            .payload
            .as_ref()
            .and_then(|payload| payload.memory)
            .map(|memory| qdrant::PayloadStorageParams {
                memory: Some(memory_to_proto(memory)),
            }),
    })
}

pub(crate) fn payload_index_params(request: &CreateIndexRequest) -> qdrant::PayloadIndexParams {
    use qdrant::payload_index_params::IndexParams;

    let options = &request.options;
    let index_params = match request.field_schema {
        IndexFieldType::Keyword => IndexParams::KeywordIndexParams(qdrant::KeywordIndexParams {
            is_tenant: options.is_tenant,
            on_disk: options.on_disk,
            enable_hnsw: options.enable_hnsw,
            prefix: options.prefix.map(|_| qdrant::KeywordPrefixParams {}),
            memory: options.memory.map(memory_to_proto),
        }),
        IndexFieldType::Integer => IndexParams::IntegerIndexParams(qdrant::IntegerIndexParams {
            lookup: options.lookup,
            range: options.range,
            is_principal: options.is_principal,
            on_disk: options.on_disk,
            enable_hnsw: options.enable_hnsw,
            memory: options.memory.map(memory_to_proto),
        }),
        IndexFieldType::Float => IndexParams::FloatIndexParams(qdrant::FloatIndexParams {
            on_disk: options.on_disk,
            is_principal: options.is_principal,
            enable_hnsw: options.enable_hnsw,
            memory: options.memory.map(memory_to_proto),
        }),
        IndexFieldType::Geo => IndexParams::GeoIndexParams(qdrant::GeoIndexParams {
            on_disk: options.on_disk,
            enable_hnsw: options.enable_hnsw,
            memory: options.memory.map(memory_to_proto),
        }),
        IndexFieldType::Bool => IndexParams::BoolIndexParams(qdrant::BoolIndexParams {
            on_disk: options.on_disk,
            enable_hnsw: options.enable_hnsw,
            memory: options.memory.map(memory_to_proto),
        }),
        IndexFieldType::Datetime => IndexParams::DatetimeIndexParams(qdrant::DatetimeIndexParams {
            on_disk: options.on_disk,
            is_principal: options.is_principal,
            enable_hnsw: options.enable_hnsw,
            memory: options.memory.map(memory_to_proto),
        }),
        IndexFieldType::Uuid => IndexParams::UuidIndexParams(qdrant::UuidIndexParams {
            is_tenant: options.is_tenant,
            on_disk: options.on_disk,
            enable_hnsw: options.enable_hnsw,
            memory: options.memory.map(memory_to_proto),
        }),
        IndexFieldType::Text => IndexParams::TextIndexParams(text_index_params(options)),
    };

    qdrant::PayloadIndexParams {
        index_params: Some(index_params),
    }
}

pub(crate) fn text_index_params(options: &IndexOptions) -> qdrant::TextIndexParams {
    let tokenizer = match options.tokenizer.unwrap_or(qql_plan::TextTokenizer::Word) {
        qql_plan::TextTokenizer::Prefix => qdrant::TokenizerType::Prefix,
        qql_plan::TextTokenizer::Whitespace => qdrant::TokenizerType::Whitespace,
        qql_plan::TextTokenizer::Multilingual => qdrant::TokenizerType::Multilingual,
        qql_plan::TextTokenizer::Word => qdrant::TokenizerType::Word,
    };
    let stopwords = options.stopwords.as_ref().map(|set| qdrant::StopwordsSet {
        languages: Vec::new(),
        custom: set.custom.clone(),
    });
    let stemmer = options.stemmer.as_ref().map(|stemmer| {
        use qdrant::stemming_algorithm::StemmingParams;
        let stemming_params = match stemmer {
            qql_plan::StemmingAlgorithm::Snowball(language) => {
                StemmingParams::Snowball(qdrant::SnowballParams {
                    language: language.clone(),
                })
            }
            qql_plan::StemmingAlgorithm::Disabled => {
                StemmingParams::Disabled(qdrant::DisabledStemmer {})
            }
        };
        qdrant::StemmingAlgorithm {
            stemming_params: Some(stemming_params),
        }
    });
    qdrant::TextIndexParams {
        tokenizer: tokenizer as i32,
        lowercase: options.lowercase,
        min_token_len: options.min_token_len,
        max_token_len: options.max_token_len,
        on_disk: options.on_disk,
        stopwords,
        phrase_matching: options.phrase_matching,
        stemmer,
        ascii_folding: options.ascii_folding,
        enable_hnsw: options.enable_hnsw,
        memory: options.memory.map(memory_to_proto),
    }
}

fn distance_to_proto(distance: VectorDistance) -> i32 {
    match distance {
        VectorDistance::Cosine => qdrant::Distance::Cosine as i32,
        VectorDistance::Dot => qdrant::Distance::Dot as i32,
        VectorDistance::Euclid => qdrant::Distance::Euclid as i32,
        VectorDistance::Manhattan => qdrant::Distance::Manhattan as i32,
    }
}

fn datatype_to_proto(datatype: VectorDatatype) -> i32 {
    match datatype {
        VectorDatatype::Float32 => qdrant::Datatype::Float32 as i32,
        VectorDatatype::Float16 => qdrant::Datatype::Float16 as i32,
        VectorDatatype::Uint8 => qdrant::Datatype::Uint8 as i32,
        VectorDatatype::Turbo4 => qdrant::Datatype::Turbo4 as i32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_query_encoding_maps_every_setting() {
        use qdrant::binary_quantization_query_encoding::{Setting, Variant};
        for (raw, expected) in [
            ("default", Setting::Default),
            ("binary", Setting::Binary),
            ("scalar4bits", Setting::Scalar4Bits),
            ("scalar8bits", Setting::Scalar8Bits),
        ] {
            let binary = qql_plan::BinaryQuantization {
                always_ram: Some(true),
                encoding: None,
                query_encoding: Some(raw.into()),
                memory: None,
            };
            let proto = binary_quantization(&binary);
            assert_eq!(
                proto.query_encoding.expect("query encoding").variant,
                Some(Variant::Setting(expected as i32)),
                "query_encoding = {raw}"
            );
        }
    }

    #[test]
    fn text_stemmer_and_stopwords_convert_to_proto() {
        let options = IndexOptions {
            tokenizer: Some(qql_plan::TextTokenizer::Word),
            stemmer: Some(qql_plan::StemmingAlgorithm::Snowball("english".into())),
            stopwords: Some(qql_plan::StopwordsSet {
                custom: vec!["the".into()],
            }),
            ..Default::default()
        };
        let proto = text_index_params(&options);
        assert_eq!(proto.tokenizer, qdrant::TokenizerType::Word as i32);
        assert_eq!(proto.stopwords.expect("stopwords").custom, ["the"]);
        let stemmer = proto.stemmer.expect("stemmer");
        assert!(matches!(
            stemmer.stemming_params,
            Some(qdrant::stemming_algorithm::StemmingParams::Snowball(_))
        ));

        let disabled = IndexOptions {
            stemmer: Some(qql_plan::StemmingAlgorithm::Disabled),
            ..Default::default()
        };
        let proto = text_index_params(&disabled);
        assert!(matches!(
            proto.stemmer.expect("stemmer").stemming_params,
            Some(qdrant::stemming_algorithm::StemmingParams::Disabled(_))
        ));
    }

    #[test]
    fn collection_params_diff_maps_payload_memory() {
        let params = qql_plan::CollectionParams {
            replication_factor: Some(2),
            payload: Some(qql_plan::PayloadStorageParams {
                memory: Some(qql_plan::MemoryPlacement::Cached),
            }),
            ..Default::default()
        };
        let proto = collection_params_diff(&params).unwrap();
        assert_eq!(proto.replication_factor, Some(2));
        assert_eq!(
            proto.payload.expect("payload").memory,
            Some(qdrant::Memory::Cached as i32)
        );
    }

    #[test]
    fn collection_params_reject_u32_overflow() {
        let params = qql_plan::CollectionParams {
            replication_factor: Some(u64::from(u32::MAX) + 1),
            ..Default::default()
        };
        let err = collection_params_diff(&params).unwrap_err();
        assert_eq!(err.code, "QQL-GRPC-DDL-RANGE");
    }
}
