use super::runtime::turbo_bits_label;
use super::*;
use crate::types::{
    CreateCollectionRequest, DenseVectorParams, DenseVectorsConfig, MemoryPlacement,
    QuantizationConfigDiff, StemmingAlgorithm, StopwordsSet, TextTokenizer,
};
use qql_core::ast::Stmt;
use qql_core::ast::VectorsConfig;
use qql_core::parser::Parser;

fn parse_stmt(s: &str) -> Stmt {
    Parser::parse(s).expect("parse failed")
}

macro_rules! rest_json {
    ($req:expr) => {
        serde_json::to_value(create_collection_rest_body($req)).unwrap()
    };
}

#[test]
fn create_collection_dense() {
    let stmt = parse_stmt("CREATE COLLECTION docs (dense VECTOR(384, COSINE));");
    let Stmt::CreateCollection(ref cc) = stmt else {
        panic!()
    };
    let req = lower_create_collection(cc).unwrap();
    let json = rest_json!(&req);
    assert_eq!(json["vectors"]["dense"]["size"], 384);
    assert_eq!(json["vectors"]["dense"]["distance"], "Cosine");
}

#[test]
fn single_vector_config_serializes_as_bare_params() {
    let req = CreateCollectionRequest {
        vectors: Some(DenseVectorsConfig::Single(DenseVectorParams {
            size: 8,
            distance: qql_core::ast::VectorDistance::Dot,
            hnsw_config: None,
            quantization_config: None,
            on_disk: None,
            memory: None,
            datatype: None,
            multivector_config: None,
        })),
        ..Default::default()
    };
    let json = rest_json!(&req);
    assert_eq!(json["vectors"]["size"], 8);
    assert_eq!(json["vectors"]["distance"], "Dot");
}

#[test]
fn create_collection_with_config() {
    let stmt = parse_stmt("CREATE COLLECTION docs (dense VECTOR(128, EUCLID)) WITH HNSW (m = 16);");
    let Stmt::CreateCollection(ref cc) = stmt else {
        panic!()
    };
    let req = lower_create_collection(cc).unwrap();
    let json = serde_json::to_value(&req).unwrap();
    assert_eq!(json["hnsw_config"]["m"], 16);
}

#[test]
fn create_index() {
    let stmt =
        parse_stmt("CREATE INDEX ON COLLECTION docs FOR title TYPE text WITH (lowercase = true);");
    let Stmt::CreateIndex(ref ci) = stmt else {
        panic!()
    };
    let req = lower_create_index(ci).unwrap();
    // IR keeps typed options separate from the schema.
    let ir = serde_json::to_value(&req).unwrap();
    assert_eq!(ir["field_name"], "title");
    assert_eq!(ir["field_schema"], "text");
    assert_eq!(ir["options"]["lowercase"], true);
    // REST OpenAPI nests options under field_schema object
    let rest = serde_json::to_value(create_index_rest_body(&req)).unwrap();
    assert_eq!(rest["field_name"], "title");
    assert_eq!(rest["field_schema"]["type"], "text");
    assert_eq!(rest["field_schema"]["lowercase"], true);
    assert!(rest.get("lowercase").is_none());
}

#[test]
fn create_index_without_options_uses_type_string() {
    let stmt = parse_stmt("CREATE INDEX ON COLLECTION docs FOR tag TYPE keyword;");
    let Stmt::CreateIndex(ref ci) = stmt else {
        panic!()
    };
    let req = lower_create_index(ci).unwrap();
    let rest = serde_json::to_value(create_index_rest_body(&req)).unwrap();
    assert_eq!(rest["field_schema"], "keyword");
}

