//! Query input parameter binding and resolution.

use super::value::{bind_point_id, resolve_param, resolve_positional};
use crate::ast::Value;
use crate::ast::statement::{ContextPair, FeedbackItem, PointId, QueryInput};
use crate::error::{QqlError, Span};
use alloc::format;

/// Recursively bind parameters into a `QueryInput` in-place.
pub fn bind_query_input<F>(
    input: &mut QueryInput,
    lookup: &F,
    positional: &[Value],
) -> Result<(), QqlError>
where
    F: Fn(&str) -> Option<Value>,
{
    match input {
        QueryInput::Param(name, span) => {
            let sp = span.as_deref().copied();
            let val = resolve_param(name, sp, lookup)?;
            *input = value_to_query_input(val, sp)?;
        }
        QueryInput::PositionalParam(idx, span) => {
            let sp = span.as_deref().copied();
            let val = resolve_positional(*idx, sp, positional)?;
            *input = value_to_query_input(val, sp)?;
        }
        QueryInput::Point(point) => {
            bind_point_id(point, lookup, positional)?;
        }
        QueryInput::Text {
            text, text_param, ..
        } => {
            if let Some(param) = text_param.take() {
                if let Some(param_name) = param.strip_prefix(':') {
                    let val = resolve_param(param_name, None, lookup)?;
                    if let Value::Str(s) = val {
                        *text = s;
                    } else {
                        return Err(QqlError::validation(
                            "QQL-BIND-TYPE-MISMATCH",
                            format!("parameter ':{param_name}' for TEXT query must be a string"),
                            None,
                        ));
                    }
                } else if let Some(idx_str) = param.strip_prefix('?') {
                    let idx = idx_str.parse::<usize>().map_err(|_| {
                        QqlError::validation(
                            "QQL-BIND-INVALID-PARAMS",
                            format!("invalid positional parameter index '?{idx_str}'"),
                            None,
                        )
                    })?;
                    let val = resolve_positional(idx, None, positional)?;
                    if let Value::Str(s) = val {
                        *text = s;
                    } else {
                        return Err(QqlError::validation(
                            "QQL-BIND-TYPE-MISMATCH",
                            format!("positional parameter ?{idx} for TEXT query must be a string"),
                            None,
                        ));
                    }
                } else {
                    let val = resolve_param(&param, None, lookup)?;
                    if let Value::Str(s) = val {
                        *text = s;
                    } else {
                        return Err(QqlError::validation(
                            "QQL-BIND-TYPE-MISMATCH",
                            format!("parameter ':{param}' for TEXT query must be a string"),
                            None,
                        ));
                    }
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// Convert a bound `Value` into a `QueryInput`, failing closed on type mismatch.
pub fn value_to_query_input(val: Value, span: Option<Span>) -> Result<QueryInput, QqlError> {
    match val {
        Value::Str(s) => Ok(QueryInput::Text {
            text: s,
            model: None,
            text_param: None,
        }),
        Value::Int(n) if n >= 0 => Ok(QueryInput::Point(PointId::Number(n as u64))),
        Value::List(_) | Value::Dict(_) => {
            let vec = crate::parser::helpers::vector_from_value(val, span)?;
            Ok(QueryInput::Vector(vec))
        }
        _ => Err(QqlError::validation(
            "QQL-BIND-TYPE-MISMATCH",
            format!("unsupported value type for query input: {:?}", val),
            span,
        )),
    }
}

/// Bind parameters into a `ContextPair` in-place.
pub fn bind_context_pair<F>(
    pair: &mut ContextPair,
    lookup: &F,
    positional: &[Value],
) -> Result<(), QqlError>
where
    F: Fn(&str) -> Option<Value>,
{
    bind_query_input(&mut pair.positive, lookup, positional)?;
    bind_query_input(&mut pair.negative, lookup, positional)?;
    Ok(())
}

/// Bind parameters into a `FeedbackItem` in-place.
pub fn bind_feedback_item<F>(
    item: &mut FeedbackItem,
    lookup: &F,
    positional: &[Value],
) -> Result<(), QqlError>
where
    F: Fn(&str) -> Option<Value>,
{
    bind_query_input(&mut item.example, lookup, positional)
}
