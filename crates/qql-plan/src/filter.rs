use crate::types::*;
use qql_core::ast::{
    ComparisonOp, FilterExpr, GeoPoint, PointIdPredicate, Value, looks_like_iso_datetime,
};
use qql_core::error::QqlError;

/// Lower a typed AST filter into the transport-neutral `FilterExpression` IR.
pub fn lower_filter(filter: &FilterExpr) -> Result<FilterExpression, QqlError> {
    Ok(match filter {
        FilterExpr::And { operands } => FilterExpression::Compound(FilterCompound {
            must: operands
                .iter()
                .map(lower_clause)
                .collect::<Result<_, _>>()?,
            must_not: Vec::new(),
            should: Vec::new(),
            min_should: None,
        }),
        FilterExpr::Or { operands } => FilterExpression::Compound(FilterCompound {
            must: Vec::new(),
            must_not: Vec::new(),
            should: operands
                .iter()
                .map(lower_clause)
                .collect::<Result<_, _>>()?,
            min_should: None,
        }),
        FilterExpr::Not { operand } => FilterExpression::Compound(FilterCompound {
            must: Vec::new(),
            must_not: vec![lower_clause(operand)?],
            should: Vec::new(),
            min_should: None,
        }),
        // Top-level at-least-N lifts to the compound object form directly:
        // nesting `{"min_should": …}` inside `must` is not a valid Condition.
        FilterExpr::MinShould {
            min_count,
            operands,
        } => FilterExpression::Compound(FilterCompound {
            must: Vec::new(),
            must_not: Vec::new(),
            should: Vec::new(),
            min_should: Some(MinShould {
                conditions: operands
                    .iter()
                    .map(lower_clause)
                    .collect::<Result<_, _>>()?,
                min_count: *min_count,
            }),
        }),
        other => FilterExpression::Single(Box::new(lower_clause(other)?)),
    })
}

/// Normalize a single top-level clause into a compound envelope.
///
/// Shard routing is **not** attached to filters. Both REST and gRPC carry
/// routing on the operation request (`shard_key` / `ShardKeySelector`).
pub fn top_level_filter(filter: &FilterExpr) -> Result<FilterExpression, QqlError> {
    Ok(match lower_filter(filter)? {
        FilterExpression::Single(clause) => FilterExpression::Compound(FilterCompound {
            must: vec![*clause],
            must_not: Vec::new(),
            should: Vec::new(),
            min_should: None,
        }),
        compound => compound,
    })
}

