#[cfg(test)]
mod tests {
    use crate::ast::*;
    use alloc::boxed::Box;
    use alloc::format;
    use alloc::vec;
    use alloc::vec::Vec;

    #[test]
    fn test_filter_compare() {
        let f = FilterExpr::Compare {
            field: "age",
            op: ">",
            value: Value::Int(18),
        };
        match f {
            FilterExpr::Compare { field, op, value } => {
                assert_eq!(field, "age");
                assert_eq!(op, ">");
                assert_eq!(value, Value::Int(18));
            }
            _ => panic!("expected Compare"),
        }
    }

    #[test]
    fn test_filter_between() {
        let f = FilterExpr::Between {
            field: "age",
            low: Value::Int(18),
            high: Value::Int(65),
        };
        match f {
            FilterExpr::Between { field, low, high } => {
                assert_eq!(field, "age");
                assert_eq!(low, Value::Int(18));
                assert_eq!(high, Value::Int(65));
            }
            _ => panic!("expected Between"),
        }
    }

    #[test]
    fn test_filter_in() {
        let f = FilterExpr::In {
            field: "color",
            values: vec![Value::Str("red"), Value::Str("blue")],
        };
        match f {
            FilterExpr::In { field, values } => {
                assert_eq!(field, "color");
                assert_eq!(values.len(), 2);
            }
            _ => panic!("expected In"),
        }
    }

    #[test]
    fn test_filter_not_in() {
        let f = FilterExpr::NotIn {
            field: "status",
            values: vec![Value::Str("deleted")],
        };
        match f {
            FilterExpr::NotIn { field, values } => {
                assert_eq!(field, "status");
                assert_eq!(values.len(), 1);
            }
            _ => panic!("expected NotIn"),
        }
    }

    #[test]
    fn test_filter_is_null() {
        let f = FilterExpr::IsNull {
            field: "deleted_at",
        };
        match f {
            FilterExpr::IsNull { field } => assert_eq!(field, "deleted_at"),
            _ => panic!("expected IsNull"),
        }
    }

    #[test]
    fn test_filter_is_not_null() {
        let f = FilterExpr::IsNotNull { field: "email" };
        match f {
            FilterExpr::IsNotNull { field } => assert_eq!(field, "email"),
            _ => panic!("expected IsNotNull"),
        }
    }

    #[test]
    fn test_filter_is_empty() {
        let f = FilterExpr::IsEmpty { field: "tags" };
        match f {
            FilterExpr::IsEmpty { field } => assert_eq!(field, "tags"),
            _ => panic!("expected IsEmpty"),
        }
    }

    #[test]
    fn test_filter_is_not_empty() {
        let f = FilterExpr::IsNotEmpty { field: "tags" };
        match f {
            FilterExpr::IsNotEmpty { field } => assert_eq!(field, "tags"),
            _ => panic!("expected IsNotEmpty"),
        }
    }

    #[test]
    fn test_filter_match_text() {
        let f = FilterExpr::MatchText {
            field: "title",
            text: "hello",
        };
        match f {
            FilterExpr::MatchText { field, text } => {
                assert_eq!(field, "title");
                assert_eq!(text, "hello");
            }
            _ => panic!("expected MatchText"),
        }
    }

    #[test]
    fn test_filter_match_any() {
        let f = FilterExpr::MatchAny {
            field: "title",
            text: "hello",
        };
        match f {
            FilterExpr::MatchAny { field, text } => {
                assert_eq!(field, "title");
                assert_eq!(text, "hello");
            }
            _ => panic!("expected MatchAny"),
        }
    }

    #[test]
    fn test_filter_match_phrase() {
        let f = FilterExpr::MatchPhrase {
            field: "title",
            text: "hello world",
        };
        match f {
            FilterExpr::MatchPhrase { field, text } => {
                assert_eq!(field, "title");
                assert_eq!(text, "hello world");
            }
            _ => panic!("expected MatchPhrase"),
        }
    }

    #[test]
    fn test_filter_and_or() {
        let f = FilterExpr::And {
            operands: vec![
                FilterExpr::Compare {
                    field: "a",
                    op: "=",
                    value: Value::Int(1),
                },
                FilterExpr::Or {
                    operands: vec![
                        FilterExpr::Compare {
                            field: "b",
                            op: "=",
                            value: Value::Int(2),
                        },
                        FilterExpr::Compare {
                            field: "c",
                            op: "=",
                            value: Value::Int(3),
                        },
                    ],
                },
            ],
        };
        match f {
            FilterExpr::And { operands } => {
                assert_eq!(operands.len(), 2);
            }
            _ => panic!("expected And"),
        }
    }

