//! Parameter binding and prepared query substitution.
//!
//! Provides type-safe substitution of named (`:name`) and positional (`?`)
//! parameter placeholders in QQL query text.
//!
//! ### Placeholders & Syntax Rules
//!
//! - **Named placeholders**: `:name` (e.g. `:category`, `:limit`).
//! - **Positional placeholders**: `?` (sequential 1-to-1 mapping with parameters list).
//!
//! In QQL, `$` is a first-class identifier character (e.g. `$category`, `$1`), so
//! parameter placeholders exclusively use `:name` and `?`. This guarantees that
//! `$`-prefixed identifiers in queries are never accidentally or silently rewritten.
//!
//! Furthermore, a colon `:` is only recognized as a parameter placeholder when it occurs
//! at a valid token boundary (preceded by whitespace, punctuation, or start of query).
//! Colons in compact dictionary syntax (e.g. `{a:b}`, `{'a':b}`) are not placeholders
//! and are preserved without modification. Note that unconventional spacing with whitespace
//! before the colon (`{a :b}`) makes `:b` lexically indistinguishable from a placeholder.
//!
//! Literals and dictionary keys are safely formatted and escaped to prevent
//! query injection breakouts. String literals (`'...'`, `r'...'`, `"""..."""`,
//! and `` `...` ``) and comments (`-- ...`) in the source query are preserved
//! verbatim and never substituted.

use crate::ast::Value;
use crate::ast::filter::{FilterExpr, PointIdPredicate};
use crate::ast::formula::FormulaExpr;
use crate::ast::statement::{
    PageSpec, PointId, PointSelector, Prefetch, PrefetchSource, QueryExpr, QueryInput, QueryStmt,
    Stmt, VectorValue,
};
use crate::error::QqlError;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Check if a string is a simple identifier (starts with ascii alphabetic or `_`,
/// followed by ascii alphanumeric or `_`).
fn is_simple_ident(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Returns true if the `:` at byte offset `i` is at a valid token boundary to begin a parameter placeholder.
///
/// If preceded immediately by an identifier character (`[a-zA-Z0-9_$]`), quote (`'`, `"`, `` ` ``),
/// or closing delimiter (`}`, `]`), the colon is part of dictionary syntax (e.g. `{a:b}`, `{'a':b}`)
/// rather than a placeholder. Delimiters are ASCII, so a byte scan matches the former char scan.
fn is_placeholder_start(bytes: &[u8], i: usize) -> bool {
    if i == 0 {
        return true;
    }
    !matches!(
        bytes[i - 1],
        b'0'..=b'9'
            | b'A'..=b'Z'
            | b'a'..=b'z'
            | b'_'
            | b'$'
            | b'\''
            | b'"'
            | b'`'
            | b'}'
            | b']'
    )
}

#[inline]
fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

#[inline]
fn is_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'.'
}

/// Advance past a `--` line comment. `i` is the first `-`. Stops before `\n`.
fn scan_line_comment(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && bytes[i] != b'\n' {
        i += 1;
    }
    i
}

/// Advance past a backtick-quoted span including the closing `` ` `` if present.
fn scan_backtick(bytes: &[u8], mut i: usize) -> usize {
    i += 1;
    while i < bytes.len() {
        if bytes[i] == b'`' {
            return i + 1;
        }
        i += 1;
    }
    i
}

/// Advance past a single-quoted literal, honoring `''` and `\` escapes.
fn scan_single_quoted(bytes: &[u8], mut i: usize) -> usize {
    i += 1;
    while i < bytes.len() {
        let sc = bytes[i];
        i += 1;
        if sc == b'\\' && i < bytes.len() {
            i += 1;
        } else if sc == b'\'' {
            if i < bytes.len() && bytes[i] == b'\'' {
                i += 1;
            } else {
                break;
            }
        }
    }
    i
}

/// Advance past a double-quoted span, honoring `\` escapes.
fn scan_double_quoted(bytes: &[u8], mut i: usize) -> usize {
    i += 1;
    while i < bytes.len() {
        let sc = bytes[i];
        i += 1;
        if sc == b'\\' && i < bytes.len() {
            i += 1;
        } else if sc == b'"' {
            break;
        }
    }
    i
}

/// Skip a comment or quoted span so placeholders inside it are not substituted.
/// Returns the new index when `bytes[i]` opens such a span.
fn skip_protected(bytes: &[u8], i: usize) -> Option<usize> {
    match bytes.get(i).copied() {
        Some(b'-') if bytes.get(i + 1) == Some(&b'-') => Some(scan_line_comment(bytes, i)),
        Some(b'`') => Some(scan_backtick(bytes, i)),
        Some(b'\'') => Some(scan_single_quoted(bytes, i)),
        Some(b'"') => Some(scan_double_quoted(bytes, i)),
        _ => None,
    }
}

/// ASCII identifier slice at `[start, end)`. Infallible because the scanner
/// only advances over `is_ident_start` / `is_ident_continue` bytes.
fn ident_at(source: &str, start: usize, end: usize) -> &str {
    source.get(start..end).unwrap_or("")
}

/// Escape a string literal for QQL single-quoted representation.
fn escape_str_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        match c {
            '\'' => out.push_str("''"),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out.push('\'');
    out
}

/// Convert an AST `Value` into its canonical, safely escaped QQL literal string.
///
/// Returns an error if a float value is non-finite (`NaN` or `infinity`).
pub fn value_to_literal(value: &Value) -> Result<String, QqlError> {
    match value {
        Value::Str(s) => Ok(escape_str_literal(s)),
        Value::Int(n) => Ok(n.to_string()),
        Value::Float(f) => {
            if !f.is_finite() {
                return Err(QqlError::validation(
                    "QQL-BIND-INVALID-FLOAT",
                    format!("cannot bind non-finite float value '{}'", f),
                    None,
                ));
            }
            if *f != 0.0 && (f.abs() >= 1e16 || f.abs() < 1e-4) {
                Ok(format!("{:e}", f))
            } else if f.fract() == 0.0 && f.abs() < 1e15 {
                Ok(format!("{:.1}", f))
            } else {
                let s = f.to_string();
                if !s.contains('.') && !s.contains('e') && !s.contains('E') {
                    Ok(format!("{}.0", s))
                } else {
                    Ok(s)
                }
            }
        }
        Value::Bool(b) => Ok(if *b { "true" } else { "false" }.to_string()),
        Value::Null => Ok("null".to_string()),
        Value::List(items) => {
            let mut out = String::from("[");
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&value_to_literal(item)?);
            }
            out.push(']');
            Ok(out)
        }
        Value::Dict(entries) => {
            let mut out = String::from("{");
            for (i, (k, v)) in entries.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                if is_simple_ident(k) {
                    out.push_str(k);
                } else {
                    out.push_str(&escape_str_literal(k));
                }
                out.push_str(": ");
                out.push_str(&value_to_literal(v)?);
            }
            out.push('}');
            Ok(out)
        }
        Value::Param(p) => Ok(alloc::format!(":{}", p)),
        Value::PositionalParam(_) => Ok("?".to_string()),
    }
}

