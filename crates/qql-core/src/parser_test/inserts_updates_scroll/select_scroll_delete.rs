use alloc::boxed::Box;

use crate::ast::{FilterExpr, Stmt};
use crate::parser_test::{assert_parse_err, assert_parse_ok, i64_val, str_val};

// ── DELETE ───────────────────────────────────────────────────

#[test]
fn test_delete_with_string_id() {
    let stmt = assert_parse_ok("DELETE FROM mycollection WHERE id = 'point-123'");
    match stmt {
        Stmt::Delete(d) => {
            assert_eq!(d.collection, "mycollection");
            assert_eq!(d.point_id, Some(str_val("point-123")));
        }
        _ => panic!("expected Delete"),
    }
}

#[test]
fn test_delete_with_integer_id() {
    let stmt = assert_parse_ok("DELETE FROM mycollection WHERE id = 42");
    match stmt {
        Stmt::Delete(d) => {
            assert_eq!(d.collection, "mycollection");
            assert_eq!(d.point_id, Some(i64_val(42)));
        }
        _ => panic!("expected Delete"),
    }
}

#[test]
fn test_delete_by_field() {
    let stmt = assert_parse_ok("DELETE FROM mycollection WHERE status = 'archived'");
    match stmt {
        Stmt::Delete(d) => {
            assert_eq!(d.collection, "mycollection");
            assert_eq!(d.field, Some("status"));
            assert_eq!(d.value, Some(str_val("archived")));
        }
        _ => panic!("expected Delete"),
    }
}

// ── SELECT ───────────────────────────────────────────────────

#[test]
fn test_select_with_string_id() {
    let stmt = assert_parse_ok("SELECT * FROM docs WHERE id = 'point-123'");
    match stmt {
        Stmt::Select(s) => {
            assert_eq!(s.collection, "docs");
            assert_eq!(s.point_id, str_val("point-123"));
        }
        _ => panic!("expected Select"),
    }
}

#[test]
fn test_select_with_integer_id() {
    let stmt = assert_parse_ok("SELECT * FROM docs WHERE id = 42");
    match stmt {
        Stmt::Select(s) => {
            assert_eq!(s.collection, "docs");
            assert_eq!(s.point_id, i64_val(42));
        }
        _ => panic!("expected Select"),
    }
}

// ── SCROLL ───────────────────────────────────────────────────

#[test]
fn test_scroll_basic() {
    let stmt = assert_parse_ok("SCROLL FROM docs LIMIT 10");
    match stmt {
        Stmt::Scroll(s) => {
            assert_eq!(s.collection, "docs");
            assert_eq!(s.limit, 10);
            assert!(s.query_filter.is_none());
            assert!(s.after.is_none());
        }
        _ => panic!("expected Scroll"),
    }
}

#[test]
fn test_scroll_rejects_non_positive_limit() {
    assert_parse_err("SCROLL FROM docs LIMIT 0");
    assert_parse_err("SCROLL FROM docs LIMIT -1");
}

#[test]
fn test_scroll_with_where() {
    let stmt = assert_parse_ok("SCROLL FROM docs WHERE status = 'active' LIMIT 5");
    match stmt {
        Stmt::Scroll(s) => {
            assert_eq!(s.collection, "docs");
            assert_eq!(s.limit, 5);
            assert_eq!(
                s.query_filter,
                Some(Box::new(FilterExpr::Compare {
                    field: "status",
                    op: "=",
                    value: str_val("active"),
                }))
            );
        }
        _ => panic!("expected Scroll"),
    }
}

#[test]
fn test_scroll_with_after() {
    let stmt = assert_parse_ok("SCROLL FROM docs AFTER 'point-123' LIMIT 10");
    match stmt {
        Stmt::Scroll(s) => {
            assert_eq!(s.collection, "docs");
            assert_eq!(s.limit, 10);
            assert_eq!(s.after, Some(str_val("point-123")));
        }
        _ => panic!("expected Scroll"),
    }
}

#[test]
fn test_scroll_with_after_integer() {
    let stmt = assert_parse_ok("SCROLL FROM docs AFTER 42 LIMIT 10");
    match stmt {
        Stmt::Scroll(s) => {
            assert_eq!(s.collection, "docs");
            assert_eq!(s.limit, 10);
            assert_eq!(s.after, Some(i64_val(42)));
        }
        _ => panic!("expected Scroll"),
    }
}

#[test]
fn test_scroll_with_where_and_after() {
    let stmt =
        assert_parse_ok("SCROLL FROM docs WHERE status = 'active' AFTER 'point-50' LIMIT 20");
    match stmt {
        Stmt::Scroll(s) => {
            assert_eq!(s.collection, "docs");
            assert_eq!(s.limit, 20);
            assert_eq!(
                s.query_filter,
                Some(Box::new(FilterExpr::Compare {
                    field: "status",
                    op: "=",
                    value: str_val("active"),
                }))
            );
            assert_eq!(s.after, Some(str_val("point-50")));
        }
        _ => panic!("expected Scroll"),
    }
}

#[test]
fn test_parse_select() {
    let stmt = assert_parse_ok("SELECT * FROM docs WHERE id = 'abc'");
    match stmt {
        Stmt::Select(_) => {}
        _ => panic!("expected Select"),
    }
}

#[test]
fn test_parse_scroll() {
    let stmt = assert_parse_ok("SCROLL FROM docs LIMIT 10");
    match stmt {
        Stmt::Scroll(_) => {}
        _ => panic!("expected Scroll"),
    }
}

#[test]
fn test_parse_delete() {
    let stmt = assert_parse_ok("DELETE FROM docs WHERE id = 'x'");
    match stmt {
        Stmt::Delete(_) => {}
        _ => panic!("expected Delete"),
    }
}

// ── Delete with filter (parse delete fallback) ────────────────

#[test]
fn test_delete_with_filter() {
    let stmt = assert_parse_ok("DELETE FROM docs WHERE id = 'xyz'");
    match stmt {
        Stmt::Delete(d) => {
            assert_eq!(d.collection, "docs");
            assert_eq!(d.point_id, Some(str_val("xyz")));
        }
        _ => panic!("expected Delete"),
    }
}
