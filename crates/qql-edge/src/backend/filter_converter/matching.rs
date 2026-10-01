//! Match / range / values-count lowering: plan match values onto edge types.
//!
//! Pure move from `filter_converter.rs` (size hygiene split).

use qdrant_edge::external::ordered_float::OrderedFloat;
use qdrant_edge::{
    AnyVariants, DateTimeWrapper, Match, MatchAny, MatchExcept, MatchPhrase, MatchPrefix,
    MatchText, MatchTextAny, MatchValue, Range, RangeInterface, ValueVariants, ValuesCount,
};
use qql_core::ast::Value as PlanValue;
use qql_core::error::QqlError;
use qql_plan::types::{
    MatchValue as PlanMatchValue, PlanRangeBound as PlanBound, RangeParams as PlanRangeParams,
    ValuesCountParams as PlanValuesCountParams,
};

use super::filter_error;

pub(crate) fn lower_match(value: &PlanMatchValue) -> Result<Match, QqlError> {
    Ok(match value {
        PlanMatchValue::Value { value } => Match::Value(MatchValue {
            value: lower_match_value(value)?,
        }),
        PlanMatchValue::Text { text } => Match::Text(MatchText { text: text.clone() }),
        PlanMatchValue::TextAny { text_any } => Match::TextAny(MatchTextAny {
            text_any: text_any.clone(),
        }),
        PlanMatchValue::Any { any } => Match::Any(MatchAny {
            any: lower_any_variants(any)?,
        }),
        PlanMatchValue::Except { except } => Match::Except(MatchExcept {
            except: lower_any_variants(except)?,
        }),
        PlanMatchValue::Phrase { phrase } => Match::Phrase(MatchPhrase {
            phrase: phrase.clone(),
        }),
        PlanMatchValue::Prefix { prefix } => Match::Prefix(MatchPrefix {
            prefix: prefix.clone(),
        }),
    })
}

/// `MatchValue::value` only exists for strings, integers, and bools on the
/// Qdrant wire (`ValueVariants`); floats are lowered to a `range` by the
/// planner before reaching this point. This arm is defensive: a hand-built
/// plan carrying a float equality value follows the gRPC rule — an integral
/// float within `i64` range is an integer match, anything else fails closed.
fn lower_match_value(value: &PlanValue) -> Result<ValueVariants, QqlError> {
    match value {
        PlanValue::Str(string) => Ok(ValueVariants::String(string.clone())),
        PlanValue::Int(n) => Ok(ValueVariants::Integer(*n)),
        PlanValue::UInt(n) => i64::try_from(*n).map(ValueVariants::Integer).map_err(|_| {
            filter_error(format!(
                "match value must be a string, integer, or bool, got {n}"
            ))
        }),
        PlanValue::Bool(flag) => Ok(ValueVariants::Bool(*flag)),
        PlanValue::Float(f) => integral_integer(*f).map(ValueVariants::Integer).ok_or_else(|| {
            filter_error(format!(
                "float match value {f} has no exact integer representation; use an exact integer value or a RANGE filter"
            ))
        }),
        PlanValue::Param(name, _) => {
            panic!("invariant violation: unbound parameter :{name} reached edge match lowering");
        }
        PlanValue::PositionalParam(idx, _) => {
            panic!(
                "invariant violation: unbound positional parameter ?{idx} reached edge match lowering"
            );
        }
        other => Err(filter_error(format!(
            "match value must be a string, integer, or bool, got {}",
            qql_plan::value_error_text(other)
        ))),
    }
}

/// `any`/`except` accept a homogeneous string or integer set, matching the
/// `AnyVariants` wire type. Integer entries follow the gRPC `IN`/`NOT IN`
/// rule: plain integers, `u64`s that fit `i64`, and integral floats within
/// `i64` range; mixed lists, non-integral floats, and overflowing `u64`s fail
/// closed. Parser-produced plans reject float lists at plan time, so the float
/// arm only serves hand-built plans.
fn lower_any_variants(values: &[PlanValue]) -> Result<AnyVariants, QqlError> {
    if values.iter().all(|v| matches!(v, PlanValue::Str(_))) {
        Ok(AnyVariants::Strings(
            values
                .iter()
                .filter_map(|v| match v {
                    PlanValue::Str(s) => Some(s.clone()),
                    _ => None,
                })
                .collect(),
        ))
    } else if values.iter().all(|v| integer_entry(v).is_some()) {
        Ok(AnyVariants::Integers(
            values.iter().filter_map(integer_entry).collect(),
        ))
    } else {
        Err(filter_error(format!(
            "match any/except values must be all strings or all integers (integral floats included), got {:?}",
            values
                .iter()
                .map(qql_plan::value_error_text)
                .collect::<Vec<_>>()
        )))
    }
}

