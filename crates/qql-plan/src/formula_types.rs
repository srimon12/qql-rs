//! Formula IR types matching the OpenAPI `Expression` schema.
//!
//! [`PlanFormula`] is the plan-owned formula tree: its `Serialize` emits the
//! OpenAPI `Expression` JSON directly (no `serde_json::Value` intermediate),
//! and the gRPC / edge adapters convert it straight into their typed
//! expression messages. [`FormulaDefault`] carries one `DEFAULTS (name = …)`
//! binding.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};

use qql_core::ast::{FormulaExpr, Value};

use crate::filter::{lower_filter, value_to_json};
use crate::filter_types::{FieldCondition, FilterClause, FilterExpression, MatchValue};

/// Decay curve family of a [`PlanFormula::Decay`] node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanDecayKind {
    /// Exponential decay (`exp_decay`).
    Exp,
    /// Linear decay (`lin_decay`).
    Lin,
    /// Gaussian decay (`gauss_decay`).
    Gauss,
}

impl PlanDecayKind {
    /// OpenAPI `Expression` key carrying the decay parameters.
    pub fn wire_key(self) -> &'static str {
        match self {
            Self::Exp => "exp_decay",
            Self::Lin => "lin_decay",
            Self::Gauss => "gauss_decay",
        }
    }

    /// Map a parser/backend decay kind spelling to the typed family.
    ///
    /// Unknown spellings fall back to Gaussian, matching the historical
    /// lowering.
    fn from_wire(kind: &str) -> Self {
        match kind.to_ascii_lowercase().as_str() {
            "exp" | "exp_decay" => Self::Exp,
            "lin" | "lin_decay" => Self::Lin,
            _ => Self::Gauss,
        }
    }
}

/// One `DEFAULTS (name = …)` binding value.
///
/// The OpenAPI `FormulaQuery.defaults` object is schemaless
/// (`additionalProperties: true`): numeric variables take numbers, datetime
/// variables take ISO-8601 strings, and the backend coerces per variable.
/// This type carries exactly that value domain without a `serde_json::Value`
/// intermediary.
#[derive(Debug, Clone, PartialEq)]
pub enum FormulaDefault {
    /// JSON `null`.
    Null,
    /// Boolean value.
    Bool(bool),
    /// Integer value.
    Int(i64),
    /// Floating-point value.
    Float(f64),
    /// String value (e.g. an ISO-8601 datetime for a datetime variable).
    String(String),
    /// JSON array.
    List(Vec<FormulaDefault>),
    /// JSON object. Keys are sorted, matching the previous `serde_json::Map`
    /// serialization.
    Object(BTreeMap<String, FormulaDefault>),
}

impl Serialize for FormulaDefault {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Null => serializer.serialize_unit(),
            Self::Bool(value) => serializer.serialize_bool(*value),
            Self::Int(value) => serializer.serialize_i64(*value),
            Self::Float(value) => serializer.serialize_f64(*value),
            Self::String(value) => serializer.serialize_str(value),
            Self::List(values) => values.serialize(serializer),
            Self::Object(entries) => {
                let mut map = serializer.serialize_map(Some(entries.len()))?;
                for (key, value) in entries {
                    map.serialize_entry(key, value)?;
                }
                map.end()
            }
        }
    }
}

impl From<&Value> for FormulaDefault {
    /// Lower an AST value into the formula-default domain.
    ///
    /// # Invariant
    ///
    /// `Param` / `PositionalParam` panic: `plan()` rejects unbound parameters
    /// before lowering, mirroring [`crate::filter::value_to_json`].
    fn from(value: &Value) -> Self {
        match value {
            Value::Str(value) => Self::String(value.clone()),
            Value::Int(value) => Self::Int(*value),
            Value::Float(value) => Self::Float(*value),
            Value::Bool(value) => Self::Bool(*value),
            Value::Null => Self::Null,
            Value::Dict(entries) => Self::Object(
                entries
                    .iter()
                    .map(|(key, value)| (key.clone(), Self::from(value)))
                    .collect(),
            ),
            Value::List(items) => Self::List(items.iter().map(Self::from).collect()),
            Value::F32Array(values) => Self::List(
                values
                    .iter()
                    .map(|value| Self::Float(*value as f64))
                    .collect(),
            ),
            Value::Param(name, _) => {
                panic!(
                    "invariant violation: unbound parameter :{name} reached formula defaults lowering"
                );
            }
            Value::PositionalParam(idx, _) => {
                panic!(
                    "invariant violation: unbound positional parameter ?{idx} reached formula defaults lowering"
                );
            }
        }
    }
}

