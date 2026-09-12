//! Formatting for formula expressions.

use crate::ast::{FormulaExpr, escape_string, is_simple_ident};
use crate::fmt::expr::{render_f64, render_name, render_value};
use crate::fmt::filter::render_filter;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt::Write;

pub fn render_formula(formula: &FormulaExpr) -> String {
    render_formula_min(formula, 0)
}

fn formula_precedence(formula: &FormulaExpr) -> u8 {
    match formula {
        FormulaExpr::Sum { .. } | FormulaExpr::Sub { .. } => 1,
        FormulaExpr::Mul { .. } | FormulaExpr::Div { .. } => 2,
        FormulaExpr::Neg { .. } => 3,
        _ => 4,
    }
}

fn render_formula_min(formula: &FormulaExpr, min_precedence: u8) -> String {
    let precedence = formula_precedence(formula);
    let rendered = match formula {
        FormulaExpr::Constant { value } => render_f64(*value),
        FormulaExpr::Variable { name } => {
            if name.starts_with('?') {
                "?".to_string()
            } else if name.starts_with(':')
                || is_simple_ident(name)
                || (name.starts_with('$') && name.len() > 1 && is_simple_ident(&name[1..]))
            {
                name.clone()
            } else {
                format!("'{}'", escape_string(name))
            }
        }
        FormulaExpr::Sum { left, right } => format!(
            "{} + {}",
            render_formula_min(left, 1),
            render_formula_min(right, 2)
        ),
        FormulaExpr::Sub { left, right } => format!(
            "{} - {}",
            render_formula_min(left, 1),
            render_formula_min(right, 2)
        ),
        FormulaExpr::Mul { left, right } => format!(
            "{} * {}",
            render_formula_min(left, 2),
            render_formula_min(right, 3)
        ),
        FormulaExpr::Div {
            left,
            right,
            by_zero_default,
        } => {
            let mut out = format!(
                "{} / {}",
                render_formula_min(left, 2),
                render_formula_min(right, 3)
            );
            if let Some(default) = by_zero_default {
                let _ = write!(out, " [DEFAULT = {}]", render_f64(*default));
            }
            out
        }
        FormulaExpr::Neg { operand } => format!("-{}", render_formula_min(operand, 3)),
        FormulaExpr::Abs { x } => format!("ABS({})", render_formula_min(x, 0)),
        FormulaExpr::Sqrt { x } => format!("SQRT({})", render_formula_min(x, 0)),
        FormulaExpr::Log { x } => format!("LOG({})", render_formula_min(x, 0)),
        FormulaExpr::Ln { x } => format!("LN({})", render_formula_min(x, 0)),
        FormulaExpr::Exp { x } => format!("EXP({})", render_formula_min(x, 0)),
        FormulaExpr::Acosh { x } => format!("ACOSH({})", render_formula_min(x, 0)),
        FormulaExpr::Max { args } => render_formula_call("MAX", args),
        FormulaExpr::Min { args } => render_formula_call("MIN", args),
        FormulaExpr::Pow { base, exponent } => format!(
            "POW({}, {})",
            render_formula_min(base, 0),
            render_formula_min(exponent, 0)
        ),
        FormulaExpr::GeoDistance { lat, lon, field } => format!(
            "GEO_DISTANCE({}, {}, {})",
            render_f64(*lat),
            render_f64(*lon),
            render_name(field)
        ),
        FormulaExpr::Decay {
            kind,
            x,
            target,
            scale,
            midpoint,
        } => {
            let mut out = format!("{}(", kind.to_ascii_uppercase());
            out.push_str(&render_formula_min(x, 0));
            if let Some(target) = target {
                let _ = write!(out, ", TARGET = {}", render_formula_min(target, 0));
            }
            if let Some(scale) = scale {
                let _ = write!(out, ", SCALE = {}", render_f64(*scale));
            }
            if let Some(midpoint) = midpoint {
                let _ = write!(out, ", MIDPOINT = {}", render_f64(*midpoint));
            }
            out.push(')');
            out
        }
        FormulaExpr::Case { cond, then_, else_ } => format!(
            "CASE WHEN {} THEN {} ELSE {} END",
            render_filter(cond),
            render_formula_min(then_, 0),
            render_formula_min(else_, 0)
        ),
        FormulaExpr::MatchCondition { field, values } => {
            if values.len() == 1 {
                format!(
                    "MATCH({}, {})",
                    render_name(field),
                    render_value(&values[0])
                )
            } else {
                format!(
                    "MATCH({}, [{}])",
                    render_name(field),
                    values
                        .iter()
                        .map(render_value)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        }
        FormulaExpr::Datetime { value } => format!("DATETIME('{}')", escape_string(value)),
        FormulaExpr::DatetimeKey { key } => format!("DATETIME_KEY('{}')", escape_string(key)),
    };
    if precedence < min_precedence {
        format!("({})", rendered)
    } else {
        rendered
    }
}

fn render_formula_call(name: &str, args: &[FormulaExpr]) -> String {
    let rendered: Vec<String> = args.iter().map(|a| render_formula_min(a, 0)).collect();
    format!("{}({})", name, rendered.join(", "))
}
