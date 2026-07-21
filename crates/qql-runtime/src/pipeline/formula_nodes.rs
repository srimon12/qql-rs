use qql_plan::filter::lower_filter;
use async_trait::async_trait;
use qql_core::ast::{self, ComparisonOp};
use qql_core::error::QqlError;
use std::collections::HashMap;

use super::ExecutionNode;
use super::QueryState;

pub struct FormulaNode {
    pub expr: serde_json::Value,
    pub defaults: Vec<(String, f64)>,
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl ExecutionNode for FormulaNode {
    async fn execute(&self, state: &mut QueryState) -> Result<(), QqlError> {
        let mut defs: HashMap<String, f64> = HashMap::new();
        for (k, v) in &self.defaults {
            defs.insert(k.clone(), *v);
        }
        state.formula = Some(self.expr.clone());
        state.formula_defaults = defs;
        Ok(())
    }
}

pub fn build_expression(expr: &ast::FormulaExpr) -> Result<serde_json::Value, QqlError> {
    match expr {
        ast::FormulaExpr::Constant { value } => Ok(serde_json::json!(value)),
        ast::FormulaExpr::Variable { name } => {
            let var_name = if *name == "score" || name.starts_with('$') {
                format!("${}", name.trim_start_matches('$'))
            } else {
                name.to_string()
            };
            Ok(serde_json::json!(var_name))
        }
        ast::FormulaExpr::Datetime { value } => Ok(serde_json::json!({"datetime": value})),
        ast::FormulaExpr::DatetimeKey { key } => Ok(serde_json::json!({"datetime_key": key})),
        ast::FormulaExpr::Sum { left, right } => {
            let l = build_expression(left)?;
            let r = build_expression(right)?;
            Ok(serde_json::json!({"sum": [l, r]}))
        }
        ast::FormulaExpr::Sub { left, right } => {
            let l = build_expression(left)?;
            let r = build_expression(right)?;
            let neg_r = serde_json::json!({"neg": r});
            Ok(serde_json::json!({"sum": [l, neg_r]}))
        }
        ast::FormulaExpr::Mul { left, right } => {
            let l = build_expression(left)?;
            let r = build_expression(right)?;
            Ok(serde_json::json!({"mult": [l, r]}))
        }
        ast::FormulaExpr::Div {
            left,
            right,
            by_zero_default,
        } => {
            let l = build_expression(left)?;
            let r = build_expression(right)?;
            let mut div = serde_json::json!({"left": l, "right": r});
            if let Some(default) = by_zero_default {
                div.as_object_mut()
                    .unwrap()
                    .insert("by_zero_default".to_string(), serde_json::json!(default));
            }
            Ok(serde_json::json!({"div": div}))
        }
        ast::FormulaExpr::Neg { operand } => {
            let op = build_expression(operand)?;
            Ok(serde_json::json!({"neg": op}))
        }
        ast::FormulaExpr::Abs { x } => {
            let inner = build_expression(x)?;
            Ok(serde_json::json!({"abs": inner}))
        }
        ast::FormulaExpr::Sqrt { x } => {
            let inner = build_expression(x)?;
            Ok(serde_json::json!({"sqrt": inner}))
        }
        ast::FormulaExpr::Log { x } => {
            let inner = build_expression(x)?;
            Ok(serde_json::json!({"log10": inner}))
        }
        ast::FormulaExpr::Ln { x } => {
            let inner = build_expression(x)?;
            Ok(serde_json::json!({"ln": inner}))
        }
        ast::FormulaExpr::Exp { x } => {
            let inner = build_expression(x)?;
            Ok(serde_json::json!({"exp": inner}))
        }
        ast::FormulaExpr::Pow { base, exponent } => {
            let b = build_expression(base)?;
            let e = build_expression(exponent)?;
            Ok(serde_json::json!({"pow": {"base": b, "exponent": e}}))
        }
        ast::FormulaExpr::GeoDistance { lat, lon, field } => Ok(
            serde_json::json!({"geo_distance": {"origin": {"lat": lat, "lon": lon}, "to": field}}),
        ),
        ast::FormulaExpr::Decay {
            kind,
            x,
            target,
            scale,
            midpoint,
        } => {
            let inner_x = build_expression(x)?;
            let mut decay = serde_json::json!({"x": inner_x});
            if let Some(t) = target {
                let target_expr = build_expression(t)?;
                decay
                    .as_object_mut()
                    .unwrap()
                    .insert("target".to_string(), target_expr);
            }
            if let Some(s) = scale {
                decay
                    .as_object_mut()
                    .unwrap()
                    .insert("scale".to_string(), serde_json::json!(s));
            }
            if let Some(m) = midpoint {
                decay
                    .as_object_mut()
                    .unwrap()
                    .insert("midpoint".to_string(), serde_json::json!(m));
            }
            let decay_key = match kind.as_str() {
                "exp_decay" => "exp_decay",
                "gauss_decay" => "gauss_decay",
                "lin_decay" => "lin_decay",
                _ => return Err(QqlError::execution("QQL-EXECUTION", format!("unknown decay kind: {}", kind), None)),
            };
            Ok(serde_json::json!({decay_key: decay}))
        }
        ast::FormulaExpr::Case { cond, then_, else_ } => {
            let cond_expr = match cond.as_ref() {
                ast::FilterExpr::Compare { field, op: ComparisonOp::Eq, value } => {
                    build_match_condition_expression(field, std::slice::from_ref(value))?
                }
                _ => {
                    let qdrant_filter = lower_filter(cond);
                    let cond_json = serde_json::to_value(&qdrant_filter).map_err(|e| {
                        QqlError::execution("QQL-EXECUTION", format!("failed to serialize filter: {}", e), None)
                    })?;
                    serde_json::json!({"filter": cond_json})
                }
            };
            let not_cond_expr = serde_json::json!({
                "sum": [1.0, {"neg": cond_expr.clone()}]
            });
            let then_expr = build_expression(then_)?;
            let else_expr = build_expression(else_)?;
            let then_part = serde_json::json!({"mult": [cond_expr, then_expr]});
            let else_part = serde_json::json!({"mult": [not_cond_expr, else_expr]});
            Ok(serde_json::json!({"sum": [then_part, else_part]}))
        }
        ast::FormulaExpr::MatchCondition { field, values } => {
            build_match_condition_expression(field, values)
        }
    }
}

pub fn build_match_condition_expression(
    field: &str,
    values: &[ast::Value],
) -> Result<serde_json::Value, QqlError> {
    if values.is_empty() {
        return Err(QqlError::execution("QQL-EXECUTION", "MATCH requires at least one value", None));
    }

    if values.len() == 1 {
        let condition = match &values[0] {
            ast::Value::Str(s) => {
                serde_json::json!({"key": field, "match": {"value": s}})
            }
            ast::Value::Int(i) => {
                serde_json::json!({"key": field, "match": {"value": i}})
            }
            ast::Value::Float(f) => {
                serde_json::json!({"key": field, "range": {"gte": f, "lte": f}})
            }
            _ => {
                return Err(QqlError::execution("QQL-EXECUTION", "MATCH value must be a string or number", None));
            }
        };
        Ok(condition)
    } else {
        let first = &values[0];
        match first {
            ast::Value::Str(_) => {
                let mut keywords = Vec::new();
                for v in values {
                    match v {
                        ast::Value::Str(s) => keywords.push(s.as_str()),
                        _ => return Err(QqlError::execution("QQL-EXECUTION", "all MATCH values must be strings", None)),
                    }
                }
                let condition = serde_json::json!({
                    "match": {"key": field, "values": keywords.iter().map(|s| serde_json::json!({"str": s})).collect::<Vec<_>>()}
                });
                Ok(serde_json::json!({"condition": condition}))
            }
            ast::Value::Int(_) | ast::Value::Float(_) => {
                let mut ints = Vec::new();
                for v in values {
                    match v {
                        ast::Value::Int(i) => ints.push(*i),
                        ast::Value::Float(f) => ints.push(*f as i64),
                        _ => return Err(QqlError::execution("QQL-EXECUTION", "all MATCH values must be numbers", None)),
                    }
                }
                let condition = serde_json::json!({
                    "match": {"key": field, "values": ints.iter().map(|i| serde_json::json!({"int": *i})).collect::<Vec<_>>()}
                });
                Ok(serde_json::json!({"condition": condition}))
            }
            _ => Err(QqlError::execution("QQL-EXECUTION", "MATCH values must be strings or numbers", None)),
        }
    }
}
