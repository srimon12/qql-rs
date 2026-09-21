//! Match / range / values-count lowering: plan match values onto edge types.
//!
//! Pure move from `filter_converter.rs` (size hygiene split).

use qdrant_edge::external::ordered_float::OrderedFloat;
use qdrant_edge::{
    AnyVariants, DateTimeWrapper, Match, MatchAny, MatchExcept, MatchPhrase, MatchPrefix,
    MatchText, MatchTextAny, MatchValue, Range, RangeInterface, ValueVariants, ValuesCount,
};
use qql_core::error::QqlError;
use qql_plan::types::{
    MatchValue as PlanMatchValue, PlanRangeBound as PlanBound, RangeParams as PlanRangeParams,
    ValuesCountParams as PlanValuesCountParams,
};
use serde_json::Value;

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
/// planner before reaching this point.
fn lower_match_value(value: &Value) -> Result<ValueVariants, QqlError> {
    match value {
        Value::String(string) => Ok(ValueVariants::String(string.clone())),
        Value::Number(number) => number.as_i64().map(ValueVariants::Integer).ok_or_else(|| {
            filter_error(format!(
                "match value must be a string, integer, or bool, got {number}"
            ))
        }),
        Value::Bool(flag) => Ok(ValueVariants::Bool(*flag)),
        other => Err(filter_error(format!(
            "match value must be a string, integer, or bool, got {other}"
        ))),
    }
}

/// `any`/`except` accept homogeneous string or integer sets. A mixed list is
/// rejected, matching the `AnyVariants` wire type.
fn lower_any_variants(values: &[Value]) -> Result<AnyVariants, QqlError> {
    if values.iter().all(Value::is_string) {
        Ok(AnyVariants::Strings(
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect(),
        ))
    } else if values.iter().all(Value::is_i64) {
        Ok(AnyVariants::Integers(
            values.iter().filter_map(Value::as_i64).collect(),
        ))
    } else {
        Err(filter_error(format!(
            "match any/except values must be all strings or all integers, got {values:?}"
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
