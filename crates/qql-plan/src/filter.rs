use crate::types::*;
use qql_core::ast::{ComparisonOp, FilterExpr, GeoPoint, PointIdPredicate, Value};

pub fn lower_filter(filter: &FilterExpr) -> FilterExpression {
    match filter {
        FilterExpr::And { operands } => FilterExpression::Compound(FilterCompound {
            must: operands.iter().map(lower_clause).collect(),
            must_not: Vec::new(),
            should: Vec::new(),
            min_should: None,
        }),
        FilterExpr::Or { operands } => FilterExpression::Compound(FilterCompound {
            must: Vec::new(),
            must_not: Vec::new(),
            should: operands.iter().map(lower_clause).collect(),
            min_should: None,
        }),
        FilterExpr::Not { operand } => FilterExpression::Compound(FilterCompound {
            must: Vec::new(),
            must_not: vec![lower_clause(operand)],
            should: Vec::new(),
            min_should: None,
        }),
        other => FilterExpression::Single(Box::new(lower_clause(other))),
    }
}

pub fn top_level_filter(filter: &FilterExpr) -> FilterExpression {
    let f = lower_filter(filter);
    match f {
        FilterExpression::Single(clause) => FilterExpression::Compound(FilterCompound {
            must: vec![*clause],
            must_not: Vec::new(),
            should: Vec::new(),
            min_should: None,
        }),
        other => other,
    }
}

fn lower_clause(filter: &FilterExpr) -> FilterClause {
    match filter {
        FilterExpr::PointId(predicate) => lower_point_id(predicate),
        FilterExpr::Compare { field, op, value } => lower_compare(field, *op, value),
        FilterExpr::Between { field, low, high } => lower_between(field, low, high),
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
        FilterExpr::Nested { path, filter } => FilterClause::Nested(NestedCondition {
            nested: NestedParams {
                key: path.clone(),
                filter: Box::new(lower_filter(filter)),
            },
        }),
        FilterExpr::HasVector { name } => FilterClause::HasVector(HasVectorCondition {
            has_vector: name.clone(),
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
        FilterExpr::And { operands } => FilterClause::Filter(Box::new(FilterCompound {
            must: operands.iter().map(lower_clause).collect(),
            must_not: Vec::new(),
            should: Vec::new(),
            min_should: None,
        })),
        FilterExpr::Or { operands } => FilterClause::Filter(Box::new(FilterCompound {
            must: Vec::new(),
            must_not: Vec::new(),
            should: operands.iter().map(lower_clause).collect(),
            min_should: None,
        })),
        FilterExpr::Not { operand } => FilterClause::Filter(Box::new(FilterCompound {
            must: Vec::new(),
            must_not: vec![lower_clause(operand)],
            should: Vec::new(),
            min_should: None,
        })),
    }
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
        PointIdPredicate::Eq(id) => vec![point_id_req(id)],
        PointIdPredicate::In(ids) => ids.iter().map(point_id_req).collect(),
    };
    FilterClause::HasId(HasIdCondition { has_id: ids })
}

fn lower_compare(field: &str, op: ComparisonOp, value: &Value) -> FilterClause {
    if op == ComparisonOp::Eq {
        return field_condition(field, |fc| {
            fc.r#match = Some(MatchValue::Value {
                value: value_to_json(value),
            })
        });
    }
    field_condition(field, |fc| fc.range = Some(comparison_range(op, value)))
}

fn lower_between(field: &str, low: &Value, high: &Value) -> FilterClause {
    field_condition(field, |fc| {
        fc.range = Some(RangeParams {
            gt: None,
            gte: Some(value_to_json(low)),
            lt: None,
            lte: Some(value_to_json(high)),
        })
    })
}

