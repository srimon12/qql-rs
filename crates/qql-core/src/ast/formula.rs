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

/// Returns `true` if `s` is a valid ISO 8601 date or datetime string (`YYYY-MM-DD` or `YYYY-MM-DD[T| ]hh:mm:ss[.s][Z|±hh[:mm]]`).
/// Strictly rejects trailing non-datetime characters (e.g. "2024-01-01XYZ").
pub fn looks_like_iso_datetime(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.len() < 10 {
        return false;
    }
    // Check YYYY-MM-DD
    if !bytes[0..4].iter().all(u8::is_ascii_digit)
        || bytes[4] != b'-'
        || !bytes[5..7].iter().all(u8::is_ascii_digit)
        || bytes[7] != b'-'
        || !bytes[8..10].iter().all(u8::is_ascii_digit)
    {
        return false;
    }
    if bytes.len() == 10 {
        return true;
    }
    // If longer, must be separated by 'T', 't', or ' '
    let sep = bytes[10];
    if sep != b'T' && sep != b't' && sep != b' ' {
        return false;
    }
    // Must have at least hh:mm:ss (8 chars) -> 10 + 1 + 8 = 19
    if bytes.len() < 19 {
        return false;
    }
    if !bytes[11..13].iter().all(u8::is_ascii_digit)
        || bytes[13] != b':'
        || !bytes[14..16].iter().all(u8::is_ascii_digit)
        || bytes[16] != b':'
        || !bytes[17..19].iter().all(u8::is_ascii_digit)
    {
        return false;
    }
    let mut i = 19;
    // Optional fractional seconds: .123...
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        let frac_start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i == frac_start {
            return false; // '.' with no digits following
        }
    }
    if i == bytes.len() {
        return true;
    }
    // Optional timezone: 'Z', 'z', or '+hh[:mm]' / '-hh[:mm]'
    if bytes[i] == b'Z' || bytes[i] == b'z' {
        return i + 1 == bytes.len();
    }
    if bytes[i] == b'+' || bytes[i] == b'-' {
        i += 1;
        let tz_start = i;
        while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b':') {
            i += 1;
        }
        let tz_len = i - tz_start;
        // Valid timezone specs: hh (2), hhmm (4), hh:mm (5)
        if (tz_len == 2 || tz_len == 4 || tz_len == 5) && i == bytes.len() {
            return true;
        }
        return false;
    }
    false
}
