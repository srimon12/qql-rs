//! Text-based parameter substitution and literal formatting.

use super::render::{truncate_vector_literals, value_to_literal};
pub use super::scan::{
    ident_at, is_ident_continue, is_ident_start, is_placeholder_start, skip_protected,
};

use crate::ast::Value;
use crate::error::{QqlError, Span};
use alloc::format;
use alloc::string::String;

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
                        return Err(QqlError::validation(
                            "QQL-BIND-NULL-PARAM",
                            format!(
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
                        format!(
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
