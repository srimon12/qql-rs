use super::build;
use crate::filter_conv::*;
use qql_core::ast::{FilterExpr, Value};

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
        value: Value::Float(12.34),
    };
    let filter = build(&expr);
    let must = filter.must.unwrap();
    assert!(matches!(&must[0],
        QdrantCondition::Range { key, gte: Some(g), lte: Some(l), .. }
        if key == "score" && (*g - 12.34).abs() < f64::EPSILON && (*l - 12.34).abs() < f64::EPSILON
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
