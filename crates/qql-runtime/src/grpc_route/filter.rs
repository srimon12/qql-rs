//! `FilterExpression` → proto [`qdrant::Filter`] converters.
//!
//! `IN`/`NOT IN` lists must be homogeneous and int64-representable (the pinned
//! proto carries repeated string or int64 lists only); anything else is a
//! structured validation error, never a silent drop.

use qql_core::error::QqlError;
use qql_plan::types::{FilterClause, FilterCompound, FilterExpression, MatchValue};

use crate::qdrant_grpc::qdrant;

use super::common::to_point_id;

pub(crate) fn to_filter(fe: &FilterExpression) -> Result<qdrant::Filter, QqlError> {
    match fe {
        FilterExpression::Compound(fc) => compound_to_filter(fc),
        FilterExpression::Single(fc) => Ok(qdrant::Filter {
            must: vec![to_condition(fc)?],
            ..Default::default()
        }),
    }
}

pub(crate) fn to_filter_opt(
    fe: Option<&FilterExpression>,
) -> Result<Option<qdrant::Filter>, QqlError> {
    fe.map(to_filter).transpose()
}

pub(crate) fn compound_to_filter(fc: &FilterCompound) -> Result<qdrant::Filter, QqlError> {
    let must = fc
        .must
        .iter()
        .map(to_condition)
        .collect::<Result<Vec<_>, _>>()?;
    let must_not = fc
        .must_not
        .iter()
        .map(to_condition)
        .collect::<Result<Vec<_>, _>>()?;
    let should = fc
        .should
        .iter()
        .map(to_condition)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(qdrant::Filter {
        must,
        must_not,
        should,
        ..Default::default()
    })
}

pub(crate) fn to_condition(clause: &FilterClause) -> Result<qdrant::Condition, QqlError> {
    use qdrant::condition::ConditionOneOf;
    let condition_one_of = match clause {
        FilterClause::Field(fc) => {
            let mut field = qdrant::FieldCondition {
                key: fc.key.clone(),
                ..Default::default()
            };
            if let Some(mv) = &fc.r#match {
                field.r#match = Some(to_match(mv)?);
            }
            if let Some(r) = &fc.range {
                field.range = Some(qdrant::Range {
                    gt: r.gt.as_ref().and_then(|v| v.as_f64()),
                    gte: r.gte.as_ref().and_then(|v| v.as_f64()),
                    lt: r.lt.as_ref().and_then(|v| v.as_f64()),
                    lte: r.lte.as_ref().and_then(|v| v.as_f64()),
                });
            }
            if let Some(b) = &fc.geo_bounding_box {
                field.geo_bounding_box = Some(qdrant::GeoBoundingBox {
                    top_left: Some(qdrant::GeoPoint {
                        lat: b.top_left.lat,
                        lon: b.top_left.lon,
                    }),
                    bottom_right: Some(qdrant::GeoPoint {
                        lat: b.bottom_right.lat,
                        lon: b.bottom_right.lon,
                    }),
                });
            }
            if let Some(r) = &fc.geo_radius {
                field.geo_radius = Some(qdrant::GeoRadius {
                    center: Some(qdrant::GeoPoint {
                        lat: r.center.lat,
                        lon: r.center.lon,
                    }),
                    radius: r.radius as f32,
                });
            }
            if let Some(vc) = &fc.values_count {
                field.values_count = Some(qdrant::ValuesCount {
                    gt: vc.gt,
                    gte: vc.gte,
                    lt: vc.lt,
                    lte: vc.lte,
                });
            }
            ConditionOneOf::Field(field)
        }
        FilterClause::IsNull(n) => ConditionOneOf::IsNull(qdrant::IsNullCondition {
            key: n.is_null.key.clone(),
        }),
        FilterClause::IsEmpty(e) => ConditionOneOf::IsEmpty(qdrant::IsEmptyCondition {
            key: e.is_empty.key.clone(),
        }),
        FilterClause::HasId(h) => ConditionOneOf::HasId(qdrant::HasIdCondition {
            has_id: h.has_id.iter().map(to_point_id).collect(),
        }),
        FilterClause::HasVector(v) => ConditionOneOf::HasVector(qdrant::HasVectorCondition {
            has_vector: v.has_vector.clone(),
        }),
        FilterClause::Nested(n) => ConditionOneOf::Nested(qdrant::NestedCondition {
            key: n.nested.key.clone(),
            filter: Some(to_filter(&n.nested.filter)?),
        }),
        FilterClause::Filter(f) => ConditionOneOf::Filter(compound_to_filter(f)?),
        FilterClause::Slice(s) => ConditionOneOf::Slice(qdrant::SliceCondition {
            total: s.slice.total as u32,
            index: s.slice.index as u32,
        }),
    };
    Ok(qdrant::Condition {
        condition_one_of: Some(condition_one_of),
    })
}