    #[test]
    fn test_filter_not() {
        let f = FilterExpr::Not {
            operand: Box::new(FilterExpr::Compare {
                field: "x",
                op: "=",
                value: Value::Bool(true),
            }),
        };
        match f {
            FilterExpr::Not { operand } => match *operand {
                FilterExpr::Compare { field, .. } => assert_eq!(field, "x"),
                _ => panic!("expected Compare inside Not"),
            },
            _ => panic!("expected Not"),
        }
    }

    #[test]
    fn test_filter_nested() {
        let f = FilterExpr::Nested {
            path: "address",
            filter: Box::new(FilterExpr::Compare {
                field: "city",
                op: "=",
                value: Value::Str("NYC"),
            }),
        };
        match f {
            FilterExpr::Nested { path, .. } => assert_eq!(path, "address"),
            _ => panic!("expected Nested"),
        }
    }

    #[test]
    fn test_filter_nested_in_not() {
        let f = FilterExpr::Not {
            operand: Box::new(FilterExpr::Nested {
                path: "tags",
                filter: Box::new(FilterExpr::Compare {
                    field: "name",
                    op: "=",
                    value: Value::Str("important"),
                }),
            }),
        };
        match f {
            FilterExpr::Not { operand } => match *operand {
                FilterExpr::Nested { path, .. } => assert_eq!(path, "tags"),
                _ => panic!("expected Nested inside Not"),
            },
            _ => panic!("expected Not"),
        }
    }

    #[test]
    fn test_filter_debug() {
        let f = FilterExpr::Compare {
            field: "age",
            op: ">=",
            value: Value::Int(18),
        };
        let debug = format!("{:?}", f);
        assert!(debug.contains("age"));
        assert!(debug.contains(">="));
        assert!(debug.contains("18"));
    }

    #[test]
    fn test_filter_clone_equal() {
        let f1 = FilterExpr::Compare {
            field: "x",
            op: "=",
            value: Value::Str("y"),
        };
        let f2 = f1.clone();
        assert_eq!(f1, f2);
    }

    #[test]
    fn test_formula_constant() {
        let f = FormulaExpr::Constant { value: 42.0 };
        match f {
            FormulaExpr::Constant { value } => assert!((value - 42.0).abs() < f64::EPSILON),
            _ => panic!("expected Constant"),
        }
    }

    #[test]
    fn test_formula_variable() {
        let f = FormulaExpr::Variable { name: "$score" };
        match f {
            FormulaExpr::Variable { name } => assert_eq!(name, "$score"),
            _ => panic!("expected Variable"),
        }
    }

    #[test]
    fn test_formula_sum() {
        let f = FormulaExpr::Sum {
            left: Box::new(FormulaExpr::Constant { value: 1.0 }),
            right: Box::new(FormulaExpr::Variable { name: "$x" }),
        };
        match f {
            FormulaExpr::Sum { left, right } => {
                assert!(
                    matches!(*left, FormulaExpr::Constant { value } if (value - 1.0).abs() < f64::EPSILON)
                );
                assert!(matches!(*right, FormulaExpr::Variable { name } if name == "$x"));
            }
            _ => panic!("expected Sum"),
        }
    }

    #[test]
    fn test_formula_sub() {
        let f = FormulaExpr::Sub {
            left: Box::new(FormulaExpr::Variable { name: "$a" }),
            right: Box::new(FormulaExpr::Variable { name: "$b" }),
        };
        match f {
            FormulaExpr::Sub { .. } => {}
            _ => panic!("expected Sub"),
        }
    }

    #[test]
    fn test_formula_mul() {
        let f = FormulaExpr::Mul {
            left: Box::new(FormulaExpr::Variable { name: "$score" }),
            right: Box::new(FormulaExpr::Constant { value: 2.0 }),
        };
        match f {
            FormulaExpr::Mul { left, right } => {
                match *left {
                    FormulaExpr::Variable { name } => assert_eq!(name, "$score"),
                    _ => panic!("expected Variable"),
                }
                match *right {
                    FormulaExpr::Constant { value } => assert!((value - 2.0).abs() < f64::EPSILON),
                    _ => panic!("expected Constant"),
                }
            }
            _ => panic!("expected Mul"),
        }
    }

    #[test]
    fn test_formula_div() {
        let f = FormulaExpr::Div {
            left: Box::new(FormulaExpr::Constant { value: 10.0 }),
            right: Box::new(FormulaExpr::Constant { value: 3.0 }),
            by_zero_default: Some(0.0),
        };
        match f {
            FormulaExpr::Div {
                left,
                right,
                by_zero_default,
            } => {
                assert!(matches!(*left, FormulaExpr::Constant { .. }));
                assert!(matches!(*right, FormulaExpr::Constant { .. }));
                assert_eq!(by_zero_default, Some(0.0));
            }
            _ => panic!("expected Div"),
        }
    }