/// Transport-neutral formula expression tree (`QUERY FORMULA <expr>`).
///
/// `Serialize` emits the OpenAPI `Expression` wire shape directly.
#[derive(Debug, Clone)]
pub enum PlanFormula {
    /// Numeric literal.
    Constant(f64),
    /// Variable reference: `$score`, `$score[i]`, or a payload key.
    Variable(String),
    /// `left + right`.
    Sum {
        /// Left term.
        left: Box<PlanFormula>,
        /// Right term.
        right: Box<PlanFormula>,
    },
    /// `left - right` (wire: `sum` of `left` and `neg` `right`).
    Sub {
        /// Left term.
        left: Box<PlanFormula>,
        /// Right term.
        right: Box<PlanFormula>,
    },
    /// `left * right`.
    Mul {
        /// Left factor.
        left: Box<PlanFormula>,
        /// Right factor.
        right: Box<PlanFormula>,
    },
    /// `left / right`, with the optional zero-division fallback.
    Div {
        /// Dividend.
        left: Box<PlanFormula>,
        /// Divisor.
        right: Box<PlanFormula>,
        /// Value substituted when the divisor is zero.
        by_zero_default: Option<f64>,
    },
    /// Unary negation.
    Neg(Box<PlanFormula>),
    /// `ABS(x)`.
    Abs(Box<PlanFormula>),
    /// `SQRT(x)`.
    Sqrt(Box<PlanFormula>),
    /// Base-10 logarithm.
    Log10(Box<PlanFormula>),
    /// Natural logarithm.
    Ln(Box<PlanFormula>),
    /// `e^x`.
    Exp(Box<PlanFormula>),
    /// Inverse hyperbolic cosine.
    Acosh(Box<PlanFormula>),
    /// N-ary maximum (at least one operand).
    Max(Vec<PlanFormula>),
    /// N-ary minimum (at least one operand).
    Min(Vec<PlanFormula>),
    /// `base` raised to `exponent`.
    Pow {
        /// Base expression.
        base: Box<PlanFormula>,
        /// Exponent expression.
        exponent: Box<PlanFormula>,
    },
    /// Geographic distance in meters from `(lat, lon)` to a geo payload `field`.
    GeoDistance {
        /// Query latitude in degrees.
        lat: f64,
        /// Query longitude in degrees.
        lon: f64,
        /// Payload geo field to measure against.
        field: String,
    },
    /// Decay curve over `x`, with optional target, scale, and midpoint.
    Decay {
        /// Curve family.
        kind: PlanDecayKind,
        /// Decaying expression.
        x: Box<PlanFormula>,
        /// Decay origin; absent means the backend default (zero).
        target: Option<Box<PlanFormula>>,
        /// Distance from `target` at which the output falls to `midpoint`.
        scale: Option<f64>,
        /// Output value at that distance from `target`.
        midpoint: Option<f64>,
    },
    /// Boolean condition evaluated as `1.0` / `0.0` (OpenAPI `Condition`).
    Condition(FilterExpression),
    /// `CASE WHEN cond THEN then_ ELSE else_ END`, lowered to
    /// `cond * then_ + (1 - cond) * else_`.
    Case {
        /// Boolean condition term (a [`PlanFormula::Condition`]).
        cond: Box<PlanFormula>,
        /// Value produced when the condition holds.
        then_: Box<PlanFormula>,
        /// Value produced otherwise.
        else_: Box<PlanFormula>,
    },
    /// ISO-8601 datetime constant.
    Datetime(String),
    /// Payload datetime field reference.
    DatetimeKey(String),
}

