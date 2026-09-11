//! Public entry points plus wrapped / bare / JSONL classification.

use serde_json::Value;

use crate::ConvertError;
use crate::decode::{self, DecodeCtx};
use crate::endpoint;
use crate::request::RequestOpts;
use qql_core::ast::Stmt;
use qql_core::fmt::format_stmt;

/// Convert a Qdrant REST JSON payload (one object or a JSONL capture) into
/// typed QQL statements.
///
/// `collection` is required for bare bodies (no `{method, path}`). Wrapped
/// requests always take the collection from the path. JSONL is selected when
/// the buffer is not a single JSON value and has two or more non-empty lines
/// — the shape `qql record --out` writes.
pub fn convert_stmts(input: &str, collection: Option<&str>) -> Result<Vec<Stmt>, ConvertError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(ConvertError::undecodable("no input"));
    }
    match serde_json::from_str::<Value>(trimmed) {
        Ok(value) => convert_value(&value, collection),
        Err(err) => {
            let nonempty = trimmed
                .lines()
                .filter(|line| !line.trim().is_empty())
                .count();
            if nonempty < 2 {
                return Err(ConvertError::InvalidJson(err.to_string()));
            }
            convert_jsonl(trimmed, collection)
        }
    }
}

/// Convert REST JSON into canonical QQL statement strings.
///
/// Each string is [`format_stmt`] output (no terminator). Callers that want a
/// script append `;`.
///
/// ```
/// let stmts = qql_convert::convert(r#"{"ids": [1]}"#, Some("docs")).unwrap();
/// assert_eq!(stmts, ["QUERY POINTS (1) FROM docs"]);
/// ```
pub fn convert(input: &str, collection: Option<&str>) -> Result<Vec<String>, ConvertError> {
    Ok(convert_stmts(input, collection)?
        .iter()
        .map(format_stmt)
        .collect())
}

fn convert_jsonl(input: &str, collection: Option<&str>) -> Result<Vec<Stmt>, ConvertError> {
    let mut statements = Vec::new();
    let mut entries = 0usize;
    for (index, raw) in input.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        entries += 1;
        let converted =
            convert_stmts(line, collection).map_err(|source| ConvertError::InvalidLine {
                line: index + 1,
                source: Box::new(source),
            })?;
        statements.extend(converted);
    }
    if entries == 0 {
        return Err(ConvertError::undecodable("no JSON lines in capture"));
    }
    Ok(statements)
}

fn convert_value(raw: &Value, collection: Option<&str>) -> Result<Vec<Stmt>, ConvertError> {
    match wrapped_request(raw)? {
        Some(wrapped) => convert_wrapped(wrapped, collection),
        None => {
            if !raw.is_object() {
                return Err(ConvertError::undecodable(format!(
                    "expected a JSON object, got {}",
                    crate::json::type_name(raw)
                )));
            }
            convert_bare(raw, collection)
        }
    }
}

fn convert_bare(raw: &Value, collection: Option<&str>) -> Result<Vec<Stmt>, ConvertError> {
    let collection = match collection {
        Some(name) if !name.is_empty() => name,
        _ => return Err(ConvertError::MissingCollection),
    };
    crate::bare::convert(raw, collection)
}

struct Wrapped<'a> {
    method: &'a str,
    path: &'a str,
    query: Option<&'a Value>,
    body: Option<&'a Value>,
}

/// Split a wrapped `{method, path, query?, body?}` object, if this is one.
fn wrapped_request(raw: &Value) -> Result<Option<Wrapped<'_>>, ConvertError> {
    let Some(obj) = raw.as_object() else {
        return Ok(None);
    };
    let Some(method) = obj.get("method").and_then(Value::as_str) else {
        return Ok(None);
    };
    let Some(path) = obj.get("path").and_then(Value::as_str) else {
        return Ok(None);
    };
    crate::json::reject_unknown(obj, "", &["method", "path", "query", "body"])?;
    Ok(Some(Wrapped {
        method,
        path,
        query: obj.get("query"),
        body: obj.get("body"),
    }))
}

fn convert_wrapped(
    wrapped: Wrapped<'_>,
    collection: Option<&str>,
) -> Result<Vec<Stmt>, ConvertError> {
    let (path, path_query) = split_path_query(wrapped.path);
    let mut opts = RequestOpts::from_query_string(path_query)?;
    opts.merge(RequestOpts::from_value(wrapped.query)?);
    let matched = endpoint::parse(wrapped.method, path)?;
    let collection = matched.collection.as_deref().or(collection).unwrap_or("");
    let ctx = DecodeCtx {
        collection,
        opts: &opts,
    };
    decode::endpoint(&matched, wrapped.body, ctx)
}

/// Split `path?query` into `(path, query)` with the leading `?` removed.
fn split_path_query(path: &str) -> (&str, &str) {
    match path.split_once('?') {
        Some((path, query)) => (path, query),
        None => (path, ""),
    }
}