fn lower_match_any(field: &str, values: &[Value]) -> FilterClause {
    let any: Vec<_> = values.iter().map(value_to_json).collect();
    field_condition(field, |fc| fc.r#match = Some(MatchValue::Any { any }))
}

fn comparison_range(op: ComparisonOp, value: &Value) -> RangeParams {
    let v = value_to_json(value);
    match op {
        ComparisonOp::Gt => RangeParams {
            gt: Some(v),
            gte: None,
            lt: None,
            lte: None,
        },
        ComparisonOp::Gte => RangeParams {
            gt: None,
            gte: Some(v),
            lt: None,
            lte: None,
        },
        ComparisonOp::Lt => RangeParams {
            gt: None,
            gte: None,
            lt: Some(v),
            lte: None,
        },
        ComparisonOp::Lte => RangeParams {
            gt: None,
            gte: None,
            lt: None,
            lte: Some(v),
        },
        ComparisonOp::Eq => unreachable!(),
    }
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

pub fn value_to_json(value: &Value) -> serde_json::Value {
    match value {
        Value::Str(s) => serde_json::Value::String(s.clone()),
        Value::Int(n) => serde_json::Value::Number((*n).into()),
        Value::Float(f) => serde_json::Number::from_f64(*f)
            .map_or(serde_json::Value::Null, serde_json::Value::Number),
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Null => serde_json::Value::Null,
        Value::List(items) => serde_json::Value::Array(items.iter().map(value_to_json).collect()),
        Value::Dict(entries) => {
            let mut map = serde_json::Map::new();
            for (k, v) in entries {
                map.insert(k.clone(), value_to_json(v));
            }
            serde_json::Value::Object(map)
        }
    }
}

pub fn point_id_req(id: &qql_core::ast::PointId) -> serde_json::Value {
    match id {
        qql_core::ast::PointId::Number(n) => serde_json::Value::Number((*n).into()),
        qql_core::ast::PointId::String(s) => serde_json::Value::String(s.clone()),
    }
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
    fn eq_comparison() {
        let f = FilterExpr::Compare {
            field: "status".into(),
            op: ComparisonOp::Eq,
            value: Value::Str("active".into()),
        };
        assert_json(
            &lower_filter(&f),
            json!({"key": "status", "match": {"value": "active"}}),
        );
    }

    #[test]
    fn range_gt() {
        let f = FilterExpr::Compare {
            field: "count".into(),
            op: ComparisonOp::Gt,
            value: Value::Int(5),
        };
        assert_json(
            &lower_filter(&f),
            json!({"key": "count", "range": {"gt": 5}}),
        );
    }

    #[test]
    fn point_id_eq() {
        let f = FilterExpr::PointId(PointIdPredicate::Eq(qql_core::ast::PointId::Number(42)));
        assert_json(&lower_filter(&f), json!({"has_id": [42]}));
    }

    #[test]
    fn point_id_in() {
        let f = FilterExpr::PointId(PointIdPredicate::In(vec![
            qql_core::ast::PointId::Number(1),
            qql_core::ast::PointId::String("uuid".into()),
        ]));
        assert_json(&lower_filter(&f), json!({"has_id": [1, "uuid"]}));
    }

    #[test]
    fn match_text() {
        let f = FilterExpr::MatchText {
            field: "title".into(),
            text: "search".into(),
        };
        assert_json(
            &lower_filter(&f),
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
            &lower_filter(&f),
            json!({"key": "content", "match": {"phrase": "exact match"}}),
        );
    }

    #[test]
    fn match_any() {
        let f = FilterExpr::MatchAny {
            field: "tags".into(),
            values: vec![Value::Str("rust".into()), Value::Str("go".into())],
        };
        assert_json(
            &lower_filter(&f),
            json!({"key": "tags", "match": {"any": ["rust", "go"]}}),
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
            &lower_filter(&f),
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
            &lower_filter(&f),
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
            &lower_filter(&f),
            json!({"must_not": [{"key": "tag", "match": {"any": ["old"]}}]}),
        );
    }

    #[test]
    fn is_null() {
        let f = FilterExpr::IsNull {
            field: "desc".into(),
        };
        assert_json(&lower_filter(&f), json!({"is_null": {"key": "desc"}}));
    }

    #[test]
    fn is_empty() {
        let f = FilterExpr::IsEmpty {
            field: "tags".into(),
        };
        assert_json(&lower_filter(&f), json!({"is_empty": {"key": "tags"}}));
    }

    #[test]
    fn has_vector() {
        let f = FilterExpr::HasVector {
            name: "dense".into(),
        };
        assert_json(&lower_filter(&f), json!({"has_vector": "dense"}));
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
            &lower_filter(&f),
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
            &lower_filter(&f),
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
            &lower_filter(&f),
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
        let json = serde_json::to_value(lower_filter(&f)).unwrap();
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
        let json = serde_json::to_value(lower_filter(&f)).unwrap();
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
        let json = serde_json::to_value(lower_filter(&f)).unwrap();
        assert_eq!(json["key"], "area");
        assert_eq!(json["geo_bounding_box"]["top_left"]["lat"], 1.0);
    }
}
