//! W2 scroll/ordering/lookup coverage: parse + canonical format round-trips.
//!
//! Items: SCROLL `ORDER BY`, SCROLL payload selectors, `ORDER BY … START
//! FROM`, group `LOOKUP` selectors, and prefetch `LOOKUP … SHARD` routing.

use qql_core::ast::{OrderDirection, QueryExpr, Stmt};
use qql_core::fmt::format_stmt;
use qql_core::parser::Parser;

fn parse(source: &str) -> Stmt {
    Parser::parse(source).unwrap_or_else(|e| panic!("parse {source}: {e}"))
}

fn assert_round_trip(source: &str, expected: &str) {
    let stmt = parse(source);
    assert_eq!(format_stmt(&stmt), expected, "canonical form of {source}");
    let reparsed = parse(expected);
    assert_eq!(reparsed, stmt, "AST mismatch for {source}");
}

/// Lowercase keywords normalize to the canonical uppercase form.
#[test]
fn scroll_order_by_parses_and_round_trips() {
    assert_round_trip(
        "SCROLL FROM docs ORDER BY created_at DESC LIMIT 10;",
        "SCROLL FROM docs ORDER BY created_at DESC LIMIT 10",
    );
    // Direction defaults to ASC and renders explicitly.
    assert_round_trip(
        "scroll from docs order by created_at limit 10;",
        "SCROLL FROM docs ORDER BY created_at ASC LIMIT 10",
    );
}

#[test]
fn scroll_order_by_start_from_values() {
    assert_round_trip(
        "SCROLL FROM docs ORDER BY score DESC START FROM 100 LIMIT 10;",
        "SCROLL FROM docs ORDER BY score DESC START FROM 100 LIMIT 10",
    );
    assert_round_trip(
        "SCROLL FROM docs ORDER BY score ASC START FROM 4.5 LIMIT 10;",
        "SCROLL FROM docs ORDER BY score ASC START FROM 4.5 LIMIT 10",
    );
    assert_round_trip(
        "SCROLL FROM docs ORDER BY created_at DESC START FROM '2024-01-01T00:00:00Z' LIMIT 10;",
        "SCROLL FROM docs ORDER BY created_at DESC START FROM '2024-01-01T00:00:00Z' LIMIT 10",
    );
}

#[test]
fn scroll_order_by_combines_with_other_clauses() {
    let stmt = parse(
        "SCROLL FROM docs WHERE status = 'active' AFTER 7 ORDER BY created_at DESC SHARD 'acme' WITH PAYLOAD INCLUDE (title) WITH VECTOR (dense) LIMIT 10;",
    );
    let Stmt::Scroll(scroll) = &stmt else {
        panic!("expected SCROLL, got {stmt:?}");
    };
    assert_eq!(scroll.collection, "docs");
    assert_eq!(scroll.limit, 10);
    assert!(scroll.filter.is_some());
    assert!(scroll.after.is_some());
    let order = scroll.order_by.as_ref().expect("order_by");
    assert_eq!(order.field, "created_at");
    assert_eq!(order.direction, OrderDirection::Desc);
    assert!(scroll.shard_key.is_some());
    assert!(scroll.with_payload.is_some());
    assert!(scroll.with_vector.is_some());
    // Canonical clause order: WHERE, AFTER, ORDER BY, SHARD, WITH PAYLOAD, WITH VECTOR, LIMIT.
    assert_eq!(
        format_stmt(&stmt),
        "SCROLL FROM docs WHERE status = 'active' AFTER 7 ORDER BY created_at DESC SHARD 'acme' WITH PAYLOAD INCLUDE (title) WITH VECTOR (dense) LIMIT 10"
    );
}

#[test]
fn scroll_order_by_rejects_bad_start() {
    // START without FROM.
    assert!(Parser::parse("SCROLL FROM docs ORDER BY x START 5 LIMIT 10;").is_err());
    // Non-scalar start values fail closed.
    assert!(Parser::parse("SCROLL FROM docs ORDER BY x START FROM true LIMIT 10;").is_err());
    assert!(Parser::parse("SCROLL FROM docs ORDER BY x START FROM null LIMIT 10;").is_err());
    assert!(Parser::parse("SCROLL FROM docs ORDER BY x START FROM [1] LIMIT 10;").is_err());
    assert!(Parser::parse("SCROLL FROM docs ORDER BY x START FROM {a: 1} LIMIT 10;").is_err());
}