fn lower_clause(filter: &FilterExpr) -> Result<FilterClause, QqlError> {
    Ok(match filter {
        FilterExpr::PointId(predicate) => lower_point_id(predicate),
        FilterExpr::Compare { field, op, value } => lower_compare(field, *op, value)?,
        FilterExpr::Between { field, low, high } => lower_between(field, low, high)?,
        FilterExpr::In { field, values } => lower_match_any(field, values),
        FilterExpr::IsNull { field } => FilterClause::IsNull(IsNullCondition {
            is_null: KeyOnly { key: field.clone() },
        }),
        FilterExpr::IsEmpty { field } => FilterClause::IsEmpty(IsEmptyCondition {
            is_empty: KeyOnly { key: field.clone() },
        }),
        FilterExpr::MatchText { field, text } => field_condition(field, |fc| {
            fc.r#match = Some(MatchValue::Text { text: text.clone() })
        }),
        FilterExpr::MatchAny { field, values } => {
            let any: Vec<_> = values.iter().map(value_to_json).collect();
            field_condition(field, |fc| fc.r#match = Some(MatchValue::Any { any }))
        }
        FilterExpr::MatchPhrase { field, text } => field_condition(field, |fc| {
            fc.r#match = Some(MatchValue::Phrase {
                phrase: text.clone(),
            })
        }),
        FilterExpr::MatchPrefix { field, prefix } => field_condition(field, |fc| {
            fc.r#match = Some(MatchValue::Prefix {
                prefix: prefix.clone(),
            })
        }),
        FilterExpr::MatchTokens { field, text } => field_condition(field, |fc| {
            fc.r#match = Some(MatchValue::TextAny {
                text_any: text.clone(),
            })
        }),
        FilterExpr::MatchExcept { field, values } => {
            let except: Vec<_> = values.iter().map(value_to_json).collect();
            field_condition(field, |fc| fc.r#match = Some(MatchValue::Except { except }))
        }
        FilterExpr::MinShould {
            min_count,
            operands,
        } => {
            // A bare `{"min_should": …}` is not a valid nested Condition, so
            // nested occurrences ride a `{"filter": …}` envelope (top-level
            // `MIN SHOULD` lifts to the compound object in `lower_filter`).
            let min_should = MinShould {
                conditions: operands
                    .iter()
                    .map(lower_clause)
                    .collect::<Result<_, _>>()?,
                min_count: *min_count,
            };
            FilterClause::Filter(Box::new(FilterCompound {
                must: Vec::new(),
                must_not: Vec::new(),
                should: Vec::new(),
                min_should: Some(min_should),
            }))
        }
        FilterExpr::Nested { path, filter } => FilterClause::Nested(NestedCondition {
            nested: NestedParams {
                key: path.clone(),
                filter: Box::new(lower_filter(filter)?),
            },
        }),
        FilterExpr::HasVector { name } => FilterClause::HasVector(HasVectorCondition {
            has_vector: name.clone(),
        }),
        FilterExpr::Slice { total, index } => FilterClause::Slice(SliceCondition {
            slice: SliceParams {
                total: *total,
                index: *index,
            },
        }),
        FilterExpr::ValuesCount { field, op, count } => {
            let mut fc = empty_field_condition(field);
            fc.values_count = Some(values_count_params(*op, *count));
            FilterClause::Field(Box::new(fc))
        }
        FilterExpr::GeoBoundingBox {
            field,
            top_left,
            bottom_right,
        } => field_condition(field, |fc| {
            fc.geo_bounding_box = Some(GeoBoundingBox {
                top_left: geo_point_req(top_left),
                bottom_right: geo_point_req(bottom_right),
            })
        }),
        FilterExpr::GeoRadius {
            field,
            center,
            radius,
        } => field_condition(field, |fc| {
            fc.geo_radius = Some(GeoRadius {
                center: geo_point_req(center),
                radius: *radius,
            })
        }),
        FilterExpr::GeoPolygon {
            field,
            exterior,
            interiors,
        } => field_condition(field, |fc| {
            fc.geo_polygon = Some(GeoPolygon {
                exterior: GeoLineString {
                    points: exterior.iter().map(geo_point_req).collect(),
                },
                interiors: interiors
                    .iter()
                    .map(|ring| GeoLineString {
                        points: ring.iter().map(geo_point_req).collect(),
                    })
                    .collect(),
            })
        }),
        FilterExpr::And { operands } => FilterClause::Filter(Box::new(FilterCompound {
            must: operands
                .iter()
                .map(lower_clause)
                .collect::<Result<_, _>>()?,
            must_not: Vec::new(),
            should: Vec::new(),
            min_should: None,
        })),
        FilterExpr::Or { operands } => FilterClause::Filter(Box::new(FilterCompound {
            must: Vec::new(),
            must_not: Vec::new(),
            should: operands
                .iter()
                .map(lower_clause)
                .collect::<Result<_, _>>()?,
            min_should: None,
        })),
        FilterExpr::Not { operand } => FilterClause::Filter(Box::new(FilterCompound {
            must: Vec::new(),
            must_not: vec![lower_clause(operand)?],
            should: Vec::new(),
            min_should: None,
        })),
    })
}

fn empty_field_condition(field: &str) -> FieldCondition {
    FieldCondition {
        key: field.into(),
        r#match: None,
        range: None,
        geo_bounding_box: None,
        geo_radius: None,
        geo_polygon: None,
        values_count: None,
        is_empty: None,
        is_null: None,
    }
}

fn field_condition(field: &str, f: impl FnOnce(&mut FieldCondition)) -> FilterClause {
    let mut fc = empty_field_condition(field);
    f(&mut fc);
    FilterClause::Field(Box::new(fc))
}

fn lower_point_id(predicate: &PointIdPredicate) -> FilterClause {
    let ids = match predicate {
        PointIdPredicate::Eq(id) => vec![point_id_req_typed(id)],
        PointIdPredicate::In(ids) => ids.iter().map(point_id_req_typed).collect(),
    };
    FilterClause::HasId(HasIdCondition { has_id: ids })
}

