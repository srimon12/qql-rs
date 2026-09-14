//! Formula expression REST/gRPC parity.

use crate::grpc_route::test_api;
use crate::qdrant_grpc::qdrant;
use qql_core::parser::Parser;
use qql_plan::PlannedOperation;
use qql_plan::plan::{plan, to_rest_route};

#[test]
fn rest_grpc_formula_parity() {
    let stmt = Parser::parse("QUERY FORMULA score + 1.0 DEFAULTS (score = 0.0) FROM docs LIMIT 5;")
        .unwrap();
    let op = plan(&stmt).unwrap();
    let PlannedOperation::Query {
        collection,
        request,
    } = &op
    else {
        panic!("expected Query");
    };
    let body = to_rest_route(&op).expect("rest route").body_json().unwrap();
    assert!(
        body["query"].get("formula").is_some(),
        "REST formula missing: {}",
        body["query"]
    );

    let grpc = test_api::to_query_points(request, collection).unwrap();
    use qdrant::query::Variant as Qv;
    match grpc.query.as_ref().and_then(|q| q.variant.as_ref()) {
        Some(Qv::Formula(f)) => {
            assert!(f.expression.is_some(), "gRPC formula must have expression");
        }
        other => panic!("expected Formula, got {other:?}"),
    }
}

/// MAX / MIN / ACOSH — new Qdrant Expression variants (proto fields 20-22).
#[test]
fn rest_grpc_formula_nary_acosh_parity() {
    let stmt = Parser::parse(
        "QUERY FORMULA MAX(score * 2.0, MIN(score, bonus)) + ACOSH(rank) \
             DEFAULTS (score = 0.0, bonus = 0.0) FROM docs LIMIT 5;",
    )
    .unwrap();
    let op = plan(&stmt).unwrap();
    let PlannedOperation::Query {
        collection,
        request,
    } = &op
    else {
        panic!("expected Query");
    };

    // REST: sum → [max: [mult, min: [...] ], acosh]
    let body = to_rest_route(&op).expect("rest route").body_json().unwrap();
    let expr = &body["query"]["formula"];
    let sum = expr["sum"].as_array().expect("sum terms");
    let max = &sum[0]["max"];
    assert!(
        max.is_array() && max.as_array().unwrap().len() == 2,
        "{max}"
    );
    assert!(
        max[1]["min"].as_array().is_some_and(|m| m.len() == 2),
        "nested MIN must lower to a 2-term array: {max}"
    );
    assert_eq!(
        sum[1]["acosh"],
        serde_json::json!("rank"),
        "ACOSH over a bare variable lowers to a string expression: {sum:?}"
    );

    // gRPC: same shape via typed proto variants.
    let grpc = test_api::to_query_points(request, collection).unwrap();
    use qdrant::expression::Variant as Ev;
    use qdrant::query::Variant as Qv;
    let Some(Qv::Formula(formula)) = grpc.query.as_ref().and_then(|q| q.variant.as_ref()) else {
        panic!("expected Formula query");
    };
    let Some(expression) = formula.expression.as_ref() else {
        panic!("formula missing expression");
    };
    let Some(Ev::Sum(sum)) = expression.variant.as_ref() else {
        panic!("expected Sum expression, got {:?}", expression.variant);
    };
    let Some(Ev::Max(max)) = sum.sum[0].variant.as_ref() else {
        panic!("expected Max expression, got {:?}", sum.sum[0].variant);
    };
    assert_eq!(max.max.len(), 2, "MAX must carry both operands");
    assert!(
        matches!(max.max[1].variant.as_ref(), Some(Ev::Min(_))),
        "nested MIN must map to MinExpression, got {:?}",
        max.max[1].variant
    );
    let Some(Ev::Acosh(_)) = sum.sum[1].variant.as_ref() else {
        panic!("expected Acosh expression, got {:?}", sum.sum[1].variant);
    };
}

