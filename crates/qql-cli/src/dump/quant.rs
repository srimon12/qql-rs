//! Quantization specification formatting and normalization for scalar, product, binary, and turbo.

use qql_plan::QuantizationConfig;
use serde_json::Value;

use super::escape::escape_string;
use super::indexes::format_index_option;

/// Format a typed collection-level quantization config as QQL
/// `WITH QUANTIZATION (type = '…', …)` body options.
///
/// Vector-level configs still arrive as generic JSON and go through
/// [`format_quantization_spec`].
pub fn format_quantization_config(quant: &QuantizationConfig) -> String {
    // Key order mirrors the previous JSON-map iteration (alphabetical within
    // the nested config object), so dump SQL stays byte-identical.
    match quant {
        QuantizationConfig::Scalar { scalar } => {
            let mut opts = vec!["type = 'scalar'".to_string()];
            push_always_ram(&mut opts, scalar.always_ram);
            push_memory(&mut opts, scalar.memory);
            if let Some(quantile) = scalar.quantile
                && let Some(number) = serde_json::Number::from_f64(quantile)
            {
                opts.push(format!("quantile = {number}"));
            }
            opts.join(", ")
        }
        QuantizationConfig::Product { product } => {
            let mut opts = vec!["type = 'product'".to_string()];
            push_always_ram(&mut opts, product.always_ram);
            opts.push(format!(
                "compression = '{}'",
                escape_string(&product.compression.to_ascii_lowercase())
            ));
            push_memory(&mut opts, product.memory);
            opts.join(", ")
        }
        QuantizationConfig::Binary { binary } => {
            let mut opts = vec!["type = 'binary'".to_string()];
            push_always_ram(&mut opts, binary.always_ram);
            if let Some(encoding) = binary
                .encoding
                .as_deref()
                .and_then(normalize_binary_encoding_str)
            {
                opts.push(format!("encoding = '{encoding}'"));
            }
            push_memory(&mut opts, binary.memory);
            if let Some(query_encoding) = binary.query_encoding.as_deref() {
                opts.push(format!(
                    "query_encoding = '{}'",
                    escape_string(query_encoding)
                ));
            }
            opts.join(", ")
        }
        QuantizationConfig::Turbo { turbo } => {
            let mut opts = vec!["type = 'turbo'".to_string()];
            push_always_ram(&mut opts, turbo.always_ram);
            if let Some(bits) = turbo.bits.as_deref().and_then(normalize_turbo_bits_str) {
                opts.push(format!("bits = {bits}"));
            }
            push_memory(&mut opts, turbo.memory);
            opts.join(", ")
        }
    }
}

fn push_always_ram(opts: &mut Vec<String>, always_ram: Option<bool>) {
    if let Some(value) = always_ram {
        opts.push(format!("always_ram = {value}"));
    }
}

fn push_memory(opts: &mut Vec<String>, memory: Option<qql_plan::types::MemoryPlacement>) {
    if let Some(value) = memory {
        opts.push(format!("memory = '{}'", value.as_str()));
    }
}

/// Canonicalize an OpenAPI binary `encoding` string for QQL re-parse.
fn normalize_binary_encoding_str(raw: &str) -> Option<String> {
    Some(
        match raw.to_ascii_lowercase().as_str() {
            "one_bit" | "onebit" | "1" => "one_bit",
            "two_bits" | "twobits" | "2" => "two_bits",
            "one_and_half_bits" | "oneandhalfbits" | "1.5" => "one_and_half_bits",
            _ => return None,
        }
        .to_string(),
    )
}

/// Map an OpenAPI turbo bits label to QQL's numeric `bits` value.
fn normalize_turbo_bits_str(raw: &str) -> Option<String> {
    Some(
        match raw.to_ascii_lowercase().as_str() {
            "bits1" | "1" => "1",
            "bits1_5" | "bits1.5" | "1.5" => "1.5",
            "bits2" | "2" => "2",
            "bits4" | "4" => "4",
            _ => return None,
        }
        .to_string(),
    )
}

