#[cfg(test)]
mod tests {
    use crate::filter_conv::*;
    use qql_core::ast::{FilterExpr, Value};

    fn build(expr: &FilterExpr) -> QdrantFilter {
        FilterConverter.build_filter(expr).unwrap().unwrap()
    }

    // ── Typed Equality Tests ─────────────────────────────────────

    #[test]
    fn test_equals_string() {
        let expr = FilterExpr::Compare {
            field: "status",
            op: "=",
            value: Value::Str("active"),
        };
        let filter = build(&expr);
        let must = filter.must.unwrap();
        assert!(matches!(&must[0],
            QdrantCondition::Match { key, value: FilterValue::Str(val) }
            if key == "status" && val == "active"
        ));
    }

    #[test]
    fn test_equals_int() {
        let expr = FilterExpr::Compare {
            field: "count",
            op: "=",
            value: Value::Int(42),
        };
        let filter = build(&expr);
        let must = filter.must.unwrap();
        assert!(matches!(&must[0],
            QdrantCondition::MatchKeywords { key, values }
            if key == "count" && values == &[FilterValue::Int(42)]
        ));
    }

    #[test]
    fn test_equals_float() {
        let expr = FilterExpr::Compare {
            field: "score",
            op: "=",
            value: Value::Float(3.14),
        };
        let filter = build(&expr);
        let must = filter.must.unwrap();
        assert!(matches!(&must[0],
            QdrantCondition::Range { key, gte: Some(g), lte: Some(l), .. }
            if key == "score" && (*g - 3.14).abs() < f64::EPSILON && (*l - 3.14).abs() < f64::EPSILON
        ));
    }

    #[test]
    fn test_equals_bool() {
        let expr = FilterExpr::Compare {
            field: "is_active",
            op: "=",
            value: Value::Bool(true),
        };
        let filter = build(&expr);
        let must = filter.must.unwrap();
        assert!(matches!(&must[0],
            QdrantCondition::Match { key, value: FilterValue::Bool(true) }
            if key == "is_active"
        ));
    }

    // ── Typed Inequality Tests ───────────────────────────────────

    #[test]
    fn test_not_equals_string() {
        let expr = FilterExpr::Compare {
            field: "status",
            op: "!=",
            value: Value::Str("archived"),
        };
        let filter = build(&expr);
        let must = filter.must.unwrap();
        assert!(matches!(&must[0],
            QdrantCondition::MatchExcept { key, value: FilterValue::Str(val) }
            if key == "status" && val == "archived"
        ));
    }

    #[test]
    fn test_not_equals_int() {
        let expr = FilterExpr::Compare {
            field: "count",
            op: "!=",
            value: Value::Int(7),
        };
        let filter = build(&expr);
        let must_not = filter.must_not.unwrap();
        assert!(matches!(&must_not[0],
            QdrantCondition::MatchKeywords { key, values }
            if key == "count" && values == &[FilterValue::Int(7)]
        ));
    }

    #[test]
    fn test_not_equals_float() {
        let expr = FilterExpr::Compare {
            field: "score",
            op: "!=",
            value: Value::Float(1.5),
        };
        let filter = build(&expr);
        let must_not = filter.must_not.unwrap();
        assert!(matches!(&must_not[0],
            QdrantCondition::Range { key, gte: Some(g), lte: Some(l), .. }
            if key == "score" && (*g - 1.5).abs() < f64::EPSILON && (*l - 1.5).abs() < f64::EPSILON
        ));
    }

    #[test]
    fn test_not_equals_bool() {
        let expr = FilterExpr::Compare {
            field: "is_active",
            op: "!=",
            value: Value::Bool(false),
        };
        let filter = build(&expr);
        let must_not = filter.must_not.unwrap();
        assert!(matches!(&must_not[0],
            QdrantCondition::Match { key, value: FilterValue::Bool(false) }
            if key == "is_active"
        ));
    }