/// Convert an `IN`/`NOT IN` value list to the gRPC `Match`.
///
/// The pinned proto's `Match` carries either a repeated string list
/// (`keywords` / `except_keywords`) or a repeated int64 list (`integers` /
/// `except_integers`); there is no repeated bool or double list. Every entry
/// must therefore be the same kind and representable as the target type.
/// Mixed lists, non-integral floats, and `u64` values above `i64::MAX`
/// produce a structured error instead of being silently dropped or wrapped
/// into negative integers.
pub(crate) fn exact_list_match(
    values: &[serde_json::Value],
    any: bool,
) -> Result<qdrant::Match, QqlError> {
    use qdrant::r#match::MatchValue as Mv;

    let any_string = values.iter().any(|v| v.is_string());
    let all_strings = values.iter().all(|v| v.is_string());
    if any_string && !all_strings {
        let offending = values
            .iter()
            .find(|v| !v.is_string())
            .map(|v| v.to_string())
            .unwrap_or_else(|| "non-string entry".to_string());
        return Err(QqlError::validation(
            "QQL-GRPC-LIST-TYPE",
            format!(
                "IN/NOT IN list mixes strings with non-string entry {offending}: the bundled Qdrant proto only supports homogeneous string or int64 lists"
            ),
            None,
        ));
    }
    if all_strings {
        let strings: Vec<String> = values
            .iter()
            .map(|v| String::from(v.as_str().expect("guarded by all_strings")))
            .collect();
        return Ok(qdrant::Match {
            match_value: Some(if any {
                Mv::Keywords(qdrant::RepeatedStrings { strings })
            } else {
                Mv::ExceptKeywords(qdrant::RepeatedStrings { strings })
            }),
        });
    }

    // No strings: every entry must fit the proto's repeated int64 list.
    // Integral floats map to integer matches (mirroring single-value
    // `to_match`); anything else is a structured error.
    let integers = values
        .iter()
        .map(list_integer)
        .collect::<Result<Vec<i64>, QqlError>>()?;
    Ok(qdrant::Match {
        match_value: Some(if any {
            Mv::Integers(qdrant::RepeatedIntegers { integers })
        } else {
            Mv::ExceptIntegers(qdrant::RepeatedIntegers { integers })
        }),
    })
}

/// Convert one `IN`/`NOT IN` entry to the int64 wire type, rejecting values
/// the bundled proto cannot carry.
pub(crate) fn list_integer(value: &serde_json::Value) -> Result<i64, QqlError> {
    if let Some(n) = value.as_i64() {
        return Ok(n);
    }
    if let Some(n) = value.as_u64() {
        return i64::try_from(n).map_err(|_| {
            QqlError::validation(
                "QQL-GRPC-LIST-INT",
                format!(
                    "integer {n} in an IN/NOT IN list cannot be represented on the gRPC path: the bundled Qdrant proto stores match lists as int64 (max {})",
                    i64::MAX
                ),
                None,
            )
        });
    }
    if let Some(f) = value.as_f64() {
        // Mirror single-value `to_match`: an integral float is carried as an
        // integer match (Qdrant compares payload numbers numerically).
        let integral = f.is_finite()
            && f.fract() == 0.0
            && f >= i64::MIN as f64
            && f <= i64::MAX as f64
            && (f as i64) as f64 == f;
        if integral {
            return Ok(f as i64);
        }
        return Err(QqlError::validation(
            "QQL-GRPC-LIST-TYPE",
            format!(
                "IN/NOT IN list entry {value} cannot be represented on the gRPC path: the bundled Qdrant proto only supports homogeneous string or int64 lists; use an exact integer value or a RANGE filter"
            ),
            None,
        ));
    }
    Err(QqlError::validation(
        "QQL-GRPC-LIST-TYPE",
        format!(
            "IN/NOT IN list entry {value} cannot be represented on the gRPC path: the bundled Qdrant proto only supports homogeneous string or int64 lists"
        ),
        None,
    ))
}

pub(crate) fn to_match(mv: &MatchValue) -> Result<qdrant::Match, QqlError> {
    use qdrant::r#match::MatchValue as Mv;
    match mv {
        MatchValue::Value { value } => {
            if let Some(s) = value.as_str() {
                Ok(qdrant::Match {
                    match_value: Some(Mv::Keyword(s.into())),
                })
            } else if let Some(b) = value.as_bool() {
                Ok(qdrant::Match {
                    match_value: Some(Mv::Boolean(b)),
                })
            } else if let Some(n) = value.as_i64() {
                Ok(qdrant::Match {
                    match_value: Some(Mv::Integer(n)),
                })
            } else if let Some(f) = value.as_f64() {
                // The pinned proto's `Match` has no `double_value` field, so a
                // float equality filter (`WHERE x = 1.5`) has no exact wire
                // representation. An integral value can be carried as an
                // integer match (Qdrant compares payload numbers numerically);
                // anything else returns a structured error instead of emitting
                // an empty `Match` and silently matching nothing.
                let integral = f.is_finite()
                    && f.fract() == 0.0
                    && f >= i64::MIN as f64
                    && f <= i64::MAX as f64
                    && (f as i64) as f64 == f;
                if integral {
                    Ok(qdrant::Match {
                        match_value: Some(Mv::Integer(f as i64)),
                    })
                } else {
                    Err(QqlError::validation(
                        "QQL-GRPC-FLOAT-MATCH",
                        format!(
                            "float equality value {f} cannot be represented on the gRPC path: the bundled Qdrant proto's Match has no double field; use a RANGE filter or an exact integer value"
                        ),
                        None,
                    ))
                }
            } else {
                Err(QqlError::validation(
                    "QQL-GRPC-MATCH-VALUE",
                    format!(
                        "match value {} cannot be represented on the gRPC path",
                        value
                    ),
                    None,
                ))
            }
        }
        MatchValue::Text { text } => Ok(qdrant::Match {
            match_value: Some(Mv::Text(text.clone())),
        }),
        MatchValue::TextAny { text } => Ok(qdrant::Match {
            match_value: Some(Mv::TextAny(text.clone())),
        }),
        MatchValue::Any { any } => exact_list_match(any, true),
        MatchValue::Except { except } => exact_list_match(except, false),
        MatchValue::Phrase { phrase } => Ok(qdrant::Match {
            match_value: Some(Mv::Phrase(phrase.clone())),
        }),
        MatchValue::Prefix { prefix } => Ok(qdrant::Match {
            match_value: Some(Mv::Prefix(prefix.clone())),
        }),
    }
}