fn lower_compare(field: &str, op: ComparisonOp, value: &Value) -> Result<FilterClause, QqlError> {
    if op == ComparisonOp::Eq {
        // Qdrant `match` rejects floats at runtime (MatchInterface has no
        // float variant); exact float equality is `range` with gte == lte.
        if let Value::Float(_) = value {
            let bound = range_bound(value)?;
            return Ok(field_condition(field, |fc| {
                fc.range = Some(RangeParams {
                    gt: None,
                    gte: Some(bound.clone()),
                    lt: None,
                    lte: Some(bound),
                })
            }));
        }
        return Ok(field_condition(field, |fc| {
            fc.r#match = Some(MatchValue::Value {
                value: value_to_json(value),
            })
        }));
    }
    let range = comparison_range(op, value)?;
    Ok(field_condition(field, |fc| {
        fc.range = Some(range);
    }))
}

fn lower_between(field: &str, low: &Value, high: &Value) -> Result<FilterClause, QqlError> {
    let gte = range_bound(low)?;
    let lte = range_bound(high)?;
    Ok(field_condition(field, |fc| {
        fc.range = Some(RangeParams {
            gt: None,
            gte: Some(gte),
            lt: None,
            lte: Some(lte),
        })
    }))
}

fn lower_match_any(field: &str, values: &[Value]) -> FilterClause {
    let any: Vec<_> = values.iter().map(value_to_json).collect();
    field_condition(field, |fc| fc.r#match = Some(MatchValue::Any { any }))
}

fn comparison_range(op: ComparisonOp, value: &Value) -> Result<RangeParams, QqlError> {
    let bound = range_bound(value)?;
    Ok(match op {
        ComparisonOp::Gt => RangeParams {
            gt: Some(bound),
            gte: None,
            lt: None,
            lte: None,
        },
        ComparisonOp::Gte => RangeParams {
            gt: None,
            gte: Some(bound),
            lt: None,
            lte: None,
        },
        ComparisonOp::Lt => RangeParams {
            gt: None,
            gte: None,
            lt: Some(bound),
            lte: None,
        },
        ComparisonOp::Lte => RangeParams {
            gt: None,
            gte: None,
            lt: None,
            lte: Some(bound),
        },
        // `lower_compare` diverts `Eq` before this point, so this arm is a
        // drift guard, not a live path. Lower equality as an exact range
        // (`gte == lte`), mirroring the float-`Eq` handling in
        // `lower_compare` and the `Eq` arm of `values_count_params`.
        ComparisonOp::Eq => RangeParams {
            gt: None,
            gte: Some(bound.clone()),
            lt: None,
            lte: Some(bound),
        },
    })
}

/// Convert a filter `Value` into a typed range bound.
///
/// The parser rejects bool/null/array/object literals as inequality and
/// `BETWEEN` bounds, and both `plan()` (`ensure_no_unbound_params`) and
/// `plan_template()` reject unbound placeholders before lowering. A *bound*
/// placeholder resolving to a mistyped value (`bind_value` substitutes
/// blindly) is a type error at the placeholder, so it fails here with
/// `QQL-PLAN-RANGE-TYPE` instead of stringifying into a bound that matches
/// wrong rows or 400s downstream.
fn range_bound(value: &Value) -> Result<PlanRangeBound, QqlError> {
    Ok(match value {
        Value::Int(n) => PlanRangeBound::Int(*n),
        // The parser only produces `UInt` for literals overflowing `i64`;
        // small programmatic values keep their integer wire shape, huge ones
        // ride a double like the f64-based edge/gRPC paths already do.
        Value::UInt(n) => i64::try_from(*n)
            .map(PlanRangeBound::Int)
            .unwrap_or(PlanRangeBound::Float(*n as f64)),
        Value::Float(f) => PlanRangeBound::Float(*f),
        // Same split the formula parser uses for `DATETIME('…')` versus a
        // variable (`parse_formula_string`): ISO-looking strings are datetime
        // bounds, everything else opaque text. The original text is preserved
        // either way — never parsed into a timestamp.
        Value::Str(s) => {
            if looks_like_iso_datetime(s) {
                PlanRangeBound::DateTime(s.clone())
            } else {
                PlanRangeBound::Text(s.clone())
            }
        }
        // Unreachable through the supported entry points (see above);
        // mirrors the `value_to_json` invariant panic for unbound placeholders.
        Value::Param(name, _) => {
            panic!("invariant violation: unbound parameter :{name} reached range lowering");
        }
        Value::PositionalParam(idx, _) => {
            panic!(
                "invariant violation: unbound positional parameter ?{idx} reached range lowering"
            );
        }
        // Parser-rejected literals, reachable only as a bound placeholder's
        // resolved value: a type error at the placeholder.
        other => {
            return Err(QqlError::validation(
                "QQL-PLAN-RANGE-TYPE",
                format!(
                    "range bound has non-scalar value {other:?}; expected a number, string, or datetime"
                ),
                value.param_span(),
            ));
        }
    })
}

fn values_count_params(op: ComparisonOp, count: u64) -> ValuesCountParams {
    match op {
        ComparisonOp::Gt => ValuesCountParams {
            gt: Some(count),
            gte: None,
            lt: None,
            lte: None,
        },
        ComparisonOp::Gte => ValuesCountParams {
            gt: None,
            gte: Some(count),
            lt: None,
            lte: None,
        },
        ComparisonOp::Lt => ValuesCountParams {
            gt: None,
            gte: None,
            lt: Some(count),
            lte: None,
        },
        ComparisonOp::Lte => ValuesCountParams {
            gt: None,
            gte: None,
            lt: None,
            lte: Some(count),
        },
        ComparisonOp::Eq => ValuesCountParams {
            gt: None,
            gte: Some(count),
            lt: None,
            lte: Some(count),
        },
    }
}

/// Convert a dynamic AST `Value` into its JSON wire representation.
///
/// # Invariant
///
/// `Param` / `PositionalParam` arms panic: `plan()` runs
/// `ensure_no_unbound_params` first and `plan_template()` runs
/// `validate_no_unbound_scalar_params`, so no unbound placeholder reaches
/// here through the supported entry points. Direct callers must preserve
/// that gating order.
pub fn value_to_json(value: &Value) -> serde_json::Value {
    match value {
        Value::Str(s) => serde_json::Value::String(s.clone()),
        Value::Int(n) => serde_json::Value::Number((*n).into()),
        Value::UInt(n) => serde_json::Value::Number((*n).into()),
        Value::Float(f) => serde_json::Number::from_f64(*f)
            .map_or(serde_json::Value::Null, serde_json::Value::Number),
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Null => serde_json::Value::Null,
        Value::F32Array(values) => serde_json::Value::Array(
            values
                .iter()
                .map(|f| {
                    serde_json::Number::from_f64(*f as f64)
                        .map_or(serde_json::Value::Null, serde_json::Value::Number)
                })
                .collect(),
        ),
        Value::List(items) => serde_json::Value::Array(items.iter().map(value_to_json).collect()),
        Value::Dict(entries) => {
            let mut map = serde_json::Map::with_capacity(entries.len());
            for (k, v) in entries {
                map.insert(k.clone(), value_to_json(v));
            }
            serde_json::Value::Object(map)
        }
        Value::Param(name, _) => {
            panic!("invariant violation: unbound parameter :{name} reached filter lowering");
        }
        Value::PositionalParam(idx, _) => {
            panic!(
                "invariant violation: unbound positional parameter ?{idx} reached filter lowering"
            );
        }
    }
}

/// Map an AST point ID to its typed plan representation.
pub fn point_id_req_typed(id: &qql_core::ast::PointId) -> crate::semantic::PlanPointId {
    crate::semantic::PlanPointId::from(id)
}

fn geo_point_req(point: &GeoPoint) -> crate::types::GeoPoint {
    crate::types::GeoPoint {
        lat: point.lat,
        lon: point.lon,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn assert_json(lowered: &FilterExpression, expected: serde_json::Value) {
        let json = serde_json::to_value(lowered).unwrap();
        assert_eq!(json, expected);
    }

    #[test]
    fn top_level_filter_normalizes_single_clause() {
        // Guards the replacement for the removed `top_level_filter_with_shard`:
        // a single clause lowers to a compound envelope, and shard routing stays
        // on the operation request (never on the filter).
        let f = FilterExpr::Compare {
            field: "status".into(),
            op: ComparisonOp::Eq,
            value: Value::Str("active".into()),
        };
        assert_json(
            &top_level_filter(&f).unwrap(),
            json!({"must": [{"key": "status", "match": {"value": "active"}}]}),
        );
    }

    #[test]
    fn eq_comparison() {
        let f = FilterExpr::Compare {
            field: "status".into(),
            op: ComparisonOp::Eq,
            value: Value::Str("active".into()),
        };
        assert_json(
            &lower_filter(&f).unwrap(),
            json!({"key": "status", "match": {"value": "active"}}),
        );
    }

    #[test]
    fn eq_float_lowers_to_range() {
        // Qdrant `match` has no float variant; exact float equality must be
        // `range` with gte == lte or the backend 400s (MatchInterface).
        let f = FilterExpr::Compare {
            field: "rating".into(),
            op: ComparisonOp::Eq,
            value: Value::Float(4.5),
        };
        assert_json(
            &lower_filter(&f).unwrap(),
            json!({"key": "rating", "range": {"gte": 4.5, "lte": 4.5}}),
        );
    }

    #[test]
    fn comparison_range_eq_is_an_exact_range() {
        // Drift guard for the `Eq` fallback in `comparison_range`
        // (`lower_compare` diverts `Eq` first): equality must lower to an
        // exact range, never panic.
        let range = comparison_range(ComparisonOp::Eq, &Value::Int(5)).unwrap();
        let json = serde_json::to_value(&range).unwrap();
        assert_eq!(json, json!({"gte": 5, "lte": 5}));
    }

    #[test]
    fn range_gt() {
        let f = FilterExpr::Compare {
            field: "count".into(),
            op: ComparisonOp::Gt,
            value: Value::Int(5),
        };
        assert_json(
            &lower_filter(&f).unwrap(),
            json!({"key": "count", "range": {"gt": 5}}),
        );
    }

    #[test]
    fn mistyped_bound_placeholder_fails_closed() {
        // A bool can only reach range lowering as a bound placeholder's
        // resolved value; it must error, never stringify into a bound.
        let f = FilterExpr::Compare {
            field: "age".into(),
            op: ComparisonOp::Gt,
            value: Value::Bool(true),
        };
        let err = lower_filter(&f).unwrap_err();
        assert_eq!(err.code, "QQL-PLAN-RANGE-TYPE");
    }

    #[test]
    fn point_id_eq() {
        let f = FilterExpr::PointId(PointIdPredicate::Eq(qql_core::ast::PointId::Number(42)));
        assert_json(&lower_filter(&f).unwrap(), json!({"has_id": [42]}));
    }

    #[test]
    fn point_id_in() {
        let f = FilterExpr::PointId(PointIdPredicate::In(vec![
            qql_core::ast::PointId::Number(1),
            qql_core::ast::PointId::String("uuid".into()),
        ]));
        assert_json(&lower_filter(&f).unwrap(), json!({"has_id": [1, "uuid"]}));
    }

    #[test]
    fn match_text() {
        let f = FilterExpr::MatchText {
            field: "title".into(),
            text: "search".into(),
        };
        assert_json(
            &lower_filter(&f).unwrap(),
            json!({"key": "title", "match": {"text": "search"}}),
        );
    }

    #[test]
    fn match_phrase() {
        let f = FilterExpr::MatchPhrase {
            field: "content".into(),
            text: "exact match".into(),
        };
        assert_json(
            &lower_filter(&f).unwrap(),
            json!({"key": "content", "match": {"phrase": "exact match"}}),
        );
    }

    #[test]
    fn match_prefix() {
        let f = FilterExpr::MatchPrefix {
            field: "title".into(),
            prefix: "Comp".into(),
        };
        assert_json(
            &lower_filter(&f).unwrap(),
            json!({"key": "title", "match": {"prefix": "Comp"}}),
        );
    }

    #[test]
    fn match_any() {
        let f = FilterExpr::MatchAny {
            field: "tags".into(),
            values: vec![Value::Str("rust".into()), Value::Str("go".into())],
        };
        assert_json(
            &lower_filter(&f).unwrap(),
            json!({"key": "tags", "match": {"any": ["rust", "go"]}}),
        );
    }

    #[test]
    fn match_tokens_lowers_to_text_any() {
        let f = FilterExpr::MatchTokens {
            field: "title".into(),
            text: "red shoes".into(),
        };
        assert_json(
            &lower_filter(&f).unwrap(),
            json!({"key": "title", "match": {"text_any": "red shoes"}}),
        );
    }

    #[test]
    fn match_except() {
        let f = FilterExpr::MatchExcept {
            field: "tags".into(),
            values: vec![Value::Str("red".into()), Value::Str("blue".into())],
        };
        assert_json(
            &lower_filter(&f).unwrap(),
            json!({"key": "tags", "match": {"except": ["red", "blue"]}}),
        );
    }

    #[test]
    fn match_except_integer_values() {
        let f = FilterExpr::MatchExcept {
            field: "code".into(),
            values: vec![Value::Int(1), Value::Int(2)],
        };
        assert_json(
            &lower_filter(&f).unwrap(),
            json!({"key": "code", "match": {"except": [1, 2]}}),
        );
    }

    #[test]
    fn min_should_lowers_to_object_form() {
        let f = FilterExpr::MinShould {
            min_count: 2,
            operands: vec![
                FilterExpr::Compare {
                    field: "a".into(),
                    op: ComparisonOp::Eq,
                    value: Value::Int(1),
                },
                FilterExpr::Compare {
                    field: "b".into(),
                    op: ComparisonOp::Eq,
                    value: Value::Int(2),
                },
            ],
        };
        assert_json(
            &lower_filter(&f).unwrap(),
            json!({"min_should": {"conditions": [
                {"key": "a", "match": {"value": 1}},
                {"key": "b", "match": {"value": 2}}
            ], "min_count": 2}}),
        );
    }

    #[test]
    fn uint_value_serializes_as_json_number() {
        let f = FilterExpr::Compare {
            field: "big".into(),
            op: ComparisonOp::Eq,
            value: Value::UInt(18446744073709551615),
        };
        assert_json(
            &lower_filter(&f).unwrap(),
            json!({"key": "big", "match": {"value": 18446744073709551615u64}}),
        );
    }

    #[test]
    fn string_range_bounds_pass_through() {
        let f = FilterExpr::Compare {
            field: "name".into(),
            op: ComparisonOp::Gt,
            value: Value::Str("m".into()),
        };
        assert_json(
            &lower_filter(&f).unwrap(),
            json!({"key": "name", "range": {"gt": "m"}}),
        );
    }

    #[test]
    fn not_simple() {
        let f = FilterExpr::Not {
            operand: Box::new(FilterExpr::Compare {
                field: "status".into(),
                op: ComparisonOp::Eq,
                value: Value::Str("deleted".into()),
            }),
        };
        assert_json(
            &lower_filter(&f).unwrap(),
            json!({"must_not": [{"key": "status", "match": {"value": "deleted"}}]}),
        );
    }

    #[test]
    fn not_compound() {
        let f = FilterExpr::Not {
            operand: Box::new(FilterExpr::And {
                operands: vec![
                    FilterExpr::Compare {
                        field: "a".into(),
                        op: ComparisonOp::Eq,
                        value: Value::Bool(true),
                    },
                    FilterExpr::Compare {
                        field: "b".into(),
                        op: ComparisonOp::Gt,
                        value: Value::Int(10),
                    },
                ],
            }),
        };
        assert_json(
            &lower_filter(&f).unwrap(),
            json!({"must_not": [{"must": [
                {"key": "a", "match": {"value": true}},
                {"key": "b", "range": {"gt": 10}}
            ]}]}),
        );
    }

    #[test]
    fn not_in() {
        let f = FilterExpr::Not {
            operand: Box::new(FilterExpr::In {
                field: "tag".into(),
                values: vec![Value::Str("old".into())],
            }),
        };
        assert_json(
            &lower_filter(&f).unwrap(),
            json!({"must_not": [{"key": "tag", "match": {"any": ["old"]}}]}),
        );
    }

    #[test]
    fn is_null() {
        let f = FilterExpr::IsNull {
            field: "desc".into(),
        };
        assert_json(
            &lower_filter(&f).unwrap(),
            json!({"is_null": {"key": "desc"}}),
        );
    }

    #[test]
    fn is_empty() {
        let f = FilterExpr::IsEmpty {
            field: "tags".into(),
        };
        assert_json(
            &lower_filter(&f).unwrap(),
            json!({"is_empty": {"key": "tags"}}),
        );
    }

    #[test]
    fn has_vector() {
        let f = FilterExpr::HasVector {
            name: "dense".into(),
        };
        assert_json(&lower_filter(&f).unwrap(), json!({"has_vector": "dense"}));
    }

    #[test]
    fn slice_condition() {
        let f = FilterExpr::Slice { total: 4, index: 1 };
        assert_json(
            &lower_filter(&f).unwrap(),
            json!({"slice": {"total": 4, "index": 1}}),
        );
    }

    #[test]
    fn and_conjunction() {
        let f = FilterExpr::And {
            operands: vec![
                FilterExpr::Compare {
                    field: "a".into(),
                    op: ComparisonOp::Eq,
                    value: Value::Bool(true),
                },
                FilterExpr::Compare {
                    field: "b".into(),
                    op: ComparisonOp::Gt,
                    value: Value::Int(0),
                },
            ],
        };
        assert_json(
            &lower_filter(&f).unwrap(),
            json!({"must": [
                {"key": "a", "match": {"value": true}},
                {"key": "b", "range": {"gt": 0}}
            ]}),
        );
    }

    #[test]
    fn between() {
        let f = FilterExpr::Between {
            field: "age".into(),
            low: Value::Int(18),
            high: Value::Int(65),
        };
        assert_json(
            &lower_filter(&f).unwrap(),
            json!({"key": "age", "range": {"gte": 18, "lte": 65}}),
        );
    }

    #[test]
    fn values_count() {
        let f = FilterExpr::ValuesCount {
            field: "tags".into(),
            op: ComparisonOp::Gt,
            count: 3,
        };
        assert_json(
            &lower_filter(&f).unwrap(),
            json!({"key": "tags", "values_count": {"gt": 3}}),
        );
    }

    #[test]
    fn nested() {
        let f = FilterExpr::Nested {
            path: "comments".into(),
            filter: Box::new(FilterExpr::Compare {
                field: "author".into(),
                op: ComparisonOp::Eq,
                value: Value::Str("alice".into()),
            }),
        };
        let json = serde_json::to_value(lower_filter(&f).unwrap()).unwrap();
        assert_eq!(json["nested"]["key"], "comments");
        assert_eq!(
            json["nested"]["filter"],
            json!({"key": "author", "match": {"value": "alice"}})
        );
    }

    #[test]
    fn geo_radius() {
        let f = FilterExpr::GeoRadius {
            field: "loc".into(),
            center: qql_core::ast::GeoPoint {
                lat: 52.5,
                lon: 13.4,
            },
            radius: 1000.0,
        };
        let json = serde_json::to_value(lower_filter(&f).unwrap()).unwrap();
        assert_eq!(json["key"], "loc");
        assert_eq!(json["geo_radius"]["radius"], 1000.0);
    }

    #[test]
    fn geo_bbox() {
        let f = FilterExpr::GeoBoundingBox {
            field: "area".into(),
            top_left: qql_core::ast::GeoPoint { lat: 1.0, lon: 2.0 },
            bottom_right: qql_core::ast::GeoPoint { lat: 3.0, lon: 4.0 },
        };
        let json = serde_json::to_value(lower_filter(&f).unwrap()).unwrap();
        assert_eq!(json["key"], "area");
        assert_eq!(json["geo_bounding_box"]["top_left"]["lat"], 1.0);
    }

    #[test]
    fn geo_polygon() {
        let f = FilterExpr::GeoPolygon {
            field: "area".into(),
            exterior: vec![
                qql_core::ast::GeoPoint {
                    lat: -70.0,
                    lon: -70.0,
                },
                qql_core::ast::GeoPoint {
                    lat: 60.0,
                    lon: -70.0,
                },
                qql_core::ast::GeoPoint {
                    lat: 60.0,
                    lon: 60.0,
                },
                qql_core::ast::GeoPoint {
                    lat: -70.0,
                    lon: 60.0,
                },
            ],
            interiors: vec![vec![
                qql_core::ast::GeoPoint {
                    lat: -50.0,
                    lon: -50.0,
                },
                qql_core::ast::GeoPoint {
                    lat: 50.0,
                    lon: -50.0,
                },
                qql_core::ast::GeoPoint {
                    lat: 50.0,
                    lon: 50.0,
                },
                qql_core::ast::GeoPoint {
                    lat: -50.0,
                    lon: 50.0,
                },
            ]],
        };
        let json = serde_json::to_value(lower_filter(&f).unwrap()).unwrap();
        assert_eq!(json["key"], "area");
        let polygon = &json["geo_polygon"];
        assert_eq!(polygon["exterior"]["points"][0]["lat"], -70.0);
        assert_eq!(polygon["exterior"]["points"][0]["lon"], -70.0);
        assert_eq!(polygon["exterior"]["points"].as_array().unwrap().len(), 4);
        assert_eq!(polygon["interiors"].as_array().unwrap().len(), 1);
        assert_eq!(
            polygon["interiors"][0]["points"].as_array().unwrap().len(),
            4
        );
    }
}