    // ── Compare Operators ────────────────────────────────────────

    #[test]
    fn test_greater_than() {
        let expr = FilterExpr::Compare {
            field: "age",
            op: ">",
            value: Value::Int(18),
        };
        let filter = build(&expr);
        let must = filter.must.unwrap();
        assert!(matches!(&must[0],
            QdrantCondition::Range { key, gt: Some(_), .. } if key == "age"
        ));
    }

    #[test]
    fn test_greater_than_equal() {
        let expr = FilterExpr::Compare {
            field: "age",
            op: ">=",
            value: Value::Int(18),
        };
        let filter = build(&expr);
        let must = filter.must.unwrap();
        assert!(matches!(&must[0],
            QdrantCondition::Range { key, gte: Some(_), .. } if key == "age"
        ));
    }

    #[test]
    fn test_less_than() {
        let expr = FilterExpr::Compare {
            field: "price",
            op: "<",
            value: Value::Float(100.0),
        };
        let filter = build(&expr);
        let must = filter.must.unwrap();
        assert!(matches!(&must[0],
            QdrantCondition::Range { key, lt: Some(_), .. } if key == "price"
        ));
    }

    #[test]
    fn test_less_than_equal() {
        let expr = FilterExpr::Compare {
            field: "price",
            op: "<=",
            value: Value::Float(100.0),
        };
        let filter = build(&expr);
        let must = filter.must.unwrap();
        assert!(matches!(&must[0],
            QdrantCondition::Range { key, lte: Some(_), .. } if key == "price"
        ));
    }

    // ── Between ──────────────────────────────────────────────────

    #[test]
    fn test_between() {
        let expr = FilterExpr::Between {
            field: "age",
            low: Value::Int(18),
            high: Value::Int(65),
        };
        let filter = build(&expr);
        let must = filter.must.unwrap();
        assert!(matches!(&must[0],
            QdrantCondition::Range { key, gte: Some(_), lte: Some(_), .. } if key == "age"
        ));
    }

    // ── IN / NOT IN ──────────────────────────────────────────────

    #[test]
    fn test_in_strings() {
        let expr = FilterExpr::In {
            field: "status",
            values: vec![Value::Str("active"), Value::Str("pending")],
        };
        let filter = build(&expr);
        let must = filter.must.unwrap();
        assert!(matches!(&must[0],
            QdrantCondition::MatchKeywords { key, values }
            if key == "status" && values.len() == 2
        ));
    }

    #[test]
    fn test_in_ints() {
        let expr = FilterExpr::In {
            field: "count",
            values: vec![Value::Int(1), Value::Int(2)],
        };
        let filter = build(&expr);
        let should = filter.should.unwrap();
        assert_eq!(should.len(), 2);
        for cond in &should {
            assert!(
                matches!(cond, QdrantCondition::MatchKeywords { key, values } if key == "count" && values.len() == 1)
            );
        }
    }

    #[test]
    fn test_in_floats() {
        let expr = FilterExpr::In {
            field: "score",
            values: vec![Value::Float(1.25), Value::Float(2.5)],
        };
        let filter = build(&expr);
        let should = filter.should.unwrap();
        assert_eq!(should.len(), 2);
        for cond in &should {
            assert!(
                matches!(cond, QdrantCondition::Range { key, gte: Some(_), lte: Some(_), .. } if key == "score")
            );
        }
    }

    #[test]
    fn test_in_bools() {
        let expr = FilterExpr::In {
            field: "is_active",
            values: vec![Value::Bool(true), Value::Bool(false)],
        };
        let filter = build(&expr);
        let should = filter.should.unwrap();
        assert_eq!(should.len(), 2);
        assert!(
            matches!(&should[0], QdrantCondition::Match { key, value: FilterValue::Bool(true) } if key == "is_active")
        );
        assert!(
            matches!(&should[1], QdrantCondition::Match { key, value: FilterValue::Bool(false) } if key == "is_active")
        );
    }

