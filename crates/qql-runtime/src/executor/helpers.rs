use crate::pipeline::{PointId, WithPayload, WithVector};
use qql_core::ast::{self, Value};
use qql_core::error::QqlError;

#[allow(dead_code)]
pub(crate) fn build_with_payload(sel: Option<&ast::PayloadSelector>) -> Option<WithPayload> {
    let sel = sel?;
    match sel {
        ast::PayloadSelector::All => Some(WithPayload {
            enable: Some(true),
            include: Vec::new(),
            exclude: Vec::new(),
        }),
        ast::PayloadSelector::None => None,
        ast::PayloadSelector::Include(fields) => Some(WithPayload {
            enable: None,
            include: fields.iter().map(|s| s.to_string()).collect(),
            exclude: Vec::new(),
        }),
        ast::PayloadSelector::Exclude(fields) => Some(WithPayload {
            enable: None,
            include: Vec::new(),
            exclude: fields.iter().map(|s| s.to_string()).collect(),
        }),
    }
}

#[allow(dead_code)]
pub(crate) fn build_with_vector(sel: Option<&ast::VectorSelector>) -> Option<WithVector> {
    let sel = sel?;
    match sel {
        ast::VectorSelector::All => Some(WithVector {
            enable: Some(true),
            vectors: Vec::new(),
        }),
        ast::VectorSelector::None => None,
        ast::VectorSelector::Names(vectors) => Some(WithVector {
            enable: None,
            vectors: vectors.iter().map(|s| s.to_string()).collect(),
        }),
    }
}

// TODO: MMR fields moved out of SearchParams; re-evaluate how to detect MMR
#[allow(dead_code)]
pub(crate) fn has_mmr(_params: Option<&ast::SearchParams>) -> bool {
    false
}

#[allow(dead_code)]
pub(crate) fn point_id_string(id: &PointId) -> String {
    match id {
        PointId::Num(n) => n.to_string(),
        PointId::Uuid(s) => s.clone(),
    }
}

pub(crate) fn to_point_id_static(val: &ast::Value) -> Result<PointId, QqlError> {
    match val {
        Value::Str(s) => {
            if let Ok(num) = s.parse::<u64>() {
                Ok(PointId::Num(num))
            } else {
                Ok(PointId::Uuid(s.to_string()))
            }
        }
        Value::Int(i) => {
            if *i < 0 {
                return Err(QqlError::execution("QQL-EXECUTION", "negative ID not supported", None));
            }
            Ok(PointId::Num(*i as u64))
        }
        Value::Float(f) => {
            let v = *f;
            if v < 0.0 || v > (1u64 << 53) as f64 || v != (v as u64) as f64 {
                return Err(QqlError::execution("QQL-EXECUTION", 
                    "unsupported point ID: non-integer or oversized float", None));
            }
            Ok(PointId::Num(v as u64))
        }
        _ => Err(QqlError::execution("QQL-EXECUTION", format!(
            "unsupported point ID type: {:?}",
            val
        ), None)),
    }
}

pub(crate) fn clone_value(val: &Value) -> Value {
    val.clone()
}

pub(crate) fn value_to_json(val: &Value) -> serde_json::Value {
    match val {
        Value::Str(s) => serde_json::Value::String(s.to_string()),
        Value::Int(i) => serde_json::Value::Number((*i).into()),
        Value::Float(f) => {
            serde_json::Value::Number(serde_json::Number::from_f64(*f).unwrap_or_else(|| {
                // Fallback: NaN, Inf, or subnormal → serialize as 0
                serde_json::Number::from_f64(0.0).unwrap_or(serde_json::Number::from(0))
            }))
        }
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Null => serde_json::Value::Null,
        Value::Dict(items) => {
            let mut map = serde_json::Map::new();
            for (k, v) in items {
                map.insert(k.to_string(), value_to_json(v));
            }
            serde_json::Value::Object(map)
        }
        Value::List(items) => serde_json::Value::Array(items.iter().map(value_to_json).collect()),
    }
}

pub(crate) fn build_quantization_config(
    quant: &ast::QuantizationConfig,
) -> Result<serde_json::Value, QqlError> {
    let mut config_map = serde_json::Map::new();
    config_map.insert(
        "always_ram".to_string(),
        serde_json::json!(quant.always_ram),
    );

    let key = match quant.qtype {
        ast::QuantizationType::Scalar => {
            config_map.insert("type".to_string(), serde_json::json!("int8"));
            if let Some(quantile) = quant.quantile {
                config_map.insert("quantile".to_string(), serde_json::json!(quantile));
            }
            "scalar"
        }
        ast::QuantizationType::Binary => "binary",
        ast::QuantizationType::Product => {
            config_map.insert("compression".to_string(), serde_json::json!("x4"));
            "product"
        }
        ast::QuantizationType::Turbo => {
            if let Some(bits) = quant.turbo_bits {
                let bit_str = if (bits - 1.0).abs() < f64::EPSILON {
                    "bits1"
                } else if (bits - 1.5).abs() < f64::EPSILON {
                    "bits1_5"
                } else if (bits - 2.0).abs() < f64::EPSILON {
                    "bits2"
                } else if (bits - 4.0).abs() < f64::EPSILON {
                    "bits4"
                } else {
                    return Err(QqlError::execution("QQL-EXECUTION", format!(
                        "unsupported TURBO bit depth {}; expected one of 1, 1.5, 2, or 4",
                        bits
                    ), None));
                };
                config_map.insert("bits".to_string(), serde_json::json!(bit_str));
            }
            "turbo"
        }
    };

    let mut wrapper = serde_json::Map::new();
    wrapper.insert(key.to_string(), serde_json::Value::Object(config_map));
    Ok(serde_json::Value::Object(wrapper))
}
