//! `PlanFormula` → proto [`qdrant::Expression`] converters.
//!
//! Direct and total: every plan variant maps to its typed proto field — no
//! JSON intermediate anywhere.

use qql_core::error::QqlError;
use qql_plan::types::{FilterExpression, FormulaDefault, PlanDecayKind, PlanFormula};

use crate::qdrant_grpc::qdrant;

use super::filter::{compound_to_filter, to_condition};

/// Convert a plan-owned formula tree into its proto expression.
pub(crate) fn plan_formula_to_grpc(expr: &PlanFormula) -> Result<qdrant::Expression, QqlError> {
    use qdrant::expression::Variant;
    let variant = match expr {
        PlanFormula::Constant(value) => Variant::Constant(*value as f32),
        PlanFormula::Variable(name) => Variant::Variable(name.clone()),
        PlanFormula::Sum { left, right } => Variant::Sum(qdrant::SumExpression {
            sum: vec![plan_formula_to_grpc(left)?, plan_formula_to_grpc(right)?],
        }),
        PlanFormula::Sub { left, right } => Variant::Sum(qdrant::SumExpression {
            sum: vec![
                plan_formula_to_grpc(left)?,
                qdrant::Expression {
                    variant: Some(Variant::Neg(Box::new(plan_formula_to_grpc(right)?))),
                },
            ],
        }),
        PlanFormula::Mul { left, right } => Variant::Mult(qdrant::MultExpression {
            mult: vec![plan_formula_to_grpc(left)?, plan_formula_to_grpc(right)?],
        }),
        PlanFormula::Div {
            left,
            right,
            by_zero_default,
        } => Variant::Div(Box::new(qdrant::DivExpression {
            left: Some(Box::new(plan_formula_to_grpc(left)?)),
            right: Some(Box::new(plan_formula_to_grpc(right)?)),
            by_zero_default: by_zero_default.map(|value| value as f32),
        })),
        PlanFormula::Neg(operand) => Variant::Neg(Box::new(plan_formula_to_grpc(operand)?)),
        PlanFormula::Abs(x) => Variant::Abs(Box::new(plan_formula_to_grpc(x)?)),
        PlanFormula::Sqrt(x) => Variant::Sqrt(Box::new(plan_formula_to_grpc(x)?)),
        PlanFormula::Log10(x) => Variant::Log10(Box::new(plan_formula_to_grpc(x)?)),
        PlanFormula::Ln(x) => Variant::Ln(Box::new(plan_formula_to_grpc(x)?)),
        PlanFormula::Exp(x) => Variant::Exp(Box::new(plan_formula_to_grpc(x)?)),
        PlanFormula::Acosh(x) => Variant::Acosh(Box::new(plan_formula_to_grpc(x)?)),
        PlanFormula::Max(args) => Variant::Max(qdrant::MaxExpression {
            max: args
                .iter()
                .map(plan_formula_to_grpc)
                .collect::<Result<Vec<_>, _>>()?,
        }),
        PlanFormula::Min(args) => Variant::Min(qdrant::MinExpression {
            min: args
                .iter()
                .map(plan_formula_to_grpc)
                .collect::<Result<Vec<_>, _>>()?,
        }),
        PlanFormula::Pow { base, exponent } => Variant::Pow(Box::new(qdrant::PowExpression {
            base: Some(Box::new(plan_formula_to_grpc(base)?)),
            exponent: Some(Box::new(plan_formula_to_grpc(exponent)?)),
        })),
        PlanFormula::GeoDistance { lat, lon, field } => Variant::GeoDistance(qdrant::GeoDistance {
            origin: Some(qdrant::GeoPoint {
                lat: *lat,
                lon: *lon,
            }),
            to: field.clone(),
        }),
        PlanFormula::Decay {
            kind,
            x,
            target,
            scale,
            midpoint,
        } => {
            let decay = Box::new(qdrant::DecayParamsExpression {
                x: Some(Box::new(plan_formula_to_grpc(x)?)),
                target: target
                    .as_ref()
                    .map(|target| plan_formula_to_grpc(target).map(Box::new))
                    .transpose()?,
                scale: scale.map(|value| value as f32),
                midpoint: midpoint.map(|value| value as f32),
            });
            match kind {
                PlanDecayKind::Exp => Variant::ExpDecay(decay),
                PlanDecayKind::Lin => Variant::LinDecay(decay),
                PlanDecayKind::Gauss => Variant::GaussDecay(decay),
            }
        }
        PlanFormula::Condition(filter) => Variant::Condition(condition_to_grpc(filter)?),
        PlanFormula::Case { cond, then_, else_ } => {
            // Same weighting as the REST lowering: cond * then + (1 - cond) * else.
            let condition = plan_formula_to_grpc(cond)?;
            let one_minus = qdrant::Expression {
                variant: Some(Variant::Sum(qdrant::SumExpression {
                    sum: vec![
                        qdrant::Expression {
                            variant: Some(Variant::Constant(1.0)),
                        },
                        qdrant::Expression {
                            variant: Some(Variant::Neg(Box::new(condition.clone()))),
                        },
                    ],
                })),
            };
            Variant::Sum(qdrant::SumExpression {
                sum: vec![
                    qdrant::Expression {
                        variant: Some(Variant::Mult(qdrant::MultExpression {
                            mult: vec![condition, plan_formula_to_grpc(then_)?],
                        })),
                    },
                    qdrant::Expression {
                        variant: Some(Variant::Mult(qdrant::MultExpression {
                            mult: vec![one_minus, plan_formula_to_grpc(else_)?],
                        })),
                    },
                ],
            })
        }
        PlanFormula::Datetime(value) => Variant::Datetime(value.clone()),
        PlanFormula::DatetimeKey(key) => Variant::DatetimeKey(key.clone()),
    };
    Ok(qdrant::Expression {
        variant: Some(variant),
    })
}

/// Wrap a condition used as a `1.0` / `0.0` expression term.
fn condition_to_grpc(filter: &FilterExpression) -> Result<qdrant::Condition, QqlError> {
    match filter {
        FilterExpression::Single(clause) => to_condition(clause),
        FilterExpression::Compound(compound) => Ok(qdrant::Condition {
            condition_one_of: Some(qdrant::condition::ConditionOneOf::Filter(
                compound_to_filter(compound)?,
            )),
        }),
    }
}

/// Convert one `DEFAULTS` binding value into its proto value.
pub(crate) fn formula_default_to_grpc(value: &FormulaDefault) -> qdrant::Value {
    use qdrant::value::Kind;
    let kind = match value {
        FormulaDefault::Null => None,
        FormulaDefault::Bool(value) => Some(Kind::BoolValue(*value)),
        FormulaDefault::Int(value) => Some(Kind::IntegerValue(*value)),
        FormulaDefault::Float(value) => Some(Kind::DoubleValue(*value)),
        FormulaDefault::String(value) => Some(Kind::StringValue(value.clone())),
        FormulaDefault::List(values) => Some(Kind::ListValue(qdrant::ListValue {
            values: values.iter().map(formula_default_to_grpc).collect(),
        })),
        FormulaDefault::Object(entries) => Some(Kind::StructValue(qdrant::Struct {
            fields: entries
                .iter()
                .map(|(key, value)| (key.clone(), formula_default_to_grpc(value)))
                .collect(),
        })),
    };
    qdrant::Value { kind }
}