    #[test]
    fn test_not_in_strings() {
        let expr = FilterExpr::NotIn {
            field: "status",
            values: vec![Value::Str("deleted"), Value::Str("archived")],
        };
        let filter = build(&expr);
        let must = filter.must.unwrap();
        assert!(matches!(&must[0],
            QdrantCondition::MatchExceptKeywords { key, values }
            if key == "status" && values.len() == 2
        ));
    }

    #[test]
    fn test_not_in_ints() {
        let expr = FilterExpr::NotIn {
            field: "count",
            values: vec![Value::Int(3), Value::Int(4)],
        };
        let filter = build(&expr);
        let must_not = filter.must_not.unwrap();
        assert_eq!(must_not.len(), 2);
        for cond in &must_not {
            assert!(
                matches!(cond, QdrantCondition::MatchKeywords { key, values } if key == "count" && values.len() == 1)
            );
        }
    }

    #[test]
    fn test_not_in_floats() {
        let expr = FilterExpr::NotIn {
            field: "score",
            values: vec![Value::Float(4.5), Value::Float(9.0)],
        };
        let filter = build(&expr);
        let must_not = filter.must_not.unwrap();
        assert_eq!(must_not.len(), 2);
        for cond in &must_not {
            assert!(
                matches!(cond, QdrantCondition::Range { key, gte: Some(_), lte: Some(_), .. } if key == "score")
            );
        }
    }

    #[test]
    fn test_not_in_bools() {
        let expr = FilterExpr::NotIn {
            field: "is_active",
            values: vec![Value::Bool(true), Value::Bool(false)],
        };
        let filter = build(&expr);
        let must_not = filter.must_not.unwrap();
        assert_eq!(must_not.len(), 2);
        assert!(
            matches!(&must_not[0], QdrantCondition::Match { key, value: FilterValue::Bool(true) } if key == "is_active")
        );
        assert!(
            matches!(&must_not[1], QdrantCondition::Match { key, value: FilterValue::Bool(false) } if key == "is_active")
        );
    }

    #[test]
    fn test_rejects_mixed_in_types() {
        let expr = FilterExpr::In {
            field: "mixed",
            values: vec![Value::Str("active"), Value::Int(1)],
        };
        let result = FilterConverter.build_filter(&expr);
        assert!(result.is_err());
    }

    // ── Null / Empty ─────────────────────────────────────────────

    #[test]
    fn test_is_null() {
        let expr = FilterExpr::IsNull {
            field: "deleted_at",
        };
        let filter = build(&expr);
        let must = filter.must.unwrap();
        assert!(matches!(&must[0],
            QdrantCondition::IsNull { key } if key == "deleted_at"
        ));
    }

    #[test]
    fn test_is_not_null() {
        let expr = FilterExpr::IsNotNull { field: "email" };
        let filter = build(&expr);
        let must_not = filter.must_not.unwrap();
        assert!(matches!(&must_not[0],
            QdrantCondition::IsNull { key } if key == "email"
        ));
    }

    #[test]
    fn test_is_empty() {
        let expr = FilterExpr::IsEmpty { field: "tags" };
        let filter = build(&expr);
        let must = filter.must.unwrap();
        assert!(matches!(&must[0],
            QdrantCondition::IsEmpty { key } if key == "tags"
        ));
    }

    #[test]
    fn test_is_not_empty() {
        let expr = FilterExpr::IsNotEmpty { field: "tags" };
        let filter = build(&expr);
        let must_not = filter.must_not.unwrap();
        assert!(matches!(&must_not[0],
            QdrantCondition::IsEmpty { key } if key == "tags"
        ));
    }

    // ── Match Expressions ────────────────────────────────────────

