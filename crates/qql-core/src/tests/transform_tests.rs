use crate::ast::{ComparisonOp, FilterExpr, Stmt, Value};
use crate::parser::Parser;

#[test]
fn inject_into_query() {
    let mut s = Parser::parse("QUERY TEXT 'x' FROM docs WHERE active = true;").unwrap();
    crate::ast::inject_filter(
        &mut s,
        "tenant",
        ComparisonOp::Eq,
        Value::Str("acme".into()),
    )
    .unwrap();
    let Stmt::Query(q) = s else { panic!() };
    assert!(matches!(*q.filter.unwrap(), FilterExpr::And { .. }));
}

#[test]
fn inject_id_filter() {
    let mut s = Parser::parse("QUERY TEXT 'x' FROM docs;").unwrap();
    crate::ast::inject_filter(&mut s, "id", ComparisonOp::Eq, Value::Str("uuid-1".into())).unwrap();
    let Stmt::Query(q) = s else { panic!() };
    assert!(matches!(*q.filter.unwrap(), FilterExpr::PointId(_)));
}

#[test]
fn inject_into_scroll() {
    let mut s = Parser::parse("SCROLL FROM docs WHERE active = true LIMIT 10;").unwrap();
    crate::ast::inject_filter(
        &mut s,
        "tenant",
        ComparisonOp::Eq,
        Value::Str("acme".into()),
    )
    .unwrap();
    let Stmt::Scroll(sc) = s else { panic!() };
    assert!(sc.filter.is_some());
}

#[test]
fn inject_into_delete() {
    let mut s = Parser::parse("DELETE FROM docs WHERE id = 1;").unwrap();
    crate::ast::inject_filter(
        &mut s,
        "tenant",
        ComparisonOp::Eq,
        Value::Str("acme".into()),
    )
    .unwrap();
    let Stmt::Delete(d) = s else { panic!() };
    assert!(matches!(d.selector, crate::ast::PointSelector::Filter(_)));
}

#[test]
fn inject_into_upsert() {
    let mut s = Parser::parse("UPSERT INTO docs VALUES {id: 1, text: 'hello'};").unwrap();
    crate::ast::inject_filter(
        &mut s,
        "tenant",
        ComparisonOp::Eq,
        Value::Str("acme".into()),
    )
    .unwrap();
    let Stmt::Upsert(u) = s else { panic!() };
    assert_eq!(
        u.points[0]
            .payload
            .iter()
            .find(|(k, _)| k == "tenant")
            .unwrap()
            .1,
        Value::Str("acme".into())
    );
}

#[test]
fn inject_into_cte_recursive() {
    let mut s = Parser::parse(
        "WITH d AS (QUERY TEXT 'x' USING dense LIMIT 100), s AS (QUERY TEXT 'x' USING sparse LIMIT 100) QUERY FUSION RRF FROM docs PREFETCH (d, s) LIMIT 10;",
    ).unwrap();
    crate::ast::inject_filter(
        &mut s,
        "tenant",
        ComparisonOp::Eq,
        Value::Str("acme".into()),
    )
    .unwrap();
    let Stmt::Query(q) = s else { panic!() };
    // Both CTEs should have the injected filter
    assert!(q.ctes[0].query.filter.is_some());
    assert!(q.ctes[1].query.filter.is_some());
}

#[test]
fn inject_id_requires_equality() {
    let mut s = Parser::parse("QUERY TEXT 'x' FROM docs;").unwrap();
    assert!(crate::ast::inject_filter(&mut s, "id", ComparisonOp::Gt, Value::Int(5)).is_err());
}

// ── Security: injection resistance tests ────────────────────────

#[test]
fn injection_resists_logical_or_bypass() {
    // Attacker tries to escape tenant boundary with OR
    let mut s =
        Parser::parse("QUERY TEXT 'x' FROM docs WHERE status = 'public' OR tenant_id = 'globex';")
            .unwrap();
    crate::ast::inject_filter(
        &mut s,
        "tenant_id",
        ComparisonOp::Eq,
        Value::Str("acme".into()),
    )
    .unwrap();
    let Stmt::Query(q) = s else { panic!() };
    match *q.filter.unwrap() {
        FilterExpr::And { operands } => {
            assert_eq!(
                operands.len(),
                2,
                "must wrap in AND with exactly 2 operands"
            );
            assert!(
                matches!(operands[0], FilterExpr::Or { .. }),
                "first operand must be the attacker's OR (structurally contained)"
            );
            assert!(
                matches!(operands[1], FilterExpr::Compare { .. }),
                "second operand must be the injected tenant filter"
            );
        }
        other => panic!("expected AND wrapper, got {other:?}"),
    }
}

#[test]
fn injection_resists_negation_bypass() {
    // Attacker writes NOT tenant_id = 'acme' — injection adds AND tenant_id = 'acme'
    // Result: NOT acme AND acme → contradiction → zero results. Safe.
    let mut s = Parser::parse("QUERY TEXT 'x' FROM docs WHERE NOT tenant_id = 'acme';").unwrap();
    crate::ast::inject_filter(
        &mut s,
        "tenant_id",
        ComparisonOp::Eq,
        Value::Str("acme".into()),
    )
    .unwrap();
    let Stmt::Query(q) = s else { panic!() };
    match *q.filter.unwrap() {
        FilterExpr::And { operands } => {
            assert_eq!(operands.len(), 2);
            assert!(matches!(operands[0], FilterExpr::Not { .. }));
            assert!(matches!(operands[1], FilterExpr::Compare { .. }));
        }
        other => panic!("expected AND wrapper, got {other:?}"),
    }
}

#[test]
fn injection_works_on_query_with_no_where_clause() {
    // No WHERE clause → injection creates one
    let mut s = Parser::parse("QUERY TEXT 'x' FROM docs LIMIT 10;").unwrap();
    crate::ast::inject_filter(
        &mut s,
        "tenant_id",
        ComparisonOp::Eq,
        Value::Str("acme".into()),
    )
    .unwrap();
    let Stmt::Query(q) = s else { panic!() };
    assert!(q.filter.is_some());
    assert!(matches!(*q.filter.unwrap(), FilterExpr::Compare { .. }));
}

#[test]
fn injection_resists_standalone_and_bypass() {
    // Attacker writes a legitimate-looking filter with AND — injection adds to it
    // Result: (year = 2024 AND status = 'public') AND tenant_id = 'acme'
    let mut s =
        Parser::parse("QUERY TEXT 'x' FROM docs WHERE year = 2024 AND status = 'public';").unwrap();
    crate::ast::inject_filter(
        &mut s,
        "tenant_id",
        ComparisonOp::Eq,
        Value::Str("acme".into()),
    )
    .unwrap();
    let Stmt::Query(q) = s else { panic!() };
    match *q.filter.unwrap() {
        FilterExpr::And { operands } => {
            // Three operands: year=2024, status='public', tenant_id='acme'
            // The original AND is flattened, so operands length depends on implementation
            assert!(operands.len() >= 2, "must have at least 2 operands");
            let has_tenant = operands
                .iter()
                .any(|op| matches!(op, FilterExpr::Compare { field, .. } if field == "tenant_id"));
            assert!(has_tenant, "tenant_id filter must be present");
        }
        other => panic!("expected AND wrapper, got {other:?}"),
    }
}
