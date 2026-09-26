//! Quantization lowering: plan quantization configs onto engine types.
//!
//! Pure move from `config_builder.rs` (size hygiene split).

use qdrant_edge::{
    BinaryQuantizationConfig, BinaryQuantizationEncoding, BinaryQuantizationQueryEncoding,
    CompressionRatio, ProductQuantizationConfig, QuantizationConfig, ScalarQuantizationConfig,
    ScalarType,
};
use qql_core::ast::VectorDatatype;
use qql_core::error::QqlError;

use super::shared::{edge_config_error, edge_memory};

/// Lower a plan quantization config onto the engine type.
///
/// Families map field-for-field. Engine `Memory` is not re-exported, so
/// placement is decoded from its keyword through `StrDeserializer` (no
/// `serde_json::Value`). Turbo is the one family that still goes through JSON:
/// `TurboQuantization`, `TurboQuantQuantizationConfig`, and
/// `TurboQuantBitSize` are not re-exported, and unlike the scalar/product/
/// binary configs there is no `From<TurboQuantization>` constructor, so the
/// nested object cannot be built by name.
#[allow(deprecated)]
pub(crate) fn edge_quantization_config(
    config: &qql_plan::QuantizationConfig,
) -> Result<QuantizationConfig, QqlError> {
    match config {
        qql_plan::QuantizationConfig::Scalar { scalar } => {
            if !scalar.qtype.eq_ignore_ascii_case("int8") {
                return Err(edge_config_error(format!(
                    "scalar quantization type '{}' is not supported (expected int8)",
                    scalar.qtype
                )));
            }
            Ok(QuantizationConfig::from(ScalarQuantizationConfig {
                r#type: ScalarType::Int8,
                quantile: scalar.quantile.map(|value| value as f32),
                always_ram: scalar.always_ram,
                memory: scalar.memory.map(edge_memory).transpose()?,
            }))
        }
        qql_plan::QuantizationConfig::Product { product } => {
            Ok(QuantizationConfig::from(ProductQuantizationConfig {
                compression: edge_compression(&product.compression)?,
                always_ram: product.always_ram,
                memory: product.memory.map(edge_memory).transpose()?,
            }))
        }
        qql_plan::QuantizationConfig::Binary { binary } => {
            Ok(QuantizationConfig::from(BinaryQuantizationConfig {
                always_ram: binary.always_ram,
                memory: binary.memory.map(edge_memory).transpose()?,
                encoding: binary
                    .encoding
                    .as_deref()
                    .map(edge_binary_encoding)
                    .transpose()?,
                query_encoding: binary
                    .query_encoding
                    .as_deref()
                    .map(edge_binary_query_encoding)
                    .transpose()?,
            }))
        }
        qql_plan::QuantizationConfig::Turbo { turbo } => {
            // `TurboQuantization` / `TurboQuantQuantizationConfig` /
            // `TurboQuantBitSize` are not re-exported by qdrant-edge 0.8 and
            // have no public constructor, so the nested body is the only way in.
            let mut body = serde_json::Map::new();
            if let Some(bits) = turbo.bits.as_ref() {
                body.insert("bits".into(), serde_json::Value::String(bits.clone()));
            }
            if let Some(always_ram) = turbo.always_ram {
                body.insert("always_ram".into(), serde_json::Value::Bool(always_ram));
            }
            if let Some(memory) = turbo.memory {
                body.insert(
                    "memory".into(),
                    serde_json::Value::String(memory.as_str().to_string()),
                );
            }
            serde_json::from_value(serde_json::json!({ "turbo": body })).map_err(|error| {
                edge_config_error(format!("invalid turbo quantization configuration: {error}"))
            })
        }
    }
}

fn edge_compression(value: &str) -> Result<CompressionRatio, QqlError> {
    match value {
        "x4" => Ok(CompressionRatio::X4),
        "x8" => Ok(CompressionRatio::X8),
        "x16" => Ok(CompressionRatio::X16),
        "x32" => Ok(CompressionRatio::X32),
        "x64" => Ok(CompressionRatio::X64),
        other => Err(edge_config_error(format!(
            "product quantization compression '{other}' is not supported (expected x4, x8, x16, x32, or x64)"
        ))),
    }
}

fn edge_binary_encoding(value: &str) -> Result<BinaryQuantizationEncoding, QqlError> {
    match value {
        "one_bit" => Ok(BinaryQuantizationEncoding::OneBit),
        "two_bits" => Ok(BinaryQuantizationEncoding::TwoBits),
        "one_and_half_bits" => Ok(BinaryQuantizationEncoding::OneAndHalfBits),
        other => Err(edge_config_error(format!(
            "binary quantization encoding '{other}' is not supported (expected one_bit, two_bits, or one_and_half_bits)"
        ))),
    }
}

fn edge_binary_query_encoding(value: &str) -> Result<BinaryQuantizationQueryEncoding, QqlError> {
    match value {
        "default" => Ok(BinaryQuantizationQueryEncoding::Default),
        "binary" => Ok(BinaryQuantizationQueryEncoding::Binary),
        "scalar4bits" => Ok(BinaryQuantizationQueryEncoding::Scalar4Bits),
        "scalar8bits" => Ok(BinaryQuantizationQueryEncoding::Scalar8Bits),
        other => Err(edge_config_error(format!(
            "binary query encoding '{other}' is not supported (expected default, binary, scalar4bits, or scalar8bits)"
        ))),
    }
}

/// Map the plan's dense/sparse datatype onto the engine's storage datatype.
pub(crate) fn edge_datatype(datatype: VectorDatatype) -> qdrant_edge::VectorStorageDatatype {
    match datatype {
        VectorDatatype::Float32 => qdrant_edge::VectorStorageDatatype::Float32,
        VectorDatatype::Float16 => qdrant_edge::VectorStorageDatatype::Float16,
        VectorDatatype::Uint8 => qdrant_edge::VectorStorageDatatype::Uint8,
        VectorDatatype::Turbo4 => qdrant_edge::VectorStorageDatatype::Turbo4,
    }
}