    #[test]
    fn test_match_text() {
        let expr = FilterExpr::MatchText {
            field: "content",
            text: "hello world",
        };
        let filter = build(&expr);
        let must = filter.must.unwrap();
        assert!(matches!(&must[0],
            QdrantCondition::MatchText { key, text }
            if key == "content" && text == "hello world"
        ));
    }

    #[test]
    fn test_match_any() {
        let expr = FilterExpr::MatchAny {
            field: "content",
            text: "hello world",
        };
        let filter = build(&expr);
        let must = filter.must.unwrap();
        assert!(matches!(&must[0],
            QdrantCondition::MatchAny { key, text }
            if key == "content" && text == "hello world"
        ));
    }

    #[test]
    fn test_match_phrase() {
        let expr = FilterExpr::MatchPhrase {
            field: "content",
            text: "hello world",
        };
        let filter = build(&expr);
        let must = filter.must.unwrap();
        assert!(matches!(&must[0],
            QdrantCondition::MatchPhrase { key, text }
            if key == "content" && text == "hello world"
        ));
    }

    // ── Logical Expressions ──────────────────────────────────────

    #[test]
    fn test_and() {
        let expr = FilterExpr::And {
            operands: vec![
                FilterExpr::Compare {
                    field: "a",
                    op: "=",
                    value: Value::Int(1),
                },
                FilterExpr::Compare {
                    field: "b",
                    op: "=",
                    value: Value::Int(2),
                },
            ],
        };
        let filter = build(&expr);
        let must = filter.must.unwrap();
        assert_eq!(must.len(), 2);
    }

    #[test]
    fn test_or() {
        let expr = FilterExpr::Or {
            operands: vec![
                FilterExpr::Compare {
                    field: "a",
                    op: "=",
                    value: Value::Int(1),
                },
                FilterExpr::Compare {
                    field: "b",
                    op: "=",
                    value: Value::Int(2),
                },
            ],
        };
        let filter = build(&expr);
        let should = filter.should.unwrap();
        assert_eq!(should.len(), 2);
    }

    #[test]
    fn test_not() {
        let expr = FilterExpr::Not {
            operand: Box::new(FilterExpr::Compare {
                field: "x",
                op: "=",
                value: Value::Bool(true),
            }),
        };
        let filter = build(&expr);
        let must_not = filter.must_not.unwrap();
        assert_eq!(must_not.len(), 1);
    }

    #[test]
    fn test_logical_expressions() {
        let expr = FilterExpr::Or {
            operands: vec![
                FilterExpr::And {
                    operands: vec![
                        FilterExpr::Compare {
                            field: "status",
                            op: "=",
                            value: Value::Str("active"),
                        },
                        FilterExpr::Between {
                            field: "score",
                            low: Value::Int(1),
                            high: Value::Int(5),
                        },
                    ],
                },
                FilterExpr::Not {
                    operand: Box::new(FilterExpr::IsNull { field: "category" }),
                },
            ],
        };
        let filter = build(&expr);
        let should = filter.should.unwrap();
        assert_eq!(should.len(), 2);
        assert!(filter.must_not.is_none() || filter.must_not.as_ref().unwrap().is_empty());

        // Left operand: And -> Boolean with must
        match &should[0] {
            QdrantCondition::Boolean(b) => {
                let inner_must = b.must.as_ref().unwrap();
                assert_eq!(inner_must.len(), 2);
                assert!(matches!(&inner_must[0], QdrantCondition::Match { .. }));
                assert!(
                    matches!(&inner_must[1], QdrantCondition::Range { key, .. } if key == "score")
                );
            }
            _ => panic!("expected Boolean for AND"),
        }

        // Right operand: Not(IsNull) -> Boolean with must_not
        match &should[1] {
            QdrantCondition::Boolean(b) => {
                let inner_must_not = b.must_not.as_ref().unwrap();
                assert_eq!(inner_must_not.len(), 1);
                assert!(
                    matches!(&inner_must_not[0], QdrantCondition::IsNull { key } if key == "category")
                );
            }
            _ => panic!("expected Boolean for NOT"),
        }
    }

