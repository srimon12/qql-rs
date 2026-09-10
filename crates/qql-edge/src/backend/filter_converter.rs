//! Typed lowering: `qql_plan` filter IR → `qdrant_edge::Filter`.
//!
//! This replaces the former `serde_json` round-trip in `convert_edge_filter`.
//! The same lowering serves every read and mutation response path
//! (`EdgeQdrant::execute_planned`), so filter semantics cannot drift between
//! them.
//!
//! Variant mapping (plan → qdrant-edge):
//!
//! | plan | qdrant-edge |
//! |---|---|
//! | `FilterExpression::Single` | `Filter { must: [condition] }` |
//! | `FilterExpression::Compound` | `Filter { must, must_not, should, min_should }` |
//! | `MatchValue::{Value,Text,Any,Except,Phrase,Prefix}` | `Match::{Value,Text,Any,Except,Phrase,Prefix}` |
//! | `RangeParams` numeric / RFC 3339 bounds | `RangeInterface::{Float,DateTime}` |
//! | `FieldCondition` geo / `values_count` / `is_empty` / `is_null` | same-named edge types |
//! | `IsNull`/`IsEmpty`/`HasId`/`HasVector`/`Nested`/`Filter`/`Slice` | same-named edge conditions |
//!
//! `FilterCompound::min_should` is the legacy integer form ("at least N of
//! `should`"). qdrant-edge 0.8 models the modern `MinShould { conditions,
//! min_count }` object, so the typed lowering moves the `should` clauses into
//! `MinShould::conditions` and clears `Filter::should`. The planner never emits
//! the legacy field today (every construction site passes `None`), so this is a
//! completeness adapter, not a live path.
//!
//! No serde fallback is needed: every plan variant maps directly.

use std::num::NonZeroU32;

use qdrant_edge::external::ordered_float::OrderedFloat;
use qdrant_edge::{
    AnyVariants, Condition, DateTimeWrapper, FieldCondition, Filter, GeoBoundingBox, GeoLineString,
    GeoPoint, GeoPolygon, GeoRadius, HasIdCondition, HasVectorCondition, IsEmptyCondition,
    IsNullCondition, Match, MatchAny, MatchExcept, MatchPhrase, MatchPrefix, MatchText, MatchValue,
    MinShould, Nested, NestedCondition, Range, RangeInterface, Slice, SliceCondition,
    ValueVariants, ValuesCount,
};
use qql_core::error::QqlError;
use qql_plan::types::{
    FieldCondition as PlanFieldCondition, FilterClause as PlanFilterClause,
    FilterCompound as PlanFilterCompound, FilterExpression as PlanFilterExpression,
    GeoBoundingBox as PlanGeoBoundingBox, GeoLineString as PlanGeoLineString,
    GeoPoint as PlanGeoPoint, GeoPolygon as PlanGeoPolygon, GeoRadius as PlanGeoRadius,
    MatchValue as PlanMatchValue, NestedCondition as PlanNestedCondition,
    RangeParams as PlanRangeParams, SliceCondition as PlanSliceCondition,
    ValuesCountParams as PlanValuesCountParams,
};
use serde_json::Value;

use super::conversions::to_edge_id;

/// Lower a plan filter into an in-process `qdrant_edge::Filter`.
///
/// The single filter converter for every edge path — legacy JSON envelopes and
/// typed responses both call this, so both observe identical filter semantics.
pub(crate) fn convert_edge_filter(
    filter: Option<&PlanFilterExpression>,
) -> Result<Option<Filter>, QqlError> {
    match filter {
        None => Ok(None),
        Some(expression) => Ok(Some(lower_expression(expression)?)),
    }
}

fn lower_expression(expression: &PlanFilterExpression) -> Result<Filter, QqlError> {
    match expression {
        // A bare `Condition` is not a `Filter` on the wire; wrapping it in
        // `must` preserves the old serde-shim semantics for every clause kind.
        PlanFilterExpression::Single(clause) => Ok(Filter {
            must: Some(vec![lower_clause(clause)?]),
            ..Filter::default()
        }),
        PlanFilterExpression::Compound(compound) => lower_compound(compound),
    }
}