    #[test]
    fn test_formula_neg() {
        let f = FormulaExpr::Neg {
            operand: Box::new(FormulaExpr::Constant { value: 5.0 }),
        };
        match f {
            FormulaExpr::Neg { operand } => {
                assert!(matches!(*operand, FormulaExpr::Constant { .. }));
            }
            _ => panic!("expected Neg"),
        }
    }

    #[test]
    fn test_formula_unary_functions() {
        let functions: Vec<FormulaExpr> = vec![
            FormulaExpr::Abs {
                x: Box::new(FormulaExpr::Constant { value: -5.0 }),
            },
            FormulaExpr::Sqrt {
                x: Box::new(FormulaExpr::Constant { value: 9.0 }),
            },
            FormulaExpr::Log {
                x: Box::new(FormulaExpr::Constant { value: 100.0 }),
            },
            FormulaExpr::Ln {
                x: Box::new(FormulaExpr::Constant { value: 2.0 }),
            },
            FormulaExpr::Exp {
                x: Box::new(FormulaExpr::Constant { value: 1.0 }),
            },
        ];
        assert_eq!(functions.len(), 5);
    }

    #[test]
    fn test_formula_abs() {
        let f = FormulaExpr::Abs {
            x: Box::new(FormulaExpr::Constant { value: -5.0 }),
        };
        match f {
            FormulaExpr::Abs { x } => match *x {
                FormulaExpr::Constant { value } => assert!((value + 5.0).abs() < f64::EPSILON),
                _ => panic!("expected Constant"),
            },
            _ => panic!("expected Abs"),
        }
    }

    #[test]
    fn test_formula_pow() {
        let f = FormulaExpr::Pow {
            base: Box::new(FormulaExpr::Constant { value: 2.0 }),
            exponent: Box::new(FormulaExpr::Constant { value: 3.0 }),
        };
        match f {
            FormulaExpr::Pow { base, exponent } => {
                assert!(
                    matches!(*base, FormulaExpr::Constant { value } if (value - 2.0).abs() < f64::EPSILON)
                );
                assert!(
                    matches!(*exponent, FormulaExpr::Constant { value } if (value - 3.0).abs() < f64::EPSILON)
                );
            }
            _ => panic!("expected Pow"),
        }
    }

    #[test]
    fn test_formula_geo_distance() {
        let f = FormulaExpr::GeoDistance {
            lat: 40.7,
            lon: -74.0,
            field: "location",
        };
        match f {
            FormulaExpr::GeoDistance { lat, lon, field } => {
                assert!((lat - 40.7).abs() < f64::EPSILON);
                assert!((lon + 74.0).abs() < f64::EPSILON);
                assert_eq!(field, "location");
            }
            _ => panic!("expected GeoDistance"),
        }
    }

    #[test]
    fn test_formula_decay() {
        let f = FormulaExpr::Decay {
            kind: "gauss_decay",
            x: Box::new(FormulaExpr::Variable { name: "age" }),
            target: Some(Box::new(FormulaExpr::Constant { value: 30.0 })),
            scale: Some(10.0),
            midpoint: None,
        };
        match f {
            FormulaExpr::Decay { kind, .. } => assert_eq!(kind, "gauss_decay"),
            _ => panic!("expected Decay"),
        }
    }

    #[test]
    fn test_formula_case() {
        let f = FormulaExpr::Case {
            cond: Box::new(FilterExpr::Compare {
                field: "age",
                op: ">",
                value: Value::Int(18),
            }),
            then_: Box::new(FormulaExpr::Constant { value: 1.0 }),
            else_: Box::new(FormulaExpr::Constant { value: 0.0 }),
        };
        match f {
            FormulaExpr::Case { .. } => {}
            _ => panic!("expected Case"),
        }
    }

    #[test]
    fn test_formula_match_condition() {
        let f = FormulaExpr::MatchCondition {
            field: "status",
            values: vec![Value::Str("active")],
        };
        match f {
            FormulaExpr::MatchCondition { field, values } => {
                assert_eq!(field, "status");
                assert_eq!(values.len(), 1);
            }
            _ => panic!("expected MatchCondition"),
        }
    }

    #[test]
    fn test_formula_datetime() {
        let f = FormulaExpr::Datetime {
            value: "2024-01-01",
        };
        match f {
            FormulaExpr::Datetime { value } => assert_eq!(value, "2024-01-01"),
            _ => panic!("expected Datetime"),
        }
    }

    #[test]
    fn test_formula_datetime_key() {
        let f = FormulaExpr::DatetimeKey { key: "created_at" };
        match f {
            FormulaExpr::DatetimeKey { key } => assert_eq!(key, "created_at"),
            _ => panic!("expected DatetimeKey"),
        }
    }