#[test]
fn create_keyword_index_with_prefix_and_memory() {
    let stmt = parse_stmt(
        "CREATE INDEX ON COLLECTION docs FOR tenant TYPE keyword WITH (prefix = true, memory = 'cached', is_tenant = true);",
    );
    let Stmt::CreateIndex(ref ci) = stmt else {
        panic!()
    };
    let req = lower_create_index(ci).unwrap();
    let rest = serde_json::to_value(create_index_rest_body(&req)).unwrap();
    assert_eq!(rest["field_schema"]["type"], "keyword");
    assert_eq!(rest["field_schema"]["prefix"], true);
    assert_eq!(rest["field_schema"]["memory"], "cached");
    assert_eq!(rest["field_schema"]["is_tenant"], true);
    // Typed options survive for the gRPC payload_index_params path.
    assert_eq!(req.options.prefix, Some(true));
    assert_eq!(req.options.memory, Some(MemoryPlacement::Cached));
}

#[test]
fn create_text_index_typed_options() {
    let stmt = parse_stmt(
        "CREATE INDEX ON COLLECTION docs FOR body TYPE text WITH (tokenizer = 'WORD', lowercase = true, min_token_len = 2, max_token_len = 10, stopwords = ['the', 'a'], stemmer = 'English');",
    );
    let Stmt::CreateIndex(ref ci) = stmt else {
        panic!()
    };
    let req = lower_create_index(ci).unwrap();
    assert_eq!(req.options.tokenizer, Some(TextTokenizer::Word));
    assert_eq!(req.options.min_token_len, Some(2));
    assert_eq!(req.options.max_token_len, Some(10));
    assert_eq!(
        req.options.stopwords,
        Some(StopwordsSet {
            languages: Vec::new(),
            custom: vec!["the".into(), "a".into()]
        })
    );
    assert_eq!(
        req.options.stemmer,
        Some(StemmingAlgorithm::Snowball("english".into()))
    );
    let rest = serde_json::to_value(create_index_rest_body(&req)).unwrap();
    assert_eq!(rest["field_schema"]["tokenizer"], "word");
    assert_eq!(rest["field_schema"]["stemmer"]["type"], "snowball");
    assert_eq!(rest["field_schema"]["stemmer"]["language"], "english");
    assert_eq!(rest["field_schema"]["stopwords"]["custom"][0], "the");
}

#[test]
fn create_index_fails_closed_on_unknown_values() {
    let Stmt::CreateIndex(ci) = parse_stmt(
        "CREATE INDEX ON COLLECTION docs FOR body TYPE text WITH (tokenizer = 'bogus');",
    ) else {
        panic!()
    };
    let err = lower_create_index(&ci).unwrap_err();
    assert_eq!(err.code, "QQL-PLAN-INDEX-OPTION");

    // The parser rejects unknown field types; a programmatically built AST
    // still fails closed in the planner.
    let ci = qql_core::ast::CreateIndexStmt {
        collection: "docs".into(),
        field: "body".into(),
        field_type: "bogus".into(),
        options: Vec::new(),
        wait: None,
    };
    let err = lower_create_index(&ci).unwrap_err();
    assert_eq!(err.code, "QQL-PLAN-INDEX-TYPE");
}

#[test]
fn index_option_errors_carry_value_spans() {
    // One rule for all paths: every index-option error threads the
    // offending value's span. Scalar literals store no span, so only
    // located placeholders can point — including on the unknown-option
    // paths that previously hardcoded None.
    let span = qql_core::error::Span::new(10, 15);
    let located = qql_core::ast::CreateIndexStmt {
        collection: "docs".into(),
        field: "body".into(),
        field_type: "keyword".into(),
        options: alloc::vec![(
            "is_tenant".to_string(),
            qql_core::ast::Value::param_with_span("flag", span),
        )],
        wait: None,
    };
    let err = lower_create_index(&located).unwrap_err();
    assert_eq!(err.code, "QQL-PLAN-INDEX-OPTION");
    assert_eq!(err.span, Some(span));

    let unknown = qql_core::ast::CreateIndexStmt {
        collection: "docs".into(),
        field: "body".into(),
        field_type: "keyword".into(),
        options: alloc::vec![(
            "bogus".to_string(),
            qql_core::ast::Value::param_with_span("v", span),
        )],
        wait: None,
    };
    let err = lower_create_index(&unknown).unwrap_err();
    assert_eq!(err.code, "QQL-PLAN-INDEX-OPTION");
    assert_eq!(err.span, Some(span));
}