impl From<&FormulaExpr> for PlanFormula {
    fn from(expr: &FormulaExpr) -> Self {
        match expr {
            FormulaExpr::Constant { value } => Self::Constant(*value),
            FormulaExpr::Variable { name } => Self::Variable(variable_wire_name(name)),
            FormulaExpr::Sum { left, right } => Self::Sum {
                left: Box::new(Self::from(&**left)),
                right: Box::new(Self::from(&**right)),
            },
            FormulaExpr::Sub { left, right } => Self::Sub {
                left: Box::new(Self::from(&**left)),
                right: Box::new(Self::from(&**right)),
            },
            FormulaExpr::Mul { left, right } => Self::Mul {
                left: Box::new(Self::from(&**left)),
                right: Box::new(Self::from(&**right)),
            },
            FormulaExpr::Div {
                left,
                right,
                by_zero_default,
            } => Self::Div {
                left: Box::new(Self::from(&**left)),
                right: Box::new(Self::from(&**right)),
                by_zero_default: *by_zero_default,
            },
            FormulaExpr::Neg { operand } => Self::Neg(Box::new(Self::from(&**operand))),
            FormulaExpr::Abs { x } => Self::Abs(Box::new(Self::from(&**x))),
            FormulaExpr::Sqrt { x } => Self::Sqrt(Box::new(Self::from(&**x))),
            FormulaExpr::Log { x } => Self::Log10(Box::new(Self::from(&**x))),
            FormulaExpr::Ln { x } => Self::Ln(Box::new(Self::from(&**x))),
            FormulaExpr::Exp { x } => Self::Exp(Box::new(Self::from(&**x))),
            FormulaExpr::Acosh { x } => Self::Acosh(Box::new(Self::from(&**x))),
            FormulaExpr::Max { args } => Self::Max(args.iter().map(Self::from).collect()),
            FormulaExpr::Min { args } => Self::Min(args.iter().map(Self::from).collect()),
            FormulaExpr::Pow { base, exponent } => Self::Pow {
                base: Box::new(Self::from(&**base)),
                exponent: Box::new(Self::from(&**exponent)),
            },
            FormulaExpr::GeoDistance { lat, lon, field } => Self::GeoDistance {
                lat: *lat,
                lon: *lon,
                field: field.clone(),
            },
            FormulaExpr::Decay {
                kind,
                x,
                target,
                scale,
                midpoint,
            } => {
                let target = target.as_ref().map(|t| Box::new(Self::from(&**t)));
                // A bare field identifier decaying against a datetime target is
                // a datetime key, not a numeric variable.
                let x = match (&**x, matches!(target.as_deref(), Some(Self::Datetime(_)))) {
                    (FormulaExpr::Variable { name }, true) => Self::DatetimeKey(name.clone()),
                    _ => Self::from(&**x),
                };
                Self::Decay {
                    kind: PlanDecayKind::from_wire(kind),
                    x: Box::new(x),
                    target,
                    scale: *scale,
                    midpoint: *midpoint,
                }
            }
            FormulaExpr::Case { cond, then_, else_ } => Self::Case {
                cond: Box::new(Self::Condition(lower_filter(cond))),
                then_: Box::new(Self::from(&**then_)),
                else_: Box::new(Self::from(&**else_)),
            },
            FormulaExpr::MatchCondition { field, values } => {
                Self::Condition(match_condition_filter(field, values))
            }
            FormulaExpr::Datetime { value } => Self::Datetime(value.clone()),
            FormulaExpr::DatetimeKey { key } => Self::DatetimeKey(key.clone()),
        }
    }
}

/// Normalize the reserved score variable to its wire spelling.
fn variable_wire_name(name: &str) -> String {
    if name == "score" {
        String::from("$score")
    } else {
        String::from(name)
    }
}

/// Build the condition term for `MATCH(field, values)`.
fn match_condition_filter(field: &str, values: &[Value]) -> FilterExpression {
    let r#match = if values.len() == 1 {
        MatchValue::Value {
            value: value_to_json(&values[0]),
        }
    } else {
        MatchValue::Any {
            any: values.iter().map(value_to_json).collect(),
        }
    };
    FilterExpression::Single(Box::new(FilterClause::Field(Box::new(FieldCondition {
        key: field.into(),
        r#match: Some(r#match),
        ..Default::default()
    }))))
}