/// Convert an AST `Value` into a human-readable literal string, truncating vectors larger than `max_vec_len`.
pub fn value_to_readable_literal(value: &Value, max_vec_len: usize) -> Result<String, QqlError> {
    match value {
        Value::List(items)
            if items.len() > max_vec_len
                && items
                    .iter()
                    .all(|v| matches!(v, Value::Float(_) | Value::Int(_))) =>
        {
            let mut out = String::from("[");
            let preview_count = max_vec_len.min(items.len());
            for (i, item) in items.iter().take(preview_count).enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                match item {
                    Value::Float(f) => {
                        let s = alloc::format!("{:.4}", f);
                        let trimmed = s.trim_end_matches('0');
                        let trimmed = if trimmed.ends_with('.') {
                            alloc::format!("{}0", trimmed)
                        } else {
                            trimmed.to_string()
                        };
                        out.push_str(&trimmed);
                    }
                    Value::Int(n) => out.push_str(&n.to_string()),
                    _ => {}
                }
            }
            out.push_str(&alloc::format!(", ... ({} dims)]", items.len()));
            Ok(out)
        }
        Value::List(items) => {
            let mut out = String::from("[");
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&value_to_readable_literal(item, max_vec_len)?);
            }
            out.push(']');
            Ok(out)
        }
        Value::Dict(entries) => {
            let mut out = String::from("{");
            for (i, (k, v)) in entries.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                if is_simple_ident(k) {
                    out.push_str(k);
                } else {
                    out.push_str(&escape_str_literal(k));
                }
                out.push_str(": ");
                out.push_str(&value_to_readable_literal(v, max_vec_len)?);
            }
            out.push('}');
            Ok(out)
        }
        other => value_to_literal(other),
    }
}

/// Truncate long numeric vector literals inside a QQL query string for compact preview.
pub fn truncate_vector_literals(source: &str, max_dims: usize) -> String {
    let bytes = source.as_bytes();
    let len = bytes.len();
    let mut out = String::with_capacity(source.len());
    let mut i = 0;
    let mut run = 0;

    while i < len {
        if let Some(next) = skip_protected(bytes, i) {
            i = next;
            continue;
        }

        if bytes[i] == b'[' {
            let bracket_start = i;
            let mut j = i + 1;
            let mut is_numeric_list = true;
            let mut depth = 1;
            while j < len {
                if let Some(next) = skip_protected(bytes, j) {
                    is_numeric_list = false;
                    j = next;
                    continue;
                }
                let b = bytes[j];
                if b == b'[' {
                    depth += 1;
                    is_numeric_list = false;
                } else if b == b']' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                } else if !(b.is_ascii_whitespace()
                    || b.is_ascii_digit()
                    || b == b'.'
                    || b == b'-'
                    || b == b'+'
                    || b == b'e'
                    || b == b'E'
                    || b == b',')
                {
                    is_numeric_list = false;
                }
                j += 1;
            }

            if depth == 0 && is_numeric_list {
                let inner = &source[bracket_start + 1..j];
                let elements: Vec<&str> = inner
                    .split(',')
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .collect();
                if elements.len() > max_dims && elements.iter().all(|s| s.parse::<f64>().is_ok()) {
                    out.push_str(&source[run..bracket_start]);
                    out.push('[');
                    for (idx, elem) in elements[..max_dims].iter().enumerate() {
                        if idx > 0 {
                            out.push_str(", ");
                        }
                        out.push_str(elem);
                    }
                    out.push_str(&alloc::format!(", ... ({} dims)]", elements.len()));
                    i = j + 1;
                    run = i;
                    continue;
                }
            }
        }

        i += 1;
    }

    out.push_str(&source[run..len]);
    out
}

/// Substitute named parameters (`:name`) into `source`.
///
/// `lookup` receives the parameter name without the `:` prefix and returns the bound `Value`.
pub fn bind_named<F>(source: &str, lookup: F) -> Result<String, QqlError>
where
    F: Fn(&str) -> Option<Value>,
{
    let bytes = source.as_bytes();
    let len = bytes.len();
    let mut out = String::with_capacity(source.len());
    let mut i = 0;
    let mut run = 0;

    while i < len {
        if let Some(next) = skip_protected(bytes, i) {
            i = next;
            continue;
        }

        let ch = bytes[i];
        if ch == b'?' {
            return Err(QqlError::validation(
                "QQL-BIND-MIXED-STYLE",
                "query contains positional placeholder '?' — use bind_positional / execute_with_positional_params instead of named parameter binding",
                None,
            ));
        }

        if ch == b':' && i + 1 < len && is_placeholder_start(bytes, i) {
            let next = bytes[i + 1];
            if next != b':' && is_ident_start(next) {
                out.push_str(&source[run..i]);
                i += 1;
                let name_start = i;
                while i < len && is_ident_continue(bytes[i]) {
                    i += 1;
                }
                while i > name_start && bytes[i - 1] == b'.' {
                    i -= 1;
                }
                let name = ident_at(source, name_start, i);
                if let Some(val) = lookup(name) {
                    if matches!(val, Value::Null) {
                        // Rendering `null` would only fail downstream with a
                        // misleading parse error — fail closed here.
                        return Err(QqlError::validation(
                            "QQL-BIND-NULL-PARAM",
                            alloc::format!(
                                "parameter ':{name}' is null; QQL cannot bind null — pass a concrete value or remove the placeholder"
                            ),
                            None,
                        ));
                    }
                    out.push_str(&value_to_literal(&val)?);
                } else {
                    return Err(QqlError::validation(
                        "QQL-BIND-MISSING-PARAM",
                        format!("missing value for named parameter ':{}'", name),
                        None,
                    ));
                }
                run = i;
                continue;
            }
        }

        i += 1;
    }

    out.push_str(&source[run..len]);
    Ok(out)
}

