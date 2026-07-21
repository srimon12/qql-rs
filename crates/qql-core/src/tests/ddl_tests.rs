use crate::ast::Stmt;
use crate::parser::Parser;

#[test]
fn create_collection_dense() {
    let s = Parser::parse("CREATE COLLECTION docs (dense VECTOR(384, COSINE));").unwrap();
    assert!(matches!(s, Stmt::CreateCollection(_)));
}

#[test]
fn create_collection_with_sparse() {
    let s = Parser::parse(
        "CREATE COLLECTION docs (dense VECTOR(768, DOT), sparse SPARSE);",
    ).unwrap();
    assert!(matches!(s, Stmt::CreateCollection(_)));
}

#[test]
fn create_collection_with_hnsw() {
    let s = Parser::parse(
        "CREATE COLLECTION docs (d VECTOR(128, EUCLID)) WITH HNSW (m = 16, ef_construct = 100);",
    ).unwrap();
    assert!(matches!(s, Stmt::CreateCollection(_)));
}

#[test]
fn create_collection_with_params() {
    let s = Parser::parse(
        "CREATE COLLECTION docs (d VECTOR(4, DOT)) WITH PARAMS (replication_factor = 3, on_disk_payload = true);",
    ).unwrap();
    assert!(matches!(s, Stmt::CreateCollection(_)));
}

#[test]
fn alter_collection() {
    let s = Parser::parse(
        "ALTER COLLECTION docs WITH VECTOR (on_disk = true) WITH HNSW (m = 32);",
    ).unwrap();
    assert!(matches!(s, Stmt::AlterCollection(_)));
}

#[test]
fn drop_collection() {
    let s = Parser::parse("DROP COLLECTION docs;").unwrap();
    assert!(matches!(s, Stmt::DropCollection(_)));
}

#[test]
fn create_index() {
    let s = Parser::parse(
        "CREATE INDEX ON COLLECTION docs FOR title TYPE text WITH (lowercase = true, tokenizer = 'word');",
    ).unwrap();
    assert!(matches!(s, Stmt::CreateIndex(_)));
}

#[test]
fn show_collections() {
    let s = Parser::parse("SHOW COLLECTIONS;").unwrap();
    assert!(matches!(s, Stmt::ShowCollections));
}

#[test]
fn show_collection() {
    let s = Parser::parse("SHOW COLLECTION docs;").unwrap();
    assert!(matches!(s, Stmt::ShowCollection(ref c) if c == "docs"));
}

#[test]
fn upsert_simple() {
    let s = Parser::parse(
        "UPSERT INTO docs VALUES {id: 1, title: 'hello', vector: [0.1, 0.2]};",
    ).unwrap();
    let Stmt::Upsert(u) = s else { panic!() };
    assert_eq!(u.points.len(), 1);
    assert_eq!(u.collection, "docs");
}

#[test]
fn upsert_with_sparse() {
    let s = Parser::parse(
        "UPSERT INTO docs VALUES {id: 'p1', title: 'doc', vector: {indices: [0, 3], values: [5.0, 8.0]}};",
    ).unwrap();
    assert!(matches!(s, Stmt::Upsert(_)));
}

#[test]
fn upsert_named_vectors() {
    let s = Parser::parse(
        "UPSERT INTO docs VALUES {id: 1, title: 'x', vector: {dense: [1.0, 2.0], sp: {indices: [7], values: [0.5]}}};",
    ).unwrap();
    assert!(matches!(s, Stmt::Upsert(_)));
}

#[test]
fn upsert_with_embedding() {
    let s = Parser::parse(
        "UPSERT INTO docs VALUES {id: 1, text: 'hello'} USING DENSE MODEL 'nomic';",
    ).unwrap();
    assert!(matches!(s, Stmt::Upsert(_)));
}

#[test]
fn upsert_with_embed_directive() {
    let s = Parser::parse(
        "UPSERT INTO docs VALUES {id: 1, title: 'doc'} EMBED title INTO dense_vec USING MODEL 'embed';",
    ).unwrap();
    assert!(matches!(s, Stmt::Upsert(_)));
}

#[test]
fn delete_by_id() {
    let s = Parser::parse("DELETE FROM docs WHERE id = 42;").unwrap();
    assert!(matches!(s, Stmt::Delete(_)));
}

#[test]
fn delete_by_filter() {
    let s = Parser::parse("DELETE FROM docs WHERE status = 'inactive';").unwrap();
    assert!(matches!(s, Stmt::Delete(_)));
}

#[test]
fn update_vector() {
    let s = Parser::parse(
        "UPDATE docs SET VECTOR dense = [0.3, 0.7] WHERE id = 'p1';",
    ).unwrap();
    assert!(matches!(s, Stmt::UpdateVector(_)));
}

#[test]
fn update_payload() {
    let s = Parser::parse(
        "UPDATE docs SET PAYLOAD = {status: 'active', priority: 5} WHERE id = 42;",
    ).unwrap();
    assert!(matches!(s, Stmt::UpdatePayload(_)));
}

#[test]
fn scroll_basic() {
    let s = Parser::parse("SCROLL FROM docs LIMIT 50;").unwrap();
    let Stmt::Scroll(sc) = s else { panic!() };
    assert_eq!(sc.collection, "docs");
    assert_eq!(sc.limit, 50);
}

#[test]
fn scroll_with_filter() {
    let s = Parser::parse("SCROLL FROM docs WHERE active = true AFTER 10 LIMIT 20;").unwrap();
    assert!(matches!(s, Stmt::Scroll(_)));
}
