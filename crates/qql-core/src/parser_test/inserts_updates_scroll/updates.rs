use alloc::boxed::Box;
use alloc::vec;

use crate::ast::{FilterExpr, Stmt};
use crate::parser_test::{assert_parse_err, assert_parse_ok, i64_val, str_val};

// ── UPDATE ───────────────────────────────────────────────────

#[test]
fn test_update_vector_by_id() {
    let stmt = assert_parse_ok("UPDATE articles SET VECTOR = [0.1, 0.2] WHERE id = 42");
    match stmt {
        Stmt::UpdateVector(u) => {
            assert_eq!(u.collection, "articles");
            assert_eq!(u.point_id, i64_val(42));
            assert_eq!(u.vector, vec![0.1f32, 0.2f32]);
            assert!(u.vector_name.is_none());
        }
        _ => panic!("expected UpdateVector"),
    }
}

#[test]
fn test_update_payload_by_filter() {
    let stmt = assert_parse_ok(
        "UPDATE articles SET PAYLOAD = {'status': 'published'} WHERE category = 'draft'",
    );
    match stmt {
        Stmt::UpdatePayload(u) => {
            assert_eq!(u.collection, "articles");
            assert!(u.point_id.is_none());
            assert_eq!(
                u.query_filter,
                Some(Box::new(FilterExpr::Compare {
                    field: "category",
                    op: "=",
                    value: str_val("draft"),
                }))
            );
            assert_eq!(u.payload, vec![("status", str_val("published"))]);
        }
        _ => panic!("expected UpdatePayload"),
    }
}

#[test]
fn test_update_custom_named_vector_by_id() {
    let stmt = assert_parse_ok("UPDATE articles SET VECTOR 'colbert' = [0.1, 0.2] WHERE id = 42");
    match stmt {
        Stmt::UpdateVector(u) => {
            assert_eq!(u.collection, "articles");
            assert_eq!(u.point_id, i64_val(42));
            assert_eq!(u.vector, vec![0.1f32, 0.2f32]);
            assert_eq!(u.vector_name, Some("colbert"));
        }
        _ => panic!("expected UpdateVector"),
    }
}

#[test]
fn test_update_payload_by_id() {
    let stmt = assert_parse_ok("UPDATE articles SET PAYLOAD = {'year': 2025} WHERE id = 'abc-123'");
    match stmt {
        Stmt::UpdatePayload(u) => {
            assert_eq!(u.collection, "articles");
            assert_eq!(u.point_id, Some(str_val("abc-123")));
            assert_eq!(u.payload, vec![("year", i64_val(2025))]);
        }
        _ => panic!("expected UpdatePayload"),
    }
}

#[test]
fn test_update_vector_rejects_bools() {
    assert_parse_err("UPDATE articles SET VECTOR = [true, 0.2] WHERE id = 1");
}

#[test]
fn test_update_rejects_invalid_target() {
    assert_parse_err("UPDATE articles SET NAME = {'x': 1} WHERE id = 1");
}

#[test]
fn test_parse_update() {
    let stmt = assert_parse_ok("UPDATE docs SET VECTOR = [1.0, 2.0] WHERE id = 1");
    match stmt {
        Stmt::UpdateVector(_) => {}
        _ => panic!("expected UpdateVector"),
    }
}
