//! Quantization IR helpers and REST nesting (flat `{type}` → OpenAPI nested).

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
        // Unknown — still emit a best-effort label so Qdrant can reject clearly.
        return Some(format!("bits{bits}"));
    };
    Some(label.into())
}

pub(crate) fn nest_vector_params_for_rest(cfg: &serde_json::Value) -> serde_json::Value {
    let Some(obj) = cfg.as_object() else {
        return cfg.clone();
    };
    let mut out = obj.clone();
    if let Some(q) = obj.get("quantization_config") {
        out.insert("quantization_config".into(), nest_quantization_for_rest(q));
    }
    serde_json::Value::Object(out)
}

/// Convert internal flat IR `{ "type": "scalar", … }` to OpenAPI
/// `{ "scalar": { "type": "int8", … } }` (and product/binary/turbo).
/// Passes through already-nested configs and update `disabled` forms.
pub fn nest_quantization_for_rest(value: &serde_json::Value) -> serde_json::Value {
    if value.as_str() == Some("Disabled") {
        return serde_json::Value::String("Disabled".into());
    }
    let Some(obj) = value.as_object() else {
        return value.clone();
    };
    // Already nested OpenAPI shape
    if obj.contains_key("scalar")
        || obj.contains_key("product")
        || obj.contains_key("binary")
        || obj.contains_key("turbo")
        || obj.contains_key("turboquant")
    {
        return value.clone();
    }
    // Update IR: { disabled: true, quantization_config?: … }
    if obj.get("disabled").and_then(|v| v.as_bool()) == Some(true) {
        return serde_json::Value::String("Disabled".into());
    }
    if let Some(inner) = obj.get("quantization_config") {
        return nest_quantization_for_rest(inner);
    }

    let kind = obj
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match kind.as_str() {
        "scalar" => {
            let mut inner = serde_json::Map::new();
            // OpenAPI ScalarType is only `int8`.
            inner.insert("type".into(), serde_json::Value::String("int8".into()));
            if let Some(q) = obj.get("quantile") {
                inner.insert("quantile".into(), q.clone());
            }
            if let Some(ar) = obj.get("always_ram") {
                inner.insert("always_ram".into(), ar.clone());
            }
            serde_json::json!({ "scalar": inner })
        }
        "product" => {
            let mut inner = serde_json::Map::new();
            let compression = obj
                .get("compression")
                .and_then(|v| v.as_str())
                .unwrap_or("x4");
            inner.insert(
                "compression".into(),
                serde_json::Value::String(compression.into()),
            );
            if let Some(ar) = obj.get("always_ram") {
                inner.insert("always_ram".into(), ar.clone());
            }
            serde_json::json!({ "product": inner })
        }
        "binary" => {
            let mut inner = serde_json::Map::new();
            if let Some(ar) = obj.get("always_ram") {
                inner.insert("always_ram".into(), ar.clone());
            }
            if let Some(enc) = obj.get("encoding").and_then(|v| v.as_str()) {
                let enc = match enc.to_ascii_lowercase().as_str() {
                    "twobits" | "two_bits" | "2" => "two_bits",
                    "oneandhalfbits" | "one_and_half_bits" | "1.5" => "one_and_half_bits",
                    _ => "one_bit",
                };
                inner.insert("encoding".into(), serde_json::Value::String(enc.into()));
            }
            if let Some(qe) = obj.get("query_encoding").and_then(|v| v.as_str()) {
                let qe = match qe.to_ascii_lowercase().as_str() {
                    "binary" => "binary",
                    "scalar4bits" | "scalar4" => "scalar4bits",
                    "scalar8bits" | "scalar8" => "scalar8bits",
                    _ => "default",
                };
                inner.insert(
                    "query_encoding".into(),
                    serde_json::Value::String(qe.into()),
                );
            }
            serde_json::json!({ "binary": inner })
        }
        "turbo" | "turboquant" => {
            let mut inner = serde_json::Map::new();
            if let Some(ar) = obj.get("always_ram") {
                inner.insert("always_ram".into(), ar.clone());
            }
            let bits = obj
                .get("bits")
                .or_else(|| obj.get("turbo_bits"))
                .and_then(|v| v.as_f64());
            if let Some(label) = bits.and_then(|b| turbo_bits_label(Some(b))) {
                inner.insert("bits".into(), serde_json::Value::String(label));
            }
            serde_json::json!({ "turbo": inner })
        }
        _ => value.clone(),
    }
}
