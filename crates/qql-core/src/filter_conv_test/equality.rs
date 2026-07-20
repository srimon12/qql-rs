use super::build;
use crate::ast::{FilterExpr, Value};
use crate::filter_conv::FilterConverter;

#[test]
fn test_equals_string() {
    let expr = FilterExpr::Compare {
        field: String::from("status"),
        op: String::from("="),
        value: Value::Str(String::from("active")),
    };
    let filter = build(&expr);
    assert_eq!(
        filter,
        serde_json::json!({
            "must": [
                {
                    "key": "status",
                    "match": { "value": "active" }
                }
            ]
        })
    );
}

#[test]
fn test_equals_int() {
    let expr = FilterExpr::Compare {
        field: String::from("count"),
        op: String::from("="),
        value: Value::Int(42),
    };
    let filter = build(&expr);
    assert_eq!(
        filter,
        serde_json::json!({
            "must": [
                {
                    "key": "count",
                    "match": { "value": 42 }
                }
            ]
        })
    );
}

#[test]
fn test_equals_float() {
    let expr = FilterExpr::Compare {
        field: String::from("score"),
        op: String::from("="),
        value: Value::Float(12.34),
    };
    let filter = build(&expr);
    assert_eq!(
        filter,
        serde_json::json!({
            "must": [
                {
                    "key": "score",
                    "range": {
                        "gte": 12.34,
                        "lte": 12.34
                    }
                }
            ]
        })
    );
}

#[test]
fn test_equals_bool() {
    let expr = FilterExpr::Compare {
        field: String::from("is_active"),
        op: String::from("="),
        value: Value::Bool(true),
    };
    let filter = build(&expr);
    assert_eq!(
        filter,
        serde_json::json!({
            "must": [
                {
                    "key": "is_active",
                    "match": { "value": true }
                }
            ]
        })
    );
}

#[test]
fn test_not_equals_string() {
    let expr = FilterExpr::Compare {
        field: String::from("status"),
        op: String::from("!="),
        value: Value::Str(String::from("archived")),
    };
    let filter = build(&expr);
    assert_eq!(
        filter,
        serde_json::json!({
            "must": [
                {
                    "key": "status",
                    "match": { "except": "archived" }
                }
            ]
        })
    );
}

#[test]
fn test_not_equals_int() {
    let expr = FilterExpr::Compare {
        field: String::from("count"),
        op: String::from("!="),
        value: Value::Int(7),
    };
    let filter = build(&expr);
    assert_eq!(
        filter,
        serde_json::json!({
            "must_not": [
                {
                    "key": "count",
                    "match": { "value": 7 }
                }
            ]
        })
    );
}

#[test]
fn test_not_equals_float() {
    let expr = FilterExpr::Compare {
        field: String::from("score"),
        op: String::from("!="),
        value: Value::Float(1.5),
    };
    let filter = build(&expr);
    assert_eq!(
        filter,
        serde_json::json!({
            "must_not": [
                {
                    "key": "score",
                    "range": {
                        "gte": 1.5,
                        "lte": 1.5
                    }
                }
            ]
        })
    );
}

#[test]
fn test_not_equals_bool() {
    let expr = FilterExpr::Compare {
        field: String::from("is_active"),
        op: String::from("!="),
        value: Value::Bool(false),
    };
    let filter = build(&expr);
    assert_eq!(
        filter,
        serde_json::json!({
            "must_not": [
                {
                    "key": "is_active",
                    "match": { "value": false }
                }
            ]
        })
    );
}

#[test]
fn test_greater_than() {
    let expr = FilterExpr::Compare {
        field: String::from("age"),
        op: String::from(">"),
        value: Value::Int(18),
    };
    let filter = build(&expr);
    assert_eq!(
        filter,
        serde_json::json!({
            "must": [
                {
                    "key": "age",
                    "range": { "gt": 18.0 }
                }
            ]
        })
    );
}

#[test]
fn test_greater_than_equal() {
    let expr = FilterExpr::Compare {
        field: String::from("age"),
        op: String::from(">="),
        value: Value::Int(18),
    };
    let filter = build(&expr);
    assert_eq!(
        filter,
        serde_json::json!({
            "must": [
                {
                    "key": "age",
                    "range": { "gte": 18.0 }
                }
            ]
        })
    );
}

#[test]
fn test_less_than() {
    let expr = FilterExpr::Compare {
        field: String::from("price"),
        op: String::from("<"),
        value: Value::Float(100.0),
    };
    let filter = build(&expr);
    assert_eq!(
        filter,
        serde_json::json!({
            "must": [
                {
                    "key": "price",
                    "range": { "lt": 100.0 }
                }
            ]
        })
    );
}