/// Integer-like match entry, mirroring the gRPC `list_integer` rule so a list
/// accepted on one transport is accepted on the other.
fn integer_entry(value: &PlanValue) -> Option<i64> {
    match value {
        PlanValue::Int(n) => Some(*n),
        PlanValue::UInt(n) => i64::try_from(*n).ok(),
        PlanValue::Float(f) => integral_integer(*f),
        _ => None,
    }
}

/// `Some(f as i64)` when `f` is finite, integral, and inside `i64` range.
fn integral_integer(f: f64) -> Option<i64> {
    // `f as i64` saturates, so the round-trip check alone would admit 2^63
    // (it re-rounds to 2^63); the range guard keeps the cast exact.
    let integral = f.is_finite()
        && f.fract() == 0.0
        && f >= i64::MIN as f64
        && f < 9_223_372_036_854_775_808.0
        && (f as i64) as f64 == f;
    integral.then_some(f as i64)
}

pub(crate) fn lower_range(range: &PlanRangeParams) -> Result<RangeInterface, QqlError> {
    // A `DateTime` bound selects the datetime interface, mirroring the old
    // `Value::is_string` scan (ISO-looking strings already classify as
    // `DateTime` at lowering; opaque `Text` never does).
    let datetime = [&range.lt, &range.gt, &range.gte, &range.lte]
        .into_iter()
        .flatten()
        .any(|bound| matches!(bound, PlanBound::DateTime(_)));
    if datetime {
        Ok(RangeInterface::DateTime(Range {
            lt: lower_datetime(range.lt.as_ref())?,
            gt: lower_datetime(range.gt.as_ref())?,
            gte: lower_datetime(range.gte.as_ref())?,
            lte: lower_datetime(range.lte.as_ref())?,
        }))
    } else {
        Ok(RangeInterface::Float(Range {
            lt: lower_float(range.lt.as_ref())?,
            gt: lower_float(range.gt.as_ref())?,
            gte: lower_float(range.gte.as_ref())?,
            lte: lower_float(range.lte.as_ref())?,
        }))
    }
}

fn lower_datetime(bound: Option<&PlanBound>) -> Result<Option<DateTimeWrapper>, QqlError> {
    match bound {
        None => Ok(None),
        Some(PlanBound::DateTime(text)) => {
            text.parse::<DateTimeWrapper>().map(Some).map_err(|error| {
                filter_error(format!("invalid datetime range bound '{text}': {error}"))
            })
        }
        Some(other) => Err(filter_error(format!(
            "datetime range bounds must be RFC 3339 strings, got {other:?}"
        ))),
    }
}

fn lower_float(bound: Option<&PlanBound>) -> Result<Option<OrderedFloat<f64>>, QqlError> {
    match bound {
        None => Ok(None),
        Some(numeric) => {
            numeric.as_f64().map(OrderedFloat).map(Some).ok_or_else(|| {
                filter_error(format!("range bounds must be numbers, got {numeric:?}"))
            })
        }
    }
}

pub(crate) fn lower_values_count(counts: &PlanValuesCountParams) -> Result<ValuesCount, QqlError> {
    Ok(ValuesCount {
        lt: lower_count(counts.lt)?,
        gt: lower_count(counts.gt)?,
        gte: lower_count(counts.gte)?,
        lte: lower_count(counts.lte)?,
    })
}

fn lower_count(count: Option<u64>) -> Result<Option<usize>, QqlError> {
    count
        .map(|value| {
            usize::try_from(value).map_err(|_| {
                filter_error(format!("values_count bound {value} exceeds platform usize"))
            })
        })
        .transpose()
}
