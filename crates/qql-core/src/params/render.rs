//! Value literal formatting and vector literal truncation.

use super::scan::skip_protected;
use crate::ast::{Value, escape_string, is_simple_ident};
use crate::error::QqlError;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Escape a string literal for QQL single-quoted representation.
pub fn escape_str_literal(s: &str) -> String {
    format!("'{}'", escape_string(s))
}

/// Convert an AST `Value` into its canonical, safely escaped QQL literal string.
///
/// Returns `QQL-BIND-TYPE-MISMATCH` if a float value is non-finite (`NaN` or `infinity`).
pub fn value_to_literal(value: &Value) -> Result<String, QqlError> {
    match value {
        Value::Str(s) => Ok(escape_str_literal(s)),
        Value::Int(n) => Ok(n.to_string()),
        Value::UInt(n) => Ok(n.to_string()),
        Value::Float(f) => {
            if !f.is_finite() {
                return Err(QqlError::validation(
                    "QQL-BIND-TYPE-MISMATCH",
                    format!("cannot bind non-finite float value '{f}'"),
                    None,
                ));
            }
            if *f != 0.0 && (f.abs() >= 1e16 || f.abs() < 1e-4) {
                Ok(format!("{f:e}"))
            } else if f.fract() == 0.0 && f.abs() < 1e15 {
                Ok(format!("{f:.1}"))
            } else {
                let s = f.to_string();
                if !s.contains('.') && !s.contains('e') && !s.contains('E') {
                    Ok(format!("{s}.0"))
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
        Value::F32Array(values) => {
            let mut out = String::from("[");
            for (i, &f) in values.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                if !f.is_finite() {
                    return Err(QqlError::validation(
                        "QQL-BIND-TYPE-MISMATCH",
                        format!("cannot bind non-finite float value '{f}'"),
                        None,
                    ));
                }
                // Shortest-f32 rendering (shared with AST vector formatting):
                // widening to f64 first would print 0.1f32 as
                // 0.10000000149011612.
                out.push_str(&crate::fmt::expr::render_f32(f));
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
        Value::Param(p, _) => Ok(format!(":{p}")),
        Value::PositionalParam(..) => Ok("?".to_string()),
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
                    out.push_str(&format!(", ... ({} dims)]", elements.len()));
                    i = j + 1;
                    run = i;
                    continue;
                }
            }
        }
        i += 1;
    }
    out.push_str(&source[run..]);
    out
}