    #[test]
    fn test_complex_nested_expression() {
        let expr = FilterExpr::And {
            operands: vec![
                FilterExpr::Compare {
                    field: "org_id",
                    op: "=",
                    value: Value::Str("acme"),
                },
                FilterExpr::Or {
                    operands: vec![
                        FilterExpr::Compare {
                            field: "role",
                            op: "=",
                            value: Value::Str("admin"),
                        },
                        FilterExpr::Compare {
                            field: "role",
                            op: "=",
                            value: Value::Str("owner"),
                        },
                    ],
                },
            ],
        };
        let filter = build(&expr);
        let must = filter.must.unwrap();
        assert_eq!(must.len(), 2);
        // First condition is Match for org_id
        assert!(matches!(&must[0], QdrantCondition::Match { .. }));
        // Second condition is Boolean with Should
        assert!(matches!(&must[1], QdrantCondition::Boolean(b) if b.should.is_some()));
    }

    // ── Nested ───────────────────────────────────────────────────

    #[test]
    fn test_nested() {
        let expr = FilterExpr::Nested {
            path: "overwritten_in",
            filter: Box::new(FilterExpr::And {
                operands: vec![
                    FilterExpr::Compare {
                        field: "by",
                        op: "=",
                        value: Value::Str("root"),
                    },
                    FilterExpr::Compare {
                        field: "seq",
                        op: "<=",
                        value: Value::Float(2.0),
                    },
                ],
            }),
        };
        let filter = build(&expr);
        let must = filter.must.unwrap();
        assert!(matches!(&must[0],
            QdrantCondition::Nested { key, filter: inner }
            if key == "overwritten_in"
        ));
        if let QdrantCondition::Nested {
            key: _,
            filter: inner,
        } = &must[0]
        {
            let inner_must = inner.must.as_ref().unwrap();
            assert_eq!(inner_must.len(), 2);
            assert!(matches!(&inner_must[0], QdrantCondition::Match { key, .. } if key == "by"));
            assert!(matches!(&inner_must[1], QdrantCondition::Range { key, .. } if key == "seq"));
        }
    }

    #[test]
    fn test_nested_simple() {
        let expr = FilterExpr::Nested {
            path: "tags",
            filter: Box::new(FilterExpr::Compare {
                field: "name",
                op: "=",
                value: Value::Str("important"),
            }),
        };
        let filter = build(&expr);
        let must = filter.must.unwrap();
        assert!(matches!(&must[0],
            QdrantCondition::Nested { key, .. } if key == "tags"
        ));
        if let QdrantCondition::Nested {
            key: _,
            filter: inner,
        } = &must[0]
        {
            let inner_must = inner.must.as_ref().unwrap();
            assert_eq!(inner_must.len(), 1);
            assert!(
                matches!(&inner_must[0], QdrantCondition::Match { key, value: FilterValue::Str(val) } if key == "name" && val == "important")
            );
        }
    }

    #[test]
    fn test_nested_with_must_not() {
        let expr = FilterExpr::Not {
            operand: Box::new(FilterExpr::Nested {
                path: "overwritten_in",
                filter: Box::new(FilterExpr::Compare {
                    field: "by",
                    op: "=",
                    value: Value::Str("root"),
                }),
            }),
        };
        let filter = build(&expr);
        let must_not = filter.must_not.unwrap();
        assert_eq!(must_not.len(), 1);
        assert!(matches!(&must_not[0],
            QdrantCondition::Nested { key, .. } if key == "overwritten_in"
        ));
    }

    // ── Basic conversion test ────────────────────────────────────

    #[test]
    fn test_basic_conversion() {
        let expr = FilterExpr::Compare {
            field: "x",
            op: "=",
            value: Value::Int(0),
        };
        let result = FilterConverter.build_filter(&expr);
        assert!(result.is_ok());
    }
}