#[test]
fn collection_config_errors_carry_value_spans() {
    // Same rule for collection config: unknown keys thread the value
    // span instead of hardcoding None. The parser rejects these first,
    // so only hand-built ASTs reach this path.
    let span = qql_core::error::Span::new(3, 8);
    let located = qql_core::ast::AlterCollectionStmt {
        collection: "docs".into(),
        config: Some(Box::new(qql_core::ast::CollectionConfig {
            vectors: None,
            hnsw: None,
            optimizers: None,
            params: None,
            quantization: None,
            quantization_update: None,
            wal: None,
            strict_mode: Some(alloc::vec![(
                "bogus".to_string(),
                qql_core::ast::Value::param_with_span("v", span),
            )]),
            metadata: None,
            vector_diffs: alloc::vec![],
            sparse_vector_diffs: alloc::vec![],
        })),
    };
    let err = lower_alter_collection(&located).unwrap_err();
    assert_eq!(err.code, "QQL-PLAN-COLLECTION-CONFIG");
    assert_eq!(err.span, Some(span));
}

#[test]
fn lower_product_quantization_includes_compression() {
    let stmt = parse_stmt(
        "CREATE COLLECTION docs (v VECTOR(128, COSINE) WITH QUANTIZATION (type = 'product', compression = 'x16', always_ram = true));",
    );
    let Stmt::CreateCollection(ref cc) = stmt else {
        panic!()
    };
    let req = lower_create_collection(cc).unwrap();
    let json = rest_json!(&req);
    let quant = &json["vectors"]["v"]["quantization_config"];
    assert_eq!(quant["product"]["compression"], "x16");
    assert_eq!(quant["product"]["always_ram"], true);
}

#[test]
fn lower_product_quantization_defaults_compression() {
    let stmt = parse_stmt(
        "CREATE COLLECTION docs (v VECTOR(128, COSINE) WITH QUANTIZATION (type = 'product', always_ram = true));",
    );
    let Stmt::CreateCollection(ref cc) = stmt else {
        panic!()
    };
    let req = lower_create_collection(cc).unwrap();
    let json = rest_json!(&req);
    assert_eq!(
        json["vectors"]["v"]["quantization_config"]["product"]["compression"],
        "x4"
    );
}

#[test]
fn lower_binary_quantization_includes_encoding() {
    let stmt = parse_stmt(
        "CREATE COLLECTION docs (v VECTOR(128, COSINE) WITH QUANTIZATION (type = 'binary', encoding = 'two_bits', always_ram = true));",
    );
    let Stmt::CreateCollection(ref cc) = stmt else {
        panic!()
    };
    let req = lower_create_collection(cc).unwrap();
    let json = rest_json!(&req);
    let quant = &json["vectors"]["v"]["quantization_config"];
    assert_eq!(quant["binary"]["encoding"], "two_bits");
    assert_eq!(quant["binary"]["always_ram"], true);
}

#[test]
fn lower_turbo_quantization_includes_bits() {
    let stmt = parse_stmt(
        "CREATE COLLECTION docs (v VECTOR(128, COSINE) WITH QUANTIZATION (type = 'turbo', bits = 1.5, always_ram = true));",
    );
    let Stmt::CreateCollection(ref cc) = stmt else {
        panic!()
    };
    let req = lower_create_collection(cc).unwrap();
    let json = rest_json!(&req);
    let quant = &json["vectors"]["v"]["quantization_config"];
    assert_eq!(quant["turbo"]["bits"], "bits1_5");
    assert_eq!(quant["turbo"]["always_ram"], true);
}

#[test]
fn turbo_bits_label_maps_known_values() {
    // Unknown values stay backend-rejectable (no silent "bits1").
    for (bits, expected) in [
        (1.0, "bits1"),
        (1.5, "bits1_5"),
        (2.0, "bits2"),
        (4.0, "bits4"),
    ] {
        assert_eq!(turbo_bits_label(Some(bits)).as_deref(), Some(expected));
    }
    assert_eq!(turbo_bits_label(Some(3.0)).as_deref(), Some("bits3"));
}

