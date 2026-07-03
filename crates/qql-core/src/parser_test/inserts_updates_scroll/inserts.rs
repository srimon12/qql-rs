use alloc::vec;

use crate::ast::Stmt;
use crate::parser_test::{assert_parse_err, assert_parse_ok, make_payload, str_val};

// ── INSERT ───────────────────────────────────────────────────

#[test]
fn test_insert_simple() {
    let stmt = assert_parse_ok("INSERT INTO test VALUES {'text': 'hello'}");
    match stmt {
        Stmt::Insert(i) => {
            assert_eq!(i.collection, "test");
            assert_eq!(i.values_list, vec![vec![("text", str_val("hello"))]]);
        }
        _ => panic!("expected Insert"),
    }
}

#[test]
fn test_insert_with_bare_keys() {
    let stmt = assert_parse_ok("INSERT INTO test VALUES {text: 'hello', topic: 'search'}");
    match stmt {
        Stmt::Insert(i) => {
            assert_eq!(i.collection, "test");
            assert_eq!(
                i.values_list,
                vec![make_payload(&[
                    ("text", str_val("hello")),
                    ("topic", str_val("search"))
                ])]
            );
        }
        _ => panic!("expected Insert"),
    }
}

#[test]
fn test_insert_with_explicit_id() {
    let stmt = assert_parse_ok("INSERT INTO test VALUES {id: 'point-123', text: 'hello'}");
    match stmt {
        Stmt::Insert(i) => {
            assert_eq!(i.collection, "test");
            assert_eq!(
                i.values_list,
                vec![make_payload(&[
                    ("id", str_val("point-123")),
                    ("text", str_val("hello")),
                ])]
            );
        }
        _ => panic!("expected Insert"),
    }
}

#[test]
fn test_insert_with_model() {
    let stmt =
        assert_parse_ok("INSERT INTO test VALUES {'text': 'hello'} USING MODEL 'model-name'");
    match stmt {
        Stmt::Insert(i) => {
            assert_eq!(i.collection, "test");
            assert_eq!(i.model, Some("model-name"));
            assert!(!i.hybrid);
        }
        _ => panic!("expected Insert"),
    }
}

#[test]
fn test_insert_with_hybrid() {
    let stmt = assert_parse_ok("INSERT INTO test VALUES {'text': 'hello'} USING HYBRID");
    match stmt {
        Stmt::Insert(i) => {
            assert_eq!(i.collection, "test");
            assert!(i.hybrid);
        }
        _ => panic!("expected Insert"),
    }
}

#[test]
fn test_insert_with_hybrid_and_models() {
    let stmt = assert_parse_ok(
            "INSERT INTO test VALUES {'text': 'hello'} USING HYBRID DENSE MODEL 'dense-model' SPARSE MODEL 'sparse-model'",
        );
    match stmt {
        Stmt::Insert(i) => {
            assert_eq!(i.collection, "test");
            assert!(i.hybrid);
            assert_eq!(i.model, Some("dense-model"));
            assert_eq!(i.sparse_model, Some("sparse-model"));
        }
        _ => panic!("expected Insert"),
    }
}

#[test]
fn test_insert_with_sparse_model_only() {
    let stmt = assert_parse_ok(
        "INSERT INTO test VALUES {'text': 'hello'} USING HYBRID SPARSE MODEL 'sparse-model'",
    );
    match stmt {
        Stmt::Insert(i) => {
            assert_eq!(i.collection, "test");
            assert!(i.hybrid);
            assert_eq!(i.sparse_model, Some("sparse-model"));
        }
        _ => panic!("expected Insert"),
    }
}

#[test]
fn test_insert_multiple_values() {
    let stmt = assert_parse_ok("INSERT INTO test VALUES {'text': 'hello'}, {'text': 'world'}");
    match stmt {
        Stmt::Insert(i) => {
            assert_eq!(i.collection, "test");
            assert_eq!(
                i.values_list,
                vec![
                    vec![("text", str_val("hello"))],
                    vec![("text", str_val("world"))],
                ]
            );
        }
        _ => panic!("expected Insert"),
    }
}

// ── INSERT with EMBED ────────────────────────────────────────

