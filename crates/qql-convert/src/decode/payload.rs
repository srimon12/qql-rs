//! Decode payload objects into ordered AST pairs.

use serde_json::Value;

use crate::ConvertError;
use crate::json::{self, child, index, invalid};
use qql_core::ast::Value as AstValue;

/// Borrow one `payloads[i]` entry as AST pairs.
pub(crate) fn payload_at(
    payloads: &[Value],
    i: usize,
    path: &str,
) -> Result<Vec<(String, AstValue)>, ConvertError> {
    match payloads.get(i) {
        None => Ok(Vec::new()),
        Some(Value::Null) => Ok(Vec::new()),
        Some(payload) => decode_payload(payload, &index(path, i)),
    }
}

/// Decode a payload object into ordered AST pairs.
///
/// Payload keys named `id` / `vector` would collide with the point-object
/// syntax (`{id: …, vector: …}`) and fail closed.
pub(crate) fn decode_payload(
    value: &Value,
    path: &str,
) -> Result<Vec<(String, AstValue)>, ConvertError> {
    let obj = json::object(value, path)?;
    let mut payload = Vec::with_capacity(obj.len());
    for (key, item) in obj {
        if key.eq_ignore_ascii_case("id") || key.eq_ignore_ascii_case("vector") {
            return Err(invalid(
                child(path, key),
                format!("payload key '{key}' collides with the point-object id/vector slots"),
            ));
        }
        payload.push((
            key.clone(),
            json::json_to_ast_value(item, &child(path, key))?,
        ));
    }
    Ok(payload)
}
