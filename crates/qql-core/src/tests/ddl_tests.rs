use crate::ast::Stmt;
use crate::parser::Parser;

#[test]
fn create_collection_dense() {
    let s = Parser::parse("CREATE COLLECTION docs (dense VECTOR(384, COSINE));").unwrap();
    assert!(matches!(s, Stmt::CreateCollection(_)));
}

#[test]
fn create_collection_vector_size_cap() {
    // VectorParams.size maximum in the Qdrant OpenAPI schema (qdrant#10324).
    assert!(Parser::parse("CREATE COLLECTION docs (d VECTOR(65536, COSINE));").is_ok());
    let err = Parser::parse("CREATE COLLECTION docs (d VECTOR(65537, COSINE));")
        .expect_err("size above the 65536 maximum must be rejected");
    assert_eq!(err.kind, crate::error::ErrorKind::Parse);
    assert_eq!(err.code, "QQL-PARSE-VECTOR-SIZE");
}

#[test]
fn create_collection_with_sparse() {
    let s =
        Parser::parse("CREATE COLLECTION docs (dense VECTOR(768, DOT), sparse SPARSE);").unwrap();
    assert!(matches!(s, Stmt::CreateCollection(_)));
}

#[test]
fn create_collection_explicit_dense_model() {
    let s = Parser::parse("CREATE COLLECTION docs USING DENSE MODEL 'all-minilm:l6-v2';").unwrap();
    assert!(matches!(s, Stmt::CreateCollection(_)));
}

#[test]
fn create_hybrid_with_arbitrary_vector_names() {
    let stmt = Parser::parse(
        "CREATE COLLECTION docs HYBRID \
         DENSE VECTOR semantic_v2 SPARSE VECTOR lexical_v2;",
    )
    .unwrap();
    let Stmt::CreateCollection(create) = stmt else {
        panic!("expected CREATE COLLECTION");
    };
    assert!(matches!(
        create.mode,
        crate::ast::CollectionMode::Hybrid {
            dense_vector: Some(ref dense),
            sparse_vector: Some(ref sparse),
        } if dense == "semantic_v2" && sparse == "lexical_v2"
    ));
}

