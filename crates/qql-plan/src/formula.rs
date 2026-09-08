//! Formula expression lowering to the OpenAPI `Expression` JSON shape.

use crate::filter::lower_filter;
use crate::routing::serialize_body;
use crate::types::*;
use qql_core::error::QqlError;

/// Lower a formula expression tree to the OpenAPI `Expression` JSON shape.
///
/// Fallible: `CASE` conditions serialize through the shared plan body helper,
/// so a `Serialize` regression surfaces as `QQL-PLAN-SERIALIZE` instead of
/// panicking the host process.
pub fn lower_formula_expr(
    expr: &qql_core::ast::FormulaExpr,
) -> Result<serde_json::Value, QqlError> {
    Ok(match expr {
        qql_core::ast::FormulaExpr::Constant { value } => serde_json::json!(value),
        qql_core::ast::FormulaExpr::Variable { name } => {
            if name == "score" {
                serde_json::json!("$score")
            } else {
                serde_json::json!(name)
            }
        }
        qql_core::ast::FormulaExpr::Sum { left, right } => serde_json::json!({
            "sum": [lower_formula_expr(left)?, lower_formula_expr(right)?]
        }),
        qql_core::ast::FormulaExpr::Sub { left, right } => serde_json::json!({
            "sum": [lower_formula_expr(left)?, { "neg": lower_formula_expr(right)? }]
        }),
        qql_core::ast::FormulaExpr::Mul { left, right } => serde_json::json!({
            "mult": [lower_formula_expr(left)?, lower_formula_expr(right)?]
        }),
        qql_core::ast::FormulaExpr::Div {
            left,
            right,
            by_zero_default,
        } => {
            let mut div = serde_json::Map::new();
            div.insert("left".into(), lower_formula_expr(left)?);
            div.insert("right".into(), lower_formula_expr(right)?);
            if let Some(default) = by_zero_default {
                div.insert("by_zero_default".into(), serde_json::json!(default));
            }
            serde_json::json!({ "div": div })
        }
        qql_core::ast::FormulaExpr::Neg { operand } => serde_json::json!({
            "neg": lower_formula_expr(operand)?
        }),
        qql_core::ast::FormulaExpr::Abs { x } => serde_json::json!({
            "abs": lower_formula_expr(x)?
        }),
        qql_core::ast::FormulaExpr::Sqrt { x } => serde_json::json!({
            "sqrt": lower_formula_expr(x)?
        }),
        qql_core::ast::FormulaExpr::Log { x } => serde_json::json!({
            "log10": lower_formula_expr(x)?
        }),
        qql_core::ast::FormulaExpr::Ln { x } => serde_json::json!({
            "ln": lower_formula_expr(x)?
        }),
        qql_core::ast::FormulaExpr::Exp { x } => serde_json::json!({
            "exp": lower_formula_expr(x)?
        }),
        qql_core::ast::FormulaExpr::Acosh { x } => serde_json::json!({
            "acosh": lower_formula_expr(x)?
        }),
        qql_core::ast::FormulaExpr::Max { args } => {
            let terms: Vec<_> = args
                .iter()
                .map(lower_formula_expr)
                .collect::<Result<Vec<_>, _>>()?;
            serde_json::json!({ "max": terms })
        }
        qql_core::ast::FormulaExpr::Min { args } => {
            let terms: Vec<_> = args
                .iter()
                .map(lower_formula_expr)
                .collect::<Result<Vec<_>, _>>()?;
            serde_json::json!({ "min": terms })
        }
        qql_core::ast::FormulaExpr::Pow { base, exponent } => serde_json::json!({
            "pow": {
                "base": lower_formula_expr(base)?,
                "exponent": lower_formula_expr(exponent)?
            }
        }),
        qql_core::ast::FormulaExpr::GeoDistance { lat, lon, field } => serde_json::json!({
            "geo_distance": {
                "origin": { "lat": lat, "lon": lon },
                "to": field
            }
        }),
        qql_core::ast::FormulaExpr::Decay {
            kind,
            x,
            target,
            scale,
            midpoint,
        } => {
            let mut params = serde_json::Map::new();
            let is_target_datetime = target
                .as_ref()
                .is_some_and(|t| matches!(&**t, qql_core::ast::FormulaExpr::Datetime { .. }));
            let x_val = match (&**x, is_target_datetime) {
                (qql_core::ast::FormulaExpr::Variable { name }, true) => {
                    serde_json::json!({ "datetime_key": name })
                }
                _ => lower_formula_expr(x)?,
            };
            params.insert("x".into(), x_val);
            if let Some(t) = target {
                params.insert("target".into(), lower_formula_expr(t)?);
            }
            if let Some(s) = scale {
                params.insert("scale".into(), serde_json::json!(s));
            }
            if let Some(m) = midpoint {
                params.insert("midpoint".into(), serde_json::json!(m));
            }
            let key = match kind.to_ascii_lowercase().as_str() {
                "lin" | "lin_decay" => "lin_decay",
                "exp" | "exp_decay" => "exp_decay",
                _ => "gauss_decay",
            };
            serde_json::json!({ key: params })
        }
        qql_core::ast::FormulaExpr::Case { cond, then_, else_ } => {
            let cond_val = match lower_filter(cond) {
                FilterExpression::Single(clause) => serialize_body(&*clause).map_err(|e| {
                    QqlError::execution(
                        "QQL-PLAN-SERIALIZE",
                        alloc::format!("formula CASE condition clause serialization failed: {e}"),
                        None,
                    )
                })?,
                FilterExpression::Compound(comp) => serialize_body(&comp).map_err(|e| {
                    QqlError::execution(
                        "QQL-PLAN-SERIALIZE",
                        alloc::format!("formula CASE condition compound serialization failed: {e}"),
                        None,
                    )
                })?,
            };
            // OpenAPI Expression accepts a Condition as a boolean 0/1 term.
            // Encode CASE as: condition * then + (1 - condition) * else.
            serde_json::json!({
                "sum": [
                    {
                        "mult": [cond_val.clone(), lower_formula_expr(then_)?]
                    },
                    {
                        "mult": [
                            {
                                "sum": [
                                    1.0,
                                    { "neg": cond_val }
                                ]
                            },
                            lower_formula_expr(else_)?
                        ]
                    }
                ]
            })
        }
        qql_core::ast::FormulaExpr::MatchCondition { field, values } => {
            // Condition expressions evaluate to 1.0 / 0.0 on the formula wire format.
            use crate::filter::value_to_json;
            if values.len() == 1 {
                let val = value_to_json(&values[0]);
                serde_json::json!({
                    "key": field,
                    "match": { "value": val }
                })
            } else {
                let any: Vec<_> = values.iter().map(value_to_json).collect();
                serde_json::json!({
                    "key": field,
                    "match": { "any": any }
                })
            }
        }
        qql_core::ast::FormulaExpr::Datetime { value } => serde_json::json!({
            "datetime": value
        }),
        qql_core::ast::FormulaExpr::DatetimeKey { key } => serde_json::json!({
            "datetime_key": key
        }),
    })
}