/// Format a Qdrant quantization_config value as QQL
/// `WITH QUANTIZATION (type = '…', …)` body options.
///
/// Accepts nested REST shapes (`{ "turbo": { … } }`) and flat
/// `{ "type": "turbo", … }` forms. Emits `bits` (not `turbo_bits`) so the
/// CREATE parser accepts turbo configs.
pub fn format_quantization_spec(quant: &Value) -> Option<String> {
    let obj = quant.as_object()?;

    // Nested OpenAPI shapes: { scalar|product|binary|turbo: {…} }
    for kind in ["scalar", "product", "binary", "turbo"] {
        if let Some(inner) = obj.get(kind).and_then(|s| s.as_object()) {
            return Some(format_quantization_opts(kind, inner));
        }
    }

    // Flat form: { "type": "scalar", "quantile": 0.99, … }
    if let Some(ty) = obj.get("type").and_then(|t| t.as_str()) {
        return Some(format_quantization_opts(ty, obj));
    }

    if obj.get("disabled").and_then(|d| d.as_bool()) == Some(true) {
        return Some("disabled = true".to_string());
    }
    None
}

pub fn format_quantization_opts(kind: &str, inner: &serde_json::Map<String, Value>) -> String {
    let kind = kind.to_ascii_lowercase();
    let mut opts = vec![format!("type = '{}'", kind)];

    for (k, v) in inner {
        if k == "type" {
            continue;
        }
        // Turbo: normalize bits / turbo_bits → QQL `bits = N`
        if kind == "turbo" && (k == "bits" || k == "turbo_bits") {
            if let Some(bits) = normalize_turbo_bits(v) {
                opts.push(format!("bits = {}", bits));
            }
            continue;
        }
        // Binary: normalize protobuf/REST encoding aliases → QQL snake_case
        if kind == "binary" && k == "encoding" {
            if let Some(enc) = normalize_binary_encoding(v) {
                opts.push(format!("encoding = '{}'", enc));
            }
            continue;
        }
        if let Some(opt) = format_index_option(k, v) {
            opts.push(opt);
        }
    }
    opts.join(", ")
}

pub fn normalize_binary_encoding(v: &Value) -> Option<String> {
    let raw = match v {
        Value::String(s) => s.to_ascii_lowercase(),
        Value::Number(n) => {
            let f = n.as_f64()?;
            if (f - 1.5).abs() < f64::EPSILON {
                "1.5".into()
            } else if (f - 2.0).abs() < f64::EPSILON {
                "2".into()
            } else if (f - 1.0).abs() < f64::EPSILON {
                "1".into()
            } else {
                return None;
            }
        }
        _ => return None,
    };
    Some(
        match raw.as_str() {
            "one_bit" | "onebit" | "1" => "one_bit",
            "two_bits" | "twobits" | "2" => "two_bits",
            "one_and_half_bits" | "oneandhalfbits" | "1.5" => "one_and_half_bits",
            _ => return None,
        }
        .into(),
    )
}

/// Map REST/gRPC turbo bit representations to QQL numeric `bits`.
pub fn normalize_turbo_bits(v: &Value) -> Option<String> {
    match v {
        Value::Number(n) => {
            let f = n.as_f64()?;
            // Only the four legal turbo bit widths.
            if (f - 1.0).abs() < f64::EPSILON
                || (f - 1.5).abs() < f64::EPSILON
                || (f - 2.0).abs() < f64::EPSILON
                || (f - 4.0).abs() < f64::EPSILON
            {
                // Prefer compact integer rendering when whole.
                if f.fract() == 0.0 {
                    Some(format!("{}", f as i64))
                } else {
                    Some(format!("{}", f))
                }
            } else {
                None
            }
        }
        Value::String(s) => match s.to_ascii_lowercase().as_str() {
            "bits1" | "1" => Some("1".into()),
            "bits1_5" | "bits1.5" | "1.5" => Some("1.5".into()),
            "bits2" | "2" => Some("2".into()),
            "bits4" | "4" => Some("4".into()),
            _ => None,
        },
        _ => None,
    }
}