fn lower_compound(compound: &PlanFilterCompound) -> Result<Filter, QqlError> {
    let should = lower_conditions(&compound.should)?;
    let (should, min_should) = match compound.min_should {
        Some(min_count) => (
            None,
            Some(MinShould {
                conditions: should.unwrap_or_default(),
                min_count,
            }),
        ),
        None => (should, None),
    };
    Ok(Filter {
        should,
        min_should,
        must: lower_conditions(&compound.must)?,
        must_not: lower_conditions(&compound.must_not)?,
    })
}

/// Lower a clause list, collapsing `[]` to `None` to match the wire
/// representation (`skip_serializing_if` drops empty lists).
fn lower_conditions(clauses: &[PlanFilterClause]) -> Result<Option<Vec<Condition>>, QqlError> {
    if clauses.is_empty() {
        return Ok(None);
    }
    clauses
        .iter()
        .map(lower_clause)
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

fn lower_clause(clause: &PlanFilterClause) -> Result<Condition, QqlError> {
    Ok(match clause {
        PlanFilterClause::Field(condition) => Condition::Field(lower_field(condition)?),
        PlanFilterClause::IsNull(condition) => {
            Condition::IsNull(IsNullCondition::from(lower_key(&condition.is_null.key)?))
        }
        PlanFilterClause::IsEmpty(condition) => {
            Condition::IsEmpty(IsEmptyCondition::from(lower_key(&condition.is_empty.key)?))
        }
        PlanFilterClause::HasId(condition) => {
            let ids = condition
                .has_id
                .iter()
                .cloned()
                .map(to_edge_id)
                .collect::<Result<Vec<_>, _>>()?;
            Condition::HasId(ids.into_iter().collect::<HasIdCondition>())
        }
        PlanFilterClause::HasVector(condition) => Condition::HasVector(HasVectorCondition {
            has_vector: condition.has_vector.clone(),
        }),
        PlanFilterClause::Nested(condition) => Condition::Nested(lower_nested(condition)?),
        PlanFilterClause::Filter(compound) => Condition::Filter(lower_compound(compound)?),
        PlanFilterClause::Slice(condition) => Condition::Slice(lower_slice(condition)?),
    })
}

fn lower_field(condition: &PlanFieldCondition) -> Result<FieldCondition, QqlError> {
    Ok(FieldCondition {
        key: lower_key(&condition.key)?,
        r#match: condition.r#match.as_ref().map(lower_match).transpose()?,
        range: condition.range.as_ref().map(lower_range).transpose()?,
        geo_bounding_box: condition
            .geo_bounding_box
            .as_ref()
            .map(lower_geo_bounding_box)
            .transpose()?,
        geo_radius: condition
            .geo_radius
            .as_ref()
            .map(lower_geo_radius)
            .transpose()?,
        geo_polygon: condition
            .geo_polygon
            .as_ref()
            .map(lower_geo_polygon)
            .transpose()?,
        values_count: condition
            .values_count
            .as_ref()
            .map(lower_values_count)
            .transpose()?,
        is_empty: condition.is_empty,
        is_null: condition.is_null,
    })
}

fn lower_nested(condition: &PlanNestedCondition) -> Result<NestedCondition, QqlError> {
    Ok(NestedCondition {
        nested: Nested {
            key: lower_key(&condition.nested.key)?,
            filter: lower_expression(&condition.nested.filter)?,
        },
    })
}

fn lower_match(value: &PlanMatchValue) -> Result<Match, QqlError> {
    Ok(match value {
        PlanMatchValue::Value { value } => Match::Value(MatchValue {
            value: lower_match_value(value)?,
        }),
        PlanMatchValue::Text { text } => Match::Text(MatchText { text: text.clone() }),
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

fn lower_range(range: &PlanRangeParams) -> Result<RangeInterface, QqlError> {
    let datetime = [&range.lt, &range.gt, &range.gte, &range.lte]
        .into_iter()
        .flatten()
        .any(Value::is_string);
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

fn lower_datetime(value: Option<&Value>) -> Result<Option<DateTimeWrapper>, QqlError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => text.parse::<DateTimeWrapper>().map(Some).map_err(|error| {
            filter_error(format!("invalid datetime range bound '{text}': {error}"))
        }),
        Some(other) => Err(filter_error(format!(
            "datetime range bounds must be RFC 3339 strings, got {other}"
        ))),
    }
}

fn lower_float(value: Option<&Value>) -> Result<Option<OrderedFloat<f64>>, QqlError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(number) => number
            .as_f64()
            .map(OrderedFloat)
            .map(Some)
            .ok_or_else(|| filter_error(format!("range bounds must be numbers, got {number}"))),
    }
}

fn lower_geo_bounding_box(polygon: &PlanGeoBoundingBox) -> Result<GeoBoundingBox, QqlError> {
    Ok(GeoBoundingBox {
        top_left: lower_geo_point(&polygon.top_left),
        bottom_right: lower_geo_point(&polygon.bottom_right),
    })
}

fn lower_geo_radius(radius: &PlanGeoRadius) -> Result<GeoRadius, QqlError> {
    Ok(GeoRadius {
        center: lower_geo_point(&radius.center),
        radius: OrderedFloat(radius.radius),
    })
}

/// Build a validated edge polygon. The wire path validated through
/// `TryFrom<GeoPolygonShadow>`; call the same validator here so direct struct
/// construction cannot smuggle in an unclosed or undersized ring.
fn lower_geo_polygon(polygon: &PlanGeoPolygon) -> Result<GeoPolygon, QqlError> {
    let exterior = lower_geo_line(&polygon.exterior);
    GeoPolygon::validate_line_string(&exterior)
        .map_err(|error| filter_error(format!("invalid geo_polygon exterior: {error}")))?;
    let interiors: Vec<GeoLineString> = polygon.interiors.iter().map(lower_geo_line).collect();
    for interior in &interiors {
        GeoPolygon::validate_line_string(interior)
            .map_err(|error| filter_error(format!("invalid geo_polygon interior: {error}")))?;
    }
    Ok(GeoPolygon {
        exterior,
        interiors: Some(interiors),
    })
}

fn lower_geo_line(line: &PlanGeoLineString) -> GeoLineString {
    GeoLineString {
        points: line.points.iter().map(lower_geo_point).collect(),
    }
}

fn lower_geo_point(point: &PlanGeoPoint) -> GeoPoint {
    GeoPoint {
        lat: OrderedFloat(point.lat),
        lon: OrderedFloat(point.lon),
    }
}

fn lower_values_count(counts: &PlanValuesCountParams) -> Result<ValuesCount, QqlError> {
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

fn lower_slice(condition: &PlanSliceCondition) -> Result<SliceCondition, QqlError> {
    let total = u32::try_from(condition.slice.total)
        .ok()
        .and_then(NonZeroU32::new)
        .ok_or_else(|| {
            filter_error(format!(
                "slice total must be in 1..=u32::MAX, got {}",
                condition.slice.total
            ))
        })?;
    let index = u32::try_from(condition.slice.index).map_err(|_| {
        filter_error(format!(
            "slice index {} exceeds u32::MAX",
            condition.slice.index
        ))
    })?;
    Ok(SliceCondition {
        slice: Slice { total, index },
    })
}

fn lower_key(key: &str) -> Result<qdrant_edge::JsonPath, QqlError> {
    key.parse()
        .map_err(|_| filter_error(format!("invalid payload key '{key}'")))
}

fn filter_error(message: impl Into<String>) -> QqlError {
    QqlError::execution("QQL-EDGE-FILTER-CONVERT", message.into(), None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use qql_plan::PlanPointId;
    use qql_plan::types::{
        FilterClause as PlanClause, FilterCompound as PlanCompound,
        FilterExpression as PlanExpression, GeoPoint as PlanGeoPoint,
        HasVectorCondition as PlanHasVector, IsEmptyCondition as PlanIsEmpty,
        IsNullCondition as PlanIsNull, KeyOnly, MatchValue as PlanMatch,
        NestedParams as PlanNestedParams, RangeParams as PlanRange, SliceParams as PlanSliceParams,
        ValuesCountParams as PlanValuesCount,
    };
    use serde_json::json;

    /// Reference implementation of the removed serde round-trip: serialize the
    /// plan filter, wrap a bare condition in `must`, deserialize into edge.
    fn serde_reference(expression: &PlanExpression) -> Filter {
        let mut value = serde_json::to_value(expression).expect("filter serializes");
        if value.get("key").is_some() {
            value = json!({ "must": [value] });
        }
        serde_json::from_value(value).expect("reference filter parses")
    }

    /// `HasIdCondition` stores IDs in a hash set, so serialization order is not
    /// stable. Sort `has_id` arrays before comparing so parity checks semantics.
    fn normalize_has_id(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                if let Some(serde_json::Value::Array(ids)) = map.get_mut("has_id") {
                    ids.sort_by_key(serde_json::Value::to_string);
                }
                for child in map.values_mut() {
                    normalize_has_id(child);
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    normalize_has_id(item);
                }
            }
            _ => {}
        }
    }

    fn assert_parity(expression: &PlanExpression, context: &str) {
        let typed = convert_edge_filter(Some(expression))
            .expect("typed lowering succeeds")
            .expect("filter present");
        let reference = serde_reference(expression);
        let mut actual = serde_json::to_value(&typed).unwrap();
        let mut expected = serde_json::to_value(&reference).unwrap();
        normalize_has_id(&mut actual);
        normalize_has_id(&mut expected);
        assert_eq!(
            actual, expected,
            "typed lowering diverged from the serde reference for {context}"
        );
    }

    fn field(key: &str, configure: impl FnOnce(&mut PlanFieldCondition)) -> PlanClause {
        let mut condition = PlanFieldCondition {
            key: key.to_string(),
            r#match: None,
            range: None,
            geo_bounding_box: None,
            geo_radius: None,
            geo_polygon: None,
            values_count: None,
            is_empty: None,
            is_null: None,
        };
        configure(&mut condition);
        PlanClause::Field(Box::new(condition))
    }

    fn match_field(key: &str, r#match: PlanMatch) -> PlanClause {
        field(key, |condition| condition.r#match = Some(r#match))
    }

    fn range_field(key: &str, range: PlanRange) -> PlanClause {
        field(key, |condition| condition.range = Some(range))
    }

    /// Every variant the plan layer supports, in one compound filter. The
    /// parity assertion compares the serialized edge filters, so any mapping
    /// divergence (range kind, geo construction, id scheme, nested shape) fails.
    #[test]
    fn typed_lowering_matches_serde_reference_for_all_variants() {
        let expression = PlanExpression::Compound(PlanCompound {
            must: vec![
                match_field(
                    "city",
                    PlanMatch::Value {
                        value: json!("NYC"),
                    },
                ),
                range_field(
                    "price",
                    PlanRange {
                        gt: None,
                        gte: Some(json!(10)),
                        lt: Some(json!(100)),
                        lte: None,
                    },
                ),
                range_field(
                    "created_at",
                    PlanRange {
                        gt: None,
                        gte: Some(json!("2024-01-01T00:00:00Z")),
                        lt: Some(json!("2025-01-01T00:00:00Z")),
                        lte: None,
                    },
                ),
                field("location", |condition| {
                    condition.geo_radius = Some(PlanGeoRadius {
                        center: PlanGeoPoint {
                            lat: 52.5,
                            lon: 13.4,
                        },
                        radius: 5000.0,
                    })
                }),
                field("area", |condition| {
                    condition.geo_bounding_box = Some(PlanGeoBoundingBox {
                        top_left: PlanGeoPoint {
                            lat: 52.6,
                            lon: 13.3,
                        },
                        bottom_right: PlanGeoPoint {
                            lat: 52.4,
                            lon: 13.5,
                        },
                    })
                }),
                field("zone", |condition| {
                    condition.geo_polygon = Some(PlanGeoPolygon {
                        exterior: PlanGeoLineString {
                            points: vec![
                                PlanGeoPoint {
                                    lat: -70.0,
                                    lon: -70.0,
                                },
                                PlanGeoPoint {
                                    lat: 60.0,
                                    lon: -70.0,
                                },
                                PlanGeoPoint {
                                    lat: 60.0,
                                    lon: 60.0,
                                },
                                PlanGeoPoint {
                                    lat: -70.0,
                                    lon: 60.0,
                                },
                                PlanGeoPoint {
                                    lat: -70.0,
                                    lon: -70.0,
                                },
                            ],
                        },
                        interiors: vec![],
                    })
                }),
                field("tags", |condition| {
                    condition.values_count = Some(PlanValuesCount {
                        lt: None,
                        gt: Some(2),
                        gte: None,
                        lte: None,
                    })
                }),
                PlanClause::IsNull(PlanIsNull {
                    is_null: KeyOnly {
                        key: "deleted_at".to_string(),
                    },
                }),
                PlanClause::IsEmpty(PlanIsEmpty {
                    is_empty: KeyOnly {
                        key: "labels".to_string(),
                    },
                }),
                PlanClause::HasId(qql_plan::types::HasIdCondition {
                    has_id: vec![
                        PlanPointId::Number(7),
                        PlanPointId::String("550e8400-e29b-41d4-a716-446655440000".to_string()),
                    ],
                }),
                PlanClause::HasVector(PlanHasVector {
                    has_vector: "dense".to_string(),
                }),
                PlanClause::Slice(PlanSliceCondition {
                    slice: PlanSliceParams { total: 4, index: 1 },
                }),
                PlanClause::Nested(PlanNestedCondition {
                    nested: PlanNestedParams {
                        key: "variants".to_string(),
                        // Compound (not `Single`) so the serde reference can
                        // parse it too: a bare nested condition serializes as
                        // `{"key": …}` which `Nested::filter: Filter` rejects.
                        filter: Box::new(PlanExpression::Compound(PlanCompound {
                            must: vec![match_field("size", PlanMatch::Value { value: json!(42) })],
                            must_not: vec![],
                            should: vec![],
                            min_should: None,
                        })),
                    },
                }),
                PlanClause::Filter(Box::new(PlanCompound {
                    must: vec![match_field(
                        "status",
                        PlanMatch::Value {
                            value: json!("active"),
                        },
                    )],
                    must_not: vec![],
                    should: vec![],
                    min_should: None,
                })),
            ],
            must_not: vec![match_field(
                "status",
                PlanMatch::Any {
                    any: vec![json!("deleted"), json!("archived")],
                },
            )],
            should: vec![
                match_field(
                    "description",
                    PlanMatch::Text {
                        text: "red shoes".to_string(),
                    },
                ),
                match_field(
                    "sku",
                    PlanMatch::Prefix {
                        prefix: "AB".to_string(),
                    },
                ),
                match_field(
                    "color",
                    PlanMatch::Except {
                        except: vec![json!("red"), json!("blue")],
                    },
                ),
                match_field(
                    "note",
                    PlanMatch::Phrase {
                        phrase: "exact wording".to_string(),
                    },
                ),
                range_field(
                    "qty",
                    PlanRange {
                        gt: None,
                        gte: None,
                        lt: None,
                        lte: Some(json!(9)),
                    },
                ),
            ],
            min_should: None,
        });
        assert_parity(&expression, "all-variants compound");
    }

    #[test]
    fn bare_field_clause_matches_serde_reference() {
        let expression = PlanExpression::Single(Box::new(match_field(
            "city",
            PlanMatch::Value {
                value: json!("NYC"),
            },
        )));
        assert_parity(&expression, "bare field condition");
        let lowered = convert_edge_filter(Some(&expression)).unwrap().unwrap();
        assert_eq!(lowered.must.as_ref().map(Vec::len), Some(1));
        assert!(lowered.should.is_none() && lowered.must_not.is_none());
    }

    #[test]
    fn empty_compound_lowers_to_match_all_filter() {
        let expression = PlanExpression::Compound(PlanCompound {
            must: vec![],
            must_not: vec![],
            should: vec![],
            min_should: None,
        });
        let lowered = convert_edge_filter(Some(&expression)).unwrap().unwrap();
        assert_eq!(lowered, Filter::default());
    }

    #[test]
    fn datetime_range_lowers_to_datetime_interface() {
        let expression = PlanExpression::Single(Box::new(range_field(
            "created_at",
            PlanRange {
                gt: None,
                gte: Some(json!("2024-01-01T00:00:00Z")),
                lt: None,
                lte: None,
            },
        )));
        let lowered = convert_edge_filter(Some(&expression)).unwrap().unwrap();
        let Some(Condition::Field(field)) = lowered.must.as_ref().and_then(|must| must.first())
        else {
            panic!("expected a field condition, got {lowered:?}");
        };
        assert!(matches!(
            field.range.as_ref(),
            Some(RangeInterface::DateTime(_))
        ));
        assert_parity(&expression, "datetime range");
    }

    /// A bare condition nested inside `nested(...)` has no serde reference
    /// (the old round-trip rejected `{"key": …}` as `Nested::filter`), but the
    /// typed lowering wraps it in `must` like any other single clause.
    #[test]
    fn bare_nested_filter_is_wrapped_in_must() {
        let expression =
            PlanExpression::Single(Box::new(PlanClause::Nested(PlanNestedCondition {
                nested: PlanNestedParams {
                    key: "variants".to_string(),
                    filter: Box::new(PlanExpression::Single(Box::new(match_field(
                        "size",
                        PlanMatch::Value { value: json!(42) },
                    )))),
                },
            })));
        let lowered = convert_edge_filter(Some(&expression)).unwrap().unwrap();
        let Some(Condition::Nested(nested)) = lowered.must.as_ref().and_then(|must| must.first())
        else {
            panic!("expected nested condition, got {lowered:?}");
        };
        assert_eq!(nested.nested.filter.must.as_ref().map(Vec::len), Some(1));
    }

    /// The legacy integer `min_should` ("at least N of `should`") maps onto the
    /// modern object form by moving the `should` clauses into the conditions.
    #[test]
    fn legacy_min_should_moves_should_conditions() {
        let expression = PlanExpression::Compound(PlanCompound {
            must: vec![],
            must_not: vec![],
            should: vec![match_field(
                "tier",
                PlanMatch::Value {
                    value: json!("gold"),
                },
            )],
            min_should: Some(1),
        });
        let lowered = convert_edge_filter(Some(&expression)).unwrap().unwrap();
        assert!(lowered.should.is_none());
        let min_should = lowered.min_should.expect("min_should present");
        assert_eq!(min_should.min_count, 1);
        assert_eq!(min_should.conditions.len(), 1);
    }

    #[test]
    fn float_match_value_fails_closed() {
        let expression = PlanExpression::Single(Box::new(match_field(
            "price",
            PlanMatch::Value { value: json!(1.5) },
        )));
        let error = convert_edge_filter(Some(&expression)).unwrap_err();
        assert_eq!(error.code, "QQL-EDGE-FILTER-CONVERT");
        assert!(error.message.contains("match value"), "{}", error.message);
    }

    #[test]
    fn mixed_any_values_fail_closed() {
        let expression = PlanExpression::Single(Box::new(match_field(
            "tag",
            PlanMatch::Any {
                any: vec![json!("one"), json!(2)],
            },
        )));
        let error = convert_edge_filter(Some(&expression)).unwrap_err();
        assert_eq!(error.code, "QQL-EDGE-FILTER-CONVERT");
        assert!(
            error.message.contains("all strings or all integers"),
            "{}",
            error.message
        );
    }

    #[test]
    fn zero_slice_total_fails_closed() {
        let expression = PlanExpression::Single(Box::new(PlanClause::Slice(PlanSliceCondition {
            slice: PlanSliceParams { total: 0, index: 0 },
        })));
        let error = convert_edge_filter(Some(&expression)).unwrap_err();
        assert_eq!(error.code, "QQL-EDGE-FILTER-CONVERT");
    }
}
