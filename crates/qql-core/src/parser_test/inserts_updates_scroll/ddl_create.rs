use alloc::string::ToString;

use crate::ast::{QuantizationType, Stmt, Value};
use crate::parser_test::{assert_parse_err, assert_parse_ok, i64_val, parse, str_val};

// ── CREATE COLLECTION ────────────────────────────────────────

#[test]
fn test_create_collection_simple() {
    let stmt = assert_parse_ok("CREATE COLLECTION mycollection");
    match stmt {
        Stmt::CreateCollection(c) => {
            assert_eq!(c.collection, "mycollection");
            assert!(!c.hybrid);
            assert!(c.model.is_none());
            assert!(c.config.is_none());
        }
        _ => panic!("expected CreateCollection"),
    }
}

#[test]
fn test_create_collection_hybrid() {
    let stmt = assert_parse_ok("CREATE COLLECTION mycollection HYBRID");
    match stmt {
        Stmt::CreateCollection(c) => {
            assert_eq!(c.collection, "mycollection");
            assert!(c.hybrid);
        }
        _ => panic!("expected CreateCollection"),
    }
}

#[test]
fn test_create_collection_with_model() {
    let stmt = assert_parse_ok("CREATE COLLECTION mycollection USING MODEL 'dense-model'");
    match stmt {
        Stmt::CreateCollection(c) => {
            assert_eq!(c.collection, "mycollection");
            assert_eq!(c.model, Some(String::from("dense-model")));
        }
        _ => panic!("expected CreateCollection"),
    }
}

#[test]
fn test_create_collection_with_scalar_quantization() {
    let stmt =
        assert_parse_ok("CREATE COLLECTION mycollection WITH QUANTIZATION (type = 'scalar')");
    match stmt {
        Stmt::CreateCollection(c) => {
            assert_eq!(c.collection, "mycollection");
            let cfg = c.config.as_ref().unwrap();
            let q = cfg.quantization.as_ref().unwrap();
            assert_eq!(q.qtype, QuantizationType::Scalar);
        }
        _ => panic!("expected CreateCollection"),
    }
}

#[test]
fn test_create_collection_with_scalar_quantization_integer_boundary() {
    let stmt = assert_parse_ok(
        "CREATE COLLECTION mycollection WITH QUANTIZATION (type = 'scalar', quantile = 1)",
    );
    match stmt {
        Stmt::CreateCollection(c) => {
            assert_eq!(c.collection, "mycollection");
            let cfg = c.config.as_ref().unwrap();
            let q = cfg.quantization.as_ref().unwrap();
            assert_eq!(q.qtype, QuantizationType::Scalar);
            assert_eq!(q.quantile, Some(1.0));
        }
        _ => panic!("expected CreateCollection"),
    }
}

#[test]
fn test_create_collection_hybrid_rerank_product_quantization() {
    let stmt = assert_parse_ok(
            "CREATE COLLECTION mycollection HYBRID RERANK WITH QUANTIZATION (type = 'product', always_ram = true)",
        );
    match stmt {
        Stmt::CreateCollection(c) => {
            assert_eq!(c.collection, "mycollection");
            assert!(c.hybrid);
            assert!(c.rerank);
            let cfg = c.config.as_ref().unwrap();
            let q = cfg.quantization.as_ref().unwrap();
            assert_eq!(q.qtype, QuantizationType::Product);
            assert!(q.always_ram);
        }
        _ => panic!("expected CreateCollection"),
    }
}

#[test]
fn test_create_collection_with_payload_hnsw() {
    let stmt = assert_parse_ok("CREATE COLLECTION mycollection WITH HNSW (payload_m = 16)");
    match stmt {
        Stmt::CreateCollection(c) => {
            assert_eq!(c.collection, "mycollection");
            let cfg = c.config.as_ref().unwrap();
            let hnsw = cfg.hnsw.as_ref().unwrap();
            assert_eq!(hnsw.payload_m, Some(16));
        }
        _ => panic!("expected CreateCollection"),
    }
}