#[test]
fn create_collection_with_hnsw() {
    let s = Parser::parse(
        "CREATE COLLECTION docs (d VECTOR(128, EUCLID)) WITH HNSW (m = 16, ef_construct = 100);",
    )
    .unwrap();
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
fn create_collection_with_memory_and_datatype() {
    let stmt = Parser::parse(
        "CREATE COLLECTION docs \
         (dense VECTOR(4, COSINE) WITH VECTOR (memory = 'cached', datatype = 'turbo4') WITH HNSW (memory = 'cold')) \
         WITH PARAMS (payload_memory = 'cold') \
         WITH QUANTIZATION (type = 'scalar', memory = 'cached');",
    )
    .unwrap();
    let Stmt::CreateCollection(create) = stmt else {
        panic!("expected CREATE COLLECTION");
    };
    let dense = create
        .vectors
        .iter()
        .find(|v| v.name == "dense")
        .expect("dense vector");
    let vectors = dense.vectors.as_ref().expect("WITH VECTOR block");
    assert_eq!(vectors.memory, Some(crate::ast::MemoryPlacement::Cached));
    assert_eq!(vectors.datatype, Some(crate::ast::VectorDatatype::Turbo4));
    let hnsw = dense.hnsw.as_ref().expect("WITH HNSW block");
    assert_eq!(hnsw.memory, Some(crate::ast::MemoryPlacement::Cold));
    let params = create.config.as_ref().and_then(|c| c.params.as_ref());
    assert_eq!(
        params.unwrap().payload_memory,
        Some(crate::ast::MemoryPlacement::Cold)
    );
    let quant = create.config.as_ref().and_then(|c| c.quantization.as_ref());
    assert_eq!(
        quant.unwrap().memory,
        Some(crate::ast::MemoryPlacement::Cached)
    );
}

#[test]
fn create_collection_rejects_bad_memory_and_datatype() {
    assert!(Parser::parse(
        "CREATE COLLECTION docs (d VECTOR(4, COSINE)) WITH VECTOR (memory = 'hot');",
    )
    .is_err());
    assert!(Parser::parse(
        "CREATE COLLECTION docs (d VECTOR(4, COSINE)) WITH VECTOR (datatype = 'float64');",
    )
    .is_err());
    assert!(Parser::parse(
        "CREATE COLLECTION docs (d VECTOR(4, COSINE)) WITH HNSW (memory = 'pinned');",
    )
    .is_ok());
    assert!(Parser::parse(
        "CREATE COLLECTION docs (d VECTOR(4, COSINE)) WITH PARAMS (payload_memory = 'pinned');",
    )
    .is_err());
}

#[test]
fn create_sparse_with_memory_roundtrips_fmt() {
    let stmt = Parser::parse(
        "CREATE COLLECTION docs (sparse SPARSE WITH SPARSE (modifier = 'idf', memory = 'cached'));",
    )
    .unwrap();
    let formatted = crate::fmt::format_stmt(&stmt);
    assert!(formatted.contains("memory = 'cached'"), "{formatted}");
    let reparsed = Parser::parse(&formatted).unwrap();
    assert_eq!(format_stmt_ast(&stmt), format_stmt_ast(&reparsed));
}

#[test]
fn show_quotas() {
    let s = Parser::parse("SHOW QUOTAS;").unwrap();
    assert!(matches!(s, Stmt::ShowQuotas));
}

#[test]
fn set_quota_parses_and_roundtrips_fmt() {
    let stmt = Parser::parse(
        "SET QUOTA (enabled = true, max_resident_memory_percent = 80, max_disk_usage_percent = 90, release_margin_percent = 5) WAIT true;",
    )
    .unwrap();
    match &stmt {
        Stmt::SetQuota(q) => {
            assert_eq!(q.wait, Some(true));
            assert_eq!(q.config.len(), 4);
        }
        other => panic!("expected SET QUOTA, got {other:?}"),
    }
    let formatted = crate::fmt::format_stmt(&stmt);
    assert!(formatted.contains("SET QUOTA ("), "{formatted}");
    assert!(formatted.contains("WAIT true"), "{formatted}");
    let reparsed = Parser::parse(&formatted).unwrap();
    assert_eq!(&stmt, &reparsed);

    // WAIT defaults to absent.
    let s = Parser::parse("SET QUOTA (enabled = true);").unwrap();
    match s {
        Stmt::SetQuota(q) => assert_eq!(q.wait, None),
        other => panic!("expected SET QUOTA, got {other:?}"),
    }
}

#[test]
fn set_quota_rejects_unknown_keys_and_bad_wait() {
    assert!(Parser::parse("SET QUOTA (bogus = 1);").is_ok()); // planner validates
    assert!(Parser::parse("SET QUOTA (enabled = true) WAIT maybe;").is_err());
}

fn format_stmt_ast(stmt: &Stmt) -> String {
    crate::fmt::format_stmt(stmt)
}

#[test]
fn alter_collection() {
    let s = Parser::parse("ALTER COLLECTION docs WITH VECTOR (on_disk = true) WITH HNSW (m = 32);")
        .unwrap();
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
fn create_index_numeric_options() {
    let s = Parser::parse(
        "CREATE INDEX ON COLLECTION docs FOR year TYPE integer \
         WITH (lookup = true, range = true, is_principal = true);",
    )
    .unwrap();
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
    let s = Parser::parse("UPSERT INTO docs VALUES {id: 1, title: 'hello', vector: [0.1, 0.2]};")
        .unwrap();
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
    let s =
        Parser::parse("UPSERT INTO docs VALUES {id: 1, text: 'hello'} USING DENSE MODEL 'nomic';")
            .unwrap();
    assert!(matches!(s, Stmt::Upsert(_)));
}

#[test]
fn upsert_embedding_targets_may_be_inferred() {
    for source in [
        "UPSERT INTO docs VALUES {id: 1, text: 'hello'} USING DENSE;",
        "UPSERT INTO docs VALUES {id: 1, text: 'hello'} USING SPARSE;",
        "UPSERT INTO docs VALUES {id: 1, text: 'hello'} USING HYBRID;",
    ] {
        assert!(matches!(Parser::parse(source), Ok(Stmt::Upsert(_))));
    }
}

#[test]
fn upsert_with_embed_directive() {
    let s = Parser::parse(
        "UPSERT INTO docs VALUES {id: 1, title: 'doc'} EMBED title INTO dense_vec USING MODEL 'embed';",
    ).unwrap();
    assert!(matches!(s, Stmt::Upsert(_)));
}

#[test]
fn embed_directive_accepts_explicit_dense_role_without_model() {
    let s = Parser::parse(
        "UPSERT INTO docs VALUES {id: 1, title: 'doc'} \
         EMBED title INTO semantic_v2 USING DENSE;",
    )
    .unwrap();
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
    let s = Parser::parse("UPDATE docs SET VECTOR dense = [0.3, 0.7] WHERE id = 'p1';").unwrap();
    assert!(matches!(s, Stmt::UpdateVector(_)));
}

#[test]
fn update_payload() {
    let s =
        Parser::parse("UPDATE docs SET PAYLOAD = {status: 'active', priority: 5} WHERE id = 42;")
            .unwrap();
    assert!(matches!(s, Stmt::UpdatePayload(_)));
}

#[test]
fn scroll_basic() {
    let s = Parser::parse("SCROLL FROM docs LIMIT 50;").unwrap();
    let Stmt::Scroll(sc) = s else { panic!() };
    assert_eq!(sc.collection, "docs");
    assert_eq!(sc.limit, 50);
    assert!(sc.with_vector.is_none());
}

#[test]
fn scroll_with_filter() {
    let s = Parser::parse("SCROLL FROM docs WHERE active = true AFTER 10 LIMIT 20;").unwrap();
    assert!(matches!(s, Stmt::Scroll(_)));
}

#[test]
fn scroll_with_vector_all() {
    use crate::ast::VectorSelector;
    let s = Parser::parse("SCROLL FROM docs WITH VECTOR LIMIT 10;").unwrap();
    let Stmt::Scroll(sc) = s else { panic!() };
    assert_eq!(sc.with_vector, Some(VectorSelector::All));
}

#[test]
fn query_bare_with_vector_defaults_to_all() {
    use crate::ast::{QueryOutput, VectorSelector};
    let s = Parser::parse("QUERY 'hello' FROM docs WITH VECTOR LIMIT 10;").unwrap();
    let Stmt::Query(q) = s else { panic!() };
    assert_eq!(
        q.output,
        QueryOutput {
            payload: None,
            vectors: Some(VectorSelector::All),
        }
    );
}

#[test]
fn create_index_rejects_legacy_quoted_type() {
    let error =
        Parser::parse("CREATE INDEX ON COLLECTION docs FOR title TYPE 'text';").unwrap_err();
    assert_eq!(error.code, "QQL-PARSE-INDEX-TYPE");
}

#[test]
fn scroll_with_vector_true_and_after() {
    use crate::ast::{PointId, VectorSelector};
    let s = Parser::parse("SCROLL FROM docs AFTER 'abc-uuid' WITH VECTOR true LIMIT 5;").unwrap();
    let Stmt::Scroll(sc) = s else { panic!() };
    assert_eq!(sc.after, Some(PointId::String("abc-uuid".into())));
    assert_eq!(sc.with_vector, Some(VectorSelector::All));
}

#[test]
fn scroll_with_named_vectors() {
    use crate::ast::VectorSelector;
    let s = Parser::parse("SCROLL FROM docs WITH VECTOR (dense, sparse) LIMIT 3;").unwrap();
    let Stmt::Scroll(sc) = s else { panic!() };
    assert_eq!(
        sc.with_vector,
        Some(VectorSelector::Names(vec!["dense".into(), "sparse".into()]))
    );
}