// ── Wire serialization ──────────────────────────────────────────
//
// Every struct below is a one-key OpenAPI object. Fields are declared in
// alphabetical order so direct `Serialize` output matches the key order of
// the previous `serde_json::Map` (BTreeMap-backed) lowering.

#[derive(Serialize)]
struct SumValue<T: Serialize> {
    sum: T,
}

#[derive(Serialize)]
struct MultValue<T: Serialize> {
    mult: T,
}

#[derive(Serialize)]
struct MaxValue<'a> {
    max: &'a [PlanFormula],
}

#[derive(Serialize)]
struct MinValue<'a> {
    min: &'a [PlanFormula],
}

#[derive(Serialize)]
struct NegValue<'a> {
    neg: &'a PlanFormula,
}

#[derive(Serialize)]
struct AbsValue<'a> {
    abs: &'a PlanFormula,
}

#[derive(Serialize)]
struct SqrtValue<'a> {
    sqrt: &'a PlanFormula,
}

#[derive(Serialize)]
struct Log10Value<'a> {
    log10: &'a PlanFormula,
}

#[derive(Serialize)]
struct LnValue<'a> {
    ln: &'a PlanFormula,
}

#[derive(Serialize)]
struct ExpValue<'a> {
    exp: &'a PlanFormula,
}

#[derive(Serialize)]
struct AcoshValue<'a> {
    acosh: &'a PlanFormula,
}

#[derive(Serialize)]
struct PowValue<'a> {
    pow: PowParams<'a>,
}

#[derive(Serialize)]
struct PowParams<'a> {
    base: &'a PlanFormula,
    exponent: &'a PlanFormula,
}

#[derive(Serialize)]
struct DivValue<'a> {
    div: DivParams<'a>,
}

#[derive(Serialize)]
struct DivParams<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    by_zero_default: Option<f64>,
    left: &'a PlanFormula,
    right: &'a PlanFormula,
}

#[derive(Serialize)]
struct GeoDistanceValue<'a> {
    geo_distance: GeoDistanceParams<'a>,
}

#[derive(Serialize)]
struct GeoDistanceParams<'a> {
    origin: GeoPointRef,
    to: &'a str,
}

#[derive(Serialize)]
struct GeoPointRef {
    lat: f64,
    lon: f64,
}

#[derive(Serialize)]
struct DecayParams<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    midpoint: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scale: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    target: Option<&'a PlanFormula>,
    x: &'a PlanFormula,
}

#[derive(Serialize)]
struct DatetimeValue<'a> {
    datetime: &'a str,
}

#[derive(Serialize)]
struct DatetimeKeyValue<'a> {
    datetime_key: &'a str,
}

impl Serialize for PlanFormula {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Constant(value) => serializer.serialize_f64(*value),
            Self::Variable(name) => serializer.serialize_str(name),
            Self::Sum { left, right } => SumValue {
                sum: (&**left, &**right),
            }
            .serialize(serializer),
            Self::Sub { left, right } => SumValue {
                sum: (&**left, NegValue { neg: right }),
            }
            .serialize(serializer),
            Self::Mul { left, right } => MultValue {
                mult: (&**left, &**right),
            }
            .serialize(serializer),
            Self::Div {
                left,
                right,
                by_zero_default,
            } => DivValue {
                div: DivParams {
                    by_zero_default: *by_zero_default,
                    left,
                    right,
                },
            }
            .serialize(serializer),
            Self::Neg(operand) => NegValue { neg: operand }.serialize(serializer),
            Self::Abs(x) => AbsValue { abs: x }.serialize(serializer),
            Self::Sqrt(x) => SqrtValue { sqrt: x }.serialize(serializer),
            Self::Log10(x) => Log10Value { log10: x }.serialize(serializer),
            Self::Ln(x) => LnValue { ln: x }.serialize(serializer),
            Self::Exp(x) => ExpValue { exp: x }.serialize(serializer),
            Self::Acosh(x) => AcoshValue { acosh: x }.serialize(serializer),
            Self::Max(args) => MaxValue { max: args }.serialize(serializer),
            Self::Min(args) => MinValue { min: args }.serialize(serializer),
            Self::Pow { base, exponent } => PowValue {
                pow: PowParams { base, exponent },
            }
            .serialize(serializer),
            Self::GeoDistance { lat, lon, field } => GeoDistanceValue {
                geo_distance: GeoDistanceParams {
                    origin: GeoPointRef {
                        lat: *lat,
                        lon: *lon,
                    },
                    to: field,
                },
            }
            .serialize(serializer),
            Self::Decay {
                kind,
                x,
                target,
                scale,
                midpoint,
            } => {
                let params = DecayParams {
                    midpoint: *midpoint,
                    scale: *scale,
                    target: target.as_deref(),
                    x,
                };
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry(kind.wire_key(), &params)?;
                map.end()
            }
            Self::Condition(filter) => filter.serialize(serializer),
            Self::Case { cond, then_, else_ } => {
                let one_minus = SumValue {
                    sum: (1.0f64, NegValue { neg: cond }),
                };
                SumValue {
                    sum: (
                        MultValue {
                            mult: (&**cond, &**then_),
                        },
                        MultValue {
                            mult: (one_minus, &**else_),
                        },
                    ),
                }
                .serialize(serializer)
            }
            Self::Datetime(value) => DatetimeValue { datetime: value }.serialize(serializer),
            Self::DatetimeKey(key) => DatetimeKeyValue { datetime_key: key }.serialize(serializer),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qql_core::ast::{QueryExpr, Stmt};
    use qql_core::parser::Parser;
    use serde_json::json;