#[test]
fn lower_vector_on_disk_and_query_encoding_and_multivector() {
    let stmt = parse_stmt(
        "CREATE COLLECTION docs (v VECTOR(64, COSINE) WITH MULTIVECTOR (comparator = 'max_sim') WITH VECTOR (on_disk = true) WITH QUANTIZATION (type = 'binary', encoding = 'two_bits', query_encoding = 'scalar4bits', always_ram = true));",
    );
    let Stmt::CreateCollection(ref cc) = stmt else {
        panic!()
    };
    let req = lower_create_collection(cc).unwrap();
    let json = rest_json!(&req);
    let v = &json["vectors"]["v"];
    assert_eq!(v["on_disk"], true);
    assert_eq!(v["multivector_config"]["comparator"], "max_sim");
    assert_eq!(v["quantization_config"]["binary"]["encoding"], "two_bits");
    assert_eq!(
        v["quantization_config"]["binary"]["query_encoding"],
        "scalar4bits"
    );
}

#[test]
fn collection_wide_vector_defaults_apply_to_every_vector() {
    let stmt = parse_stmt(
        "CREATE COLLECTION docs (v VECTOR(8, COSINE), w VECTOR(4, DOT)) WITH VECTOR (on_disk = true, memory = 'cached', datatype = 'float16');",
    );
    let Stmt::CreateCollection(ref cc) = stmt else {
        panic!()
    };
    let req = lower_create_collection(cc).unwrap();
    let json = rest_json!(&req);
    for name in ["v", "w"] {
        assert_eq!(json["vectors"][name]["on_disk"], true);
        assert_eq!(json["vectors"][name]["memory"], "cached");
        assert_eq!(json["vectors"][name]["datatype"], "float16");
    }
}

#[test]
fn collection_wide_vector_defaults_do_not_override_per_vector() {
    let stmt = parse_stmt(
        "CREATE COLLECTION docs (v VECTOR(8, COSINE) WITH VECTOR (on_disk = false, memory = 'cold')) WITH VECTOR (on_disk = true, memory = 'cached', datatype = 'float16');",
    );
    let Stmt::CreateCollection(ref cc) = stmt else {
        panic!()
    };
    let req = lower_create_collection(cc).unwrap();
    let json = rest_json!(&req);
    assert_eq!(json["vectors"]["v"]["on_disk"], false);
    assert_eq!(json["vectors"]["v"]["memory"], "cold");
    // Unset per-vector fields still pick up the collection default.
    assert_eq!(json["vectors"]["v"]["datatype"], "float16");
}

#[test]
fn lower_optimizers_auto_threads() {
    let stmt = parse_stmt(
        "CREATE COLLECTION docs (v VECTOR(8, COSINE)) WITH OPTIMIZERS (max_optimization_threads = 'auto', indexing_threshold = 1000);",
    );
    let Stmt::CreateCollection(ref cc) = stmt else {
        panic!()
    };
    let req = lower_create_collection(cc).unwrap();
    let json = serde_json::to_value(&req).unwrap();
    assert_eq!(
        json["optimizers_config"]["max_optimization_threads"],
        "auto"
    );
    assert_eq!(json["optimizers_config"]["indexing_threshold"], 1000);
}

