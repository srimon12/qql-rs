use alloc::string::ToString;

use crate::ast::Stmt;
use crate::parser_test::{assert_parse_err, assert_parse_ok, parse};

// ── MANAGE: ALTER ────────────────────────────────────────────

#[test]
fn test_alter_collection() {
    let stmt = assert_parse_ok("ALTER COLLECTION docs WITH HNSW (m = 32)");
    match stmt {
        Stmt::AlterCollection(a) => {
            assert_eq!(a.collection, "docs");
            assert!(a.config.is_some());
        }
        _ => panic!("expected AlterCollection"),
    }
}

#[test]
fn test_alter_rejects_non_positive_values() {
    let err =
        parse("ALTER COLLECTION docs WITH PARAMS ( read_fan_out_delay_ms = -1 )").unwrap_err();
    assert!(
        err.to_string()
            .contains("read_fan_out_delay_ms must be a non-negative integer"),
        "got: {}",
        err
    );
}

// ── MANAGE: DROP ─────────────────────────────────────────────

#[test]
fn test_drop_collection() {
    let stmt = assert_parse_ok("DROP COLLECTION mycollection");
    match stmt {
        Stmt::DropCollection(d) => {
            assert_eq!(d.collection, "mycollection");
        }
        _ => panic!("expected DropCollection"),
    }
}

// ── MANAGE: SHOW ─────────────────────────────────────────────

#[test]
fn test_show_collections() {
    let stmt = assert_parse_ok("SHOW COLLECTIONS");
    match stmt {
        Stmt::ShowCollections => {}
        _ => panic!("expected ShowCollections"),
    }
}

#[test]
fn test_show_collection_simple() {
    let stmt = assert_parse_ok("SHOW COLLECTION docs");
    match stmt {
        Stmt::ShowCollection(c) => {
            assert_eq!(c, "docs");
        }
        _ => panic!("expected ShowCollection"),
    }
}

#[test]
fn test_show_collection_case_insensitive() {
    let stmt = assert_parse_ok("show collection MY_COL");
    match stmt {
        Stmt::ShowCollection(c) => {
            assert_eq!(c, "MY_COL");
        }
        _ => panic!("expected ShowCollection"),
    }
}

#[test]
fn test_show_collection_error_without_name() {
    assert_parse_err("SHOW COLLECTION");
}
