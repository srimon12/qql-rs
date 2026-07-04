use super::build;
use crate::filter_conv::*;
use qql_core::ast::{FilterExpr, Value};
use std::boxed::Box;

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
                        value: Value::Str(std::borrow::Cow::Borrowed("active")),
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
            assert!(matches!(&inner_must[1], QdrantCondition::Range { key, .. } if key == "score"));
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
                value: Value::Str(std::borrow::Cow::Borrowed("acme")),
            },
            FilterExpr::Or {
                operands: vec![
                    FilterExpr::Compare {
                        field: "role",
                        op: "=",
                        value: Value::Str(std::borrow::Cow::Borrowed("admin")),
                    },
                    FilterExpr::Compare {
                        field: "role",
                        op: "=",
                        value: Value::Str(std::borrow::Cow::Borrowed("owner")),
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

#[test]
fn test_nested() {
    let expr = FilterExpr::Nested {
        path: "overwritten_in",
        filter: Box::new(FilterExpr::And {
            operands: vec![
                FilterExpr::Compare {
                    field: "by",
                    op: "=",
                    value: Value::Str(std::borrow::Cow::Borrowed("root")),
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
        QdrantCondition::Nested { key, filter: _inner }
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
            value: Value::Str(std::borrow::Cow::Borrowed("important")),
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
                value: Value::Str(std::borrow::Cow::Borrowed("root")),
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
