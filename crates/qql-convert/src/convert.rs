//! Public entry points plus wrapped / bare / JSONL classification.

use serde_json::Value;

use crate::decode::{self, DecodeCtx};
use crate::endpoint;
use crate::request::RequestOpts;
use crate::{ConvertError, ConvertOptions, ReadFile};
use qql_core::ast::Stmt;
use qql_core::fmt::format_stmt;
use std::borrow::Cow;

/// Convert a Qdrant REST JSON payload (one object or a JSONL capture) into
/// typed QQL statements.
///
/// `collection` is required for bare bodies (no `{method, path}`). Wrapped
/// requests always take the collection from the path. JSONL is selected when
/// the buffer is not a single JSON value and has two or more non-empty lines
/// — the shape `qql record --out` writes.
pub fn convert_stmts(input: &str, collection: Option<&str>) -> Result<Vec<Stmt>, ConvertError> {
    convert_stmts_with(input, collection, &ConvertOptions::default())
}

/// [`convert_stmts`] with placeholder expansion and file-body resolution.
pub fn convert_stmts_with(
    input: &str,
    collection: Option<&str>,
    options: &ConvertOptions<'_>,
) -> Result<Vec<Stmt>, ConvertError> {
    let owned;
    let trimmed = match strip_fence_lines(input.trim()) {
        Some(unfenced) => {
            owned = unfenced;
            owned.as_str()
        }
        None => input.trim(),
    };
    if trimmed.is_empty() {
        return Err(ConvertError::undecodable("no input"));
    }
    let expanded = expand_vars(trimmed, options.vars)?;
    let trimmed = expanded.as_ref();
    match serde_json::from_str::<Value>(trimmed) {
        Ok(value) => convert_value(&value, collection),
        Err(json_err) => {
            if let Some(pasted) =
                crate::snippet::convert_pasted(trimmed, collection, options.read_file)
            {
                return pasted;
            }
            let nonempty = trimmed
                .lines()
                .filter(|line| !line.trim().is_empty())
                .count();
            if nonempty < 2 {
                return Err(ConvertError::InvalidJson(json_err.to_string()));
            }
            convert_jsonl(trimmed, collection, options.read_file)
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
    convert_with(input, collection, &ConvertOptions::default())
}

/// [`convert`] with placeholder expansion and file-body resolution.
pub fn convert_with(
    input: &str,
    collection: Option<&str>,
    options: &ConvertOptions<'_>,
) -> Result<Vec<String>, ConvertError> {
    Ok(convert_stmts_with(input, collection, options)?
        .iter()
        .map(format_stmt)
        .collect())
}

fn convert_jsonl(
    input: &str,
    collection: Option<&str>,
    read_file: ReadFile<'_>,
) -> Result<Vec<Stmt>, ConvertError> {
    let mut statements = Vec::new();
    let mut entries = 0usize;
    let options = ConvertOptions {
        vars: &[],
        read_file,
    };
    for (index, raw) in input.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        entries += 1;
        let converted = convert_stmts_with(line, collection, &options).map_err(|source| {
            ConvertError::InvalidLine {
                line: index + 1,
                source: Box::new(source),
            }
        })?;
        statements.extend(converted);
    }
    if entries == 0 {
        return Err(ConvertError::undecodable("no JSON lines in capture"));
    }
    Ok(statements)
}

/// Expand `$NAME`, `${NAME}`, and `<NAME>` before anything is classified.
///
/// The tokenizer is the wrong place for this: it rejects `$VAR` inside double
/// quotes, so a substitution pass after tokenizing would never see the common
/// `-H "Authorization: $TOKEN"` shape. Text already pasted into JSON gets the
/// same treatment. Only provided names expand; `$NAME`, `${NAME}`, and
/// identifier-shaped `<NAME>` references with no value fail closed. Any other
/// `<…>` text is left for the existing placeholder handling downstream.
fn expand_vars<'a>(
    input: &'a str,
    vars: &[(String, String)],
) -> Result<Cow<'a, str>, ConvertError> {
    if vars.is_empty() {
        return Ok(Cow::Borrowed(input));
    }
    let lookup = |name: &str| {
        vars.iter()
            .rev()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    };
    let mut out = String::with_capacity(input.len());
    let mut unresolved: Vec<String> = Vec::new();
    let mut rest = input;
    while let Some(offset) = rest.find(['$', '<']) {
        out.push_str(&rest[..offset]);
        let tail = &rest[offset..];
        if let Some(after) = tail.strip_prefix('$') {
            let (name, consumed) = variable_ref(after);
            match name {
                Some(name) => match lookup(name) {
                    Some(value) => {
                        out.push_str(value);
                        rest = &after[consumed..];
                    }
                    None => {
                        unresolved.push(name.to_string());
                        out.push('$');
                        rest = after;
                    }
                },
                None => {
                    out.push('$');
                    rest = after;
                }
            }
        } else {
            match tail[1..].find('>') {
                Some(end) => {
                    let name = &tail[1..1 + end];
                    if !is_var_name(name) {
                        out.push('<');
                        rest = &tail[1..];
                        continue;
                    }
                    match lookup(name) {
                        Some(value) => {
                            out.push_str(value);
                            rest = &tail[end + 2..];
                        }
                        None => {
                            unresolved.push(name.to_string());
                            out.push('<');
                            rest = &tail[1..];
                        }
                    }
                }
                None => {
                    out.push('<');
                    rest = &tail[1..];
                }
            }
        }
    }
    out.push_str(rest);
    if !unresolved.is_empty() {
        unresolved.sort();
        unresolved.dedup();
        return Err(ConvertError::UnresolvedVariables { names: unresolved });
    }
    Ok(Cow::Owned(out))
}

