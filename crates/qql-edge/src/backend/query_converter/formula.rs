//! Formula lowering: plan `PlanFormula` trees → qdrant-edge expressions.
//!
//! Split out of `query_converter` so each file stays near the repo's size
//! target. Module path and callers are unchanged (re-exported/imported by the
//! parent).
//!
//! `DEFAULTS` are deliberately converted to `serde_json::Value`: the formula
//! API consumes JSON bindings, so JSON is the boundary domain here, not an
//! internal IR.

use qdrant_edge::external::ordered_float::OrderedFloat;
use qdrant_edge::{DecayKind, GeoPoint};

use qql_core::error::QqlError;
use qql_plan::types::{FormulaDefault, PlanDecayKind, PlanFormula};

use super::parse_json_path;

/// Lower a typed `PlanFormula` into the edge API expression tree.
pub(super) fn plan_formula_to_edge(
    expr: &PlanFormula,
) -> Result<qdrant_edge::Expression, QqlError> {
    use qdrant_edge::Expression;
    Ok(match expr {
        PlanFormula::Constant(value) => Expression::Constant(*value as f32),
        PlanFormula::Variable(name) => Expression::Variable(name.clone()),
        PlanFormula::Sum { left, right } => Expression::Sum(vec![
            plan_formula_to_edge(left)?,
            plan_formula_to_edge(right)?,
        ]),
        PlanFormula::Sub { left, right } => Expression::Sum(vec![
            plan_formula_to_edge(left)?,
            Expression::Neg(Box::new(plan_formula_to_edge(right)?)),
        ]),
        PlanFormula::Mul { left, right } => Expression::Mult(vec![
            plan_formula_to_edge(left)?,
            plan_formula_to_edge(right)?,
        ]),
        PlanFormula::Div {
            left,
            right,
            by_zero_default,
        } => Expression::Div {
            left: Box::new(plan_formula_to_edge(left)?),
            right: Box::new(plan_formula_to_edge(right)?),
            by_zero_default: by_zero_default.map(|value| value as f32),
        },
        PlanFormula::Neg(operand) => Expression::Neg(Box::new(plan_formula_to_edge(operand)?)),
        PlanFormula::Abs(x) => Expression::Abs(Box::new(plan_formula_to_edge(x)?)),
        PlanFormula::Sqrt(x) => Expression::Sqrt(Box::new(plan_formula_to_edge(x)?)),
        PlanFormula::Log10(x) => Expression::Log10(Box::new(plan_formula_to_edge(x)?)),
        PlanFormula::Ln(x) => Expression::Ln(Box::new(plan_formula_to_edge(x)?)),
        PlanFormula::Exp(x) => Expression::Exp(Box::new(plan_formula_to_edge(x)?)),
        // MAX / MIN / ACOSH are parseable QQL but the pinned qdrant-edge has
        // no matching Expression variants — fail with the catalog error.
        PlanFormula::Acosh(_) | PlanFormula::Max(_) | PlanFormula::Min(_) => {
            return Err(crate::backend::unsupported::EdgeUnsupported::FormulaNary.error());
        }
        PlanFormula::Pow { base, exponent } => Expression::Pow {
            base: Box::new(plan_formula_to_edge(base)?),
            exponent: Box::new(plan_formula_to_edge(exponent)?),
        },
        PlanFormula::GeoDistance { lat, lon, field } => Expression::GeoDistance {
            origin: GeoPoint {
                lat: OrderedFloat(*lat),
                lon: OrderedFloat(*lon),
            },
            to: parse_json_path(field)?,
        },
        PlanFormula::Decay {
            kind,
            x,
            target,
            scale,
            midpoint,
        } => Expression::Decay {
            kind: match kind {
                PlanDecayKind::Lin => DecayKind::Lin,
                PlanDecayKind::Exp => DecayKind::Exp,
                PlanDecayKind::Gauss => DecayKind::Gauss,
            },
            x: Box::new(plan_formula_to_edge(x)?),
            target: target
                .as_ref()
                .map(|target| plan_formula_to_edge(target).map(Box::new))
                .transpose()?,
            midpoint: midpoint.map(|value| value as f32),
            scale: scale.map(|value| value as f32),
        },
        PlanFormula::Condition(filter) => Expression::Condition(Box::new(
            crate::backend::filter_converter::convert_formula_condition(filter)?,
        )),
        PlanFormula::Case { cond, then_, else_ } => {
            // Same weighting as the REST lowering: cond * then + (1 - cond) * else.
            let condition = plan_formula_to_edge(cond)?;
            let one_minus = Expression::Sum(vec![
                Expression::Constant(1.0),
                Expression::Neg(Box::new(condition.clone())),
            ]);
            Expression::Sum(vec![
                Expression::Mult(vec![condition, plan_formula_to_edge(then_)?]),
                Expression::Mult(vec![one_minus, plan_formula_to_edge(else_)?]),
            ])
        }
        PlanFormula::Datetime(value) => Expression::Datetime(value.clone()),
        PlanFormula::DatetimeKey(key) => Expression::DatetimeKey(parse_json_path(key)?),
    })
}

/// Convert a typed `DEFAULTS` binding to the edge API's JSON value domain.
pub(super) fn formula_default_to_json(value: &FormulaDefault) -> serde_json::Value {
    match value {
        FormulaDefault::Null => serde_json::Value::Null,
        FormulaDefault::Bool(value) => serde_json::Value::Bool(*value),
        FormulaDefault::Int(value) => serde_json::Value::Number((*value).into()),
        FormulaDefault::Float(value) => serde_json::Number::from_f64(*value)
            .map_or(serde_json::Value::Null, serde_json::Value::Number),
        FormulaDefault::String(value) => serde_json::Value::String(value.clone()),
        FormulaDefault::List(values) => {
            serde_json::Value::Array(values.iter().map(formula_default_to_json).collect())
        }
        FormulaDefault::Object(entries) => serde_json::Value::Object(
            entries
                .iter()
                .map(|(key, value)| (key.clone(), formula_default_to_json(value)))
                .collect(),
        ),
    }
}