#[test]
fn test_insert_embed_single() {
    let stmt = assert_parse_ok(
            "INSERT INTO arxiv VALUES {id: 'p1', text: 'chunk', title: 'Paper'} EMBED text INTO dense_chunk",
        );
    match stmt {
        Stmt::Insert(i) => {
            assert_eq!(i.collection, "arxiv");
            assert_eq!(i.embed_directives.len(), 1);
            assert_eq!(i.embed_directives[0].source_field, "text");
            assert_eq!(i.embed_directives[0].target_vector, "dense_chunk");
        }
        _ => panic!("expected Insert"),
    }
}

#[test]
fn test_insert_embed_multiple() {
    let stmt = assert_parse_ok(
            "INSERT INTO arxiv VALUES {id: 'p1', text: 'chunk', title: 'Paper', abstract: 'Full abstract'} EMBED text INTO dense_chunk, title INTO dense_title, abstract INTO dense_abstract",
        );
    match stmt {
        Stmt::Insert(i) => {
            assert_eq!(i.collection, "arxiv");
            assert_eq!(i.embed_directives.len(), 3);
            assert_eq!(i.embed_directives[0].source_field, "text");
            assert_eq!(i.embed_directives[0].target_vector, "dense_chunk");
            assert_eq!(i.embed_directives[1].source_field, "title");
            assert_eq!(i.embed_directives[1].target_vector, "dense_title");
            assert_eq!(i.embed_directives[2].source_field, "abstract");
            assert_eq!(i.embed_directives[2].target_vector, "dense_abstract");
        }
        _ => panic!("expected Insert"),
    }
}

#[test]
fn test_insert_embed_with_sparse() {
    let stmt = assert_parse_ok(
            "INSERT INTO arxiv VALUES {id: 'p1', title: 'Paper'} EMBED title INTO sparse_title USING SPARSE",
        );
    match stmt {
        Stmt::Insert(i) => {
            assert_eq!(i.collection, "arxiv");
            assert_eq!(i.embed_directives.len(), 1);
            assert_eq!(i.embed_directives[0].source_field, "title");
            assert_eq!(i.embed_directives[0].target_vector, "sparse_title");
            assert_eq!(i.embed_directives[0].sparse_model, Some(""));
        }
        _ => panic!("expected Insert"),
    }
}

#[test]
fn test_insert_embed_with_explicit_model() {
    let stmt = assert_parse_ok(
            "INSERT INTO arxiv VALUES {id: 'p1', title: 'Paper'} EMBED title INTO dense_title USING MODEL 'custom-model'",
        );
    match stmt {
        Stmt::Insert(i) => {
            assert_eq!(i.collection, "arxiv");
            assert_eq!(i.embed_directives.len(), 1);
            assert_eq!(i.embed_directives[0].source_field, "title");
            assert_eq!(i.embed_directives[0].target_vector, "dense_title");
            assert_eq!(i.embed_directives[0].model, Some("custom-model"));
        }
        _ => panic!("expected Insert"),
    }
}

#[test]
fn test_insert_embed_mixed_dense_sparse() {
    let stmt = assert_parse_ok(
            "INSERT INTO arxiv VALUES {id: 'p1', text: 'chunk', title: 'Paper'} EMBED text INTO dense_chunk, title INTO sparse_title USING SPARSE",
        );
    match stmt {
        Stmt::Insert(i) => {
            assert_eq!(i.collection, "arxiv");
            assert_eq!(i.embed_directives.len(), 2);
            assert_eq!(i.embed_directives[0].source_field, "text");
            assert_eq!(i.embed_directives[0].target_vector, "dense_chunk");
            assert_eq!(i.embed_directives[0].sparse_model, None);
            assert_eq!(i.embed_directives[1].source_field, "title");
            assert_eq!(i.embed_directives[1].target_vector, "sparse_title");
            assert_eq!(i.embed_directives[1].sparse_model, Some(""));
        }
        _ => panic!("expected Insert"),
    }
}

// ── INSERT Errors ────────────────────────────────────────────

#[test]
fn test_insert_errors() {
    assert_parse_err("INSERT INTO test VALUES");
}

// ── QUERY: Simple basic parse ────────────────────────────────

#[test]
fn test_parse_insert() {
    let stmt = assert_parse_ok("INSERT INTO test VALUES {'text': 'hello'}");
    match stmt {
        Stmt::Insert(_) => {}
        _ => panic!("expected Insert"),
    }
}