/// The `$NAME` / `${NAME}` reference in `after` (the text following `$`), with
/// the number of bytes it spans. `None` when the shape is not a recognizable
/// name, so the text passes through for the tokenizer to judge.
fn variable_ref(after: &str) -> (Option<&str>, usize) {
    if let Some(braced) = after.strip_prefix('{') {
        let Some(end) = braced.find('}') else {
            return (None, 0);
        };
        let name = &braced[..end];
        return if is_var_name(name) {
            (Some(name), end + 2)
        } else {
            (None, 0)
        };
    }
    let len = after
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(after.len());
    let name = &after[..len];
    if is_var_name(name) {
        (Some(name), len)
    } else {
        (None, 0)
    }
}

fn is_var_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
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
    crate::endpoint::reject_template_collection(collection)?;
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
    convert_method_path_body(
        wrapped.method,
        wrapped.path,
        wrapped.query,
        wrapped.body,
        collection,
    )
}

/// Shared lowering for every envelope: wrapped requests plus the pasted
/// snippet / curl shapes in [`crate::snippet`].
///
/// `path` may carry a `?query` suffix; `query` is the wrapped envelope's
/// optional query object (pasted shapes have none). Collection names that look
/// like pasted placeholders fail via [`crate::endpoint::parse`].
pub(crate) fn convert_method_path_body(
    method: &str,
    path: &str,
    query: Option<&Value>,
    body: Option<&Value>,
    collection: Option<&str>,
) -> Result<Vec<Stmt>, ConvertError> {
    let (path, path_query) = split_path_query(path);
    let mut opts = RequestOpts::from_query_string(path_query)?;
    opts.merge(RequestOpts::from_value(query)?);
    let matched = endpoint::parse(method, path)?;
    let collection = matched.collection.as_deref().or(collection).unwrap_or("");
    let ctx = DecodeCtx {
        collection,
        opts: &opts,
    };
    decode::endpoint(&matched, body, ctx)
}

/// Markdown fences are paste noise (` ```http … ``` ` from docs), not input.
fn strip_fence_lines(trimmed: &str) -> Option<String> {
    let mut lines: Vec<&str> = trimmed.lines().collect();
    if !lines
        .first()
        .is_some_and(|l| l.trim_start().starts_with("```"))
    {
        return None;
    }
    lines.remove(0);
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    if lines.last().is_some_and(|l| l.trim() == "```") {
        lines.pop();
    }
    Some(lines.join("\n"))
}

/// Split `path?query` into `(path, query)` with the leading `?` removed.
fn split_path_query(path: &str) -> (&str, &str) {
    match path.split_once('?') {
        Some((path, query)) => (path, query),
        None => (path, ""),
    }
}