#[test]
fn test_create_collection_config_case_variant_keys_are_deterministic() {
    let stmt = assert_parse_ok("CREATE COLLECTION docs WITH HNSW ( M = 32, m = 16 )");
    match stmt {
        Stmt::CreateCollection(c) => {
            let cfg = c.config.as_ref().unwrap();
            let hnsw = cfg.hnsw.as_ref().unwrap();
            // Rust parser returns first match (case-insensitive)
            assert_eq!(hnsw.m, Some(32));
        }
        _ => panic!("expected CreateCollection"),
    }
}

#[test]
fn test_create_collection_rejects_non_positive_values() {
    let err = parse("CREATE COLLECTION docs WITH HNSW ( m = 3 )").unwrap_err();
    assert!(
        err.to_string().contains("m must be 0 or >= 4"),
        "got: {}",
        err
    );

    let err = parse("CREATE COLLECTION docs WITH PARAMS ( replication_factor = 0 )").unwrap_err();
    assert!(
        err.to_string()
            .contains("replication_factor must be a positive integer"),
        "got: {}",
        err
    );

    let err = parse("CREATE COLLECTION docs WITH HNSW ( full_scan_threshold = -1 )").unwrap_err();
    assert!(
        err.to_string()
            .contains("full_scan_threshold must be a non-negative integer"),
        "got: {}",
        err
    );

    let err =
        parse("CREATE COLLECTION docs WITH OPTIMIZERS ( indexing_threshold = -1 )").unwrap_err();
    assert!(
        err.to_string()
            .contains("indexing_threshold must be a non-negative integer"),
        "got: {}",
        err
    );
}

#[test]
fn test_create_collection_quantize_errors() {
    assert_parse_err("CREATE COLLECTION docs WITH QUANTIZATION (type = 'full')");
    assert_parse_err("CREATE COLLECTION docs WITH QUANTIZATION (type = 'scalar', quantile = 1.5)");
    assert_parse_err("CREATE COLLECTION docs WITH QUANTIZATION (type = 'scalar', quantile = 2)");
}

// ── CREATE INDEX ─────────────────────────────────────────────

#[test]
fn test_create_index_simple() {
    let stmt = assert_parse_ok("CREATE INDEX ON COLLECTION mycollection FOR field");
    match stmt {
        Stmt::CreateIndex(i) => {
            assert_eq!(i.collection, "mycollection");
            assert_eq!(i.field, "field");
            assert_eq!(i.field_type, "keyword");
        }
        _ => panic!("expected CreateIndex"),
    }
}

#[test]
fn test_create_index_with_keyword_type() {
    let stmt = assert_parse_ok("CREATE INDEX ON COLLECTION mycollection FOR field TYPE keyword");
    match stmt {
        Stmt::CreateIndex(i) => {
            assert_eq!(i.collection, "mycollection");
            assert_eq!(i.field, "field");
            assert_eq!(i.field_type, "keyword");
        }
        _ => panic!("expected CreateIndex"),
    }
}

#[test]
fn test_create_index_with_integer_type() {
    let stmt =
        assert_parse_ok("CREATE INDEX ON COLLECTION mycollection FOR patient_id TYPE integer");
    match stmt {
        Stmt::CreateIndex(i) => {
            assert_eq!(i.collection, "mycollection");
            assert_eq!(i.field, "patient_id");
            assert_eq!(i.field_type, "integer");
        }
        _ => panic!("expected CreateIndex"),
    }
}

#[test]
fn test_create_index_with_float_type() {
    let stmt = assert_parse_ok("CREATE INDEX ON COLLECTION mycollection FOR score TYPE float");
    match stmt {
        Stmt::CreateIndex(i) => {
            assert_eq!(i.collection, "mycollection");
            assert_eq!(i.field, "score");
            assert_eq!(i.field_type, "float");
        }
        _ => panic!("expected CreateIndex"),
    }
}

#[test]
fn test_create_index_with_bool_type() {
    let stmt = assert_parse_ok("CREATE INDEX ON COLLECTION mycollection FOR is_active TYPE bool");
    match stmt {
        Stmt::CreateIndex(i) => {
            assert_eq!(i.collection, "mycollection");
            assert_eq!(i.field, "is_active");
            assert_eq!(i.field_type, "bool");
        }
        _ => panic!("expected CreateIndex"),
    }
}

