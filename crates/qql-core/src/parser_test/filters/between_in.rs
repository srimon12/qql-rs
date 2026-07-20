use alloc::boxed::Box;
use alloc::vec;

use crate::ast::{FilterExpr, Stmt};
use crate::parser_test::{assert_parse_ok, i64_val, str_val};

// ── Filter: Between ──────────────────────────────────────────

#[test]
fn test_filter_between() {
    let stmt = assert_parse_ok("SCROLL FROM c WHERE age BETWEEN 18 AND 65 LIMIT 10");
    match stmt {
        Stmt::Scroll(s) => {
            assert_eq!(
                s.query_filter,
                Some(Box::new(FilterExpr::Between {
                    field: String::from("age"),
                    low: i64_val(18),
                    high: i64_val(65),
                }))
            );
        }
        _ => panic!("expected Scroll stmt"),
    }
}

// ── Filter: IN ───────────────────────────────────────────────

#[test]
fn test_filter_in() {
    let stmt = assert_parse_ok("SCROLL FROM c WHERE status IN ('active', 'pending') LIMIT 10");
    match stmt {
        Stmt::Scroll(s) => {
            assert_eq!(
                s.query_filter,
                Some(Box::new(FilterExpr::In {
                    field: String::from("status"),
                    values: vec![str_val("active"), str_val("pending")],
                }))
            );
        }
        _ => panic!("expected Scroll stmt"),
    }
}

// ── Filter: NOT IN ───────────────────────────────────────────

#[test]
fn test_filter_not_in() {
    let stmt =
        assert_parse_ok("SCROLL FROM c WHERE status NOT IN ('deleted', 'archived') LIMIT 10");
    match stmt {
        Stmt::Scroll(s) => {
            assert_eq!(
                s.query_filter,
                Some(Box::new(FilterExpr::NotIn {
                    field: String::from("status"),
                    values: vec![str_val("deleted"), str_val("archived")],
                }))
            );
        }
        _ => panic!("expected Scroll stmt"),
    }
}

// ── Filter: IS NULL / IS NOT NULL / IS EMPTY / IS NOT EMPTY ──

#[test]
fn test_filter_is_null() {
    let stmt = assert_parse_ok("SCROLL FROM c WHERE field IS NULL LIMIT 10");
    match stmt {
        Stmt::Scroll(s) => {
            assert_eq!(
                s.query_filter,
                Some(Box::new(FilterExpr::IsNull {
                    field: String::from("field")
                }))
            );
        }
        _ => panic!("expected Scroll stmt"),
    }
}

#[test]
fn test_filter_is_not_null() {
    let stmt = assert_parse_ok("SCROLL FROM c WHERE field IS NOT NULL LIMIT 10");
    match stmt {
        Stmt::Scroll(s) => {
            assert_eq!(
                s.query_filter,
                Some(Box::new(FilterExpr::IsNotNull {
                    field: String::from("field")
                }))
            );
        }
        _ => panic!("expected Scroll stmt"),
    }
}

#[test]
fn test_filter_is_empty() {
    let stmt = assert_parse_ok("SCROLL FROM c WHERE field IS EMPTY LIMIT 10");
    match stmt {
        Stmt::Scroll(s) => {
            assert_eq!(
                s.query_filter,
                Some(Box::new(FilterExpr::IsEmpty {
                    field: String::from("field")
                }))
            );
        }
        _ => panic!("expected Scroll stmt"),
    }
}

#[test]
fn test_filter_is_not_empty() {
    let stmt = assert_parse_ok("SCROLL FROM c WHERE field IS NOT EMPTY LIMIT 10");
    match stmt {
        Stmt::Scroll(s) => {
            assert_eq!(
                s.query_filter,
                Some(Box::new(FilterExpr::IsNotEmpty {
                    field: String::from("field")
                }))
            );
        }
        _ => panic!("expected Scroll stmt"),
    }
}

// ── Filter: MATCH ────────────────────────────────────────────

#[test]
fn test_filter_match_text() {
    let stmt = assert_parse_ok("SCROLL FROM c WHERE content MATCH 'hello world' LIMIT 10");
    match stmt {
        Stmt::Scroll(s) => {
            assert_eq!(
                s.query_filter,
                Some(Box::new(FilterExpr::MatchText {
                    field: String::from("content"),
                    text: String::from("hello world"),
                }))
            );
        }
        _ => panic!("expected Scroll stmt"),
    }
}

#[test]
fn test_filter_match_any() {
    let stmt = assert_parse_ok("SCROLL FROM c WHERE content MATCH ANY 'hello world' LIMIT 10");
    match stmt {
        Stmt::Scroll(s) => {
            assert_eq!(
                s.query_filter,
                Some(Box::new(FilterExpr::MatchAny {
                    field: String::from("content"),
                    text: String::from("hello world"),
                }))
            );
        }
        _ => panic!("expected Scroll stmt"),
    }
}

#[test]
fn test_filter_match_phrase() {
    let stmt = assert_parse_ok("SCROLL FROM c WHERE content MATCH PHRASE 'hello world' LIMIT 10");
    match stmt {
        Stmt::Scroll(s) => {
            assert_eq!(
                s.query_filter,
                Some(Box::new(FilterExpr::MatchPhrase {
                    field: String::from("content"),
                    text: String::from("hello world"),
                }))
            );
        }
        _ => panic!("expected Scroll stmt"),
    }
}
