use alloc::boxed::Box;

use crate::ast::{FilterExpr, Stmt};
use crate::parser_test::{assert_parse_ok, float_val, i64_val, str_val};

// ── Filter: Comparisons ──────────────────────────────────────

#[test]
fn test_filter_equals() {
    let stmt = assert_parse_ok("SCROLL FROM c WHERE field = 'value' LIMIT 10");
    match stmt {
        Stmt::Scroll(s) => {
            assert_eq!(
                s.query_filter,
                Some(Box::new(FilterExpr::Compare {
                    field: "field",
                    op: "=",
                    value: str_val("value"),
                }))
            );
        }
        _ => panic!("expected Scroll stmt"),
    }
}

#[test]
fn test_filter_not_equals() {
    let stmt = assert_parse_ok("SCROLL FROM c WHERE field != 'value' LIMIT 10");
    match stmt {
        Stmt::Scroll(s) => {
            assert_eq!(
                s.query_filter,
                Some(Box::new(FilterExpr::Compare {
                    field: "field",
                    op: "!=",
                    value: str_val("value"),
                }))
            );
        }
        _ => panic!("expected Scroll stmt"),
    }
}

#[test]
fn test_filter_greater_than() {
    let stmt = assert_parse_ok("SCROLL FROM c WHERE count > 5 LIMIT 10");
    match stmt {
        Stmt::Scroll(s) => {
            assert_eq!(
                s.query_filter,
                Some(Box::new(FilterExpr::Compare {
                    field: "count",
                    op: ">",
                    value: i64_val(5),
                }))
            );
        }
        _ => panic!("expected Scroll stmt"),
    }
}

#[test]
fn test_filter_greater_than_or_equals() {
    let stmt = assert_parse_ok("SCROLL FROM c WHERE count >= 5 LIMIT 10");
    match stmt {
        Stmt::Scroll(s) => {
            assert_eq!(
                s.query_filter,
                Some(Box::new(FilterExpr::Compare {
                    field: "count",
                    op: ">=",
                    value: i64_val(5),
                }))
            );
        }
        _ => panic!("expected Scroll stmt"),
    }
}

#[test]
fn test_filter_less_than() {
    let stmt = assert_parse_ok("SCROLL FROM c WHERE count < 10 LIMIT 10");
    match stmt {
        Stmt::Scroll(s) => {
            assert_eq!(
                s.query_filter,
                Some(Box::new(FilterExpr::Compare {
                    field: "count",
                    op: "<",
                    value: i64_val(10),
                }))
            );
        }
        _ => panic!("expected Scroll stmt"),
    }
}

#[test]
fn test_filter_less_than_or_equals() {
    let stmt = assert_parse_ok("SCROLL FROM c WHERE count <= 10 LIMIT 10");
    match stmt {
        Stmt::Scroll(s) => {
            assert_eq!(
                s.query_filter,
                Some(Box::new(FilterExpr::Compare {
                    field: "count",
                    op: "<=",
                    value: i64_val(10),
                }))
            );
        }
        _ => panic!("expected Scroll stmt"),
    }
}

#[test]
fn test_filter_equals_integer() {
    let stmt = assert_parse_ok("SCROLL FROM c WHERE count = 42 LIMIT 10");
    match stmt {
        Stmt::Scroll(s) => {
            assert_eq!(
                s.query_filter,
                Some(Box::new(FilterExpr::Compare {
                    field: "count",
                    op: "=",
                    value: i64_val(42),
                }))
            );
        }
        _ => panic!("expected Scroll stmt"),
    }
}

#[test]
fn test_filter_equals_float() {
    let stmt = assert_parse_ok("SCROLL FROM c WHERE score = 12.34 LIMIT 10");
    match stmt {
        Stmt::Scroll(s) => {
            assert_eq!(
                s.query_filter,
                Some(Box::new(FilterExpr::Compare {
                    field: "score",
                    op: "=",
                    value: float_val(12.34),
                }))
            );
        }
        _ => panic!("expected Scroll stmt"),
    }
}
