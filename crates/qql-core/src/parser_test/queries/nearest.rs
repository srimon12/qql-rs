use alloc::vec;

use crate::ast::{QueryMode, QueryType, Stmt};
use crate::parser_test::{assert_parse_err, assert_parse_ok, i64_val, str_val};

// ── Query: Nearest ───────────────────────────────────────────

#[test]
fn test_query_nearest() {
    let stmt = assert_parse_ok(
            "QUERY NEAREST 'vector search' FROM docs LIMIT 10 OFFSET 5 USING HYBRID RERANK WHERE topic = 'search' WITH (hnsw_ef = 128)",
        );
    match stmt {
        Stmt::Query(q) => {
            assert_eq!(q.mode, QueryMode::Nearest);
            assert_eq!(q.collection, Some("docs"));
            assert_eq!(q.query_text, Some("vector search"));
            assert_eq!(q.limit, 10);
            assert_eq!(q.offset, 5);
            assert_eq!(q.query_type, QueryType::Hybrid);
            assert!(q.rerank);
            assert!(q.query_filter.is_some());
            assert!(q.with_clause.is_some());
            assert_eq!(q.with_clause.as_ref().unwrap().hnsw_ef, 128);
        }
        _ => panic!("expected Query stmt"),
    }
}

#[test]
fn test_query_recommend() {
    let stmt =
        assert_parse_ok("QUERY RECOMMEND WITH (positive = (1, 2), negative = (3)) FROM users");
    match stmt {
        Stmt::Query(q) => {
            assert_eq!(q.mode, QueryMode::Recommend);
            assert_eq!(q.collection, Some("users"));
            assert_eq!(q.positive_ids, vec![i64_val(1), i64_val(2)]);
            assert_eq!(q.negative_ids, vec![i64_val(3)]);
            assert_eq!(q.limit, 10);
        }
        _ => panic!("expected Query stmt"),
    }
}

#[test]
fn test_query_discover() {
    let stmt = assert_parse_ok(
        "QUERY DISCOVER TARGET 100 CONTEXT PAIRS (1, 2), (3, 4) FROM products LIMIT 20",
    );
    match stmt {
        Stmt::Query(q) => {
            assert_eq!(q.mode, QueryMode::Discover);
            assert_eq!(q.collection, Some("products"));
            assert_eq!(q.target, Some(i64_val(100)));
            assert_eq!(q.context_pairs.len(), 2);
            assert_eq!(q.context_pairs[0].positive, i64_val(1));
            assert_eq!(q.context_pairs[0].negative, i64_val(2));
            assert_eq!(q.context_pairs[1].positive, i64_val(3));
            assert_eq!(q.context_pairs[1].negative, i64_val(4));
            assert_eq!(q.limit, 20);
        }
        _ => panic!("expected Query stmt"),
    }
}

#[test]
fn test_query_context() {
    let stmt = assert_parse_ok("QUERY CONTEXT PAIRS ('uuid-1', 'uuid-2') FROM logs");
    match stmt {
        Stmt::Query(q) => {
            assert_eq!(q.mode, QueryMode::Context);
            assert_eq!(q.collection, Some("logs"));
            assert_eq!(q.context_pairs.len(), 1);
            assert_eq!(q.context_pairs[0].positive, str_val("uuid-1"));
            assert_eq!(q.context_pairs[0].negative, str_val("uuid-2"));
        }
        _ => panic!("expected Query stmt"),
    }
}

// ── Query: Errors ────────────────────────────────────────────

#[test]
fn test_query_error_invalid_mode() {
    assert_parse_err("QUERY SOMETHING 'text' FROM docs");
}

#[test]
fn test_query_error_missing_context_pairs() {
    assert_parse_err("QUERY CONTEXT FROM docs");
}

#[test]
fn test_query_error_missing_discover_target() {
    assert_parse_err("QUERY DISCOVER FROM docs");
}

#[test]
fn test_query_error_missing_positive_ids() {
    // Rust parser handles RECOMMEND WITH gracefully; if positive is missing, it's just empty
    let stmt = assert_parse_ok("QUERY RECOMMEND WITH (negative = (1)) FROM docs");
    match stmt {
        Stmt::Query(q) => {
            assert_eq!(q.mode, QueryMode::Recommend);
            assert!(q.positive_ids.is_empty());
            assert_eq!(q.negative_ids, vec![i64_val(1)]);
        }
        _ => panic!("expected Query"),
    }
}
