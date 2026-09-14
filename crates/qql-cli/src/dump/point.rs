//! Point extraction, QQL literal serialization, and batched UPSERT streaming.

use std::error::Error;
use std::io::Write;

use qql_core::ast::ShardKey;
use serde_json::Value;

use super::escape::{escape_string, format_ident, is_simple_ident};

/// Convert a scroll point JSON object into an UPSERT record object
/// (`{id, vector?, …payload}`). Returns `None` only when the point has no usable `id`.
pub fn point_to_upsert_object(point: &Value) -> Option<Value> {
    let id = point.get("id")?.clone();
    let mut map = serde_json::Map::new();
    map.insert("id".into(), id);

    if let Some(payload) = point.get("payload").and_then(|p| p.as_object()) {
        for (k, v) in payload {
            map.insert(k.clone(), v.clone());
        }
    }

    if let Some(vector) = point.get("vector")
        && !vector.is_null()
    {
        map.insert("vector".into(), vector.clone());
    }

    Some(Value::Object(map))
}

/// Format a batch of upsert records as a QQL `UPSERT INTO … VALUES …` statement.
///
/// `shard_key` routes the batch on custom-sharded collections
/// (`… SHARD 'tenant'`); `None` keeps the unrouted form.
pub fn format_upsert_statement(
    collection: &str,
    records: &[Value],
    shard_key: Option<&ShardKey>,
) -> String {
    let mut body = format!("UPSERT INTO {} VALUES\n", format_ident(collection));
    for (idx, rec) in records.iter().enumerate() {
        body.push_str("  ");
        body.push_str(&format_point_literal(rec));
        if idx + 1 < records.len() {
            body.push(',');
        }
        body.push('\n');
    }
    if let Some(key) = shard_key {
        body.push_str(&format!(" SHARD {key}"));
    }
    body
}

pub fn write_upsert_batch(
    out: &mut impl Write,
    collection: &str,
    records: &[Value],
    shard_key: Option<&ShardKey>,
) -> Result<(), Box<dyn Error>> {
    write!(
        out,
        "{}",
        format_upsert_statement(collection, records, shard_key)
    )?;
    writeln!(out, ";")?;
    writeln!(out)?;
    Ok(())
}

/// Render one point as a QQL object literal: `{id: …, vector: …, …}`.
pub fn format_point_literal(point: &Value) -> String {
    let Some(obj) = point.as_object() else {
        return "{}".into();
    };

    let mut parts = Vec::new();

    if let Some(id) = obj.get("id") {
        parts.push(format!("id: {}", format_point_id_value(id)));
    }
    if let Some(vector) = obj.get("vector") {
        parts.push(format!("vector: {}", format_qql_value(vector)));
    }

    let mut payload_keys: Vec<&String> = obj
        .keys()
        .filter(|k| k.as_str() != "id" && k.as_str() != "vector")
        .collect();
    payload_keys.sort();
    for k in payload_keys {
        if let Some(v) = obj.get(k) {
            parts.push(format!("{}: {}", format_field_key(k), format_qql_value(v)));
        }
    }

    format!("{{{}}}", parts.join(", "))
}

pub fn format_point_id_value(id: &Value) -> String {
    match id {
        Value::Number(n) => n.to_string(),
        Value::String(s) => format!("'{}'", escape_string(s)),
        other => format!("'{}'", escape_string(&other.to_string())),
    }
}

pub fn format_field_key(key: &str) -> String {
    if is_simple_ident(key) {
        key.to_string()
    } else {
        format!("'{}'", escape_string(key))
    }
}

pub fn format_qql_value(value: &Value) -> String {
    match value {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => format!("'{}'", escape_string(s)),
        Value::Array(arr) => {
            let items: Vec<String> = arr.iter().map(format_qql_value).collect();
            format!("[{}]", items.join(", "))
        }
        Value::Object(map) => {
            if map.contains_key("indices") && map.contains_key("values") && map.len() == 2 {
                return format!(
                    "{{indices: {}, values: {}}}",
                    format_qql_value(&map["indices"]),
                    format_qql_value(&map["values"])
                );
            }
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let items: Vec<String> = keys
                .iter()
                .map(|k| format!("{}: {}", format_field_key(k), format_qql_value(&map[*k])))
                .collect();
            format!("{{{}}}", items.join(", "))
        }
    }
}
