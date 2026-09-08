//! Index generation and parameter formatting for payload indexes.

use qql::backend::PayloadIndexSpec;
use serde_json::Value;

use super::escape::{escape_string, format_ident};

/// Emit `CREATE INDEX` statements from typed payload index specs.
pub fn generate_index_statements(collection: &str, indexes: &[PayloadIndexSpec]) -> Vec<String> {
    let coll = format_ident(collection);
    let mut stmts = Vec::with_capacity(indexes.len());

    for idx in indexes {
        let mut stmt = format!(
            "CREATE INDEX ON COLLECTION {} FOR {} TYPE {}",
            coll,
            format_ident(&idx.field),
            idx.data_type.to_ascii_lowercase()
        );

        let mut opts = Vec::new();
        for (k, v) in &idx.params {
            if let Some(opt) = format_index_option(k, v) {
                opts.push(opt);
            }
        }
        if let Some(tenant) = idx.is_tenant
            && !opts.iter().any(|o| o.starts_with("is_tenant"))
        {
            opts.push(format!("is_tenant = {}", tenant));
        }
        if !opts.is_empty() {
            opts.sort();
            stmt.push_str(&format!(" WITH ({})", opts.join(", ")));
        }
        stmts.push(stmt);
    }

    stmts.sort();
    stmts
}

/// Format an individual index or config option into QQL syntax (`key = value`).
pub fn format_index_option(key: &str, value: &Value) -> Option<String> {
    match value {
        Value::Bool(b) => Some(format!("{} = {}", key, b)),
        Value::Number(n) => Some(format!("{} = {}", key, n)),
        Value::String(s) => Some(format!("{} = '{}'", key, escape_string(s))),
        Value::Array(arr)
            if arr
                .iter()
                .all(|v| v.is_string() || v.is_number() || v.is_boolean()) =>
        {
            let items: Vec<String> = arr
                .iter()
                .map(|v| match v {
                    Value::String(s) => format!("'{}'", escape_string(s)),
                    Value::Number(n) => n.to_string(),
                    Value::Bool(b) => b.to_string(),
                    _ => String::new(),
                })
                .collect();
            Some(format!("{} = [{}]", key, items.join(", ")))
        }
        _ => None,
    }
}
