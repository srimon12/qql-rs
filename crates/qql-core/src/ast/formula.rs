use super::{FilterExpr, Value};
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

/// Formula expression tree evaluated by `QUERY FORMULA` to rescore points.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum FormulaExpr {
    /// Numeric literal.
    Constant {
        /// Literal numeric value.
        value: f64,
    },
    /// Variable reference: `$score`, a `DEFAULTS`-bound name, or a datetime key.
    Variable {
        /// Variable name, e.g. `$score` or a `DEFAULTS` key.
        name: String,
    },
    /// `left + right`.
    Sum {
        /// Left operand.
        left: Box<FormulaExpr>,
        /// Right operand.
        right: Box<FormulaExpr>,
    },
    /// `left - right`.
    Sub {
        /// Left operand.
        left: Box<FormulaExpr>,
        /// Right operand.
        right: Box<FormulaExpr>,
    },
    /// `left * right`.
    Mul {
        /// Left operand.
        left: Box<FormulaExpr>,
        /// Right operand.
        right: Box<FormulaExpr>,
    },
    /// `left / right`, with optional `[DEFAULT n]` zero-division fallback.
    Div {
        /// Dividend.
        left: Box<FormulaExpr>,
        /// Divisor.
        right: Box<FormulaExpr>,
        /// Value substituted when the divisor is zero.
        by_zero_default: Option<f64>,
    },
    /// Unary negation.
    Neg {
        /// Expression to negate.
        operand: Box<FormulaExpr>,
    },
    /// `ABS(x)`.
    Abs {
        /// Argument.
        x: Box<FormulaExpr>,
    },
    /// `SQRT(x)`.
    Sqrt {
        /// Argument.
        x: Box<FormulaExpr>,
    },
    /// `LOG(x)` — base-10 logarithm.
    Log {
        /// Argument.
        x: Box<FormulaExpr>,
    },
    /// `LN(x)` — natural logarithm.
    Ln {
        /// Argument.
        x: Box<FormulaExpr>,
    },
    /// `EXP(x)` — e raised to `x`.
    Exp {
        /// Argument.
        x: Box<FormulaExpr>,
    },
    /// `ACOSH(x)` — inverse hyperbolic cosine.
    Acosh {
        /// Argument.
        x: Box<FormulaExpr>,
    },
    /// `POW(base, exponent)`.
    Pow {
        /// Base expression.
        base: Box<FormulaExpr>,
        /// Exponent expression.
        exponent: Box<FormulaExpr>,
    },
    /// N-ary `MAX(...)`. At least one operand (parser-enforced).
    Max {
        /// Folded operands (n >= 1).
        args: Vec<FormulaExpr>,
    },
    /// N-ary `MIN(...)`. At least one operand (parser-enforced).
    Min {
        /// Folded operands (n >= 1).
        args: Vec<FormulaExpr>,
    },
    /// `GEO_DISTANCE(lat, lon, field)` — meters between the coordinate and a geo field.
    GeoDistance {
        /// Query latitude in degrees.
        lat: f64,
        /// Query longitude in degrees.
        lon: f64,
        /// Payload geo field to measure against.
        field: String,
    },
    /// `EXP_DECAY` / `GAUSS_DECAY` / `LIN_DECAY` decay curve over `x`.
    Decay {
        /// Curve family: `exp_decay`, `gauss_decay`, or `lin_decay`.
        kind: String,
        /// Decaying expression, e.g. a datetime key or `$score`.
        x: Box<FormulaExpr>,
        /// Decay origin; `None` defaults to zero.
        target: Option<Box<FormulaExpr>>,
        /// Distance from `target` at which the output falls to `midpoint`.
        scale: Option<f64>,
        /// Output value at that distance from `target`.
        midpoint: Option<f64>,
    },
    /// `CASE WHEN cond THEN then_ ELSE else_ END`.
    Case {
        /// Boolean condition, evaluated as a filter.
        cond: Box<FilterExpr>,
        /// Value produced when the condition holds.
        then_: Box<FormulaExpr>,
        /// Value produced otherwise.
        else_: Box<FormulaExpr>,
    },
    /// Inline `MATCH(field, values)` boolean used as a 0/1 condition.
    MatchCondition {
        /// Payload field to test.
        field: String,
        /// Accepted values (any-of).
        values: Vec<Value>,
    },
    /// `DATETIME('…')` — ISO 8601 datetime constant.
    Datetime {
        /// ISO 8601 datetime string.
        value: String,
    },
    /// `DATETIME_KEY('field')` — payload datetime field read as a datetime.
    DatetimeKey {
        /// Payload datetime field name.
        key: String,
    },
}