#[test]
fn scroll_payload_selectors() {
    assert_round_trip(
        "SCROLL FROM docs WITH PAYLOAD false LIMIT 10;",
        "SCROLL FROM docs WITH PAYLOAD false LIMIT 10",
    );
    assert_round_trip(
        "SCROLL FROM docs WITH PAYLOAD INCLUDE (title, url) LIMIT 10;",
        "SCROLL FROM docs WITH PAYLOAD INCLUDE (title, url) LIMIT 10",
    );
    assert_round_trip(
        "SCROLL FROM docs WITH PAYLOAD EXCLUDE (secret) LIMIT 10;",
        "SCROLL FROM docs WITH PAYLOAD EXCLUDE (secret) LIMIT 10",
    );
    // No clause means the default (all payload).
    let Stmt::Scroll(scroll) = parse("SCROLL FROM docs LIMIT 10;") else {
        panic!("expected SCROLL");
    };
    assert!(scroll.with_payload.is_none());
}

#[test]
fn query_order_by_start_from() {
    assert_round_trip(
        "QUERY ORDER BY created_at DESC START FROM '2024-01-01T00:00:00Z' FROM docs LIMIT 20;",
        "QUERY ORDER BY created_at DESC START FROM '2024-01-01T00:00:00Z'\nFROM docs\nLIMIT 20",
    );
    let Stmt::Query(query) = parse("QUERY ORDER BY score ASC START FROM 100 FROM docs LIMIT 5;")
    else {
        panic!("expected QUERY");
    };
    let QueryExpr::OrderBy {
        field,
        direction,
        start_from,
    } = &query.expression
    else {
        panic!("expected ORDER BY, got {:?}", query.expression);
    };
    assert_eq!(field, "score");
    assert_eq!(*direction, OrderDirection::Asc);
    assert_eq!(*start_from, Some(qql_core::ast::Value::Int(100)));
    // Bare ORDER BY without START FROM stays None.
    let Stmt::Query(plain) = parse("QUERY ORDER BY rank ASC FROM docs LIMIT 5;") else {
        panic!("expected QUERY");
    };
    assert!(matches!(
        plain.expression,
        QueryExpr::OrderBy {
            start_from: None,
            ..
        }
    ));
}

#[test]
fn group_lookup_full_selectors() {
    assert_round_trip(
        "QUERY TEXT 'news' MODEL 'e5' FROM docs GROUP BY topic SIZE 5 LOOKUP FROM topics WITH PAYLOAD INCLUDE (title) WITH VECTOR (dense) LIMIT 20;",
        "QUERY TEXT 'news' MODEL 'e5'\nFROM docs\nGROUP BY topic SIZE 5 LOOKUP FROM topics WITH PAYLOAD INCLUDE (title) WITH VECTOR (dense)\nLIMIT 20",
    );
    let Stmt::Query(query) = parse(
        "QUERY TEXT 'x' FROM docs GROUP BY topic LOOKUP FROM t WITH PAYLOAD false WITH VECTOR false LIMIT 5;",
    ) else {
        panic!("expected QUERY");
    };
    let group = query.group.as_ref().expect("group");
    let lookup = group.lookup.as_ref().expect("lookup");
    assert_eq!(lookup.collection, "t");
    assert_eq!(lookup.payload, Some(qql_core::ast::PayloadSelector::None));
    assert_eq!(lookup.vectors, Some(qql_core::ast::VectorSelector::None));
    // Bare lookup keeps both selectors absent.
    let Stmt::Query(bare) = parse("QUERY TEXT 'x' FROM docs GROUP BY topic LOOKUP FROM t LIMIT 5;")
    else {
        panic!("expected QUERY");
    };
    let lookup = bare
        .group
        .as_ref()
        .expect("group")
        .lookup
        .as_ref()
        .expect("lookup");
    assert!(lookup.payload.is_none());
    assert!(lookup.vectors.is_none());
}

#[test]
fn prefetch_lookup_shard_key() {
    assert_round_trip(
        "WITH a AS (QUERY TEXT 'x' MODEL 'e5' FROM docs USING dense LIMIT 50) QUERY FUSION RRF FROM docs PREFETCH (a LOOKUP FROM docs2 VECTOR dense SHARD 'acme') LIMIT 10;",
        "WITH\n  a AS (QUERY TEXT 'x' MODEL 'e5' FROM docs USING dense LIMIT 50)\nQUERY FUSION RRF\nFROM docs\nPREFETCH (a LOOKUP FROM docs2 VECTOR dense SHARD 'acme')\nLIMIT 10",
    );
    // Numeric shard keys survive as numbers.
    let Stmt::Query(query) = parse(
        "QUERY TEXT 'x' FROM docs USING dense PREFETCH (QUERY TEXT 'y' FROM docs USING dense LIMIT 5 LOOKUP FROM c SHARD 101) LIMIT 10;",
    ) else {
        panic!("expected QUERY");
    };
    let QueryExpr::Nearest { prefetch, .. } = &query.expression else {
        panic!("expected nearest, got {:?}", query.expression);
    };
    let lookup = prefetch[0].lookup.as_ref().expect("lookup");
    assert_eq!(lookup.collection, "c");
    assert_eq!(lookup.shard_key, Some(qql_core::ast::ShardKey::Number(101)));
}
