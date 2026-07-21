use crate::offline;

fn compile_ok(input: &str) -> serde_json::Value {
    let compiled = offline::compile(input).expect("compile should succeed");
    compiled.payload
}

fn assert_json_field(json: &serde_json::Value, field: &str, expected: &str) {
    let actual = json[field].to_string();
    assert_eq!(
        actual, expected,
        "field '{}' mismatch: expected '{}', got '{}'",
        field, expected, actual
    );
}

// ── Query tests ────────────────────────────────────────────────────

#[test]
fn test_compile_query_simple() {
    let payload = compile_ok("QUERY 'search' FROM docs LIMIT 10");
    assert_eq!(payload["collection_name"], "docs");
    assert_eq!(payload["limit"], 10);
    assert_eq!(payload["query"]["nearest"]["document"]["text"], "search");
}

#[test]
fn test_compile_query_with_vector_specific() {
    let payload = compile_ok("QUERY 'search' FROM docs LIMIT 10 WITH VECTOR ('dense', 'colbert')");
    assert_eq!(payload["with_vector"]["vectors"][0], "dense");
    assert_eq!(payload["with_vector"]["vectors"][1], "colbert");
    assert!(payload["with_vector"]["enable"].is_null());
}

#[test]
fn test_compile_query_with_payload_false() {
    let payload = compile_ok("QUERY 'search' FROM docs LIMIT 5 WITH PAYLOAD false");
    assert_eq!(payload["with_payload"]["enable"], false);
}

#[test]
fn test_compile_query_hybrid() {
    let payload = compile_ok("QUERY 'search' FROM docs LIMIT 10 USING HYBRID");
    // USING HYBRID sets query_type, not the using_ field — verify basic structure
    assert_eq!(payload["collection_name"], "docs");
    assert_eq!(payload["limit"], 10);
    assert_eq!(payload["query"]["nearest"]["document"]["text"], "search");
}

#[test]
fn test_compile_query_offset_threshold() {
    let payload = compile_ok("QUERY 'search' FROM docs LIMIT 10 OFFSET 5 SCORE THRESHOLD 0.7");
    assert_eq!(payload["offset"], 5);
    assert_eq!(payload["score_threshold"], 0.7);
}

#[test]
fn test_compile_query_hnsw_ef() {
    let payload = compile_ok("QUERY 'search' FROM docs LIMIT 10 WITH (hnsw_ef = 256)");
    assert_eq!(payload["params"]["hnsw_ef"], 256);
}

#[test]
fn test_compile_query_exact() {
    let payload = compile_ok("QUERY 'search' FROM docs LIMIT 10 EXACT");
    assert_eq!(payload["params"]["exact"], true);
}

#[test]
fn test_compile_query_rerank() {
    let payload = compile_ok("QUERY 'search' FROM docs LIMIT 10 RERANK");
    assert_eq!(payload["rerank"], serde_json::json!({}));
}

#[test]
fn test_compile_query_raw_vector() {
    let payload = compile_ok("QUERY [0.1, 0.2, 0.3] FROM docs LIMIT 5");
    assert_eq!(payload["query"]["nearest"][0], 0.1);
    assert_eq!(payload["query"]["nearest"][1], 0.2);
    assert_eq!(payload["query"]["nearest"][2], 0.3);
}

#[test]
fn test_compile_query_fusion() {
    let payload = compile_ok(
        "WITH a AS (QUERY 'search' USING dense LIMIT 10) QUERY 'search' FROM docs LIMIT 5 PREFETCH (a) FUSION RRF",
    );
    assert_eq!(payload["fusion"], "RRF");
    assert!(!payload["prefetch"].as_array().unwrap().is_empty());
}

#[test]
fn test_compile_cte_full_demo() {
    let payload = compile_ok(
        "WITH dense AS (QUERY 'vector database performance' USING dense LIMIT 200 WHERE category = 'tech'), sparse AS (QUERY 'vector database performance' USING sparse LIMIT 300) QUERY 'vector database performance' FROM articles LIMIT 10 PREFETCH (dense SCORE THRESHOLD 0.6, sparse SCORE THRESHOLD 0.3) FUSION RRF WITH (rrf_k = 20, rrf_weights = [0.6, 0.4])",
    );
    assert_eq!(payload["collection_name"], "articles");
    assert_eq!(payload["fusion"], "RRF");
    assert_eq!(payload["limit"], 10);
    assert!(!payload["prefetch"].as_array().unwrap().is_empty());
}

// ── Upsert tests ───────────────────────────────────────────────────

#[test]
fn test_compile_upsert_simple() {
    let payload =
        compile_ok("UPSERT INTO docs VALUES {id: 1, text: 'hello world', category: 'test'}");
    assert_eq!(payload["collection_name"], "docs");
    let points = payload["points"].as_array().unwrap();
    assert_eq!(points.len(), 1);
    assert_eq!(points[0]["id"], 1);
    assert_eq!(points[0]["payload"]["text"], "hello world");
    assert_eq!(points[0]["payload"]["category"], "test");
}

#[test]
fn test_compile_upsert_multiple() {
    let payload =
        compile_ok("UPSERT INTO docs VALUES {id: 1, text: 'first'}, {id: 2, text: 'second'}");
    assert_eq!(payload["points"].as_array().unwrap().len(), 2);
}