#[test]
fn test_less_than_equal() {
    let expr = FilterExpr::Compare {
        field: String::from("price"),
        op: String::from("<="),
        value: Value::Float(100.0),
    };
    let filter = build(&expr);
    assert_eq!(
        filter,
        serde_json::json!({
            "must": [
                {
                    "key": "price",
                    "range": { "lte": 100.0 }
                }
            ]
        })
    );
}

#[test]
fn test_between() {
    let expr = FilterExpr::Between {
        field: String::from("age"),
        low: Value::Int(18),
        high: Value::Int(65),
    };
    let filter = build(&expr);
    assert_eq!(
        filter,
        serde_json::json!({
            "must": [
                {
                    "key": "age",
                    "range": {
                        "gte": 18.0,
                        "lte": 65.0
                    }
                }
            ]
        })
    );
}

#[test]
fn test_is_null() {
    let expr = FilterExpr::IsNull {
        field: String::from("deleted_at"),
    };
    let filter = build(&expr);
    assert_eq!(
        filter,
        serde_json::json!({
            "must": [
                {
                    "is_null": { "key": "deleted_at" }
                }
            ]
        })
    );
}

#[test]
fn test_is_not_null() {
    let expr = FilterExpr::IsNotNull {
        field: String::from("email"),
    };
    let filter = build(&expr);
    assert_eq!(
        filter,
        serde_json::json!({
            "must_not": [
                { "is_null": { "key": "email" } }
            ]
        })
    );
}

#[test]
fn test_is_empty() {
    let expr = FilterExpr::IsEmpty {
        field: String::from("tags"),
    };
    let filter = build(&expr);
    assert_eq!(
        filter,
        serde_json::json!({
            "must": [
                {
                    "is_empty": { "key": "tags" }
                }
            ]
        })
    );
}

#[test]
fn test_is_not_empty() {
    let expr = FilterExpr::IsNotEmpty {
        field: String::from("tags"),
    };
    let filter = build(&expr);
    assert_eq!(
        filter,
        serde_json::json!({
            "must_not": [
                { "is_empty": { "key": "tags" } }
            ]
        })
    );
}

#[test]
fn test_basic_conversion() {
    let expr = FilterExpr::Compare {
        field: String::from("x"),
        op: String::from("="),
        value: Value::Int(0),
    };
    let result = FilterConverter.build_filter(&expr);
    assert!(result.is_ok());
}

#[test]
fn test_has_vector() {
    let expr = FilterExpr::HasVector {
        name: String::from("my_vector"),
    };
    let filter = build(&expr);
    assert_eq!(
        filter,
        serde_json::json!({
            "must": [
                {
                    "has_vector": "my_vector"
                }
            ]
        })
    );
}

#[test]
fn test_values_count() {
    let expr = FilterExpr::ValuesCount {
        field: String::from("tags"),
        op: String::from(">"),
        count: 5,
    };
    let filter = build(&expr);
    assert_eq!(
        filter,
        serde_json::json!({
            "must": [
                {
                    "key": "tags",
                    "values_count": { "gt": 5 }
                }
            ]
        })
    );
}

#[test]
fn test_geo_bounding_box() {
    let expr = FilterExpr::GeoBoundingBox {
        field: String::from("location"),
        top_left_lat: 52.520711,
        top_left_lon: 13.403683,
        bottom_right_lat: 52.520712,
        bottom_right_lon: 13.403684,
    };
    let filter = build(&expr);
    assert_eq!(
        filter,
        serde_json::json!({
            "must": [
                {
                    "key": "location",
                    "geo_bounding_box": {
                        "top_left": {
                            "lat": 52.520711,
                            "lon": 13.403683
                        },
                        "bottom_right": {
                            "lat": 52.520712,
                            "lon": 13.403684
                        }
                    }
                }
            ]
        })
    );
}

#[test]
fn test_geo_radius() {
    let expr = FilterExpr::GeoRadius {
        field: String::from("location"),
        lat: 52.520711,
        lon: 13.403683,
        radius: 1000.0,
    };
    let filter = build(&expr);
    assert_eq!(
        filter,
        serde_json::json!({
            "must": [
                {
                    "key": "location",
                    "geo_radius": {
                        "center": {
                            "lat": 52.520711,
                            "lon": 13.403683
                        },
                        "radius": 1000.0
                    }
                }
            ]
        })
    );
}
