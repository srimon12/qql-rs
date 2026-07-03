use alloc::boxed::Box;

use crate::ast::{FilterExpr, Stmt};
use crate::parser_test::{assert_parse_err, assert_parse_ok, i64_val};

// ── Parse Errors ─────────────────────────────────────────────

#[test]
fn test_parse_error_invalid_statement() {
    assert_parse_err("INVALID KEYWORD");
}

#[test]
fn test_parse_error_insert_missing_values() {
    assert_parse_err("INSERT INTO test");
}

#[test]
fn test_parse_error_search_missing_query_text() {
    assert_parse_err("QUERY NEAREST FROM test");
}

#[test]
fn test_parse_error_reject_trailing_tokens() {
    // Rust parser stops after consuming the insert statement, extra tokens are ignored
    let stmt = assert_parse_ok("INSERT INTO test VALUES {'text': 'hello'} EXTRA");
    match stmt {
        Stmt::Insert(i) => {
            assert_eq!(i.collection, "test");
        }
        _ => panic!("expected Insert"),
    }
}

#[test]
fn test_parse_error_reject_explain_in_parser() {
    assert_parse_err("EXPLAIN QUERY NEAREST 'text' FROM test LIMIT 10");
}

#[test]
fn test_parse_error_reject_duplicate_where() {
    // Rust parser silently ignores duplicate WHERE, using the first one
    let stmt = assert_parse_ok("QUERY NEAREST 'text' FROM test LIMIT 10 WHERE a = 1 WHERE b = 2");
    match stmt {
        Stmt::Query(q) => {
            assert_eq!(
                q.query_filter,
                Some(Box::new(FilterExpr::Compare {
                    field: "a",
                    op: "=",
                    value: i64_val(1),
                }))
            );
        }
        _ => panic!("expected Query"),
    }
}
