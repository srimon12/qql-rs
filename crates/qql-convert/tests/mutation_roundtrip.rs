//! Mutation and DDL decoding round-trip invariants.

mod common;

use common::{convert, expect_one, wrapped};
use qql_convert::json_to_qql;

// ── Mutations ───────────────────────────────────────────────────

#[test]
fn upsert_shapes() {
    assert_eq!(
        convert(r#"{"points": [{"id": 1, "vector": [0.1, 0.2], "payload": {"title": "hello"}}]}"#),
        ["UPSERT INTO docs VALUES {id: 1, vector: [0.1, 0.2], title: 'hello'}"]
    );
    assert_eq!(
        convert(
            r#"{"batch": {"ids": [1, 2], "vectors": [[0.1], [0.2]], "payloads": [{"a": 1}, {"a": 2}]}}"#
        ),
        [
            "UPSERT INTO docs VALUES\n  {id: 1, vector: [0.1], a: 1},\n  {id: 2, vector: [0.2], a: 2}"
        ]
    );
    assert_eq!(
        convert(
            r#"{"points": [{"id": 1, "vector": {"dense": [0.1], "sparse": {"indices": [1], "values": [0.5]}}}]}"#
        ),
        [
            "UPSERT INTO docs VALUES {id: 1, vector: {dense: [0.1], sparse: {indices: [1], values: [0.5]}}}"
        ]
    );
    assert_eq!(
        convert(r#"{"points": [{"id": 1, "vector": [[0.1, 0.2], [0.3, 0.4]]}]}"#),
        ["UPSERT INTO docs VALUES {id: 1, vector: [[0.1, 0.2], [0.3, 0.4]]}"]
    );
    assert_eq!(
        convert(r#"{"points": [{"id": 1, "payload": {"a": 1}}]}"#),
        ["UPSERT INTO docs VALUES {id: 1, a: 1}"]
    );
}

#[test]
fn delete_and_payload_mutations() {
    assert_eq!(
        convert(r#"{"points": [1, 2]}"#),
        ["DELETE FROM docs WHERE id IN (1, 2)"]
    );
    assert_eq!(
        convert(r#"{"filter": {"must": [{"key": "age", "range": {"gt": 18}}]}}"#),
        ["DELETE FROM docs WHERE age > 18"]
    );
    assert_eq!(
        convert(r#"{"points": [1]}"#),
        ["DELETE FROM docs WHERE id IN (1)"]
    );
    assert_eq!(
        convert(r#"{"filter": {"must": [{"key": "k", "match": {"value": 1}}]}}"#),
        ["DELETE FROM docs WHERE k = 1"]
    );
    assert_eq!(
        convert(r#"{"payload": {"a": 1}, "points": [1, 2]}"#),
        ["UPDATE docs SET PAYLOAD = {a: 1} WHERE id IN (1, 2)"]
    );
    assert_eq!(
        convert(
            r#"{"payload": {"a": "x"}, "filter": {"must": [{"key": "k", "match": {"value": 1}}]}}"#
        ),
        ["UPDATE docs SET PAYLOAD = {a: 'x'} WHERE k = 1"]
    );
    assert_eq!(
        convert(r#"{"keys": ["a", "b"], "points": [1]}"#),
        ["DELETE PAYLOAD a, b FROM docs WHERE id IN (1)"]
    );
    assert_eq!(
        convert(
            r#"{"vector": ["dense"], "filter": {"must": [{"key": "k", "match": {"value": 1}}]}}"#
        ),
        ["DELETE VECTOR dense FROM docs WHERE k = 1"]
    );
    let update_vector = |body: serde_json::Value| {
        let input = wrapped("PUT", "/collections/docs/points/vectors", body);
        json_to_qql(&input).expect("wrapped update vectors")
    };
    assert_eq!(
        update_vector(serde_json::json!({"points": [{"id": 1, "vector": [0.5, 0.6]}]})),
        ["UPDATE docs SET VECTOR = [0.5, 0.6] WHERE id = 1"]
    );
    assert_eq!(
        update_vector(serde_json::json!({"points": [{"id": 1, "vector": {"dense": [0.5]}}]})),
        ["UPDATE docs SET VECTOR dense = [0.5] WHERE id = 1"]
    );
}

// ── DDL ─────────────────────────────────────────────────────────

#[test]
fn create_collection_shapes() {
    assert_eq!(convert(r#"{"vectors": {}}"#), ["CREATE COLLECTION docs"]);
    expect_one(
        r#"{"vectors": {"size": 4, "distance": "Cosine"}}"#,
        "CREATE COLLECTION docs (dense VECTOR(4, COSINE))",
    );
    expect_one(
        r#"{"vectors": {"dense": {"size": 128, "distance": "Dot"}}, "sparse_vectors": {"sparse": {"modifier": "idf"}}}"#,
        "CREATE COLLECTION docs (dense VECTOR(128, DOT), sparse SPARSE WITH SPARSE (modifier = 'idf'))",
    );
    expect_one(
        r#"{"vectors": {"dense": {"size": 8, "distance": "Euclid", "hnsw_config": {"m": 16, "ef_construct": 100}, "quantization_config": {"scalar": {"type": "int8", "quantile": 0.99}}, "on_disk": true, "datatype": "float16", "multivector_config": {"comparator": "max_sim"}}}}"#,
        "CREATE COLLECTION docs (dense VECTOR(8, EUCLID) WITH HNSW (m = 16, ef_construct = 100) \
         WITH QUANTIZATION (type = 'scalar', quantile = 0.99) \
         WITH MULTIVECTOR (comparator = 'max_sim') \
         WITH VECTOR (on_disk = true, datatype = 'float16'))",
    );
    let create_params = wrapped(
        "PUT",
        "/collections/docs",
        serde_json::json!({"shard_number": 4, "sharding_method": "custom",
            "replication_factor": 2, "write_consistency_factor": 1,
            "on_disk_payload": true, "hnsw_config": {"m": 32},
            "optimizers_config": {"indexing_threshold": 1000, "max_optimization_threads": "auto"}}),
    );
    expect_one(
        &create_params,
        "CREATE COLLECTION docs WITH HNSW (m = 32) \
         WITH OPTIMIZERS (indexing_threshold = 1000, max_optimization_threads = 'auto') \
         WITH PARAMS (replication_factor = 2, write_consistency_factor = 1, \
         on_disk_payload = true, shard_number = 4, sharding_method = 'custom')",
    );
}

#[test]
fn alter_collection_shapes() {
    let alter = |body: serde_json::Value| {
        let input = wrapped("PATCH", "/collections/docs", body);
        json_to_qql(&input).expect("wrapped alter")
    };
    assert_eq!(
        alter(serde_json::json!({"optimizers_config": {"indexing_threshold": 500}})),
        ["ALTER COLLECTION docs WITH OPTIMIZERS (indexing_threshold = 500)"]
    );
    assert_eq!(
        alter(serde_json::json!({"quantization_config": "Disabled"})),
        ["ALTER COLLECTION docs WITH QUANTIZATION (disabled = true)"]
    );
    assert_eq!(
        alter(
            serde_json::json!({"vectors": {"dense": {"hnsw_config": {"m": 8}, "on_disk": true}}})
        ),
        ["ALTER COLLECTION docs WITH VECTOR dense (HNSW (m = 8), VECTOR (on_disk = true))"]
    );
    assert_eq!(
        alter(serde_json::json!({"sparse_vectors": {"sparse": {"modifier": "none"}}})),
        ["ALTER COLLECTION docs WITH SPARSE sparse (SPARSE (modifier = 'none'))"]
    );
    assert_eq!(alter(serde_json::json!({})), ["ALTER COLLECTION docs"]);
}

#[test]
fn index_and_shard_key_shapes() {
    assert_eq!(
        convert(r#"{"field_name": "city", "field_schema": "keyword"}"#),
        ["CREATE INDEX ON COLLECTION docs FOR city TYPE keyword"]
    );
    assert_eq!(
        convert(r#"{"field_name": "loc", "field_schema": {"type": "geo"}}"#),
        ["CREATE INDEX ON COLLECTION docs FOR loc TYPE geo"]
    );
    assert_eq!(
        convert(
            r#"{"field_name": "body", "field_schema": {"type": "text", "tokenizer": "word", "lowercase": true, "min_token_len": 2, "stopwords": {"custom": ["the"]}, "stemmer": {"type": "snowball", "language": "english"}}}"#
        ),
        [
            "CREATE INDEX ON COLLECTION docs FOR body TYPE text WITH (lowercase = true, min_token_len = 2, stemmer = 'english', stopwords = ['the'], tokenizer = 'word')"
        ]
    );
    assert_eq!(
        convert(r#"{"shard_key": "acme", "shards_number": 3, "replication_factor": 2}"#),
        [
            "CREATE SHARD KEY 'acme' ON COLLECTION docs WITH (shards_number = 3, replication_factor = 2)"
        ]
    );
    let drop_shard = wrapped(
        "POST",
        "/collections/docs/shards/delete",
        serde_json::json!({"shard_key": 101}),
    );
    assert_eq!(
        json_to_qql(&drop_shard).expect("wrapped drop shard"),
        ["DROP SHARD KEY 101 ON COLLECTION docs"]
    );
    assert_eq!(
        convert(
            r#"{"enabled": true, "max_resident_memory_percent": 80, "release_margin_percent": 5}"#
        ),
        [
            "SET QUOTA (enabled = true, max_resident_memory_percent = 80, release_margin_percent = 5)"
        ]
    );
}