#[test]
fn test_create_index_with_keyword_options() {
    let stmt = assert_parse_ok(
            "CREATE INDEX ON COLLECTION mycollection FOR tenant_id TYPE keyword WITH (is_tenant = true, on_disk = true, enable_hnsw = false)",
        );
    match stmt {
        Stmt::CreateIndex(i) => {
            assert_eq!(i.collection, "mycollection");
            assert_eq!(i.field, "tenant_id");
            assert_eq!(i.field_type, "keyword");
            assert!(i
                .options
                .contains(&(String::from("is_tenant"), Value::Bool(true))));
            assert!(i
                .options
                .contains(&(String::from("on_disk"), Value::Bool(true))));
            assert!(i
                .options
                .contains(&(String::from("enable_hnsw"), Value::Bool(false))));
        }
        _ => panic!("expected CreateIndex"),
    }
}

#[test]
fn test_create_index_with_text_options() {
    let stmt = assert_parse_ok(
            "CREATE INDEX ON COLLECTION mycollection FOR title TYPE text WITH (tokenizer = 'word', min_token_len = 2, max_token_len = 20, lowercase = true)",
        );
    match stmt {
        Stmt::CreateIndex(i) => {
            assert_eq!(i.collection, "mycollection");
            assert_eq!(i.field, "title");
            assert_eq!(i.field_type, "text");
            assert!(i
                .options
                .contains(&(String::from("tokenizer"), str_val("word"))));
            assert!(i
                .options
                .contains(&(String::from("min_token_len"), i64_val(2))));
            assert!(i
                .options
                .contains(&(String::from("max_token_len"), i64_val(20))));
            assert!(i
                .options
                .contains(&(String::from("lowercase"), Value::Bool(true))));
        }
        _ => panic!("expected CreateIndex"),
    }
}

#[test]
fn test_create_index_validation_errors() {
    assert_parse_err(
        "CREATE INDEX ON COLLECTION mycollection FOR tenant_id TYPE keyword WITH (is_tenant = 123)",
    );
    assert_parse_err(
        "CREATE INDEX ON COLLECTION mycollection FOR title TYPE text WITH (min_token_len = -5)",
    );
    assert_parse_err(
        "CREATE INDEX ON COLLECTION mycollection FOR title TYPE text WITH (tokenizer = 123)",
    );
    assert_parse_err(
        "CREATE INDEX ON COLLECTION mycollection FOR title TYPE text WITH (stopwords = 'foo')",
    );
    assert_parse_err(
        "CREATE INDEX ON COLLECTION mycollection FOR title TYPE text WITH (stopwords = [123])",
    );
    assert_parse_err(
        "CREATE INDEX ON COLLECTION mycollection FOR title TYPE text WITH (unknown_option = true)",
    );
}

#[test]
fn test_create_collection_with_turbo_quantization_default() {
    let stmt = assert_parse_ok("CREATE COLLECTION docs WITH QUANTIZATION (type = 'turbo')");
    match stmt {
        Stmt::CreateCollection(c) => {
            let cfg = c.config.as_ref().unwrap();
            let q = cfg.quantization.as_ref().unwrap();
            assert_eq!(q.qtype, QuantizationType::Turbo);
            assert!(!q.always_ram);
            assert!(q.turbo_bits.is_none());
        }
        _ => panic!("expected CreateCollection"),
    }
}

#[test]
fn test_create_collection_with_turbo_quantization_bits_1_5() {
    let stmt =
        assert_parse_ok("CREATE COLLECTION docs WITH QUANTIZATION (type = 'turbo', bits = 1.5)");
    match stmt {
        Stmt::CreateCollection(c) => {
            let cfg = c.config.as_ref().unwrap();
            let q = cfg.quantization.as_ref().unwrap();
            assert_eq!(q.qtype, QuantizationType::Turbo);
            assert_eq!(q.turbo_bits, Some(1.5));
        }
        _ => panic!("expected CreateCollection"),
    }
}

