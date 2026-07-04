use crate::pipeline::{PointId, WithPayload, WithVectors};
use qql_core::ast::{self, Value};
use qql_core::error::QqlError;

pub(crate) fn build_with_payload(sel: Option<&ast::PayloadSelector>) -> Option<WithPayload> {
    let sel = sel?;
    if let Some(enable) = sel.enable {
        return Some(WithPayload {
            enable: Some(enable),
            include: Vec::new(),
            exclude: Vec::new(),
        });
    }
    if !sel.include.is_empty() {
        return Some(WithPayload {
            enable: None,
            include: sel.include.iter().map(|s| s.to_string()).collect(),
            exclude: Vec::new(),
        });
    }
    if !sel.exclude.is_empty() {
        return Some(WithPayload {
            enable: None,
            include: Vec::new(),
            exclude: sel.exclude.iter().map(|s| s.to_string()).collect(),
        });
    }
    None
}

pub(crate) fn build_with_vectors(sel: Option<&ast::VectorsSelector>) -> Option<WithVectors> {
    let sel = sel?;
    if let Some(enable) = sel.enable {
        return Some(WithVectors {
            enable: Some(enable),
            vectors: Vec::new(),
        });
    }
    if !sel.vectors.is_empty() {
        return Some(WithVectors {
            enable: None,
            vectors: sel.vectors.iter().map(|s| s.to_string()).collect(),
        });
    }
    None
}

pub(crate) fn has_mmr(with_clause: Option<&ast::SearchWith>) -> bool {
    match with_clause {
        Some(wc) => wc.mmr_diversity.is_some() || wc.mmr_candidates.is_some(),
        None => false,
    }
}

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
                return Err(QqlError::runtime("negative ID not supported"));
            }
            Ok(PointId::Num(*i as u64))
        }
        Value::Float(f) => {
            let v = *f;
            if v < 0.0 || v > (1u64 << 53) as f64 || v != (v as u64) as f64 {
                return Err(QqlError::runtime(
                    "unsupported point ID: non-integer or oversized float",
                ));
            }
            Ok(PointId::Num(v as u64))
        }
        _ => Err(QqlError::runtime(format!(
            "unsupported point ID type: {:?}",
            val
        ))),
    }
}

pub(crate) fn clone_value(val: &Value<'_>) -> Value<'static> {
    val.to_static()
}

pub(crate) fn value_to_json(val: &Value) -> serde_json::Value {
    match val {
        Value::Str(s) => serde_json::Value::String(s.to_string()),
        Value::Int(i) => serde_json::Value::Number((*i).into()),
        Value::Float(f) => serde_json::Value::Number(
            serde_json::Number::from_f64(*f).unwrap_or(serde_json::Number::from_f64(0.0).unwrap()),
        ),
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
                    return Err(QqlError::runtime(format!(
                        "unsupported TURBO bit depth {}; expected one of 1, 1.5, 2, or 4",
                        bits
                    )));
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