    fn lower(source: &str) -> serde_json::Value {
        let stmt = Parser::parse(source).unwrap();
        let Stmt::Query(query) = stmt else {
            panic!("expected query");
        };
        let QueryExpr::Formula { expression, .. } = &query.expression else {
            panic!("expected formula");
        };
        serde_json::to_value(PlanFormula::from(expression.as_ref())).unwrap()
    }

    #[test]
    fn arithmetic_and_function_shapes_match_openapi_expression() {
        assert_eq!(
            lower("QUERY FORMULA score * 2 FROM docs;"),
            json!({"mult": ["$score", 2.0]})
        );
        assert_eq!(
            lower("QUERY FORMULA score - 2 FROM docs;"),
            json!({"sum": ["$score", {"neg": 2.0}]})
        );
        assert_eq!(
            lower("QUERY FORMULA (score / views [DEFAULT = 1.0]) FROM docs;"),
            json!({"div": {"by_zero_default": 1.0, "left": "$score", "right": "views"}})
        );
        assert_eq!(
            lower("QUERY FORMULA ABS(score) + SQRT(bonus) FROM docs;"),
            json!({"sum": [{"abs": "$score"}, {"sqrt": "bonus"}]})
        );
        assert_eq!(
            lower("QUERY FORMULA LOG(score) + LN(bonus) FROM docs;"),
            json!({"sum": [{"log10": "$score"}, {"ln": "bonus"}]})
        );
        assert_eq!(
            lower("QUERY FORMULA EXP(score) + ACOSH(bonus) FROM docs;"),
            json!({"sum": [{"exp": "$score"}, {"acosh": "bonus"}]})
        );
        assert_eq!(
            lower("QUERY FORMULA MAX(score, bonus) + MIN(rank) FROM docs;"),
            json!({"sum": [{"max": ["$score", "bonus"]}, {"min": ["rank"]}]})
        );
        assert_eq!(
            lower("QUERY FORMULA POW(score, 2) FROM docs;"),
            json!({"pow": {"base": "$score", "exponent": 2.0}})
        );
    }

    #[test]
    fn geo_distance_and_datetime_shapes() {
        assert_eq!(
            lower("QUERY FORMULA GEO_DISTANCE(52.5, 13.4, location) FROM docs;"),
            json!({"geo_distance": {"origin": {"lat": 52.5, "lon": 13.4}, "to": "location"}})
        );
        assert_eq!(
            lower("QUERY FORMULA DATETIME('2024-01-01T00:00:00Z') FROM docs;"),
            json!({"datetime": "2024-01-01T00:00:00Z"})
        );
        assert_eq!(
            lower("QUERY FORMULA DATETIME_KEY('created_at') FROM docs;"),
            json!({"datetime_key": "created_at"})
        );
    }