#[test]
fn rest_grpc_formula_case_condition_parity() {
    let stmt = Parser::parse(
        "QUERY FORMULA CASE WHEN status = 'active' THEN $score * 2 ELSE $score END \
             FROM docs LIMIT 5;",
    )
    .unwrap();
    let op = plan(&stmt).unwrap();
    let PlannedOperation::Query {
        collection,
        request,
    } = &op
    else {
        panic!("expected Query");
    };

    // REST: `cond * then + (1 - cond) * else`, condition as a bare 0/1 term.
    let body = to_rest_route(&op).expect("rest route").body_json().unwrap();
    let cond = &body["query"]["formula"]["sum"][0]["mult"][0];
    assert_eq!(cond["key"], "status");
    assert_eq!(cond["match"]["value"], "active");

    // gRPC: the same weighting, with the condition as a typed Condition.
    let grpc = test_api::to_query_points(request, collection).unwrap();
    use qdrant::expression::Variant as Ev;
    use qdrant::query::Variant as Qv;
    let Some(Qv::Formula(formula)) = grpc.query.as_ref().and_then(|q| q.variant.as_ref()) else {
        panic!("expected Formula query");
    };
    let Some(expression) = formula.expression.as_ref() else {
        panic!("formula missing expression");
    };
    let Some(Ev::Sum(sum)) = expression.variant.as_ref() else {
        panic!("expected Sum expression, got {:?}", expression.variant);
    };
    let Some(Ev::Mult(mult)) = sum.sum[0].variant.as_ref() else {
        panic!("expected Mult expression, got {:?}", sum.sum[0].variant);
    };
    let Some(Ev::Condition(condition)) = mult.mult[0].variant.as_ref() else {
        panic!(
            "expected Condition expression, got {:?}",
            mult.mult[0].variant
        );
    };
    let Some(qdrant::condition::ConditionOneOf::Field(field)) = condition.condition_one_of.as_ref()
    else {
        panic!(
            "expected Field condition, got {:?}",
            condition.condition_one_of
        );
    };
    assert_eq!(field.key, "status");
}

#[test]
fn rest_grpc_formula_match_condition_parity() {
    let stmt = Parser::parse(
        "QUERY FORMULA CASE WHEN a = 1 AND b = 2 THEN $score ELSE 0 END FROM docs LIMIT 5;",
    )
    .unwrap();
    let op = plan(&stmt).unwrap();
    let PlannedOperation::Query {
        collection,
        request,
    } = &op
    else {
        panic!("expected Query");
    };

    let body = to_rest_route(&op).expect("rest route").body_json().unwrap();
    let compound = &body["query"]["formula"]["sum"][0]["mult"][0];
    assert_eq!(compound["must"][0]["key"], "a");
    assert_eq!(compound["must"][1]["key"], "b");

    // A compound condition maps to a Filter condition on the gRPC side.
    let grpc = test_api::to_query_points(request, collection).unwrap();
    use qdrant::expression::Variant as Ev;
    use qdrant::query::Variant as Qv;
    let Some(Qv::Formula(formula)) = grpc.query.as_ref().and_then(|q| q.variant.as_ref()) else {
        panic!("expected Formula query");
    };
    let Some(Ev::Sum(sum)) = formula
        .expression
        .as_ref()
        .and_then(|expression| expression.variant.as_ref())
    else {
        panic!("expected Sum expression");
    };
    let Some(Ev::Mult(mult)) = sum.sum[0].variant.as_ref() else {
        panic!("expected Mult expression, got {:?}", sum.sum[0].variant);
    };
    let Some(Ev::Condition(condition)) = mult.mult[0].variant.as_ref() else {
        panic!(
            "expected Condition expression, got {:?}",
            mult.mult[0].variant
        );
    };
    let Some(qdrant::condition::ConditionOneOf::Filter(filter)) =
        condition.condition_one_of.as_ref()
    else {
        panic!(
            "expected Filter condition, got {:?}",
            condition.condition_one_of
        );
    };
    assert_eq!(filter.must.len(), 2, "AND carries both clauses: {filter:?}");
}
