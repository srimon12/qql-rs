use alloc::vec;

use crate::ast::{QueryMode, Stmt};
use crate::parser_test::{assert_parse_err, assert_parse_ok};

// ── Query: ORDER BY ──────────────────────────────────────────

#[test]
fn test_query_order_by() {
    let stmt = assert_parse_ok("QUERY ORDER BY timestamp ASC FROM logs LIMIT 100");
    match stmt {
        Stmt::Query(q) => {
            assert_eq!(q.mode, QueryMode::OrderBy);
            assert_eq!(q.order_by_field, Some(String::from("timestamp")));
            assert_eq!(q.order_by_asc, Some(true));
            assert_eq!(q.collection, Some(String::from("logs")));
            assert_eq!(q.limit, 100);
        }
        _ => panic!("expected Query stmt"),
    }
}

// ── Query: SAMPLE ────────────────────────────────────────────

#[test]
fn test_query_sample() {
    let stmt = assert_parse_ok("QUERY SAMPLE FROM docs LIMIT 10");
    match stmt {
        Stmt::Query(q) => {
            assert_eq!(q.mode, QueryMode::Sample);
            assert_eq!(q.collection, Some(String::from("docs")));
            assert_eq!(q.limit, 10);
        }
        _ => panic!("expected Query stmt"),
    }
}

#[test]
fn test_query_sample_with_filter() {
    let stmt = assert_parse_ok("QUERY SAMPLE FROM docs LIMIT 10 WHERE category = 'tech'");
    match stmt {
        Stmt::Query(q) => {
            assert_eq!(q.mode, QueryMode::Sample);
            assert_eq!(q.collection, Some(String::from("docs")));
            assert_eq!(q.limit, 10);
            assert!(q.query_filter.is_some());
        }
        _ => panic!("expected Query stmt"),
    }
}

// ── Query: WITH PAYLOAD / WITH VECTORS ───────────────────────

#[test]
fn test_query_with_payload_and_vectors() {
    let stmt = assert_parse_ok(
            "QUERY 'search' FROM docs WITH PAYLOAD (include = ['title'], exclude = ['metadata']) WITH VECTORS true",
        );
    match stmt {
        Stmt::Query(q) => {
            let wp = q.with_payload.as_ref().unwrap();
            assert_eq!(wp.include, vec!["title"]);
            assert_eq!(wp.exclude, vec!["metadata"]);
            assert!(wp.enable.is_none());
            let wv = q.with_vector.as_ref().unwrap();
            assert_eq!(wv.enable, Some(true));
            assert!(wv.vectors.is_empty());
        }
        _ => panic!("expected Query stmt"),
    }

    let stmt2 = assert_parse_ok(
        "QUERY 'search' FROM docs WITH PAYLOAD false WITH VECTORS ('dense', 'sparse')",
    );
    match stmt2 {
        Stmt::Query(q) => {
            let wp = q.with_payload.as_ref().unwrap();
            assert_eq!(wp.enable, Some(false));
            let wv = q.with_vector.as_ref().unwrap();
            assert!(wv.enable.is_none());
            assert_eq!(wv.vectors, vec!["dense", "sparse"]);
        }
        _ => panic!("expected Query stmt"),
    }
}

#[test]
fn test_query_multiple_with_clauses() {
    let stmt = assert_parse_ok(
            "QUERY 'search' FROM docs WITH MODEL 'foo' WITH PAYLOAD (include = ['title']) WITH VECTORS true WITH (exact = true)",
        );
    match stmt {
        Stmt::Query(q) => {
            assert_eq!(q.model, Some(String::from("foo")));
            let wp = q.with_payload.as_ref().unwrap();
            assert_eq!(wp.include, vec!["title"]);
            let wv = q.with_vector.as_ref().unwrap();
            assert_eq!(wv.enable, Some(true));
            let wc = q.with_clause.as_ref().unwrap();
            assert!(wc.exact);
        }
        _ => panic!("expected Query stmt"),
    }
}

// ── Query: WITH PAYLOAD/VECTORS errors ───────────────────────

#[test]
fn test_query_with_payload_vectors_errors() {
    assert_parse_err("QUERY FROM docs WITH PAYLOAD (badkey = ['a'])");
    assert_parse_err("QUERY FROM docs WITH PAYLOAD include = ['a']");
    assert_parse_err("QUERY FROM docs WITH PAYLOAD (include = 'a')");
    assert_parse_err("QUERY FROM docs WITH PAYLOAD (include ['a'])");
    assert_parse_err("QUERY FROM docs WITH PAYLOAD (include = [123])");
    assert_parse_err("QUERY FROM docs WITH PAYLOAD (include = ['a'");
    assert_parse_err("QUERY FROM docs WITH PAYLOAD (include = ['a']");
    assert_parse_err("QUERY FROM docs WITH VECTORS (123)");
    assert_parse_err("QUERY FROM docs WITH VECTORS ('dense'");
    assert_parse_err("QUERY FROM docs WITH VECTORS (['dense'])");
    assert_parse_err("QUERY ORDER BY FROM docs");
    assert_parse_err("QUERY ORDER timestamp FROM docs");
}

// ── Query: Raw Vector ────────────────────────────────────────

#[test]
fn test_query_raw_vector() {
    let stmt = assert_parse_ok("QUERY [0.1, 0.2, 0.3] FROM docs LIMIT 5");
    match stmt {
        Stmt::Query(q) => {
            assert_eq!(q.collection, Some(String::from("docs")));
            assert_eq!(q.raw_vector, vec![0.1, 0.2, 0.3]);
            assert_eq!(q.limit, 5);
        }
        _ => panic!("expected Query stmt"),
    }
}
