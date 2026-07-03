use super::build;
use crate::filter_conv::*;
use qql_core::ast::{FilterExpr, Value};

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
