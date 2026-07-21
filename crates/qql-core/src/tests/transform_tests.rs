use crate::ast::{ComparisonOp, FilterExpr, Stmt, Value};
use crate::parser::Parser;

#[test]
fn inject_into_query() {
    let mut s = Parser::parse("QUERY TEXT 'x' FROM docs WHERE active = true;").unwrap();
    crate::ast::inject_filter(&mut s, "tenant", ComparisonOp::Eq, Value::Str("acme".into()))
        .unwrap();
    let Stmt::Query(q) = s else { panic!() };
    assert!(matches!(*q.filter.unwrap(), FilterExpr::And { .. }));
}

#[test]
fn inject_id_filter() {
    let mut s = Parser::parse("QUERY TEXT 'x' FROM docs;").unwrap();
    crate::ast::inject_filter(&mut s, "id", ComparisonOp::Eq, Value::Str("uuid-1".into()))
        .unwrap();
    let Stmt::Query(q) = s else { panic!() };
    assert!(matches!(*q.filter.unwrap(), FilterExpr::PointId(_)));
}

#[test]
fn inject_into_scroll() {
    let mut s = Parser::parse("SCROLL FROM docs WHERE active = true LIMIT 10;").unwrap();
    crate::ast::inject_filter(&mut s, "tenant", ComparisonOp::Eq, Value::Str("acme".into()))
        .unwrap();
    let Stmt::Scroll(sc) = s else { panic!() };
    assert!(sc.filter.is_some());
}

#[test]
fn inject_into_delete() {
    let mut s = Parser::parse("DELETE FROM docs WHERE id = 1;").unwrap();
    crate::ast::inject_filter(&mut s, "tenant", ComparisonOp::Eq, Value::Str("acme".into()))
        .unwrap();
    let Stmt::Delete(d) = s else { panic!() };
    assert!(matches!(d.selector, crate::ast::PointSelector::Filter(_)));
}

#[test]
fn inject_into_upsert() {
    let mut s = Parser::parse("UPSERT INTO docs VALUES {id: 1, text: 'hello'};").unwrap();
    crate::ast::inject_filter(&mut s, "tenant", ComparisonOp::Eq, Value::Str("acme".into()))
        .unwrap();
    let Stmt::Upsert(u) = s else { panic!() };
    assert_eq!(
        u.points[0].payload.iter().find(|(k, _)| k == "tenant").unwrap().1,
        Value::Str("acme".into())
    );
}

#[test]
fn inject_into_cte_recursive() {
    let mut s = Parser::parse(
        "WITH d AS (QUERY TEXT 'x' USING dense LIMIT 100), s AS (QUERY TEXT 'x' USING sparse LIMIT 100) QUERY FUSION RRF FROM docs PREFETCH (d, s) LIMIT 10;",
    ).unwrap();
    crate::ast::inject_filter(&mut s, "tenant", ComparisonOp::Eq, Value::Str("acme".into()))
        .unwrap();
    let Stmt::Query(q) = s else { panic!() };
    // Both CTEs should have the injected filter
    assert!(q.ctes[0].query.filter.is_some());
    assert!(q.ctes[1].query.filter.is_some());
}

#[test]
fn inject_id_requires_equality() {
    let mut s = Parser::parse("QUERY TEXT 'x' FROM docs;").unwrap();
    assert!(
        crate::ast::inject_filter(&mut s, "id", ComparisonOp::Gt, Value::Int(5)).is_err()
    );
}