/// Substitute positional parameters (`?`) sequentially into `source`.
pub fn bind_positional(source: &str, params: &[Value]) -> Result<String, QqlError> {
    let bytes = source.as_bytes();
    let len = bytes.len();
    let mut out = String::with_capacity(source.len());
    let mut i = 0;
    let mut run = 0;
    let mut param_index = 0;

    while i < len {
        if let Some(next) = skip_protected(bytes, i) {
            i = next;
            continue;
        }

        let ch = bytes[i];
        if ch == b':' && i + 1 < len && is_placeholder_start(bytes, i) {
            let next = bytes[i + 1];
            if next != b':' && is_ident_start(next) {
                return Err(QqlError::validation(
                    "QQL-BIND-MIXED-STYLE",
                    "query contains named placeholder ':name' — use bind_named / execute_with_params instead of positional parameter binding",
                    None,
                ));
            }
        }

        if ch == b'?' {
            if param_index < params.len() {
                if matches!(params[param_index], Value::Null) {
                    return Err(QqlError::validation(
                        "QQL-BIND-NULL-PARAM",
                        alloc::format!(
                            "positional parameter ?{} is null; QQL cannot bind null — pass a concrete value or remove the placeholder",
                            param_index + 1
                        ),
                        None,
                    ));
                }
                out.push_str(&source[run..i]);
                out.push_str(&value_to_literal(&params[param_index])?);
                param_index += 1;
                i += 1;
                run = i;
                continue;
            }
            return Err(QqlError::validation(
                "QQL-BIND-MISSING-PARAM",
                format!(
                    "positional parameter ? index {} out of range (total provided: {})",
                    param_index + 1,
                    params.len()
                ),
                None,
            ));
        }

        i += 1;
    }

    out.push_str(&source[run..len]);

    if param_index < params.len() {
        let msg = if param_index == 0 {
            format!(
                "no '?' placeholders found in query, but {} positional parameters were supplied",
                params.len()
            )
        } else {
            format!(
                "too many positional parameters provided: bound {}, but {} were supplied",
                param_index,
                params.len()
            )
        };
        return Err(QqlError::validation("QQL-BIND-UNUSED-PARAMS", msg, None));
    }

    Ok(out)
}

/// Substitute named parameters (`:name`) into `source`, truncating long vector literals for readable preview.
pub fn bind_named_readable<F>(source: &str, lookup: F, max_dims: usize) -> Result<String, QqlError>
where
    F: Fn(&str) -> Option<Value>,
{
    let bound = bind_named(source, lookup)?;
    Ok(truncate_vector_literals(&bound, max_dims))
}

/// Substitute positional parameters (`?`) sequentially into `source`, truncating long vector literals for readable preview.
pub fn bind_positional_readable(
    source: &str,
    params: &[Value],
    max_dims: usize,
) -> Result<String, QqlError> {
    let bound = bind_positional(source, params)?;
    Ok(truncate_vector_literals(&bound, max_dims))
}

fn resolve_param<F>(name: &str, lookup: &F) -> Result<Value, QqlError>
where
    F: Fn(&str) -> Option<Value>,
{
    let val = lookup(name).ok_or_else(|| {
        QqlError::validation(
            "QQL-BIND-MISSING-PARAM",
            alloc::format!("missing value for named parameter ':{}'", name),
            None,
        )
    })?;
    if matches!(val, Value::Null) {
        // A literal `None`/`null` parameter used to render as the text
        // `null`, which then failed downstream with a misleading
        // "query input requires …" parse error. Fail closed here instead.
        return Err(QqlError::validation(
            "QQL-BIND-NULL-PARAM",
            alloc::format!(
                "parameter ':{name}' is null; QQL cannot bind null — pass a concrete value or remove the placeholder"
            ),
            None,
        ));
    }
    Ok(val)
}

fn resolve_positional(idx: usize, positional: &[Value]) -> Result<Value, QqlError> {
    let val = positional.get(idx).cloned().ok_or_else(|| {
        QqlError::validation(
            "QQL-BIND-MISSING-PARAM",
            alloc::format!(
                "positional parameter ? index {} out of range (total provided: {})",
                idx + 1,
                positional.len()
            ),
            None,
        )
    })?;
    if matches!(val, Value::Null) {
        return Err(QqlError::validation(
            "QQL-BIND-NULL-PARAM",
            alloc::format!(
                "positional parameter ?{} is null; QQL cannot bind null — pass a concrete value or remove the placeholder",
                idx + 1
            ),
            None,
        ));
    }
    Ok(val)
}

