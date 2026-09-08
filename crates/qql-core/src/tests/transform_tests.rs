use crate::ast::{ComparisonOp, FilterExpr, QueryExpr, Stmt, Value};
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
            .as_inline()
            .expect("inline point")
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

#[test]
fn shard_key_access_supports_delete_payload() {
    let mut statement =
        Parser::parse("DELETE PAYLOAD draft FROM docs WHERE status = 'archived';").unwrap();

    assert_eq!(statement.shard_key(), None);
    assert!(statement.set_shard_key(Some("tenant-a".into())));
    assert_eq!(statement.shard_key(), Some("tenant-a"));

    assert!(statement.set_shard_key(None));
    assert_eq!(statement.shard_key(), None);
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

fn tenant_eq() -> (ComparisonOp, Value) {
    (ComparisonOp::Eq, Value::Str("acme".into()))
}

#[test]
fn inject_into_facet_count_and_payload_mutations() {
    let (op, value) = tenant_eq();
    for source in [
        "FACET category FROM docs;",
        "COUNT FROM docs;",
        "CLEAR PAYLOAD FROM docs WHERE id = 1;",
        "DELETE PAYLOAD draft FROM docs WHERE id = 1;",
        "DELETE VECTOR dense FROM docs WHERE id = 1;",
        "UPDATE docs SET PAYLOAD = {flag: true} WHERE id = 1;",
    ] {
        let mut s = Parser::parse(source).unwrap();
        crate::ast::inject_filter(&mut s, "tenant", op, value.clone()).unwrap();
        match &s {
            Stmt::Facet(f) => assert!(f.filter.is_some(), "{source}"),
            Stmt::Count(c) => assert!(c.filter.is_some(), "{source}"),
            Stmt::ClearPayload(p) => {
                assert!(matches!(p.selector, crate::ast::PointSelector::Filter(_)))
            }
            Stmt::DeletePayload(p) => {
                assert!(matches!(p.selector, crate::ast::PointSelector::Filter(_)))
            }
            Stmt::DeleteVector(p) => {
                assert!(matches!(p.selector, crate::ast::PointSelector::Filter(_)))
            }
            Stmt::UpdatePayload(p) => {
                assert!(matches!(p.selector, crate::ast::PointSelector::Filter(_)))
            }
            other => panic!("unexpected statement for {source}: {other:?}"),
        }
    }
}

#[test]
fn inject_rejects_ddl_show_update_vector_and_upsert_combos() {
    let (op, value) = tenant_eq();
    for source in [
        "CREATE COLLECTION docs (d VECTOR(4, COSINE));",
        "SHOW COLLECTIONS;",
        "UPDATE docs SET VECTOR dense = [0.1] WHERE id = 1;",
    ] {
        let mut s = Parser::parse(source).unwrap();
        let err = crate::ast::inject_filter(&mut s, "tenant", op, value.clone()).unwrap_err();
        assert_eq!(err.code, "QQL-VALIDATION-FILTER-INJECT");
        assert!(
            err.message
                .contains("does not apply to this statement type"),
            "{}",
            err.message
        );
    }

    let mut upsert = Parser::parse("UPSERT INTO docs VALUES {id: 1, title: 'a'};").unwrap();
    let err = crate::ast::inject_filter(&mut upsert, "title", ComparisonOp::Gt, Value::Int(1))
        .unwrap_err();
    assert_eq!(err.code, "QQL-VALIDATION-FILTER-INJECT");
    assert!(
        err.message
            .contains("inject_filter into UPSERT requires Eq"),
        "{}",
        err.message
    );

    let mut upsert_id = Parser::parse("UPSERT INTO docs VALUES {id: 1, title: 'a'};").unwrap();
    let err = crate::ast::inject_filter(&mut upsert_id, "id", ComparisonOp::Eq, Value::Int(2))
        .unwrap_err();
    assert_eq!(err.code, "QQL-VALIDATION-FILTER-INJECT");
    assert!(
        err.message.contains("non-id payload field"),
        "{}",
        err.message
    );
}

#[test]
fn inject_recurses_into_inline_prefetch_query() {
    let mut s = Parser::parse(
        "QUERY FUSION RRF FROM docs PREFETCH (QUERY TEXT 'x' FROM docs LIMIT 10) LIMIT 5;",
    )
    .unwrap();
    let (op, value) = tenant_eq();
    crate::ast::inject_filter(&mut s, "tenant", op, value).unwrap();
    let Stmt::Query(q) = s else { panic!() };
    assert!(q.filter.is_some());
    let QueryExpr::Fusion { prefetch, .. } = q.expression else {
        panic!("expected fusion");
    };
    assert!(prefetch[0].filter.is_some());
}

#[test]
fn parse_inject_op_is_case_insensitive() {
    use crate::ast::ComparisonOp;
    assert_eq!(
        ComparisonOp::parse_inject_op("EQ").unwrap(),
        ComparisonOp::Eq
    );
    assert_eq!(
        ComparisonOp::parse_inject_op(" Gt ").unwrap(),
        ComparisonOp::Gt
    );
    assert_eq!(
        ComparisonOp::parse_inject_op("GTE").unwrap(),
        ComparisonOp::Gte
    );
    let err = ComparisonOp::parse_inject_op("!=").unwrap_err();
    assert_eq!(err.code, "QQL-VALIDATION-FILTER-INJECT");
    assert!(err.message.contains("wrap with NOT"));
    let err = ComparisonOp::parse_inject_op("bogus").unwrap_err();
    assert_eq!(err.code, "QQL-VALIDATION-FILTER-INJECT");
}