    #[test]
    fn test_formula_debug() {
        let f = FormulaExpr::Constant { value: 12.34 };
        let debug = format!("{:?}", f);
        assert!(debug.contains("12.34"));
    }

    #[test]
    fn test_value_variants() {
        let values = [
            Value::Str("hello"),
            Value::Int(42),
            Value::Float(12.34),
            Value::Bool(true),
            Value::Null,
        ];
        assert_eq!(values.len(), 5);
    }

    #[test]
    fn test_value_debug() {
        assert_eq!(format!("{:?}", Value::Str("hi")), "Str(\"hi\")");
        assert_eq!(format!("{:?}", Value::Int(7)), "Int(7)");
        assert_eq!(format!("{:?}", Value::Bool(false)), "Bool(false)");
        assert_eq!(format!("{:?}", Value::Null), "Null");
    }

    #[test]
    fn test_value_clone_equal() {
        assert_eq!(Value::Int(1).clone(), Value::Int(1));
        assert_ne!(Value::Int(1), Value::Int(2));
    }

    #[test]
    fn test_query_mode_debug() {
        assert_eq!(format!("{:?}", QueryMode::Nearest), "Nearest");
        assert_eq!(format!("{:?}", QueryMode::Recommend), "Recommend");
        assert_eq!(format!("{:?}", QueryMode::Discover), "Discover");
        assert_eq!(format!("{:?}", QueryMode::OrderBy), "OrderBy");
        assert_eq!(format!("{:?}", QueryMode::Sample), "Sample");
        assert_eq!(format!("{:?}", QueryMode::Context), "Context");
    }

    #[test]
    fn test_query_type_debug() {
        assert_eq!(format!("{:?}", QueryType::Dense), "Dense");
        assert_eq!(format!("{:?}", QueryType::Sparse), "Sparse");
        assert_eq!(format!("{:?}", QueryType::Hybrid), "Hybrid");
    }

    fn dummy_query_stmt() -> QueryStmt<'static> {
        QueryStmt {
            collection: None,
            mode: QueryMode::Nearest,
            query_type: QueryType::Dense,
            query_text: None,
            query_id: None,
            raw_vector: Vec::new(),
            positive_ids: Vec::new(),
            negative_ids: Vec::new(),
            context_pairs: Vec::new(),
            target: None,
            order_by_field: None,
            order_by_asc: None,
            limit: 10,
            offset: 0,
            score_threshold: None,
            strategy: None,
            query_filter: None,
            group_by: None,
            group_size: None,
            with_clause: None,
            with_payload: None,
            with_vectors: None,
            lookup_from: None,
            lookup_vector: None,
            with_lookup_collection: None,
            using_: None,
            model: None,
            ctes: Vec::new(),
            prefetch_refs: Vec::new(),
            fusion_type: None,
            rerank: false,
            rerank_model: None,
            formula: None,
            formula_defaults: Vec::new(),
            feedback_target: None,
            feedback_items: Vec::new(),
            feedback_strategy: None,
        }
    }

    #[test]
    fn test_ast_inject_filter() {
        let mut inner_query = dummy_query_stmt();
        inner_query.collection = Some("docs");
        inner_query.limit = 5;
        inner_query.query_filter = Some(Box::new(FilterExpr::Compare {
            field: "status",
            op: "=",
            value: Value::Str("published"),
        }));

        let inner_cte = CTE {
            name: "prefetch_1",
            stmt: Box::new(inner_query),
        };

        let mut query = dummy_query_stmt();
        query.collection = Some("docs");
        query.limit = 10;
        query.ctes = vec![inner_cte];

        let mut stmt = Stmt::Query(Box::new(query));

        inject_filter(&mut stmt, "org_id", "=", &Value::Str("acme-corp"));

        if let Stmt::Query(q) = stmt {
            let main_filter = q.query_filter.unwrap();
            assert_eq!(
                *main_filter,
                FilterExpr::Compare {
                    field: "org_id",
                    op: "=",
                    value: Value::Str("acme-corp"),
                }
            );

            let cte_stmt = &q.ctes[0].stmt;
            let cte_filter = cte_stmt.query_filter.as_ref().unwrap();
            match &**cte_filter {
                FilterExpr::And { operands } => {
                    assert_eq!(operands.len(), 2);
                    assert_eq!(
                        operands[0],
                        FilterExpr::Compare {
                            field: "status",
                            op: "=",
                            value: Value::Str("published"),
                        }
                    );
                    assert_eq!(
                        operands[1],
                        FilterExpr::Compare {
                            field: "org_id",
                            op: "=",
                            value: Value::Str("acme-corp"),
                        }
                    );
                }
                _ => panic!("expected And filter"),
            }
        } else {
            panic!("expected Stmt::Query");
        }
    }
}
