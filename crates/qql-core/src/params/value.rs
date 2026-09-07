//! Core value, point ID, and scalar parameter binding and resolution.

use crate::ast::Value;
use crate::ast::statement::PointId;
use crate::error::{QqlError, Span};
use alloc::format;

/// Resolve a named parameter from the lookup function.
pub fn resolve_param<F>(name: &str, span: Option<Span>, lookup: &F) -> Result<Value, QqlError>
where
    F: Fn(&str) -> Option<Value>,
{
    let val = lookup(name).ok_or_else(|| {
        QqlError::validation(
            "QQL-BIND-MISSING-PARAM",
            format!("missing value for named parameter ':{}'", name),
            span,
        )
    })?;
    if matches!(val, Value::Null) {
        return Err(QqlError::validation(
            "QQL-BIND-NULL-PARAM",
            format!(
                "parameter ':{name}' is null; QQL cannot bind null — pass a concrete value or remove the placeholder"
            ),
            span,
        ));
    }
    Ok(val)
}

/// Resolve a positional parameter from the positional slice.
pub fn resolve_positional(
    idx: usize,
    span: Option<Span>,
    positional: &[Value],
) -> Result<Value, QqlError> {
    let val = positional.get(idx).cloned().ok_or_else(|| {
        QqlError::validation(
            "QQL-BIND-MISSING-PARAM",
            format!(
                "positional parameter ? index {} out of range (total provided: {})",
                idx + 1,
                positional.len()
            ),
            span,
        )
    })?;
    if matches!(val, Value::Null) {
        return Err(QqlError::validation(
            "QQL-BIND-NULL-PARAM",
            format!(
                "positional parameter ?{} is null; QQL cannot bind null — pass a concrete value or remove the placeholder",
                idx + 1
            ),
            span,
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
        Value::Param(name, span) => {
            let resolved = resolve_param(name, *span, lookup)?;
            *value = resolved;
        }
        Value::PositionalParam(idx, span) => {
            let resolved = resolve_positional(*idx, *span, positional)?;
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
        PointId::Param(name, span) => {
            let val = resolve_param(name, *span, lookup)?;
            *id = value_to_point_id(&val, *span)?;
        }
        PointId::PositionalParam(idx, span) => {
            let val = resolve_positional(*idx, *span, positional)?;
            *id = value_to_point_id(&val, *span)?;
        }
        _ => {}
    }
    Ok(())
}

/// Convert a bound `Value` into a `PointId`, failing closed on type mismatch.
pub fn value_to_point_id(val: &Value, span: Option<Span>) -> Result<PointId, QqlError> {
    match val {
        Value::Int(n) if *n >= 0 => Ok(PointId::Number(*n as u64)),
        Value::Str(s) => Ok(PointId::String(s.clone())),
        _ => Err(QqlError::validation(
            "QQL-BIND-TYPE-MISMATCH",
            format!("cannot bind {val:?} as point ID: expected non-negative integer or string"),
            span,
        )),
    }
}

/// Convert a bound `Value` into a `u64`, failing closed on type mismatch.
pub fn value_to_u64(val: &Value, clause: &str, span: Option<Span>) -> Result<u64, QqlError> {
    match val {
        Value::Int(n) if *n >= 0 => Ok(*n as u64),
        _ => Err(QqlError::validation(
            "QQL-BIND-TYPE-MISMATCH",
            format!("{clause} parameter must be a non-negative integer"),
            span,
        )),
    }
}

/// Like `value_to_u64`, but rejects `0` for clauses requiring positive integers (e.g. `LIMIT`).
pub fn value_to_positive_u64(
    val: &Value,
    clause: &str,
    span: Option<Span>,
) -> Result<u64, QqlError> {
    let n = value_to_u64(val, clause, span)?;
    if n == 0 {
        return Err(QqlError::validation(
            "QQL-BIND-TYPE-MISMATCH",
            format!("{clause} parameter must be a positive integer"),
            span,
        ));
    }
    Ok(n)
}

/// Resolve a parameter string (`:name` or `?N`) to a `u64`.
pub fn resolve_param_u64<F>(
    param: &str,
    span: Option<Span>,
    lookup: &F,
    positional: &[Value],
    clause: &str,
    positive: bool,
) -> Result<u64, QqlError>
where
    F: Fn(&str) -> Option<Value>,
{
    let val = if let Some(name) = param.strip_prefix(':') {
        resolve_param(name, span, lookup)?
    } else if let Some(idx_str) = param.strip_prefix('?') {
        let idx: usize = idx_str.parse().map_err(|_| {
            QqlError::validation(
                "QQL-BIND-TYPE-MISMATCH",
                format!("invalid positional parameter reference in {clause}: {param}"),
                span,
            )
        })?;
        resolve_positional(idx, span, positional)?
    } else {
        resolve_param(param, span, lookup)?
    };
    if positive {
        value_to_positive_u64(&val, clause, span)
    } else {
        value_to_u64(&val, clause, span)
    }
}
