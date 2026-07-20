use super::build;
use crate::ast::{FilterExpr, Value};
use std::boxed::Box;

#[test]
fn test_and() {
    let expr = FilterExpr::And {
        operands: vec![
            FilterExpr::Compare {
                field: String::from("a"),
                op: String::from("="),
                value: Value::Int(1),
            },
            FilterExpr::Compare {
                field: String::from("b"),
                op: String::from("="),
                value: Value::Int(2),
            },
        ],
    };
    let filter = build(&expr);
    assert_eq!(
        filter,
        serde_json::json!({
            "must": [
                { "key": "a", "match": { "value": 1 } },
                { "key": "b", "match": { "value": 2 } }
            ]
        })
    );
}

#[test]
fn test_or() {
    let expr = FilterExpr::Or {
        operands: vec![
            FilterExpr::Compare {
                field: String::from("a"),
                op: String::from("="),
                value: Value::Int(1),
            },
            FilterExpr::Compare {
                field: String::from("b"),
                op: String::from("="),
                value: Value::Int(2),
            },
        ],
    };
    let filter = build(&expr);
    assert_eq!(
        filter,
        serde_json::json!({
            "should": [
                { "key": "a", "match": { "value": 1 } },
                { "key": "b", "match": { "value": 2 } }
            ]
        })
    );
}

#[test]
fn test_not() {
    let expr = FilterExpr::Not {
        operand: Box::new(FilterExpr::Compare {
            field: String::from("x"),
            op: String::from("="),
            value: Value::Bool(true),
        }),
    };
    let filter = build(&expr);
    assert_eq!(
        filter,
        serde_json::json!({
            "must_not": [
                { "key": "x", "match": { "value": true } }
            ]
        })
    );
}

#[test]
fn test_logical_expressions() {
    let expr = FilterExpr::Or {
        operands: vec![
            FilterExpr::And {
                operands: vec![
                    FilterExpr::Compare {
                        field: String::from("status"),
                        op: String::from("="),
                        value: Value::Str(String::from("active")),
                    },
                    FilterExpr::Between {
                        field: String::from("score"),
                        low: Value::Int(1),
                        high: Value::Int(5),
                    },
                ],
            },
            FilterExpr::Not {
                operand: Box::new(FilterExpr::IsNull {
                    field: String::from("category"),
                }),
            },
        ],
    };
    let filter = build(&expr);
    assert_eq!(
        filter,
        serde_json::json!({
            "should": [
                {
                    "must": [
                        { "key": "status", "match": { "value": "active" } },
                        { "key": "score", "range": { "gte": 1.0, "lte": 5.0 } }
                    ]
                },
                {
                    "must_not": [
                        { "is_null": { "key": "category" } }
                    ]
                }
            ]
        })
    );
}

#[test]
fn test_complex_nested_expression() {
    let expr = FilterExpr::And {
        operands: vec![
            FilterExpr::Compare {
                field: String::from("org_id"),
                op: String::from("="),
                value: Value::Str(String::from("acme")),
            },
            FilterExpr::Or {
                operands: vec![
                    FilterExpr::Compare {
                        field: String::from("role"),
                        op: String::from("="),
                        value: Value::Str(String::from("admin")),
                    },
                    FilterExpr::Compare {
                        field: String::from("role"),
                        op: String::from("="),
                        value: Value::Str(String::from("owner")),
                    },
                ],
            },
        ],
    };
    let filter = build(&expr);
    assert_eq!(
        filter,
        serde_json::json!({
            "must": [
                { "key": "org_id", "match": { "value": "acme" } },
                {
                    "should": [
                        { "key": "role", "match": { "value": "admin" } },
                        { "key": "role", "match": { "value": "owner" } }
                    ]
                }
            ]
        })
    );
}

#[test]
fn test_nested() {
    let expr = FilterExpr::Nested {
        path: String::from("overwritten_in"),
        filter: Box::new(FilterExpr::And {
            operands: vec![
                FilterExpr::Compare {
                    field: String::from("by"),
                    op: String::from("="),
                    value: Value::Str(String::from("root")),
                },
                FilterExpr::Compare {
                    field: String::from("seq"),
                    op: String::from("<="),
                    value: Value::Float(2.0),
                },
            ],
        }),
    };
    let filter = build(&expr);
    assert_eq!(
        filter,
        serde_json::json!({
            "must": [
                {
                    "nested": {
                        "key": "overwritten_in",
                        "filter": {
                            "must": [
                                { "key": "by", "match": { "value": "root" } },
                                { "key": "seq", "range": { "lte": 2.0 } }
                            ]
                        }
                    }
                }
            ]
        })
    );
}

#[test]
fn test_nested_simple() {
    let expr = FilterExpr::Nested {
        path: String::from("tags"),
        filter: Box::new(FilterExpr::Compare {
            field: String::from("name"),
            op: String::from("="),
            value: Value::Str(String::from("important")),
        }),
    };
    let filter = build(&expr);
    assert_eq!(
        filter,
        serde_json::json!({
            "must": [
                {
                    "nested": {
                        "key": "tags",
                        "filter": {
                            "must": [
                                { "key": "name", "match": { "value": "important" } }
                            ]
                        }
                    }
                }
            ]
        })
    );
}

#[test]
fn test_nested_with_must_not() {
    let expr = FilterExpr::Not {
        operand: Box::new(FilterExpr::Nested {
            path: String::from("overwritten_in"),
            filter: Box::new(FilterExpr::Compare {
                field: String::from("by"),
                op: String::from("="),
                value: Value::Str(String::from("root")),
            }),
        }),
    };
    let filter = build(&expr);
    assert_eq!(
        filter,
        serde_json::json!({
            "must_not": [
                {
                    "nested": {
                        "key": "overwritten_in",
                        "filter": {
                            "must": [
                                { "key": "by", "match": { "value": "root" } }
                            ]
                        }
                    }
                }
            ]
        })
    );
}
