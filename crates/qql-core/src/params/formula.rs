//! Formula parameter binding and constant conversion.

use super::value::{bind_value, resolve_param, resolve_positional};
use crate::ast::Value;
use crate::ast::filter::FilterExpr;
use crate::ast::formula::FormulaExpr;
use crate::ast::looks_like_iso_datetime;
use crate::error::{QqlError, Span};
use alloc::format;

/// Recursively bind parameters into a `FormulaExpr` in-place.
pub fn bind_formula<F>(
    formula: &mut FormulaExpr,
    lookup: &F,
    positional: &[Value],
    bind_filter_fn: &impl Fn(&mut FilterExpr, &F, &[Value]) -> Result<(), QqlError>,
) -> Result<(), QqlError>
where
    F: Fn(&str) -> Option<Value>,
{
    match formula {
        FormulaExpr::Variable { name } => {
            if let Some(param_name) = name.strip_prefix(':') {
                let val = resolve_param(param_name, None, lookup)?;
                *formula = value_to_formula_constant(val, None)?;
            } else if let Some(idx_str) = name.strip_prefix('?')
                && let Ok(idx) = idx_str.parse::<usize>()
            {
                let val = resolve_positional(idx, None, positional)?;
                *formula = value_to_formula_constant(val, None)?;
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
            bind_formula(left, lookup, positional, bind_filter_fn)?;
            bind_formula(right, lookup, positional, bind_filter_fn)?;
        }
        FormulaExpr::Neg { operand }
        | FormulaExpr::Abs { x: operand }
        | FormulaExpr::Sqrt { x: operand }
        | FormulaExpr::Log { x: operand }
        | FormulaExpr::Ln { x: operand }
        | FormulaExpr::Exp { x: operand }
        | FormulaExpr::Acosh { x: operand } => {
            bind_formula(operand, lookup, positional, bind_filter_fn)?;
        }
        FormulaExpr::Max { args } | FormulaExpr::Min { args } => {
            for arg in args {
                bind_formula(arg, lookup, positional, bind_filter_fn)?;
            }
        }
        FormulaExpr::Decay { x, target, .. } => {
            bind_formula(x, lookup, positional, bind_filter_fn)?;
            if let Some(t) = target {
                bind_formula(t, lookup, positional, bind_filter_fn)?;
            }
        }
        FormulaExpr::Case { cond, then_, else_ } => {
            bind_filter_fn(cond, lookup, positional)?;
            bind_formula(then_, lookup, positional, bind_filter_fn)?;
            bind_formula(else_, lookup, positional, bind_filter_fn)?;
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

/// Convert a bound `Value` into a `FormulaExpr` constant, datetime, or variable.
pub fn value_to_formula_constant(val: Value, span: Option<Span>) -> Result<FormulaExpr, QqlError> {
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
            "QQL-BIND-TYPE-MISMATCH",
            format!("formula parameter cannot be bound to value: {:?}", val),
            span,
        )),
    }
}
