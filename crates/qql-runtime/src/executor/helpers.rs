use qql_core::ast::{self, Value};
use qql_core::error::QqlError;

pub(crate) fn clone_value(val: &Value) -> Value {
    val.clone()
}

pub(crate) fn value_to_json(val: &Value) -> serde_json::Value {
    match val {
        Value::Str(s) => serde_json::Value::String(s.to_string()),
        Value::Int(i) => serde_json::Value::Number((*i).into()),
        Value::Float(f) => {
            serde_json::Value::Number(serde_json::Number::from_f64(*f).unwrap_or_else(|| {
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
                    return Err(QqlError::execution(
                        "QQL-QUANTIZATION",
                        format!(
                            "unsupported TURBO bit depth {}; expected one of 1, 1.5, 2, or 4",
                            bits
                        ),
                        None,
                    ));
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
