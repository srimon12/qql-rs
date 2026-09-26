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
/// planner before reaching this point. Mirrors the old `serde_json::Number`
/// rule exactly: plain integers and `u64` values that fit `i64` pass, floats
/// (even integral ones) fail.
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

/// `any`/`except` accept homogeneous string or integer sets. A mixed list is
/// rejected, matching the `AnyVariants` wire type. Mirrors `is_i64`
/// semantics: integers and fitting `u64`s count, integral floats do not.
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
    } else if values.iter().all(|v| match v {
        PlanValue::Int(_) => true,
        PlanValue::UInt(n) => i64::try_from(*n).is_ok(),
        _ => false,
    }) {
        Ok(AnyVariants::Integers(
            values
                .iter()
                .filter_map(|v| match v {
                    PlanValue::Int(n) => Some(*n),
                    PlanValue::UInt(n) => i64::try_from(*n).ok(),
                    _ => None,
                })
                .collect(),
        ))
    } else {
        Err(filter_error(format!(
            "match any/except values must be all strings or all integers, got {:?}",
            values
                .iter()
                .map(qql_plan::value_error_text)
                .collect::<Vec<_>>()
        )))
    }
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
