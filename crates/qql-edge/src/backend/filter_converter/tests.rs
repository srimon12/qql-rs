use super::*;
use qdrant_edge::{Condition, Filter, RangeInterface};
use qql_plan::PlanPointId;
use qql_plan::types::{
    FieldCondition as PlanFieldCondition, GeoBoundingBox as PlanGeoBoundingBox,
    GeoLineString as PlanGeoLineString, GeoPolygon as PlanGeoPolygon, GeoRadius as PlanGeoRadius,
    NestedCondition as PlanNestedCondition, SliceCondition as PlanSliceCondition,
};
use qql_plan::types::{
    FilterClause as PlanClause, FilterCompound as PlanCompound, FilterExpression as PlanExpression,
    GeoPoint as PlanGeoPoint, HasVectorCondition as PlanHasVector, IsEmptyCondition as PlanIsEmpty,
    IsNullCondition as PlanIsNull, KeyOnly, MatchValue as PlanMatch,
    NestedParams as PlanNestedParams, PlanRangeBound as PlanBound, RangeParams as PlanRange,
    SliceParams as PlanSliceParams, ValuesCountParams as PlanValuesCount,
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
/// Test-only parity helper; production lowering never touches `Value`.
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
                    value: qql_core::ast::Value::Str("NYC".into()),
                },
            ),
            range_field(
                "price",
                PlanRange {
                    gt: None,
                    gte: Some(PlanBound::Int(10)),
                    lt: Some(PlanBound::Int(100)),
                    lte: None,
                },
            ),
            range_field(
                "created_at",
                PlanRange {
                    gt: None,
                    gte: Some(PlanBound::DateTime("2024-01-01T00:00:00Z".to_string())),
                    lt: Some(PlanBound::DateTime("2025-01-01T00:00:00Z".to_string())),
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
                        must: vec![match_field(
                            "size",
                            PlanMatch::Value {
                                value: qql_core::ast::Value::Int(42),
                            },
                        )],
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
                        value: qql_core::ast::Value::Str("active".into()),
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
                any: vec![
                    qql_core::ast::Value::Str("deleted".into()),
                    qql_core::ast::Value::Str("archived".into()),
                ],
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
                    except: vec![
                        qql_core::ast::Value::Str("red".into()),
                        qql_core::ast::Value::Str("blue".into()),
                    ],
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
                    lte: Some(PlanBound::Int(9)),
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
            value: qql_core::ast::Value::Str("NYC".into()),
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
            gte: Some(PlanBound::DateTime("2024-01-01T00:00:00Z".to_string())),
            lt: None,
            lte: None,
        },
    )));
    let lowered = convert_edge_filter(Some(&expression)).unwrap().unwrap();
    let Some(Condition::Field(field)) = lowered.must.as_ref().and_then(|must| must.first()) else {
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
    let expression = PlanExpression::Single(Box::new(PlanClause::Nested(PlanNestedCondition {
        nested: PlanNestedParams {
            key: "variants".to_string(),
            filter: Box::new(PlanExpression::Single(Box::new(match_field(
                "size",
                PlanMatch::Value {
                    value: qql_core::ast::Value::Int(42),
                },
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

/// The object `min_should` (`{"conditions", "min_count"}`) passes through
/// with its own conditions (no `should`-moving: the threshold carries
/// its candidate list).
#[test]
fn legacy_min_should_moves_should_conditions() {
    let expression = PlanExpression::Compound(PlanCompound {
        must: vec![],
        must_not: vec![],
        should: vec![],
        min_should: Some(qql_plan::types::MinShould {
            conditions: vec![match_field(
                "tier",
                PlanMatch::Value {
                    value: qql_core::ast::Value::Str("gold".into()),
                },
            )],
            min_count: 1,
        }),
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
        PlanMatch::Value {
            value: qql_core::ast::Value::Float(1.5),
        },
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
            any: vec![
                qql_core::ast::Value::Str("one".into()),
                qql_core::ast::Value::Int(2),
            ],
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