#[test]
fn rest_body_flattens_params_and_nests_quantization() {
    let stmt = parse_stmt(
        "CREATE COLLECTION docs (v VECTOR(128, COSINE) WITH QUANTIZATION (type = 'scalar', quantile = 0.99, always_ram = true)) \
         WITH HNSW (m = 16) \
         WITH PARAMS (replication_factor = 2, write_consistency_factor = 1, on_disk_payload = true, shard_number = 4, sharding_method = 'custom', shard_keys = ['a', 'b']);",
    );
    let Stmt::CreateCollection(ref cc) = stmt else {
        panic!()
    };
    let req = lower_create_collection(cc).unwrap();
    let rest = rest_json!(&req);
    // OpenAPI top-level params
    assert_eq!(rest["replication_factor"], 2);
    assert_eq!(rest["write_consistency_factor"], 1);
    assert_eq!(rest["on_disk_payload"], true);
    assert_eq!(rest["shard_number"], 4);
    assert_eq!(rest["sharding_method"], "custom");
    assert!(
        rest.get("params").is_none(),
        "params must not be nested on create"
    );
    assert!(
        rest.get("shard_keys").is_none(),
        "shard_keys not on CreateCollection"
    );
    // Nested quantization
    assert_eq!(
        rest["vectors"]["v"]["quantization_config"]["scalar"]["type"],
        "int8"
    );
    assert_eq!(
        rest["vectors"]["v"]["quantization_config"]["scalar"]["quantile"],
        0.99
    );
    assert_eq!(rest["hnsw_config"]["m"], 16);
    // Deferred fan-out only when IR carries those keys (ALTER-only in grammar;
    // still projected for gRPC/REST multi-step if present).
    let mut with_fanout = req.clone();
    with_fanout
        .params
        .as_mut()
        .expect("params")
        .read_fan_out_factor = Some(3);
    let deferred = serde_json::to_value(
        create_collection_deferred_params_rest(&with_fanout).expect("deferred params"),
    )
    .unwrap();
    assert_eq!(deferred["params"]["read_fan_out_factor"], 3);
    // IR keeps typed vectors and shard keys
    let ir = serde_json::to_value(&req).unwrap();
    assert_eq!(
        ir["vectors"]["v"]["quantization_config"]["scalar"]["type"],
        "int8"
    );
    assert_eq!(ir["params"]["replication_factor"], 2);
    assert_eq!(ir["sharding_method"], "custom");
    assert_eq!(ir["shard_keys"], serde_json::json!(["a", "b"]));
}

#[test]
fn rest_body_config_sections_are_objects_not_null() {
    // Regression: these sections used `serde_json::to_value(...).unwrap_or_default()`,
    // silently inserting JSON `null` on any serialization failure. Assert the
    // create REST body carries real objects on the wire.
    let stmt = parse_stmt(
        "CREATE COLLECTION docs (v VECTOR(128, COSINE)) WITH HNSW (m = 16) WITH OPTIMIZERS (indexing_threshold = 1000) WITH QUANTIZATION (type = 'scalar', quantile = 0.5);",
    );
    let Stmt::CreateCollection(ref cc) = stmt else {
        panic!()
    };
    let req = lower_create_collection(cc).unwrap();
    let rest = rest_json!(&req);
    assert!(rest["hnsw_config"].is_object());
    assert_eq!(rest["hnsw_config"]["m"], 16);
    assert!(rest["optimizers_config"].is_object());
    assert_eq!(rest["optimizers_config"]["indexing_threshold"], 1000);
    assert!(rest["quantization_config"].is_object());
    assert_eq!(rest["quantization_config"]["scalar"]["type"], "int8");
    assert_eq!(rest["quantization_config"]["scalar"]["quantile"], 0.5);
}

#[test]
fn update_rest_body_config_sections_are_objects_not_null() {
    // Regression for the PATCH body (same swallowed-serialization footgun
    // as create). PATCH body must carry real hnsw/optimizers objects.
    let stmt = parse_stmt(
        "ALTER COLLECTION docs WITH HNSW (m = 32) WITH OPTIMIZERS (indexing_threshold = 500);",
    );
    let Stmt::AlterCollection(ref ac) = stmt else {
        panic!()
    };
    let req = lower_alter_collection(ac).unwrap();
    let rest = serde_json::to_value(&req).unwrap();
    assert!(rest["hnsw_config"].is_object());
    assert_eq!(rest["hnsw_config"]["m"], 32);
    assert!(rest["optimizers_config"].is_object());
    assert_eq!(rest["optimizers_config"]["indexing_threshold"], 500);
}