#[test]
fn test_create_collection_with_turbo_quantization_bits_1_always_ram() {
    let stmt = assert_parse_ok(
        "CREATE COLLECTION docs WITH QUANTIZATION (type = 'turbo', bits = 1, always_ram = true)",
    );
    match stmt {
        Stmt::CreateCollection(c) => {
            let cfg = c.config.as_ref().unwrap();
            let q = cfg.quantization.as_ref().unwrap();
            assert_eq!(q.qtype, QuantizationType::Turbo);
            assert_eq!(q.turbo_bits, Some(1.0));
            assert!(q.always_ram);
        }
        _ => panic!("expected CreateCollection"),
    }
}

#[test]
fn test_create_collection_turbo_quantization_invalid_bits() {
    let err =
        parse("CREATE COLLECTION docs WITH QUANTIZATION (type = 'turbo', bits = 3)").unwrap_err();
    assert!(
        err.to_string()
            .contains("bits must be one of 1, 1.5, 2, or 4"),
        "got: {}",
        err
    );
}

#[test]
fn test_create_multi_vector() {
    let stmt = assert_parse_ok(
        "CREATE COLLECTION knowledge_graph (
                dense_text VECTOR(384, COSINE),
                clip_img VECTOR(512, DOT),
                bm25_text SPARSE
            ) WITH HNSW ( m = 32 )",
    );
    match stmt {
        Stmt::CreateCollection(c) => {
            assert_eq!(c.collection, "knowledge_graph");
            assert_eq!(c.vectors.len(), 2);
            assert_eq!(c.sparse_vectors.len(), 1);
            assert_eq!(c.vectors[0].name, "dense_text");
            assert_eq!(c.vectors[0].size, 384);
            assert_eq!(c.vectors[0].distance, crate::ast::VectorDistance::Cosine);
            assert_eq!(c.vectors[1].name, "clip_img");
            assert_eq!(c.vectors[1].size, 512);
            assert_eq!(c.vectors[1].distance, crate::ast::VectorDistance::Dot);
            assert_eq!(c.sparse_vectors[0].name, "bm25_text");
            let cfg = c.config.as_ref().unwrap();
            let hnsw = cfg.hnsw.as_ref().unwrap();
            assert_eq!(hnsw.m, Some(32));
        }
        _ => panic!("expected CreateCollection"),
    }
}

#[test]
fn test_create_multi_vector_with_overrides() {
    let stmt = assert_parse_ok(
            "CREATE COLLECTION test_overrides (
                dense_vec VECTOR(384, COSINE) WITH HNSW ( m = 16 ) WITH QUANTIZATION (type = 'scalar', always_ram = true),
                colbert_vec VECTOR(128, DOT) WITH QUANTIZATION (type = 'turbo', bits = 2)
            )",
        );
    match stmt {
        Stmt::CreateCollection(c) => {
            assert_eq!(c.collection, "test_overrides");
            assert_eq!(c.vectors.len(), 2);
            assert_eq!(c.vectors[0].name, "dense_vec");
            let h0 = c.vectors[0].hnsw.as_ref().unwrap();
            assert_eq!(h0.m, Some(16));
            let q0 = c.vectors[0].quantization.as_ref().unwrap();
            assert_eq!(q0.qtype, QuantizationType::Scalar);
            assert!(q0.always_ram);
            assert_eq!(c.vectors[1].name, "colbert_vec");
            assert!(c.vectors[1].hnsw.is_none());
            let q1 = c.vectors[1].quantization.as_ref().unwrap();
            assert_eq!(q1.qtype, QuantizationType::Turbo);
            assert_eq!(q1.turbo_bits, Some(2.0));
        }
        _ => panic!("expected CreateCollection"),
    }
}

#[test]
fn test_create_rejects_alter_only_params() {
    let err = parse("CREATE COLLECTION docs WITH PARAMS ( Read_Fan_Out_Factor = 4 )").unwrap_err();
    assert!(
        err.to_string()
            .contains("supported only for ALTER COLLECTION"),
        "got: {}",
        err
    );
}

#[test]
fn test_parse_create() {
    let stmt = assert_parse_ok("CREATE COLLECTION test");
    match stmt {
        Stmt::CreateCollection(_) => {}
        _ => panic!("expected CreateCollection"),
    }
}
