//! Text-based parameter substitution and literal formatting.

use crate::ast::{Value, escape_string, is_simple_ident};
use crate::error::{QqlError, Span};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

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
    b.is_ascii_alphanumeric() || b == b'_'
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

#[inline]
fn has_closing_triple(bytes: &[u8], start: usize, triple: &[u8; 3]) -> bool {
    start + 2 < bytes.len() && &bytes[start..start + 3] == triple
}

/// Advance past a triple-quoted string (`'''...'''` or `"""..."""`).
fn scan_triple_quoted(bytes: &[u8], mut i: usize, quote: u8) -> usize {
    i += 3;
    let triple = [quote, quote, quote];
    while i < bytes.len() {
        if has_closing_triple(bytes, i, &triple) {
            return i + 3;
        }
        i += 1;
    }
    i
}

/// Advance past a raw string literal `r'...'` or `r"..."`.
fn scan_raw_string(bytes: &[u8], mut i: usize, quote: u8) -> usize {
    i += 2;
    while i < bytes.len() {
        if bytes[i] == quote {
            return i + 1;
        }
        i += 1;
    }
    i
}

/// Advance past a double-quoted string literal (which may contain `\"`).
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

/// If `bytes[i]` begins a comment, string literal, or backtick identifier,
/// returns `Some(next_offset)` skipping the protected region. Otherwise `None`.
fn skip_protected(bytes: &[u8], i: usize) -> Option<usize> {
    match bytes[i] {
        b'-' if i + 1 < bytes.len() && bytes[i + 1] == b'-' => {
            Some(scan_line_comment(bytes, i + 2))
        }
        b'`' => Some(scan_backtick(bytes, i)),
        b'\'' => {
            if i + 2 < bytes.len() && bytes[i + 1] == b'\'' && bytes[i + 2] == b'\'' {
                Some(scan_triple_quoted(bytes, i, b'\''))
            } else {
                Some(scan_single_quoted(bytes, i))
            }
        }
        b'"' => {
            if i + 2 < bytes.len() && bytes[i + 1] == b'"' && bytes[i + 2] == b'"' {
                Some(scan_triple_quoted(bytes, i, b'"'))
            } else {
                Some(scan_double_quoted(bytes, i))
            }
        }
        b'r' if i + 1 < bytes.len() && (bytes[i + 1] == b'\'' || bytes[i + 1] == b'"') => {
            let quote = bytes[i + 1];
            if i + 3 < bytes.len() && bytes[i + 2] == quote && bytes[i + 3] == quote {
                Some(scan_triple_quoted(bytes, i + 1, quote))
            } else {
                Some(scan_raw_string(bytes, i, quote))
            }
        }
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
    format!("'{}'", escape_string(s))
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
                Some(Span {
                    start: i,
                    end: i + 1,
                }),
            ));
        }

        if ch == b':' && i + 1 < len && is_placeholder_start(bytes, i) {
            let next = bytes[i + 1];
            if next != b':' && is_ident_start(next) {
                out.push_str(&source[run..i]);
                let colon_pos = i;
                i += 1;
                let name_start = i;
                while i < len {
                    if is_ident_continue(bytes[i]) {
                        i += 1;
                    } else if bytes[i] == b'.' && i + 1 < len && is_ident_start(bytes[i + 1]) {
                        i += 2;
                    } else {
                        break;
                    }
                }
                let name = ident_at(source, name_start, i);
                let span = Some(Span {
                    start: colon_pos,
                    end: i,
                });
                if let Some(val) = lookup(name) {
                    if matches!(val, Value::Null) {
                        // Rendering `null` would only fail downstream with a
                        // misleading parse error — fail closed here.
                        return Err(QqlError::validation(
                            "QQL-BIND-NULL-PARAM",
                            alloc::format!(
                                "parameter ':{name}' is null; QQL cannot bind null — pass a concrete value or remove the placeholder"
                            ),
                            span,
                        ));
                    }
                    out.push_str(&value_to_literal(&val)?);
                } else {
                    return Err(QqlError::validation(
                        "QQL-BIND-MISSING-PARAM",
                        format!("missing value for named parameter ':{}'", name),
                        span,
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
                    Some(Span {
                        start: i,
                        end: i + 2,
                    }),
                ));
            }
        }

        if ch == b'?' {
            let q_span = Some(Span {
                start: i,
                end: i + 1,
            });
            if param_index < params.len() {
                if matches!(params[param_index], Value::Null) {
                    return Err(QqlError::validation(
                        "QQL-BIND-NULL-PARAM",
                        alloc::format!(
                            "positional parameter ?{} is null; QQL cannot bind null — pass a concrete value or remove the placeholder",
                            param_index + 1
                        ),
                        q_span,
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
                    "missing value for positional parameter ?{} (only {} parameter(s) provided)",
                    param_index + 1,
                    params.len()
                ),
                q_span,
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