    #[test]
    fn decay_datetime_target_infers_datetime_key() {
        assert_eq!(
            lower(
                "QUERY FORMULA EXP_DECAY(judgment_date, TARGET = '2026-09-04T00:00:00Z', \
                 SCALE = 630720000, MIDPOINT = 0.5) FROM docs;"
            ),
            json!({
                "exp_decay": {
                    "midpoint": 0.5,
                    "scale": 630720000.0,
                    "target": {"datetime": "2026-09-04T00:00:00Z"},
                    "x": {"datetime_key": "judgment_date"}
                }
            })
        );
        // A non-datetime target keeps the numeric variable form.
        assert_eq!(
            lower("QUERY FORMULA LIN_DECAY(score, TARGET = 0.5) FROM docs;"),
            json!({"lin_decay": {"target": 0.5, "x": "$score"}})
        );
        assert_eq!(
            lower("QUERY FORMULA GAUSS_DECAY(payload_key) FROM docs;"),
            json!({"gauss_decay": {"x": "payload_key"}})
        );
    }

    #[test]
    fn score_variable_normalizes_to_wire_spelling() {
        assert_eq!(
            lower("QUERY FORMULA $score + score FROM docs;"),
            json!({"sum": ["$score", "$score"]})
        );
        assert_eq!(lower("QUERY FORMULA score FROM docs;"), json!("$score"));
    }

    #[test]
    fn match_conditions_lower_to_key_match_objects() {
        assert_eq!(
            lower("QUERY FORMULA MATCH(is_superhost, true) FROM docs;"),
            json!({"key": "is_superhost", "match": {"value": true}})
        );
        assert_eq!(
            lower("QUERY FORMULA MATCH_ANY(tags, ['a', 'b']) FROM docs;"),
            json!({"key": "tags", "match": {"any": ["a", "b"]}})
        );
    }

    #[test]
    fn case_lowers_to_condition_weighted_terms() {
        let value = lower(
            "QUERY FORMULA CASE WHEN status = 'active' THEN $score * 2 ELSE $score END FROM docs;",
        );
        let cond = json!({"key": "status", "match": {"value": "active"}});
        assert_eq!(value["sum"][0]["mult"][0], cond);
        assert_eq!(value["sum"][0]["mult"][1], json!({"mult": ["$score", 2.0]}));
        assert_eq!(value["sum"][1]["mult"][0]["sum"][0], json!(1.0));
        assert_eq!(value["sum"][1]["mult"][0]["sum"][1]["neg"], cond);
        assert_eq!(value["sum"][1]["mult"][1], json!("$score"));

        let compound =
            lower("QUERY FORMULA CASE WHEN a = 1 AND b = 2 THEN $score ELSE 0 END FROM docs;");
        assert_eq!(
            compound["sum"][0]["mult"][0],
            json!({"must": [
                {"key": "a", "match": {"value": 1}},
                {"key": "b", "match": {"value": 2}}
            ]})
        );
    }

    #[test]
    fn ast_values_lower_to_typed_defaults() {
        let defaults: BTreeMap<String, FormulaDefault> = [
            ("score".to_string(), FormulaDefault::Float(0.0)),
            ("boost".to_string(), FormulaDefault::Int(2)),
            (
                "start".to_string(),
                FormulaDefault::String("2024-01-01T00:00:00Z".into()),
            ),
            (
                "nested".to_string(),
                FormulaDefault::Object(BTreeMap::from([(
                    "a".to_string(),
                    FormulaDefault::List(vec![FormulaDefault::Int(1), FormulaDefault::Null]),
                )])),
            ),
        ]
        .into();
        assert_eq!(
            serde_json::to_value(defaults).unwrap(),
            json!({
                "score": 0.0,
                "boost": 2,
                "start": "2024-01-01T00:00:00Z",
                "nested": {"a": [1, null]}
            })
        );

        assert_eq!(
            FormulaDefault::from(&Value::F32Array(vec![0.5, 1.0])),
            FormulaDefault::List(vec![FormulaDefault::Float(0.5), FormulaDefault::Float(1.0)])
        );
    }
}
