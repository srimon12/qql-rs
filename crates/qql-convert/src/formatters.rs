//! Value, vector, and payload-dict formatting plus QQL string helpers.

use serde_json::Value;

/// Convert a `using` field value to its QQL representation.
pub(crate) fn using_str(using: &str) -> String {
    match using.to_lowercase().as_str() {
        "hybrid" => "USING HYBRID".to_string(),
        "sparse" => "USING SPARSE".to_string(),
        _ => format!("USING '{}'", escape_qql_string(using)),
    }
}

/// Convert a `lookup_from` object to its QQL representation.
pub(crate) fn lookup_from_str(lookup: &serde_json::Map<String, Value>) -> Option<String> {
    let coll = lookup.get("collection").and_then(|v| v.as_str())?;
    let vec_name = lookup.get("vector").and_then(|v| v.as_str());
    Some(match vec_name {
        Some(vn) => format!("LOOKUP FROM {} VECTOR '{}'", coll, escape_qql_string(vn)),
        None => format!("LOOKUP FROM {coll}"),
    })
}

/// Format any JSON value as a QQL literal.
pub(crate) fn format_value(v: &Value) -> String {
    match v {
        Value::Null => "null".to_string(),
        Value::Bool(b) => if *b { "true" } else { "false" }.to_string(),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.to_string()
            } else if let Some(f) = n.as_f64() {
                if f.fract() == 0.0 && f.is_finite() {
                    format!("{}", f as i64)
                } else {
                    format!("{f}")
                }
            } else {
                format!("{n}")
            }
        }
        Value::String(s) => format!("'{}'", escape_qql_string(s)),
        Value::Array(arr) => {
            let items: Vec<String> = arr.iter().map(format_value).collect();
            format!("[{}]", items.join(", "))
        }
        Value::Object(map) => format_map(map),
    }
}

fn format_map(map: &serde_json::Map<String, Value>) -> String {
    let mut parts = Vec::new();
    for (k, v) in map {
        parts.push(format!("'{k}': {}", format_value(v)));
    }
    format!("{{{}}}", parts.join(", "))
}

/// Format a dense vector literal; `None` when empty or not an array.
pub(crate) fn format_vector(vec: &Value) -> Option<String> {
    match vec {
        Value::Array(arr) => {
            let items: Vec<String> = arr
                .iter()
                .map(|v| match v {
                    Value::Number(n) => n.to_string(),
                    _ => v.to_string(),
                })
                .collect();
            if items.is_empty() {
                None
            } else {
                Some(format!("[{}]", items.join(", ")))
            }
        }
        _ => None,
    }
}

/// Format a payload object as a QQL `{...}` dict.
pub(crate) fn build_payload_dict(payload: &serde_json::Map<String, Value>) -> String {
    if payload.is_empty() {
        return "{}".to_string();
    }
    let mut parts = Vec::new();
    for (k, v) in payload {
        parts.push(format!("'{k}': {}", format_value(v)));
    }
    format!("{{{}}}", parts.join(", "))
}

/// Escape a string for single-quoted QQL embedding.
pub(crate) fn escape_qql_string(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{format_value, format_vector};
    use crate::sanitize::format_id;

    #[test]
    fn escaping_handles_special_characters() {
        assert_eq!(format_id(&json!("hello\\world\n")), "'hello\\\\world\\n'");
        assert_eq!(format_value(&json!("a'b\tc")), "'a\\'b\\tc'");
    }

    #[test]
    fn vector_formatting_rejects_empty_and_non_arrays() {
        assert_eq!(
            format_vector(&json!([0.1, 0.2])).as_deref(),
            Some("[0.1, 0.2]")
        );
        assert_eq!(format_vector(&json!([])), None);
        assert_eq!(format_vector(&json!({"a": 1})), None);
    }
}
