use super::build;
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
            field: "x",
            op: "=",
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
        path: "tags",
        filter: Box::new(FilterExpr::Compare {
            field: "name",
            op: "=",
            value: Value::Str(std::borrow::Cow::Borrowed("important")),
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
            path: "overwritten_in",
            filter: Box::new(FilterExpr::Compare {
                field: "by",
                op: "=",
                value: Value::Str(std::borrow::Cow::Borrowed("root")),
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