#[test]
fn update_rest_body_disabled_quantization_and_product_default() {
    let stmt = parse_stmt("ALTER COLLECTION docs WITH QUANTIZATION (disabled = true);");
    let Stmt::AlterCollection(ref ac) = stmt else {
        panic!()
    };
    let req = lower_alter_collection(ac).unwrap();
    assert!(matches!(
        req.quantization_config,
        Some(QuantizationConfigDiff::Disabled)
    ));
    let json = serde_json::to_value(&req).unwrap();
    assert_eq!(json["quantization_config"], "Disabled");

    let stmt = parse_stmt(
        "ALTER COLLECTION docs WITH QUANTIZATION (type = 'product', always_ram = true);",
    );
    let Stmt::AlterCollection(ref ac) = stmt else {
        panic!()
    };
    let req = lower_alter_collection(ac).unwrap();
    let json = serde_json::to_value(&req).unwrap();
    assert_eq!(json["quantization_config"]["product"]["compression"], "x4");
    assert_eq!(json["quantization_config"]["product"]["always_ram"], true);
}

#[test]
fn update_rest_body_nests_params_with_payload() {
    let stmt = parse_stmt(
        "ALTER COLLECTION docs WITH PARAMS (replication_factor = 3, payload_memory = 'cached');",
    );
    let Stmt::AlterCollection(ref ac) = stmt else {
        panic!()
    };
    let req = lower_alter_collection(ac).unwrap();
    let json = serde_json::to_value(&req).unwrap();
    assert_eq!(json["params"]["replication_factor"], 3);
    assert_eq!(json["params"]["payload"]["memory"], "cached");
}

#[test]
fn update_rest_body_lowers_named_vector_diffs() {
    let stmt = parse_stmt(
        "ALTER COLLECTION docs \
         WITH VECTOR dense (HNSW (m = 32, memory = 'cold'), QUANTIZATION (type = 'binary', encoding = 'two_bits'), VECTOR (on_disk = true, memory = 'cached')) \
         WITH VECTOR colbert (QUANTIZATION (disabled = true)) \
         WITH SPARSE bm25 (SPARSE (modifier = 'none', full_scan_threshold = 5000, memory = 'pinned', datatype = 'float16'));",
    );
    let Stmt::AlterCollection(ref ac) = stmt else {
        panic!()
    };
    let req = lower_alter_collection(ac).unwrap();
    let json = serde_json::to_value(&req).unwrap();

    let dense = &json["vectors"]["dense"];
    assert_eq!(dense["hnsw_config"]["m"], 32);
    assert_eq!(dense["hnsw_config"]["memory"], "cold");
    assert_eq!(
        dense["quantization_config"]["binary"]["encoding"],
        "two_bits"
    );
    assert_eq!(dense["on_disk"], true);
    assert_eq!(dense["memory"], "cached");
    // Unset keys stay absent (field-wise diff), never serialized as null.
    assert!(dense.get("datatype").is_none());

    // `disabled = true` clears the per-vector quantization config.
    assert_eq!(
        json["vectors"]["colbert"]["quantization_config"],
        "Disabled"
    );

    let sparse = &json["sparse_vectors"]["bm25"];
    assert_eq!(sparse["modifier"], "none");
    assert_eq!(sparse["index"]["full_scan_threshold"], 5000);
    assert_eq!(sparse["index"]["memory"], "pinned");
    assert_eq!(sparse["index"]["datatype"], "float16");
}

#[test]
fn update_rest_body_unnamed_vector_diff_uses_empty_key() {
    let stmt = parse_stmt("ALTER COLLECTION docs WITH VECTOR (on_disk = true, memory = 'cold');");
    let Stmt::AlterCollection(ref ac) = stmt else {
        panic!()
    };
    let req = lower_alter_collection(ac).unwrap();
    let json = serde_json::to_value(&req).unwrap();
    assert_eq!(json["vectors"][""]["on_disk"], true);
    assert_eq!(json["vectors"][""]["memory"], "cold");
    assert!(json.get("sparse_vectors").is_none());
}