/// Recursively bind parameters into an AST `Value` in-place.
pub fn bind_value<F>(value: &mut Value, lookup: &F, positional: &[Value]) -> Result<(), QqlError>
where
    F: Fn(&str) -> Option<Value>,
{
    match value {
        Value::Param(name) => {
            let resolved = resolve_param(name, lookup)?;
            *value = resolved;
        }
        Value::PositionalParam(idx) => {
            let resolved = resolve_positional(*idx, positional)?;
            *value = resolved;
        }
        Value::List(items) => {
            for item in items {
                bind_value(item, lookup, positional)?;
            }
        }
        Value::Dict(entries) => {
            for (_k, v) in entries {
                bind_value(v, lookup, positional)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Bind parameters into a `PointId` in-place.
pub fn bind_point_id<F>(id: &mut PointId, lookup: &F, positional: &[Value]) -> Result<(), QqlError>
where
    F: Fn(&str) -> Option<Value>,
{
    match id {
        PointId::Param(name) => {
            let val = resolve_param(name, lookup)?;
            *id = value_to_point_id(&val)?;
        }
        PointId::PositionalParam(idx) => {
            let val = resolve_positional(*idx, positional)?;
            *id = value_to_point_id(&val)?;
        }
        _ => {}
    }
    Ok(())
}

fn value_to_point_id(val: &Value) -> Result<PointId, QqlError> {
    match val {
        Value::Int(n) if *n >= 0 => Ok(PointId::Number(*n as u64)),
        Value::Str(s) => Ok(PointId::String(s.clone())),
        _ => Err(QqlError::validation(
            "QQL-BIND-INVALID-POINT-ID",
            alloc::format!("cannot convert parameter value '{:?}' to point id", val),
            None,
        )),
    }
}

/// Bind parameters into a `QueryInput` in-place.
pub fn bind_query_input<F>(
    input: &mut QueryInput,
    lookup: &F,
    positional: &[Value],
) -> Result<(), QqlError>
where
    F: Fn(&str) -> Option<Value>,
{
    match input {
        QueryInput::Param(name) => {
            let val = resolve_param(name, lookup)?;
            *input = value_to_query_input(val)?;
        }
        QueryInput::PositionalParam(idx) => {
            let val = resolve_positional(*idx, positional)?;
            *input = value_to_query_input(val)?;
        }
        QueryInput::Point(point) => {
            bind_point_id(point, lookup, positional)?;
        }
        QueryInput::Text { text, .. } => {
            if let Some(param_name) = text.strip_prefix(':') {
                let val = resolve_param(param_name, lookup)?;
                if let Value::Str(s) = val {
                    *text = s;
                } else {
                    return Err(QqlError::validation(
                        "QQL-BIND-TYPE-MISMATCH",
                        alloc::format!(
                            "parameter ':{}' for TEXT query must be a string",
                            param_name
                        ),
                        None,
                    ));
                }
            } else if let Some(idx_str) = text.strip_prefix('?')
                && let Ok(idx) = idx_str.parse::<usize>()
            {
                let val = resolve_positional(idx, positional)?;
                if let Value::Str(s) = val {
                    *text = s;
                } else {
                    return Err(QqlError::validation(
                        "QQL-BIND-TYPE-MISMATCH",
                        alloc::format!(
                            "positional parameter ?{} for TEXT query must be a string",
                            idx
                        ),
                        None,
                    ));
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn value_to_query_input(val: Value) -> Result<QueryInput, QqlError> {
    match val {
        Value::Str(s) => Ok(QueryInput::Text {
            text: s,
            model: None,
        }),
        Value::List(items) => {
            if items.is_empty() {
                return Ok(QueryInput::Vector(VectorValue::Dense(Vec::new())));
            }
            // A matrix (list of lists) is a multi-vector bag (ColBERT-style).
            // The textual path already accepts `[[0.1, ...], [0.2, ...]]`
            // literals — the AST path must too, so ColBERT queries can be
            // prepared statements.
            let all_rows = items.iter().all(|i| matches!(i, Value::List(_)));
            if all_rows {
                let mut rows = Vec::with_capacity(items.len());
                for row in items {
                    let Value::List(cells) = row else {
                        return Err(QqlError::validation(
                            "QQL-BIND-TYPE-MISMATCH",
                            "matrix parameter bound to query input must contain only lists of numbers",
                            None,
                        ));
                    };
                    let mut floats = Vec::with_capacity(cells.len());
                    for cell in cells {
                        match cell {
                            Value::Float(f) => floats.push(f as f32),
                            Value::Int(i) => floats.push(i as f32),
                            _ => {
                                return Err(QqlError::validation(
                                    "QQL-BIND-TYPE-MISMATCH",
                                    "matrix parameter bound to query input must contain only numbers",
                                    None,
                                ));
                            }
                        }
                    }
                    if floats.is_empty() {
                        return Err(QqlError::validation(
                            "QQL-BIND-TYPE-MISMATCH",
                            "matrix parameter bound to query input must not contain empty rows",
                            None,
                        ));
                    }
                    rows.push(floats);
                }
                return Ok(QueryInput::Vector(VectorValue::MultiDense(rows)));
            }
            let mut floats = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    Value::Float(f) => floats.push(f as f32),
                    Value::Int(i) => floats.push(i as f32),
                    _ => {
                        return Err(QqlError::validation(
                            "QQL-BIND-TYPE-MISMATCH",
                            "list parameter bound to query input must contain only numbers (a matrix of number lists binds as a multi-vector)",
                            None,
                        ));
                    }
                }
            }
            Ok(QueryInput::Vector(VectorValue::Dense(floats)))
        }
        Value::Int(n) if n >= 0 => Ok(QueryInput::Point(PointId::Number(n as u64))),
        _ => Err(QqlError::validation(
            "QQL-BIND-TYPE-MISMATCH",
            alloc::format!("unsupported value type for query input: {:?}", val),
            None,
        )),
    }
}

/// Recursively bind parameters into a `FilterExpr` in-place.
pub fn bind_filter<F>(
    filter: &mut FilterExpr,
    lookup: &F,
    positional: &[Value],
) -> Result<(), QqlError>
where
    F: Fn(&str) -> Option<Value>,
{
    match filter {
        FilterExpr::PointId(pred) => match pred {
            PointIdPredicate::Eq(id) => bind_point_id(id, lookup, positional)?,
            PointIdPredicate::In(ids) => {
                for id in ids {
                    bind_point_id(id, lookup, positional)?;
                }
            }
        },
        FilterExpr::Compare { value, .. } => {
            bind_value(value, lookup, positional)?;
        }
        FilterExpr::Between { low, high, .. } => {
            bind_value(low, lookup, positional)?;
            bind_value(high, lookup, positional)?;
        }
        FilterExpr::In { values, .. } | FilterExpr::MatchAny { values, .. } => {
            for v in values {
                bind_value(v, lookup, positional)?;
            }
        }
        FilterExpr::And { operands } | FilterExpr::Or { operands } => {
            for op in operands {
                bind_filter(op, lookup, positional)?;
            }
        }
        FilterExpr::Not { operand } => {
            bind_filter(operand, lookup, positional)?;
        }
        FilterExpr::Nested { filter, .. } => {
            bind_filter(filter, lookup, positional)?;
        }
        _ => {}
    }
    Ok(())
}

/// Recursively bind parameters into a `FormulaExpr` in-place.
pub fn bind_formula<F>(
    formula: &mut FormulaExpr,
    lookup: &F,
    positional: &[Value],
) -> Result<(), QqlError>
where
    F: Fn(&str) -> Option<Value>,
{
    match formula {
        FormulaExpr::Variable { name } => {
            if let Some(param_name) = name.strip_prefix(':') {
                let val = resolve_param(param_name, lookup)?;
                *formula = value_to_formula_constant(val)?;
            } else if let Some(idx_str) = name.strip_prefix('?')
                && let Ok(idx) = idx_str.parse::<usize>()
            {
                let val = resolve_positional(idx, positional)?;
                *formula = value_to_formula_constant(val)?;
            }
        }
        FormulaExpr::Sum { left, right }
        | FormulaExpr::Sub { left, right }
        | FormulaExpr::Mul { left, right }
        | FormulaExpr::Div { left, right, .. }
        | FormulaExpr::Pow {
            base: left,
            exponent: right,
        } => {
            bind_formula(left, lookup, positional)?;
            bind_formula(right, lookup, positional)?;
        }
        FormulaExpr::Neg { operand }
        | FormulaExpr::Abs { x: operand }
        | FormulaExpr::Sqrt { x: operand }
        | FormulaExpr::Log { x: operand }
        | FormulaExpr::Ln { x: operand }
        | FormulaExpr::Exp { x: operand }
        | FormulaExpr::Acosh { x: operand } => {
            bind_formula(operand, lookup, positional)?;
        }
        FormulaExpr::Max { args } | FormulaExpr::Min { args } => {
            for arg in args {
                bind_formula(arg, lookup, positional)?;
            }
        }
        FormulaExpr::Decay { x, target, .. } => {
            bind_formula(x, lookup, positional)?;
            if let Some(t) = target {
                bind_formula(t, lookup, positional)?;
            }
        }
        FormulaExpr::Case { cond, then_, else_ } => {
            bind_filter(cond, lookup, positional)?;
            bind_formula(then_, lookup, positional)?;
            bind_formula(else_, lookup, positional)?;
        }
        FormulaExpr::MatchCondition { values, .. } => {
            for v in values {
                bind_value(v, lookup, positional)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn looks_like_iso_datetime(s: &str) -> bool {
    let bytes = s.as_bytes();
    bytes.len() >= 10
        && bytes[0..4].iter().all(|b| b.is_ascii_digit())
        && bytes[4] == b'-'
        && bytes[5..7].iter().all(|b| b.is_ascii_digit())
        && bytes[7] == b'-'
        && bytes[8..10].iter().all(|b| b.is_ascii_digit())
}

fn value_to_u64(val: &Value, clause: &str) -> Result<u64, QqlError> {
    match val {
        Value::Int(n) if *n >= 0 => Ok(*n as u64),
        _ => Err(QqlError::validation(
            "QQL-BIND-INVALID-INTEGER",
            alloc::format!("{} parameter must be a non-negative integer", clause),
            None,
        )),
    }
}

fn value_to_formula_constant(val: Value) -> Result<FormulaExpr, QqlError> {
    match val {
        Value::Float(f) => Ok(FormulaExpr::Constant { value: f }),
        Value::Int(i) => Ok(FormulaExpr::Constant { value: i as f64 }),
        Value::Str(s) => {
            if looks_like_iso_datetime(&s) {
                Ok(FormulaExpr::Datetime { value: s })
            } else if let Ok(f) = s.parse::<f64>() {
                Ok(FormulaExpr::Constant { value: f })
            } else {
                Ok(FormulaExpr::Variable { name: s })
            }
        }
        _ => Err(QqlError::validation(
            "QQL-BIND-FORMULA-TYPE",
            alloc::format!("formula parameter cannot be bound to value: {:?}", val),
            None,
        )),
    }
}

/// Bind parameters into a `PageSpec` in-place.
pub fn bind_page_spec<F>(
    page: &mut PageSpec,
    lookup: &F,
    positional: &[Value],
) -> Result<(), QqlError>
where
    F: Fn(&str) -> Option<Value>,
{
    if let Some(param) = &page.limit_param {
        if let Some(name) = param.strip_prefix(':') {
            let val = resolve_param(name, lookup)?;
            page.limit = Some(value_to_u64(&val, "LIMIT")?);
        } else if let Some(idx_str) = param.strip_prefix('?') {
            if let Ok(idx) = idx_str.parse::<usize>() {
                let val = resolve_positional(idx, positional)?;
                page.limit = Some(value_to_u64(&val, "LIMIT")?);
            }
        } else {
            let val = resolve_param(param, lookup)?;
            page.limit = Some(value_to_u64(&val, "LIMIT")?);
        }
        page.limit_param = None;
    }
    if let Some(param) = &page.offset_param {
        if let Some(name) = param.strip_prefix(':') {
            let val = resolve_param(name, lookup)?;
            page.offset = Some(value_to_u64(&val, "OFFSET")?);
        } else if let Some(idx_str) = param.strip_prefix('?') {
            if let Ok(idx) = idx_str.parse::<usize>() {
                let val = resolve_positional(idx, positional)?;
                page.offset = Some(value_to_u64(&val, "OFFSET")?);
            }
        } else {
            let val = resolve_param(param, lookup)?;
            page.offset = Some(value_to_u64(&val, "OFFSET")?);
        }
        page.offset_param = None;
    }
    Ok(())
}

fn bind_point_selector<F>(
    sel: &mut PointSelector,
    lookup: &F,
    positional: &[Value],
) -> Result<(), QqlError>
where
    F: Fn(&str) -> Option<Value>,
{
    match sel {
        PointSelector::Id(id) => bind_point_id(id, lookup, positional),
        PointSelector::Ids(ids) => {
            for id in ids {
                bind_point_id(id, lookup, positional)?;
            }
            Ok(())
        }
        PointSelector::Filter(filter) => bind_filter(filter, lookup, positional),
    }
}

fn bind_prefetch<F>(
    prefetch: &mut Prefetch,
    lookup: &F,
    positional: &[Value],
) -> Result<(), QqlError>
where
    F: Fn(&str) -> Option<Value>,
{
    match &mut prefetch.source {
        PrefetchSource::Query(sub) => bind_query_stmt(sub, lookup, positional)?,
        PrefetchSource::Cte(_) => {}
    }
    if let Some(f) = &mut prefetch.filter {
        bind_filter(f, lookup, positional)?;
    }
    Ok(())
}

/// Recursively bind parameters into a `QueryExpr` in-place.
pub fn bind_query_expr<F>(
    expr: &mut QueryExpr,
    lookup: &F,
    positional: &[Value],
) -> Result<(), QqlError>
where
    F: Fn(&str) -> Option<Value>,
{
    match expr {
        QueryExpr::Points { ids } => {
            for id in ids {
                bind_point_id(id, lookup, positional)?;
            }
        }
        QueryExpr::Nearest {
            input, prefetch, ..
        } => {
            bind_query_input(input, lookup, positional)?;
            for p in prefetch {
                bind_prefetch(p, lookup, positional)?;
            }
        }
        QueryExpr::Recommend {
            positive,
            negative,
            prefetch,
            ..
        } => {
            for pos in positive {
                bind_query_input(pos, lookup, positional)?;
            }
            for neg in negative {
                bind_query_input(neg, lookup, positional)?;
            }
            for p in prefetch {
                bind_prefetch(p, lookup, positional)?;
            }
        }
        QueryExpr::Context {
            pairs, prefetch, ..
        } => {
            for pair in pairs {
                bind_query_input(&mut pair.positive, lookup, positional)?;
                bind_query_input(&mut pair.negative, lookup, positional)?;
            }
            for p in prefetch {
                bind_prefetch(p, lookup, positional)?;
            }
        }
        QueryExpr::Discover {
            target,
            context,
            prefetch,
            ..
        } => {
            bind_query_input(target, lookup, positional)?;
            for pair in context {
                bind_query_input(&mut pair.positive, lookup, positional)?;
                bind_query_input(&mut pair.negative, lookup, positional)?;
            }
            for p in prefetch {
                bind_prefetch(p, lookup, positional)?;
            }
        }
        QueryExpr::OrderBy { .. } | QueryExpr::SampleRandom => {}
        QueryExpr::Fusion { prefetch, .. } => {
            for p in prefetch {
                bind_prefetch(p, lookup, positional)?;
            }
        }
        QueryExpr::Formula {
            expression,
            defaults,
            prefetch,
        } => {
            bind_formula(expression, lookup, positional)?;
            for (_k, v) in defaults {
                bind_value(v, lookup, positional)?;
            }
            for p in prefetch {
                bind_prefetch(p, lookup, positional)?;
            }
        }
        QueryExpr::RelevanceFeedback {
            target,
            feedback,
            prefetch,
            ..
        } => {
            bind_query_input(target, lookup, positional)?;
            for item in feedback {
                bind_query_input(&mut item.example, lookup, positional)?;
            }
            for p in prefetch {
                bind_prefetch(p, lookup, positional)?;
            }
        }
        QueryExpr::Hybrid { text, .. } => {
            if let Some(param_name) = text.strip_prefix(':') {
                let val = resolve_param(param_name, lookup)?;
                if let Value::Str(s) = val {
                    *text = s;
                } else {
                    return Err(QqlError::validation(
                        "QQL-BIND-TYPE-MISMATCH",
                        alloc::format!(
                            "parameter ':{}' for HYBRID query must be a string",
                            param_name
                        ),
                        None,
                    ));
                }
            } else if let Some(idx_str) = text.strip_prefix('?')
                && let Ok(idx) = idx_str.parse::<usize>()
            {
                let val = resolve_positional(idx, positional)?;
                if let Value::Str(s) = val {
                    *text = s;
                } else {
                    return Err(QqlError::validation(
                        "QQL-BIND-TYPE-MISMATCH",
                        alloc::format!(
                            "positional parameter ?{} for HYBRID query must be a string",
                            idx
                        ),
                        None,
                    ));
                }
            }
        }
        QueryExpr::Rerank {
            input, prefetch, ..
        } => {
            bind_query_input(input, lookup, positional)?;
            for p in prefetch {
                bind_prefetch(p, lookup, positional)?;
            }
        }
        QueryExpr::CrossRerank {
            query, prefetch, ..
        } => {
            if let Some(param_name) = query.strip_prefix(':') {
                let val = resolve_param(param_name, lookup)?;
                if let Value::Str(s) = val {
                    *query = s;
                } else {
                    return Err(QqlError::validation(
                        "QQL-BIND-TYPE-MISMATCH",
                        alloc::format!(
                            "parameter ':{}' for CROSS RERANK query must be a string",
                            param_name
                        ),
                        None,
                    ));
                }
            } else if let Some(idx_str) = query.strip_prefix('?')
                && let Ok(idx) = idx_str.parse::<usize>()
            {
                let val = resolve_positional(idx, positional)?;
                if let Value::Str(s) = val {
                    *query = s;
                } else {
                    return Err(QqlError::validation(
                        "QQL-BIND-TYPE-MISMATCH",
                        alloc::format!(
                            "positional parameter ?{} for CROSS RERANK query must be a string",
                            idx
                        ),
                        None,
                    ));
                }
            }
            for p in prefetch {
                bind_prefetch(p, lookup, positional)?;
            }
        }
    }
    Ok(())
}

/// Recursively bind parameters into a `QueryStmt` in-place.
pub fn bind_query_stmt<F>(
    query: &mut QueryStmt,
    lookup: &F,
    positional: &[Value],
) -> Result<(), QqlError>
where
    F: Fn(&str) -> Option<Value>,
{
    for cte in &mut query.ctes {
        bind_query_stmt(&mut cte.query, lookup, positional)?;
    }
    bind_query_expr(&mut query.expression, lookup, positional)?;
    if let Some(filter) = &mut query.filter {
        bind_filter(filter, lookup, positional)?;
    }
    bind_page_spec(&mut query.page, lookup, positional)?;
    Ok(())
}

/// Bind parameters into a parsed AST `Stmt` in-place.
pub fn bind_stmt<F>(stmt: &mut Stmt, lookup: F, positional: &[Value]) -> Result<(), QqlError>
where
    F: Fn(&str) -> Option<Value>,
{
    match stmt {
        Stmt::Query(query) => bind_query_stmt(query, &lookup, positional),
        Stmt::Scroll(scroll) => {
            if let Some(filter) = &mut scroll.filter {
                bind_filter(filter, &lookup, positional)?;
            }
            Ok(())
        }
        Stmt::Upsert(upsert) => {
            for point in &mut upsert.points {
                bind_point_id(&mut point.id, &lookup, positional)?;
                for (_k, v) in &mut point.payload {
                    bind_value(v, &lookup, positional)?;
                }
            }
            Ok(())
        }
        Stmt::Delete(del) => bind_point_selector(&mut del.selector, &lookup, positional),
        Stmt::ClearPayload(cp) => bind_point_selector(&mut cp.selector, &lookup, positional),
        Stmt::DeletePayload(dp) => bind_point_selector(&mut dp.selector, &lookup, positional),
        Stmt::DeleteVector(dv) => bind_point_selector(&mut dv.selector, &lookup, positional),
        Stmt::UpdateVector(uv) => bind_point_id(&mut uv.point_id, &lookup, positional),
        Stmt::UpdatePayload(up) => {
            bind_point_selector(&mut up.selector, &lookup, positional)?;
            for (_k, v) in &mut up.payload {
                bind_value(v, &lookup, positional)?;
            }
            Ok(())
        }
        Stmt::Count(count) => {
            if let Some(filter) = &mut count.filter {
                bind_filter(filter, &lookup, positional)?;
            }
            Ok(())
        }
        Stmt::Facet(facet) => {
            if let Some(filter) = &mut facet.filter {
                bind_filter(filter, &lookup, positional)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;

    fn check_value_roundtrip(val: Value) {
        let lit = value_to_literal(&val).expect("value_to_literal should succeed");
        let parsed = Parser::parse_value(&lit).expect("Parser::parse_value should succeed");
        assert_eq!(
            parsed, val,
            "value roundtrip mismatch for literal '{}'",
            lit
        );
    }

    #[test]
    fn test_value_to_literal_escaping() {
        assert_eq!(
            value_to_literal(&Value::Str("O'Connor and \\path".into())).unwrap(),
            "'O''Connor and \\\\path'"
        );
        assert_eq!(value_to_literal(&Value::Int(42)).unwrap(), "42");
        assert_eq!(value_to_literal(&Value::Float(3.75)).unwrap(), "3.75");
        assert_eq!(value_to_literal(&Value::Float(1.0)).unwrap(), "1.0");
        assert_eq!(value_to_literal(&Value::Float(1e300)).unwrap(), "1e300");
        assert_eq!(value_to_literal(&Value::Bool(true)).unwrap(), "true");
        assert_eq!(value_to_literal(&Value::Null).unwrap(), "null");
    }

    #[test]
    fn test_value_ast_equality_roundtrips() {
        check_value_roundtrip(Value::Str("plain".into()));
        check_value_roundtrip(Value::Str(
            "escaped 'quote' and \\backslash\nnewline".into(),
        ));
        check_value_roundtrip(Value::Int(12345));
        check_value_roundtrip(Value::Float(45.5));
        check_value_roundtrip(Value::Float(1.25e-5));
        check_value_roundtrip(Value::Bool(true));
        check_value_roundtrip(Value::Bool(false));
        check_value_roundtrip(Value::Null);
        check_value_roundtrip(Value::List(alloc::vec![
            Value::Int(1),
            Value::Str("two".into()),
            Value::Bool(true),
        ]));
        check_value_roundtrip(Value::Dict(alloc::vec![
            ("simple".into(), Value::Int(1)),
            ("".into(), Value::Str("empty key".into())),
            ("$special_ident".into(), Value::Int(2)),
            ("foo.bar".into(), Value::Int(3)),
            ("a: 1, b".into(), Value::Int(4)),
            ("weird 'quoted'".into(), Value::Bool(false)),
        ]));
    }

    #[test]
    fn test_parse_value_rejects_trailing_tokens() {
        assert!(Parser::parse_value("42 trailing").is_err());
        assert!(Parser::parse_value("'string' extra").is_err());
    }

    #[test]
    fn test_dict_key_escaping_prevents_injection() {
        let dict = Value::Dict(alloc::vec![
            ("simple_key".into(), Value::Int(1)),
            ("a: 1, b".into(), Value::Int(5)),
            ("weird 'quote".into(), Value::Str("val".into())),
        ]);
        let lit = value_to_literal(&dict).unwrap();
        assert_eq!(lit, "{simple_key: 1, 'a: 1, b': 5, 'weird ''quote': 'val'}");

        let query = format!("UPSERT INTO test VALUES {{id: 1, payload: {}}};", lit);
        let parsed = Parser::parse(&query);
        assert!(parsed.is_ok(), "parsed error: {:?}", parsed.err());
    }

    #[test]
    fn test_non_finite_floats_rejected() {
        assert!(value_to_literal(&Value::Float(f64::NAN)).is_err());
        assert!(value_to_literal(&Value::Float(f64::INFINITY)).is_err());
        assert!(value_to_literal(&Value::Float(f64::NEG_INFINITY)).is_err());
    }

    #[test]
    fn test_dollar_identifiers_never_corrupted() {
        let query =
            "QUERY TEXT 'chest pain' FROM docs WHERE $category = 'medical' AND $1 = 5 LIMIT 5;";
        let bound = bind_named(query, |name| match name {
            "category" => Some(Value::Str("ignored".into())),
            _ => None,
        })
        .unwrap();

        assert_eq!(bound, query);
    }

    #[test]
    fn test_bind_preserves_compact_dict_syntax() {
        // In {id: 1, a:b} or {'a':b}, the colon after 'a' or ''a'' is a dictionary separator, NOT a placeholder :b.
        let query = "UPSERT INTO t VALUES {id: 1, a:b, 'c':d, \"e\":f, `g`:h};";
        let bound = bind_named(query, |name| match name {
            "b" | "d" | "f" | "h" => Some(Value::Int(99)),
            _ => None,
        })
        .unwrap();

        assert_eq!(bound, query);

        // Positional binder must also ignore compact dict colons and not flag false-positive mixed style
        let query_pos = "UPSERT INTO t VALUES {id: ?, a:b};";
        let bound_pos = bind_positional(query_pos, &[Value::Int(1)]).unwrap();
        assert_eq!(bound_pos, "UPSERT INTO t VALUES {id: 1, a:b};");
    }

    #[test]
    fn test_bind_named_variables() {
        let query =
            "QUERY TEXT :q FROM docs WHERE category = :cat AND active = :is_active LIMIT :lim;";
        let result = bind_named(query, |name| match name {
            "q" => Some(Value::Str("chest pain".into())),
            "cat" => Some(Value::Str("medical".into())),
            "is_active" => Some(Value::Bool(true)),
            "lim" => Some(Value::Int(10)),
            _ => None,
        })
        .unwrap();

        assert_eq!(
            result,
            "QUERY TEXT 'chest pain' FROM docs WHERE category = 'medical' AND active = true LIMIT 10;"
        );
    }

    #[test]
    fn test_mixed_placeholder_style_errors() {
        let query_with_q = "QUERY TEXT :q FROM docs LIMIT ?;";
        assert!(bind_named(query_with_q, |_| Some(Value::Str("x".into()))).is_err());

        let query_with_name = "QUERY TEXT ? FROM docs WHERE cat = :cat;";
        assert!(bind_positional(query_with_name, &[Value::Str("x".into())]).is_err());
    }

    #[test]
    fn test_bind_preserves_literals_comments_and_backticks() {
        let query = "-- Search for :cat in comments\nQUERY TEXT 'hello :q' FROM docs WHERE path = `C:\\docs\\:name` AND status = :status;";
        let result = bind_named(query, |name| match name {
            "status" => Some(Value::Str("active".into())),
            _ => None,
        })
        .unwrap();

        assert_eq!(
            result,
            "-- Search for :cat in comments\nQUERY TEXT 'hello :q' FROM docs WHERE path = `C:\\docs\\:name` AND status = 'active';"
        );
    }

    #[test]
    fn test_bind_positional_variables() {
        let query = "QUERY TEXT ? FROM docs WHERE tenant = ? AND score >= ? LIMIT ?;";
        let params = alloc::vec![
            Value::Str("acme".into()),
            Value::Str("acme_tenant".into()),
            Value::Float(0.85),
            Value::Int(5),
        ];
        let result = bind_positional(query, &params).unwrap();
        assert_eq!(
            result,
            "QUERY TEXT 'acme' FROM docs WHERE tenant = 'acme_tenant' AND score >= 0.85 LIMIT 5;"
        );
    }

    #[test]
    fn test_bind_positional_count_mismatch() {
        let query = "QUERY TEXT ? FROM docs LIMIT ?;";
        let too_few = alloc::vec![Value::Str("q".into())];
        assert!(bind_positional(query, &too_few).is_err());

        let too_many = alloc::vec![Value::Str("q".into()), Value::Int(5), Value::Int(10)];
        assert!(bind_positional(query, &too_many).is_err());

        let query_no_placeholders = "QUERY TEXT 'test' FROM docs LIMIT 5;";
        let err = bind_positional(query_no_placeholders, &[Value::Int(1)]).unwrap_err();
        assert!(
            err.to_string()
                .contains("no '?' placeholders found in query")
        );
    }

    #[test]
    fn test_dotted_parameter_names() {
        let query = "QUERY TEXT :center.query FROM docs WHERE lat = :center.lat AND lon = :center.lon LIMIT 5;";
        let result = bind_named(query, |name| match name {
            "center.query" => Some(Value::Str("coffee".into())),
            "center.lat" => Some(Value::Float(37.7749)),
            "center.lon" => Some(Value::Float(-122.4194)),
            _ => None,
        })
        .unwrap();

        assert_eq!(
            result,
            "QUERY TEXT 'coffee' FROM docs WHERE lat = 37.7749 AND lon = -122.4194 LIMIT 5;"
        );
    }

    #[test]
    fn test_bind_stmt_ast() {
        let query =
            "QUERY TEXT :q FROM docs WHERE category = :cat AND score > :min_score LIMIT :lim;";
        let mut stmt = Parser::parse(query).expect("query with parameters should parse into AST");

        bind_stmt(
            &mut stmt,
            |name| match name {
                "q" => Some(Value::Str("headache".into())),
                "cat" => Some(Value::Str("medical".into())),
                "min_score" => Some(Value::Float(0.75)),
                "lim" => Some(Value::Int(10)),
                _ => None,
            },
            &[],
        )
        .expect("bind_stmt should succeed");

        let formatted = crate::fmt::format_stmt(&stmt);
        assert_eq!(
            formatted,
            "QUERY 'headache' FROM docs WHERE category = 'medical' AND score > 0.75 LIMIT 10"
        );
    }

    #[test]
    fn test_truncate_vector_literals() {
        let qql = "QUERY VECTOR [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8] FROM docs LIMIT 5;";
        let truncated = truncate_vector_literals(qql, 3);
        assert_eq!(
            truncated,
            "QUERY VECTOR [0.1, 0.2, 0.3, ... (8 dims)] FROM docs LIMIT 5;"
        );

        // String literals inside queries are preserved
        let with_str = "QUERY TEXT '[0.1, 0.2, 0.3, 0.4, 0.5, 0.6]' FROM docs LIMIT 5;";
        let not_truncated = truncate_vector_literals(with_str, 3);
        assert_eq!(not_truncated, with_str);
    }
}
