//! Decode collection/vector quantization configs and their ALTER diffs.

use serde_json::Value;

use crate::ConvertError;
use crate::decode::config::memory;
use crate::json::{self, child, invalid};
use qql_core::ast::{QuantizationConfig, QuantizationType, QuantizationUpdate};

/// Decode a `QuantizationConfig` (`{scalar|product|binary|turbo: {…}}`).
pub(crate) fn quantization(value: &Value, path: &str) -> Result<QuantizationConfig, ConvertError> {
    let obj = json::object(value, path)?;
    let variants: Vec<&str> = ["scalar", "product", "binary", "turbo"]
        .into_iter()
        .filter(|key| obj.contains_key(*key))
        .collect();
    let [kind] = variants.as_slice() else {
        return Err(invalid(
            path,
            "quantization config must contain exactly one of scalar / product / binary / turbo",
        ));
    };
    let params_path = child(path, kind);
    let params = json::object(obj.get(*kind).expect("variant present"), &params_path)?;
    // Each family accepts only its own sub-fields: misplaced keys (e.g.
    // `quantile` on binary, `bits` on scalar) fail closed instead of being
    // silently dropped.
    let allowed: &[&str] = match *kind {
        "scalar" => &["type", "quantile", "always_ram", "memory"],
        "product" => &["compression", "always_ram", "memory"],
        "binary" => &["always_ram", "memory", "encoding", "query_encoding"],
        _ => &["bits", "always_ram", "memory"],
    };
    for key in params.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(invalid(
                child(&params_path, key),
                format!("unknown {kind} quantization option '{key}'"),
            ));
        }
    }
    // `always_ram` is a plain bool in the AST: QQL plans always serialize it.
    let always_ram = json::opt_bool(params, "always_ram", &params_path)?.unwrap_or(false);
    let memory = memory(params, "memory", &params_path)?;
    let config = match *kind {
        "scalar" => {
            if let Some(t) = params.get("type").filter(|v| !v.is_null()) {
                let raw = json::string_at(t, &child(&params_path, "type"))?;
                if !raw.eq_ignore_ascii_case("int8") {
                    return Err(invalid(
                        child(&params_path, "type"),
                        format!("unsupported scalar quantization type '{raw}' (expected int8)"),
                    ));
                }
            }
            QuantizationConfig {
                qtype: QuantizationType::Scalar,
                always_ram,
                quantile: json::opt_f64(params, "quantile", &params_path)?,
                bits: None,
                compression: None,
                encoding: None,
                query_encoding: None,
                memory,
            }
        }
        "product" => {
            let compression = json::opt_string(params, "compression", &params_path)?;
            if let Some(c) = &compression {
                let known = ["x4", "x8", "x16", "x32", "x64"];
                if !known.contains(&c.to_ascii_lowercase().as_str()) {
                    return Err(invalid(
                        child(&params_path, "compression"),
                        format!("unknown product compression '{c}'"),
                    ));
                }
            }
            QuantizationConfig {
                qtype: QuantizationType::Product,
                always_ram,
                quantile: None,
                bits: None,
                compression,
                encoding: None,
                query_encoding: None,
                memory,
            }
        }
        "binary" => {
            let encoding = match json::opt_string(params, "encoding", &params_path)? {
                None => None,
                Some(raw) => Some(check_binary_encoding(
                    &raw,
                    &child(&params_path, "encoding"),
                )?),
            };
            let query_encoding = match json::opt_string(params, "query_encoding", &params_path)? {
                None => None,
                Some(raw) => Some(check_binary_query_encoding(
                    &raw,
                    &child(&params_path, "query_encoding"),
                )?),
            };
            QuantizationConfig {
                qtype: QuantizationType::Binary,
                always_ram,
                quantile: None,
                bits: None,
                compression: None,
                encoding,
                query_encoding,
                memory,
            }
        }
        "turbo" => QuantizationConfig {
            qtype: QuantizationType::Turbo,
            always_ram,
            quantile: None,
            bits: match json::opt_string(params, "bits", &params_path)? {
                None => None,
                Some(label) => Some(turbo_bits(&label).ok_or_else(|| {
                    invalid(
                        child(&params_path, "bits"),
                        format!("unknown turbo bit size '{label}'"),
                    )
                })?),
            },
            compression: None,
            encoding: None,
            query_encoding: None,
            memory,
        },
        other => unreachable!("variant {other}"),
    };
    Ok(config)
}

/// Decode `TurboQuantBitSize` labels (`bits1`, `bits1_5`, `bits2`, `bits4`).
fn turbo_bits(label: &str) -> Option<f64> {
    let raw = label.strip_prefix("bits")?;
    let normalized = raw.replace('_', ".");
    let value: f64 = normalized.parse().ok()?;
    match value {
        1.0 | 1.5 | 2.0 | 4.0 => Some(value),
        _ => None,
    }
}

/// Validate a `BinaryQuantizationEncoding` name, normalizing to lowercase.
fn check_binary_encoding(raw: &str, path: &str) -> Result<String, ConvertError> {
    let lower = raw.to_ascii_lowercase();
    if matches!(lower.as_str(), "one_bit" | "two_bits" | "one_and_half_bits") {
        Ok(lower)
    } else {
        Err(invalid(
            path,
            format!("unknown binary quantization encoding '{raw}'"),
        ))
    }
}

/// Validate a `BinaryQuantizationQueryEncoding` name, normalizing to lowercase.
fn check_binary_query_encoding(raw: &str, path: &str) -> Result<String, ConvertError> {
    let lower = raw.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "default" | "binary" | "scalar4bits" | "scalar8bits"
    ) {
        Ok(lower)
    } else {
        Err(invalid(
            path,
            format!("unknown binary quantization query encoding '{raw}'"),
        ))
    }
}

/// Decode `QuantizationConfigDiff`: a config object or `"Disabled"`.
pub(crate) fn quantization_update(
    value: &Value,
    path: &str,
) -> Result<QuantizationUpdate, ConvertError> {
    if let Value::String(s) = value
        && s == "Disabled"
    {
        return Ok(QuantizationUpdate {
            disabled: true,
            config: None,
        });
    }
    Ok(QuantizationUpdate {
        disabled: false,
        config: Some(Box::new(quantization(value, path)?)),
    })
}