#[test]
fn update_rest_body_per_vector_hnsw_only() {
    let stmt = parse_stmt("ALTER COLLECTION docs WITH VECTOR dense (HNSW (ef_construct = 200));");
    let Stmt::AlterCollection(ref ac) = stmt else {
        panic!()
    };
    let req = lower_alter_collection(ac).unwrap();
    let json = serde_json::to_value(&req).unwrap();
    let dense = &json["vectors"]["dense"];
    assert_eq!(dense["hnsw_config"]["ef_construct"], 200);
    assert_eq!(dense.as_object().unwrap().len(), 1, "only the HNSW key");
}

#[test]
fn update_plan_rejects_datatype_and_duplicate_names() {
    // The parser rejects `datatype` inside a diff; a hand-built AST must
    // fail closed in the planner instead of silently dropping it.
    let stmt = parse_stmt("ALTER COLLECTION docs WITH VECTOR dense (HNSW (m = 16));");
    let Stmt::AlterCollection(mut ac) = stmt else {
        panic!()
    };
    ac.config.as_mut().unwrap().vector_diffs[0].vectors = Some(Box::new(VectorsConfig {
        on_disk: None,
        memory: None,
        datatype: Some(qql_core::ast::VectorDatatype::Float16),
    }));
    let err = lower_alter_collection(&ac).unwrap_err();
    assert_eq!(err.code, "QQL-PLAN-VECTOR-DIFF");
    assert!(err.message.contains("datatype"), "{err}");

    // The unnamed/default form shares the create-shaped VECTOR block
    // parser, so the datatype rejection also lands at planning.
    let stmt = parse_stmt("ALTER COLLECTION docs WITH VECTOR (datatype = 'float16');");
    let Stmt::AlterCollection(ref ac) = stmt else {
        panic!()
    };
    let err = lower_alter_collection(ac).unwrap_err();
    assert_eq!(err.code, "QQL-PLAN-VECTOR-DIFF");
    assert!(err.message.contains("datatype"), "{err}");

    // Duplicate diff names (possible only via a hand-built AST) fail closed
    // instead of letting the BTreeMap keep the last entry.
    let stmt = parse_stmt("ALTER COLLECTION docs WITH VECTOR dense (HNSW (m = 16));");
    let Stmt::AlterCollection(mut ac) = stmt else {
        panic!()
    };
    let mut duplicate = ac.config.as_ref().unwrap().vector_diffs[0].clone();
    duplicate.hnsw = None;
    ac.config.as_mut().unwrap().vector_diffs.push(duplicate);
    let err = lower_alter_collection(&ac).unwrap_err();
    assert_eq!(err.code, "QQL-PLAN-VECTOR-DIFF");
    assert!(err.message.contains("duplicate vector diff"), "{err}");
}

#[test]
fn lower_sparse_vector_full_config_and_sharding_method() {
    let stmt = parse_stmt(
        "CREATE COLLECTION docs (bm25 SPARSE WITH SPARSE (modifier = 'idf', full_scan_threshold = 10000, on_disk = true, datatype = 'float32')) WITH PARAMS (sharding_method = 'custom', shard_number = 2);",
    );
    let Stmt::CreateCollection(ref cc) = stmt else {
        panic!()
    };
    let req = lower_create_collection(cc).unwrap();
    let json = rest_json!(&req);
    let sparse = &json["sparse_vectors"]["bm25"];
    assert_eq!(sparse["modifier"], "idf");
    assert_eq!(sparse["index"]["full_scan_threshold"], 10000);
    assert_eq!(sparse["index"]["on_disk"], true);
    assert_eq!(sparse["index"]["datatype"], "float32");
    assert_eq!(json["sharding_method"], "custom");
    assert_eq!(json["shard_number"], 2);
}

#[test]
fn lower_sparse_default_modifier_is_idf() {
    let stmt = parse_stmt("CREATE COLLECTION docs (bm25 SPARSE);");
    let Stmt::CreateCollection(ref cc) = stmt else {
        panic!()
    };
    let req = lower_create_collection(cc).unwrap();
    let json = rest_json!(&req);
    assert_eq!(json["sparse_vectors"]["bm25"]["modifier"], "idf");
}