#[test]
fn test_compile_upsert_with_named_vectors() {
    let payload = compile_ok(
        "UPSERT INTO docs VALUES {id: 1, text: 'hello', vector: {'dense': [0.1, 0.2], 'sparse': [0.3, 0.4]}}",
    );
    let vectors = &payload["points"][0]["vector"];
    assert_eq!(vectors["dense"][0], 0.1);
    assert_eq!(vectors["sparse"][0], 0.3);
}

// ── Scroll tests ───────────────────────────────────────────────────

#[test]
fn test_compile_scroll() {
    let payload = compile_ok("SCROLL FROM docs LIMIT 50");
    assert_eq!(payload["collection_name"], "docs");
    assert_eq!(payload["limit"], 50);
}

// ── Select tests ───────────────────────────────────────────────────

#[test]
fn test_compile_select() {
    let payload = compile_ok("SELECT * FROM docs WHERE id = 42");
    assert_eq!(payload["collection_name"], "docs");
    assert_eq!(payload["point_id"], 42);
}

#[test]
fn test_compile_select_uuid() {
    let payload =
        compile_ok("SELECT * FROM docs WHERE id = '550e8400-e29b-41d4-a716-446655440000'");
    assert_eq!(payload["point_id"], "550e8400-e29b-41d4-a716-446655440000");
}

// ── Collection management tests ────────────────────────────────────

#[test]
fn test_compile_create_collection() {
    let payload = compile_ok("CREATE COLLECTION docs");
    assert_eq!(payload["collection_name"], "docs");
}

#[test]
fn test_compile_create_collection_hybrid() {
    let payload = compile_ok("CREATE COLLECTION docs HYBRID");
    assert!(!payload["sparse_vectors_config"].is_null());
}

#[test]
fn test_compile_create_collection_hybrid_rerank() {
    let payload = compile_ok("CREATE COLLECTION docs HYBRID RERANK");
    assert!(!payload["sparse_vectors_config"].is_null());
}

#[test]
fn test_compile_create_collection_with_quantization() {
    let payload =
        compile_ok("CREATE COLLECTION docs WITH QUANTIZATION (type = 'scalar', quantile = 0.95)");
    assert_eq!(payload["collection_name"], "docs");
    let qc = &payload["quantization_config"];
    assert_eq!(qc["qtype"], "Scalar");
    assert_eq!(qc["quantile"], 0.95);
}

#[test]
fn test_compile_create_collection_with_hnsw() {
    let payload = compile_ok("CREATE COLLECTION docs WITH HNSW (m = 32, ef_construct = 100)");
    assert_eq!(payload["hnsw_config"]["m"], 32);
    assert_eq!(payload["hnsw_config"]["ef_construct"], 100);
}

#[test]
fn test_compile_alter_collection() {
    let payload = compile_ok("ALTER COLLECTION docs WITH OPTIMIZERS (max_segment_size = 500000)");
    assert_eq!(payload["collection_name"], "docs");
    assert_eq!(payload["optimizers_config"]["max_segment_size"], 500000);
}

#[test]
fn test_compile_drop_collection() {
    let payload = compile_ok("DROP COLLECTION docs");
    assert_eq!(payload["collection_name"], "docs");
}

#[test]
fn test_compile_show_collections() {
    let payload = compile_ok("SHOW COLLECTIONS");
    assert!(payload.is_null());
}

#[test]
fn test_compile_show_collection() {
    let payload = compile_ok("SHOW COLLECTION docs");
    assert_eq!(payload["collection_name"], "docs");
}

// ── Delete tests ───────────────────────────────────────────────────

#[test]
fn test_compile_delete() {
    let payload = compile_ok("DELETE FROM docs WHERE id = 42");
    assert_eq!(payload["collection_name"], "docs");
}

// ── Update tests ───────────────────────────────────────────────────

#[test]
fn test_compile_update_vector() {
    let payload = compile_ok("UPDATE docs SET VECTOR = [0.1, 0.2, 0.3] WHERE id = 1");
    assert_eq!(payload["collection_name"], "docs");
    assert_eq!(payload["point_id"], 1);
}

#[test]
fn test_compile_update_payload() {
    let payload = compile_ok("UPDATE docs SET PAYLOAD = {status: 'archived'} WHERE id = 1");
    assert_eq!(payload["collection_name"], "docs");
    assert_eq!(payload["payload"]["status"], "archived");
}

// ── Index tests ────────────────────────────────────────────────────

#[test]
fn test_compile_create_index() {
    let payload = compile_ok("CREATE INDEX ON COLLECTION docs FOR category TYPE keyword");
    assert_eq!(payload["collection_name"], "docs");
    assert_eq!(payload["field"], "category");
    assert_eq!(payload["field_type"], "keyword");
}

// ── Error cases ────────────────────────────────────────────────────

#[test]
fn test_compile_invalid_syntax() {
    let result = offline::compile("QUERY FROM");
    assert!(result.is_err());
}

#[test]
fn test_compile_query_no_input() {
    let result = offline::compile("QUERY FROM docs LIMIT 10");
    assert!(result.is_err());
}
