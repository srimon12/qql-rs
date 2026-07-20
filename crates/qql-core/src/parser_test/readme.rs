use alloc::boxed::Box;
use alloc::vec;

use crate::ast::{FilterExpr, Stmt};
use crate::parser_test::{assert_parse_ok, str_val};

// ── Documented examples ──────────────────────────────────────

#[test]
fn test_readme_create_hybrid_collection() {
    let stmt = assert_parse_ok("CREATE COLLECTION docs HYBRID");
    match stmt {
        Stmt::CreateCollection(c) => {
            assert_eq!(c.collection, "docs");
            assert!(c.hybrid);
        }
        _ => panic!("expected CreateCollection stmt"),
    }
}

#[test]
fn test_readme_create_hybrid_rerank_collection() {
    let stmt = assert_parse_ok("CREATE COLLECTION docs HYBRID RERANK");
    match stmt {
        Stmt::CreateCollection(c) => {
            assert_eq!(c.collection, "docs");
            assert!(c.hybrid);
            assert!(c.rerank);
        }
        _ => panic!("expected CreateCollection stmt"),
    }
}

#[test]
fn test_readme_hybrid_upsert() {
    let stmt = assert_parse_ok(
        "UPSERT INTO docs VALUES {'text': 'Qdrant stores vectors', 'topic': 'search'} USING HYBRID",
    );
    match stmt {
        Stmt::Upsert(i) => {
            assert_eq!(i.collection, "docs");
            assert!(i.hybrid);
            assert_eq!(
                i.values_list,
                vec![vec![
                    (String::from("text"), str_val("Qdrant stores vectors")),
                    (String::from("topic"), str_val("search")),
                ]]
            );
        }
        _ => panic!("expected Insert stmt"),
    }
}

#[test]
fn test_readme_hybrid_search() {
    let stmt = assert_parse_ok("QUERY NEAREST 'vector database' FROM docs LIMIT 5 USING HYBRID");
    match stmt {
        Stmt::Query(q) => {
            assert_eq!(q.collection, Some(String::from("docs")));
            assert_eq!(q.query_text, Some(String::from("vector database")));
            assert_eq!(q.limit, 5);
        }
        _ => panic!("expected Query stmt"),
    }
}

#[test]
fn test_readme_hybrid_search_with_filter() {
    let stmt = assert_parse_ok(
        "QUERY NEAREST 'vector search' FROM notes LIMIT 5 USING HYBRID WHERE topic = 'search'",
    );
    match stmt {
        Stmt::Query(q) => {
            assert_eq!(q.collection, Some(String::from("notes")));
            assert_eq!(
                q.query_filter,
                Some(Box::new(FilterExpr::Compare {
                    field: String::from("topic"),
                    op: String::from("="),
                    value: str_val("search"),
                }))
            );
        }
        _ => panic!("expected Query stmt"),
    }
}

#[test]
fn test_readme_hybrid_rerank_search() {
    let stmt =
        assert_parse_ok("QUERY NEAREST 'vector database' FROM docs LIMIT 5 USING HYBRID RERANK");
    match stmt {
        Stmt::Query(q) => {
            assert_eq!(q.collection, Some(String::from("docs")));
        }
        _ => panic!("expected Query stmt"),
    }
}

#[test]
fn test_readme_delete_by_id() {
    let stmt = assert_parse_ok("DELETE FROM notes WHERE id = 'uuid'");
    match stmt {
        Stmt::Delete(d) => {
            assert_eq!(d.collection, "notes");
            assert_eq!(d.point_id, Some(str_val("uuid")));
        }
        _ => panic!("expected Delete stmt"),
    }
}

#[test]
fn test_readme_delete_by_field() {
    let stmt = assert_parse_ok("DELETE FROM notes WHERE specialty = 'search'");
    match stmt {
        Stmt::Delete(d) => {
            assert_eq!(d.collection, "notes");
            assert_eq!(d.field, Some(String::from("specialty")));
            assert_eq!(d.value, Some(str_val("search")));
        }
        _ => panic!("expected Delete stmt"),
    }
}
