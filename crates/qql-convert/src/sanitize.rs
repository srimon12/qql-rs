//! Collection-name and point-ID sanitizing.

use serde_json::Value;

use crate::formatters::escape_qql_string;

/// Keep only collection-safe characters; fall back to `"unknown"`.
pub(crate) fn sanitize_collection_name(name: &str) -> String {
    if name.is_empty() {
        return "unknown".to_string();
    }
    let out: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    if out.is_empty() {
        "unknown".to_string()
    } else {
        out
    }
}

/// Format a point ID as QQL: numbers bare, strings single-quoted.
pub(crate) fn format_id(id: &Value) -> String {
    match id {
        Value::String(s) => format!("'{}'", escape_qql_string(s)),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.to_string()
            } else if let Some(f) = n.as_f64() {
                if f.fract() == 0.0 {
                    format!("{}", f as i64)
                } else {
                    format!("{}", f)
                }
            } else {
                format!("'{n}'")
            }
        }
        _ => format!("'{id}'"),
    }
}

/// Format a point-ID array as a comma-joined QQL list.
pub(crate) fn format_id_list(ids: &[Value]) -> String {
    ids.iter().map(format_id).collect::<Vec<_>>().join(", ")
}
