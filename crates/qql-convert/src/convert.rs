//! Public entry points plus wrapped/bare input classification.

use serde_json::Value;

use crate::ConvertError;
use crate::decode;
use crate::endpoint;
use qql_core::ast::Stmt;
use qql_core::fmt::format_stmt;

/// Convert a Qdrant REST JSON payload to QQL statements.
///
/// Accepts a wrapped `{method, path, body}` request (collection derived from
/// the path) or a bare body (collection falls back to `"unknown"`; prefer
/// [`json_to_qql_with_collection`] when the collection is known).
pub fn json_to_qql(input: &str) -> Result<Vec<String>, ConvertError> {
    json_to_qql_with_collection(input, "unknown")
}

/// Convert a Qdrant REST JSON payload to QQL statements.
///
/// `collection` is used when the payload carries no path information (bare
/// body). Wrapped requests always derive the collection from their path.
/// An empty collection falls back to `"unknown"`.
///
/// Every returned string comes from [`qql_core::fmt::format_stmt`] and
/// therefore re-parses as QQL by construction.
pub fn json_to_qql_with_collection(
    input: &str,
    collection: &str,
) -> Result<Vec<String>, ConvertError> {
    let collection = if collection.is_empty() {
        "unknown"
    } else {
        collection
    };
    let raw: Value =
        serde_json::from_str(input.trim()).map_err(|e| ConvertError::InvalidJson(e.to_string()))?;

    let statements: Vec<Stmt> = match wrapped_parts(&raw) {
        Some((method, path, body)) => {
            let matched = endpoint::parse(method, path)?;
            decode::endpoint(&matched, body, collection)?
        }
        None => crate::bare::convert(&raw, collection)?,
    };
    Ok(statements.iter().map(format_stmt).collect())
}

/// Split a wrapped `{method, path, body}` object, if this is one.
fn wrapped_parts(raw: &Value) -> Option<(&str, &str, Option<&Value>)> {
    let obj = raw.as_object()?;
    let method = obj.get("method").and_then(Value::as_str)?;
    let path = obj.get("path").and_then(Value::as_str)?;
    let body = obj.get("body").or_else(|| obj.get("request"));
    Some((method, path, body))
}
