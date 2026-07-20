use super::build;
use crate::ast::{FilterExpr, Value};
use crate::filter_conv::FilterConverter;

#[test]
fn test_in_strings() {
    let expr = FilterExpr::In {
        field: String::from("status"),
        values: vec![
            Value::Str(String::from("active")),
            Value::Str(String::from("pending")),
        ],
    };
    let filter = build(&expr);
    assert_eq!(
        filter,
        serde_json::json!({
            "must": [
                {
                    "key": "status",
                    "match": {
                        "any": ["active", "pending"]
                    }
                }
            ]
        })
    );
}

#[test]
fn test_in_ints() {
    let expr = FilterExpr::In {
        field: String::from("count"),
        values: vec![Value::Int(1), Value::Int(2)],
    };
    let filter = build(&expr);
    assert_eq!(
        filter,
        serde_json::json!({
            "should": [
                { "key": "count", "match": { "value": 1 } },
                { "key": "count", "match": { "value": 2 } }
            ]
        })
    );
}

#[test]
fn test_in_floats() {
    let expr = FilterExpr::In {
        field: String::from("score"),
        values: vec![Value::Float(1.25), Value::Float(2.5)],
    };
    let filter = build(&expr);
    assert_eq!(
        filter,
        serde_json::json!({
            "should": [
                { "key": "score", "range": { "gte": 1.25, "lte": 1.25 } },
                { "key": "score", "range": { "gte": 2.5, "lte": 2.5 } }
            ]
        })
    );
}

#[test]
fn test_in_bools() {
    let expr = FilterExpr::In {
        field: String::from("is_active"),
        values: vec![Value::Bool(true), Value::Bool(false)],
    };
    let filter = build(&expr);
    assert_eq!(
        filter,
        serde_json::json!({
            "should": [
                { "key": "is_active", "match": { "value": true } },
                { "key": "is_active", "match": { "value": false } }
            ]
        })
    );
}

#[test]
fn test_not_in_strings() {
    let expr = FilterExpr::NotIn {
        field: String::from("status"),
        values: vec![
            Value::Str(String::from("deleted")),
            Value::Str(String::from("archived")),
        ],
    };
    let filter = build(&expr);
    assert_eq!(
        filter,
        serde_json::json!({
            "must": [
                {
                    "key": "status",
                    "match": {
                        "except": ["deleted", "archived"]
                    }
                }
            ]
        })
    );
}

#[test]
fn test_not_in_ints() {
    let expr = FilterExpr::NotIn {
        field: String::from("count"),
        values: vec![Value::Int(3), Value::Int(4)],
    };
    let filter = build(&expr);
    assert_eq!(
        filter,
        serde_json::json!({
            "must_not": [
                { "key": "count", "match": { "value": 3 } },
                { "key": "count", "match": { "value": 4 } }
            ]
        })
    );
}

#[test]
fn test_not_in_floats() {
    let expr = FilterExpr::NotIn {
        field: String::from("score"),
        values: vec![Value::Float(4.5), Value::Float(9.0)],
    };
    let filter = build(&expr);
    assert_eq!(
        filter,
        serde_json::json!({
            "must_not": [
                { "key": "score", "range": { "gte": 4.5, "lte": 4.5 } },
                { "key": "score", "range": { "gte": 9.0, "lte": 9.0 } }
            ]
        })
    );
}

#[test]
fn test_not_in_bools() {
    let expr = FilterExpr::NotIn {
        field: String::from("is_active"),
        values: vec![Value::Bool(true), Value::Bool(false)],
    };
    let filter = build(&expr);
    assert_eq!(
        filter,
        serde_json::json!({
            "must_not": [
                { "key": "is_active", "match": { "value": true } },
                { "key": "is_active", "match": { "value": false } }
            ]
        })
    );
}

#[test]
fn test_rejects_mixed_in_types() {
    let expr = FilterExpr::In {
        field: String::from("mixed"),
        values: vec![Value::Str(String::from("active")), Value::Int(1)],
    };
    let result = FilterConverter.build_filter(&expr);
    assert!(result.is_err());
}

#[test]
fn test_match_text() {
    let expr = FilterExpr::MatchText {
        field: String::from("content"),
        text: String::from("hello world"),
    };
    let filter = build(&expr);
    assert_eq!(
        filter,
        serde_json::json!({
            "must": [
                {
                    "key": "content",
                    "match": { "text": "hello world" }
                }
            ]
        })
    );
}

#[test]
fn test_match_any() {
    let expr = FilterExpr::MatchAny {
        field: String::from("content"),
        text: String::from("hello world"),
    };
    let filter = build(&expr);
    assert_eq!(
        filter,
        serde_json::json!({
            "must": [
                {
                    "key": "content",
                    "match": { "any": ["hello world"] }
                }
            ]
        })
    );
}

#[test]
fn test_match_phrase() {
    let expr = FilterExpr::MatchPhrase {
        field: String::from("content"),
        text: String::from("hello world"),
    };
    let filter = build(&expr);
    assert_eq!(
        filter,
        serde_json::json!({
            "must": [
                {
                    "key": "content",
                    "match": { "phrase": "hello world" }
                }
            ]
        })
    );
}
