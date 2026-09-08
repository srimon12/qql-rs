use qql::backend::{
    CollectionInfo, CollectionParamsSpec, CollectionSchema, PayloadIndexSpec, VectorSpec,
};
use qql_plan::semantic::PlanPointId;
use serde_json::json;

use super::escape::*;
use super::point::*;
use super::quant::*;
use super::{next_scroll_cursor, scroll_page_complete, *};

fn info_with_vectors(vectors: Vec<VectorSpec>, sparse: Vec<String>) -> CollectionInfo {
    CollectionInfo {
        status: "green".into(),
        points_count: 0,
        segments_count: 1,
        schema: CollectionSchema {
            dense_vectors: vectors.iter().filter_map(|v| v.name.clone()).collect(),
            sparse_vectors: sparse
                .into_iter()
                .map(|name| qql::backend::SparseVectorSpec {
                    name,
                    index: None,
                    modifier: None,
                })
                .collect(),
            vectors,
            payload_indexes: Vec::new(),
            params: CollectionParamsSpec::default(),
            hnsw: None,
            optimizers: None,
            quantization: None,
        },
    }
}

#[test]
fn escape_string_matches_qql_core() {
    assert_eq!(escape_string(r#"a'b"#), r#"a\'b"#);
    assert_eq!(escape_string("a\\b"), "a\\\\b");
    assert_eq!(escape_string("a\nb"), "a\\nb");
    assert_eq!(escape_string("a\tb"), "a\\tb");
    // \0 is not supported by the parser — must not emit it.
    assert_eq!(escape_string("a\0b"), "ab");
    let lit = format!("'{}'", escape_string("line\nnext\tend"));
    // Round-trip through the real decoder via a string literal in UPSERT.
    let stmt = format!("UPSERT INTO docs VALUES {{id: 1, t: {}}};", lit);
    qql_core::parser::Parser::parse(&stmt).expect("escaped string should parse");
}

#[test]
fn format_ident_quotes_special_names() {
    assert_eq!(format_ident("docs"), "docs");
    assert_eq!(format_ident("my-docs"), "'my-docs'");
    assert_eq!(format_ident("weird name"), "'weird name'");
}

#[test]
fn create_unnamed_vector_collection() {
    let info = info_with_vectors(
        vec![VectorSpec {
            name: None,
            size: 4,
            distance: "Cosine".into(),
            hnsw: None,
            quantization: None,
            multivector: None,
            on_disk: None,
            datatype: None,
            memory: None,
        }],
        vec![],
    );
    let stmt = generate_create_statement("docs", &info);
    assert_eq!(stmt, "CREATE COLLECTION docs (dense VECTOR(4, COSINE))");
    qql_core::parser::Parser::parse(&format!("{};", stmt))
        .expect("unnamed vector CREATE should parse");
}

#[test]
fn create_named_hybrid_collection() {
    let mut info = info_with_vectors(
        vec![
            VectorSpec {
                name: Some("dense".into()),
                size: 384,
                distance: "Cosine".into(),
                hnsw: None,
                quantization: None,
                multivector: None,
                on_disk: None,
                datatype: None,
                memory: None,
            },
            VectorSpec {
                name: Some("image".into()),
                size: 512,
                distance: "Dot".into(),
                hnsw: None,
                quantization: None,
                multivector: None,
                on_disk: None,
                datatype: None,
                memory: None,
            },
        ],
        vec!["sparse".into()],
    );
    info.schema.params = CollectionParamsSpec {
        shard_number: Some(2),
        sharding_method: None,
        on_disk_payload: Some(true),
        payload_memory: None,
        replication_factor: None,
    };
    let stmt = generate_create_statement("hybrid_docs", &info);
    assert!(stmt.starts_with("CREATE COLLECTION hybrid_docs ("));
    assert!(stmt.contains("dense VECTOR(384, COSINE)"));
    assert!(stmt.contains("image VECTOR(512, DOT)"));
    assert!(stmt.contains("sparse SPARSE"));
    assert!(stmt.contains("WITH PARAMS ("));
    assert!(stmt.contains("shard_number = 2"));
    assert!(stmt.contains("on_disk_payload = true"));
    qql_core::parser::Parser::parse(&format!("{};", stmt)).expect("create should parse");
}

#[test]
fn create_falls_back_when_no_schema() {
    let info = CollectionInfo::default();
    let stmt = generate_create_statement("empty", &info);
    assert_eq!(stmt, "CREATE COLLECTION empty");
}

#[test]
fn indexes_from_typed_specs() {
    let indexes = vec![
        PayloadIndexSpec {
            field: "title".into(),
            data_type: "text".into(),
            params: {
                let mut m = serde_json::Map::new();
                m.insert("tokenizer".into(), json!("word"));
                m.insert("lowercase".into(), json!(true));
                m
            },
            is_tenant: None,
        },
        PayloadIndexSpec {
            field: "tenant_id".into(),
            data_type: "keyword".into(),
            params: serde_json::Map::new(),
            is_tenant: Some(true),
        },
    ];
    let stmts = generate_index_statements("docs", &indexes);
    assert_eq!(stmts.len(), 2);
    assert!(stmts[0].contains("FOR tenant_id TYPE keyword"));
    assert!(stmts[0].contains("is_tenant = true"));
    assert!(stmts[1].contains("FOR title TYPE text"));
    assert!(stmts[1].contains("tokenizer = 'word'"));
    assert!(stmts[1].contains("lowercase = true"));
    for s in &stmts {
        qql_core::parser::Parser::parse(&format!("{};", s)).expect("index should parse");
    }
}

#[test]
fn point_to_upsert_keeps_vector_and_payload() {
    let point = json!({
        "id": 42,
        "payload": { "title": "hello", "year": 2024 },
        "vector": [0.1, 0.2, 0.3]
    });
    let rec = point_to_upsert_object(&point).unwrap();
    assert_eq!(rec["id"], 42);
    assert_eq!(rec["title"], "hello");
    assert_eq!(rec["year"], 2024);
    assert_eq!(rec["vector"], json!([0.1, 0.2, 0.3]));
}

#[test]
fn point_without_payload_still_exported() {
    let point = json!({
        "id": "uuid-1",
        "vector": { "dense": [1.0, 2.0] }
    });
    let rec = point_to_upsert_object(&point).unwrap();
    assert_eq!(rec["id"], "uuid-1");
    assert!(rec.get("vector").is_some());
}

#[test]
fn point_without_id_is_skipped() {
    let point = json!({ "payload": { "x": 1 } });
    assert!(point_to_upsert_object(&point).is_none());
}

#[test]
fn format_point_literal_matches_upsert_grammar() {
    let rec = json!({
        "id": 1,
        "title": "café",
        "vector": [0.1, 0.2]
    });
    let lit = format_point_literal(&rec);
    assert!(lit.starts_with("{id: 1, vector: [0.1, 0.2]"));
    assert!(lit.contains("title: 'café'"));
    let stmt = format!("UPSERT INTO docs VALUES {};", lit);
    qql_core::parser::Parser::parse(&stmt).expect("upsert should parse");
}

#[test]
fn format_named_and_sparse_vectors() {
    let rec = json!({
        "id": "p1",
        "vector": {
            "dense": [0.5, 0.5],
            "sparse": { "indices": [1, 7], "values": [0.2, 0.9] }
        }
    });
    let lit = format_point_literal(&rec);
    let stmt = format!("UPSERT INTO docs VALUES {};", lit);
    qql_core::parser::Parser::parse(&stmt).expect("named+sparse upsert should parse");
}

#[test]
fn format_string_id_with_quote_escapes() {
    let rec = json!({ "id": "o'reilly", "vector": [1.0] });
    let lit = format_point_literal(&rec);
    assert!(lit.contains("id: 'o\\'reilly'"));
    let stmt = format!("UPSERT INTO docs VALUES {};", lit);
    qql_core::parser::Parser::parse(&stmt).expect("escaped id should parse");
}

#[test]
fn format_upsert_batch_statement() {
    let records = vec![
        json!({"id": 1, "vector": [0.1], "t": "a"}),
        json!({"id": 2, "vector": [0.2], "t": "b"}),
    ];
    let stmt = format_upsert_statement("docs", &records);
    assert!(stmt.starts_with("UPSERT INTO docs VALUES\n"));
    let full = format!("{};", stmt.trim_end());
    qql_core::parser::Parser::parse(&full).expect("batch upsert should parse");
}

#[test]
fn extract_scroll_page_with_next_offset() {
    let response = json!({
        "result": {
            "points": [
                { "id": 1, "vector": [0.1], "payload": {} },
                { "id": 2, "vector": [0.2], "payload": { "x": 1 } }
            ],
            "next_page_offset": 2
        }
    });
    let (points, next) = extract_scroll_page(&response);
    assert_eq!(points.len(), 2);
    assert_eq!(next, Some(PlanPointId::Number(2)));
}

#[test]
fn extract_scroll_page_string_offset() {
    let response = json!({
        "result": {
            "points": [{ "id": "a" }],
            "next_page_offset": "a"
        }
    });
    let (_, next) = extract_scroll_page(&response);
    assert_eq!(next, Some(PlanPointId::String("a".into())));
}

#[test]
fn extract_empty_page() {
    let response = json!({ "result": { "points": [] } });
    let (points, next) = extract_scroll_page(&response);
    assert!(points.is_empty());
    assert!(next.is_none());
}

#[test]
fn next_scroll_cursor_falls_back_to_last_id() {
    let points = vec![json!({"id": 7}), json!({"id": 8})];
    assert_eq!(
        next_scroll_cursor(None, &points),
        Some(PlanPointId::Number(8))
    );
    assert_eq!(
        next_scroll_cursor(Some(PlanPointId::Number(9)), &points),
        Some(PlanPointId::Number(9))
    );
}

#[test]
fn scroll_page_complete_detects_short_and_stuck() {
    let points = vec![json!({"id": 1})];
    assert!(scroll_page_complete(
        &points,
        Some(&PlanPointId::Number(1)),
        None,
        10
    ));
    let full = vec![json!({"id": 1}), json!({"id": 2})];
    assert!(scroll_page_complete(
        &full,
        Some(&PlanPointId::Number(1)),
        Some(&PlanPointId::Number(1)),
        2
    ));
    assert!(!scroll_page_complete(
        &full,
        Some(&PlanPointId::Number(2)),
        Some(&PlanPointId::Number(1)),
        2
    ));
}

#[test]
fn dumped_script_splits_cleanly() {
    let create = "CREATE COLLECTION docs (dense VECTOR(4, COSINE));";
    let index = "CREATE INDEX ON COLLECTION docs FOR title TYPE text;";
    let upsert = "UPSERT INTO docs VALUES\n  {id: 1, vector: [0.1, 0.2, 0.3, 0.4], title: 'x'};";
    let script = format!("{}\n\n{}\n\n{}\n", create, index, upsert);
    let stmts = crate::script::split_statements(&script).expect("split");
    assert_eq!(stmts.len(), 3);
}

#[test]
fn schema_from_rest_result_feeds_create() {
    let result = json!({
        "config": {
            "params": {
                "vectors": { "size": 8, "distance": "Euclid" },
                "sparse_vectors": { "bm25": {} },
                "shard_number": 1
            }
        },
        "payload_schema": {
            "city": { "data_type": "keyword" }
        }
    });
    let schema = qql::backend::schema_from_rest_result(&result);
    let info = CollectionInfo {
        status: "green".into(),
        points_count: 0,
        segments_count: 1,
        schema,
    };
    let create = format!("{};", generate_create_statement("docs", &info));
    qql_core::parser::Parser::parse(&create).expect("create from rest schema");
    let indexes = generate_index_statements("docs", &info.schema.payload_indexes);
    assert_eq!(indexes.len(), 1);
    qql_core::parser::Parser::parse(&format!("{};", indexes[0])).expect("index from rest");
}

#[test]
fn create_omits_zero_positive_only_hnsw_and_optimizer_keys() {
    let mut hnsw = serde_json::Map::new();
    hnsw.insert("m".into(), json!(16));
    hnsw.insert("max_indexing_threads".into(), json!(0));
    let mut opts = serde_json::Map::new();
    opts.insert("default_segment_number".into(), json!(0));
    opts.insert("indexing_threshold".into(), json!(20000));
    let mut info = info_with_vectors(
        vec![VectorSpec {
            name: Some("dense".into()),
            size: 4,
            distance: "Cosine".into(),
            hnsw: Some(hnsw),
            quantization: None,
            multivector: None,
            on_disk: None,
            datatype: None,
            memory: None,
        }],
        vec![],
    );
    info.schema.hnsw = None;
    info.schema.optimizers = Some(opts);
    let stmt = generate_create_statement("docs", &info);
    assert!(
        !stmt.contains("max_indexing_threads"),
        "zero max_indexing_threads should be omitted: {stmt}"
    );
    assert!(
        !stmt.contains("default_segment_number"),
        "zero default_segment_number should be omitted: {stmt}"
    );
    assert!(stmt.contains("indexing_threshold = 20000"));
    qql_core::parser::Parser::parse(&format!("{};", stmt))
        .expect("CREATE with omitted auto-zeros should parse");
}

#[test]
fn create_vector_with_hnsw_and_quantization() {
    let mut hnsw = serde_json::Map::new();
    hnsw.insert("m".into(), json!(16));
    hnsw.insert("ef_construct".into(), json!(100));

    let info = info_with_vectors(
        vec![VectorSpec {
            name: Some("dense".into()),
            size: 384,
            distance: "Cosine".into(),
            hnsw: Some(hnsw),
            quantization: Some(json!({
                "scalar": {
                    "type": "scalar",
                    "quantile": 0.99,
                    "always_ram": true
                }
            })),
            multivector: None,
            on_disk: Some(true),
            datatype: None,
            memory: None,
        }],
        vec![],
    );
    let stmt = generate_create_statement("docs", &info);
    assert!(stmt.contains("WITH HNSW ("));
    assert!(stmt.contains("m = 16"));
    assert!(stmt.contains("ef_construct = 100"));
    assert!(stmt.contains("WITH QUANTIZATION ("));
    assert!(stmt.contains("type = 'scalar'"));
    // Vector storage on_disk must use VECTOR, not be folded into HNSW.
    assert!(stmt.contains("WITH VECTOR (on_disk = true)"));
    let hnsw_idx = stmt.find("WITH HNSW (").unwrap();
    let hnsw_end = stmt[hnsw_idx..].find(')').unwrap() + hnsw_idx;
    assert!(
        !stmt[hnsw_idx..=hnsw_end].contains("on_disk"),
        "vector on_disk leaked into HNSW block: {}",
        &stmt[hnsw_idx..=hnsw_end]
    );
    qql_core::parser::Parser::parse(&format!("{};", stmt))
        .expect("HNSW+quantization CREATE should parse");
}

#[test]
fn product_and_binary_quantization_roundtrip_parse() {
    let product_stmt = "CREATE COLLECTION docs (v VECTOR(128, COSINE) WITH QUANTIZATION (type = 'product', compression = 'x16', always_ram = true));";
    let parsed = qql_core::parser::Parser::parse(product_stmt)
        .expect("product quantization CREATE should parse");
    if let qql_core::ast::Stmt::CreateCollection(stmt) = parsed {
        let q = stmt.vectors[0].quantization.as_ref().unwrap();
        assert_eq!(q.qtype, qql_core::ast::QuantizationType::Product);
        assert_eq!(q.compression.as_deref(), Some("x16"));
        assert!(q.always_ram);
    } else {
        panic!("expected CreateCollection");
    }

    let binary_stmt = "CREATE COLLECTION docs (v VECTOR(128, COSINE) WITH QUANTIZATION (type = 'binary', encoding = 'two_bits', always_ram = true));";
    let parsed = qql_core::parser::Parser::parse(binary_stmt)
        .expect("binary quantization CREATE should parse");
    if let qql_core::ast::Stmt::CreateCollection(stmt) = parsed {
        let q = stmt.vectors[0].quantization.as_ref().unwrap();
        assert_eq!(q.qtype, qql_core::ast::QuantizationType::Binary);
        assert_eq!(q.encoding.as_deref(), Some("two_bits"));
        assert!(q.always_ram);
    } else {
        panic!("expected CreateCollection");
    }
}

#[test]
fn create_vector_with_turbo_quantization() {
    let info = info_with_vectors(
        vec![VectorSpec {
            name: Some("dense".into()),
            size: 768,
            distance: "Cosine".into(),
            hnsw: None,
            // Nested REST/OpenAPI shape
            quantization: Some(json!({
                "turbo": {
                    "always_ram": true,
                    "bits": "bits1_5"
                }
            })),
            multivector: None,
            on_disk: None,
            datatype: None,
            memory: None,
        }],
        vec![],
    );
    let stmt = generate_create_statement("docs", &info);
    assert!(stmt.contains("WITH QUANTIZATION ("));
    assert!(stmt.contains("type = 'turbo'"));
    assert!(stmt.contains("bits = 1.5"));
    assert!(stmt.contains("always_ram = true"));
    qql_core::parser::Parser::parse(&format!("{};", stmt))
        .expect("turbo quantization CREATE should parse");
}

#[test]
fn format_quantization_turbo_flat_and_nested() {
    let nested = json!({"turbo": {"bits": 2, "always_ram": false}});
    let s = format_quantization_spec(&nested).unwrap();
    assert!(s.contains("type = 'turbo'"));
    assert!(s.contains("bits = 2"));

    let flat = json!({"type": "turbo", "turbo_bits": 4.0, "always_ram": true});
    let s = format_quantization_spec(&flat).unwrap();
    assert!(s.contains("type = 'turbo'"));
    assert!(s.contains("bits = 4"));
    assert!(!s.contains("turbo_bits"));
}

#[test]
fn format_quantization_product_and_binary() {
    let product = json!({"product": {"compression": "x16", "always_ram": true}});
    let s = format_quantization_spec(&product).unwrap();
    assert!(s.contains("type = 'product'"));
    assert!(s.contains("compression = 'x16'"));

    let binary = json!({"binary": {"always_ram": false, "encoding": "two_bits"}});
    let s = format_quantization_spec(&binary).unwrap();
    assert!(s.contains("type = 'binary'"));
    assert!(s.contains("encoding = 'two_bits'"));

    // Protobuf-style enum names from gRPC adapters must normalize.
    let binary_proto = json!({"binary": {"encoding": "TwoBits"}});
    let s = format_quantization_spec(&binary_proto).unwrap();
    assert!(s.contains("encoding = 'two_bits'"));
}

#[test]
fn binary_encoding_numeric_alias_parses_canonical() {
    let stmt = "CREATE COLLECTION docs (v VECTOR(8, COSINE) WITH QUANTIZATION (type = 'binary', encoding = 2, query_encoding = 'scalar8bits'));";
    let parsed = qql_core::parser::Parser::parse(stmt).expect("numeric encoding should parse");
    if let qql_core::ast::Stmt::CreateCollection(c) = parsed {
        let q = c.vectors[0].quantization.as_ref().unwrap();
        assert_eq!(q.encoding.as_deref(), Some("two_bits"));
        assert_eq!(q.query_encoding.as_deref(), Some("scalar8bits"));
    } else {
        panic!("expected CreateCollection");
    }
}

#[test]
fn multivector_and_collection_level_blocks_roundtrip() {
    let stmt = "CREATE COLLECTION docs (mv VECTOR(128, COSINE) WITH MULTIVECTOR (comparator = 'max_sim')) WITH HNSW (m = 16) WITH OPTIMIZERS (indexing_threshold = 20000);";
    let parsed = qql_core::parser::Parser::parse(stmt)
        .expect("multivector + collection blocks should parse");
    if let qql_core::ast::Stmt::CreateCollection(c) = parsed {
        assert_eq!(
            c.vectors[0].multivector.as_ref().unwrap().comparator,
            qql_core::ast::MultivectorComparator::MaxSim
        );
        let cfg = c.config.as_ref().unwrap();
        assert_eq!(cfg.hnsw.as_ref().unwrap().m, Some(16));
        assert_eq!(
            cfg.optimizers.as_ref().unwrap().indexing_threshold,
            Some(20000)
        );
    } else {
        panic!("expected CreateCollection");
    }
}

#[test]
fn dump_emits_multivector_collection_blocks_and_query_encoding() {
    let mut hnsw = serde_json::Map::new();
    hnsw.insert("m".into(), json!(16));
    let mut optimizers = serde_json::Map::new();
    optimizers.insert("indexing_threshold".into(), json!(20000));
    optimizers.insert("max_optimization_threads".into(), json!("auto"));
    // Noise keys that must not be emitted (would fail re-parse).
    optimizers.insert("unknown_qdrant_field".into(), json!(1));

    let mut multivector = serde_json::Map::new();
    multivector.insert("comparator".into(), json!("max_sim"));

    let mut info = info_with_vectors(
        vec![VectorSpec {
            name: Some("mv".into()),
            size: 128,
            distance: "Cosine".into(),
            hnsw: None,
            quantization: Some(json!({
                "binary": {
                    "type": "binary",
                    "encoding": "two_bits",
                    "query_encoding": "scalar8bits",
                    "always_ram": true
                }
            })),
            multivector: Some(multivector),
            on_disk: Some(true),
            datatype: None,
            memory: None,
        }],
        vec![],
    );
    info.schema.hnsw = Some(hnsw);
    info.schema.optimizers = Some(optimizers);
    info.schema.quantization = Some(json!({
        "scalar": { "type": "scalar", "quantile": 0.99, "always_ram": true }
    }));

    let stmt = generate_create_statement("docs", &info);
    assert!(stmt.contains("WITH MULTIVECTOR (comparator = 'max_sim')"));
    assert!(stmt.contains("WITH VECTOR (on_disk = true)"));
    assert!(stmt.contains("query_encoding = 'scalar8bits'"));
    assert!(stmt.contains("WITH HNSW (m = 16)"));
    assert!(stmt.contains("WITH OPTIMIZERS ("));
    assert!(stmt.contains("indexing_threshold = 20000"));
    assert!(stmt.contains("max_optimization_threads = 'auto'"));
    assert!(!stmt.contains("unknown_qdrant_field"));
    assert!(stmt.contains("WITH QUANTIZATION ("));
    assert!(stmt.contains("type = 'scalar'"));

    qql_core::parser::Parser::parse(&format!("{};", stmt))
        .expect("full dump CREATE should re-parse");
}

#[test]
fn vector_on_disk_parses_into_vectors_config_not_hnsw() {
    let stmt = "CREATE COLLECTION docs (v VECTOR(8, COSINE) WITH HNSW (m = 8, on_disk = false) WITH VECTOR (on_disk = true));";
    let parsed = qql_core::parser::Parser::parse(stmt).expect("should parse");
    let qql_core::ast::Stmt::CreateCollection(c) = parsed else {
        panic!("expected CreateCollection");
    };
    assert_eq!(c.vectors[0].hnsw.as_ref().unwrap().on_disk, Some(false));
    assert_eq!(c.vectors[0].vectors.as_ref().unwrap().on_disk, Some(true));
}

#[test]
fn sparse_vector_full_config_roundtrip() {
    let stmt = "CREATE COLLECTION docs (bm25 SPARSE WITH SPARSE (modifier = 'idf', full_scan_threshold = 10000, on_disk = true, datatype = 'float32'));";
    let parsed =
        qql_core::parser::Parser::parse(stmt).expect("sparse vector full config should parse");
    let qql_core::ast::Stmt::CreateCollection(c) = parsed else {
        panic!("expected CreateCollection");
    };
    assert_eq!(c.sparse_vectors[0].name, "bm25");
    assert_eq!(c.sparse_vectors[0].modifier.as_deref(), Some("idf"));
    let idx = c.sparse_vectors[0].index.as_ref().unwrap();
    assert_eq!(idx.full_scan_threshold, Some(10000));
    assert_eq!(idx.on_disk, Some(true));
    assert_eq!(idx.datatype, Some(qql_core::ast::VectorDatatype::Float32));
}

#[test]
fn sparse_with_sparse_and_index_blocks_merge() {
    let stmt = "CREATE COLLECTION docs (bm25 SPARSE WITH SPARSE (modifier = 'idf') WITH INDEX (full_scan_threshold = 5000, on_disk = true));";
    let parsed = qql_core::parser::Parser::parse(stmt).expect("merged sparse blocks");
    let qql_core::ast::Stmt::CreateCollection(c) = parsed else {
        panic!("expected CreateCollection");
    };
    assert_eq!(c.sparse_vectors[0].modifier.as_deref(), Some("idf"));
    let idx = c.sparse_vectors[0].index.as_ref().unwrap();
    assert_eq!(idx.full_scan_threshold, Some(5000));
    assert_eq!(idx.on_disk, Some(true));
}

#[test]
fn dump_emits_sparse_full_config_and_sharding_method() {
    let mut index = serde_json::Map::new();
    index.insert("full_scan_threshold".into(), json!(10000));
    index.insert("on_disk".into(), json!(true));
    index.insert("datatype".into(), json!("Float32")); // protobuf-style → normalize
    index.insert("unknown_field".into(), json!(1)); // must not be emitted

    let mut info = CollectionInfo::default();
    info.schema.sparse_vectors = vec![qql::backend::SparseVectorSpec {
        name: "bm25".into(),
        index: Some(index),
        modifier: Some("idf".into()),
    }];
    info.schema.params.sharding_method = Some("custom".into());
    info.schema.params.shard_number = Some(3);

    let stmt = generate_create_statement("docs", &info);
    assert!(stmt.contains("bm25 SPARSE WITH SPARSE ("));
    assert!(stmt.contains("modifier = 'idf'"));
    assert!(stmt.contains("full_scan_threshold = 10000"));
    assert!(stmt.contains("on_disk = true"));
    assert!(stmt.contains("datatype = 'float32'"));
    assert!(!stmt.contains("unknown_field"));
    assert!(stmt.contains("sharding_method = 'custom'"));
    assert!(stmt.contains("shard_number = 3"));
    qql_core::parser::Parser::parse(&format!("{};", stmt))
        .expect("dumped sparse CREATE should re-parse");
}
