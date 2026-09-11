//! OpenAPI contract coverage gate.
//!
//! Loads the runtime's `openapi.json` at test time and asserts that:
//!
//! 1. every "Statement → Endpoint Matrix" route (AGENTS.md) exists in the
//!    spec, and
//! 2. a representative request body for each covered operation decodes to
//!    canonical, re-parseable QQL.
//!
//! No network access; the spec is read from the workspace.

use serde_json::Value;

use qql_convert::{ConvertError, convert};
use qql_core::fmt::format_stmt;
use qql_core::parser::Parser;

fn openapi() -> Value {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../qql-runtime/openapi.json");
    let raw = std::fs::read_to_string(path).expect("read crates/qql-runtime/openapi.json");
    serde_json::from_str(&raw).expect("openapi.json is valid JSON")
}

/// (method, OpenAPI path template, matrix row description)
const MATRIX: &[(&str, &str, &str)] = &[
    (
        "POST",
        "/collections/{collection_name}/points/query",
        "QUERY",
    ),
    (
        "POST",
        "/collections/{collection_name}/points/query/groups",
        "QUERY GROUP BY",
    ),
    (
        "POST",
        "/collections/{collection_name}/points",
        "QUERY POINTS",
    ),
    ("POST", "/collections/{collection_name}/facet", "FACET"),
    (
        "POST",
        "/collections/{collection_name}/points/scroll",
        "SCROLL",
    ),
    (
        "POST",
        "/collections/{collection_name}/points/count",
        "COUNT",
    ),
    ("PUT", "/collections/{collection_name}/points", "UPSERT"),
    (
        "POST",
        "/collections/{collection_name}/points/delete",
        "DELETE",
    ),
    (
        "POST",
        "/collections/{collection_name}/points/payload/clear",
        "CLEAR PAYLOAD",
    ),
    (
        "POST",
        "/collections/{collection_name}/points/payload/delete",
        "DELETE PAYLOAD",
    ),
    (
        "POST",
        "/collections/{collection_name}/points/vectors/delete",
        "DELETE VECTOR",
    ),
    (
        "PUT",
        "/collections/{collection_name}/points/vectors",
        "UPDATE VECTOR",
    ),
    (
        "POST",
        "/collections/{collection_name}/points/payload",
        "UPDATE PAYLOAD",
    ),
    ("PUT", "/collections/{collection_name}", "CREATE COLLECTION"),
    (
        "PATCH",
        "/collections/{collection_name}",
        "ALTER COLLECTION",
    ),
    (
        "DELETE",
        "/collections/{collection_name}",
        "DROP COLLECTION",
    ),
    (
        "PUT",
        "/collections/{collection_name}/index",
        "CREATE INDEX",
    ),
    (
        "DELETE",
        "/collections/{collection_name}/index/{field_name}",
        "DROP INDEX",
    ),
    (
        "PUT",
        "/collections/{collection_name}/shards",
        "CREATE SHARD KEY",
    ),
    (
        "POST",
        "/collections/{collection_name}/shards/delete",
        "DROP SHARD KEY",
    ),
    (
        "GET",
        "/collections/{collection_name}/shards",
        "SHOW SHARD KEYS",
    ),
    ("GET", "/collections", "SHOW COLLECTIONS"),
    ("GET", "/collections/{collection_name}", "SHOW COLLECTION"),
    ("GET", "/quotas", "SHOW QUOTAS"),
    ("PUT", "/quotas", "SET QUOTA"),
];

#[test]
fn every_matrix_route_exists_in_openapi() {
    let spec = openapi();
    for (method, path, row) in MATRIX {
        let op = spec
            .get("paths")
            .and_then(|paths| paths.get(path))
            .and_then(|item| item.get(method.to_ascii_lowercase()));
        assert!(
            op.is_some(),
            "matrix row {row} ({method} {path}) missing from openapi.json"
        );
    }
    // The alias helper is the 26th matrix route and deliberately has no QQL
    // statement; it must still exist in the spec.
    assert!(
        spec["paths"]["/collections/aliases"]["post"].is_object(),
        "change_aliases helper route missing from openapi.json"
    );
}

#[test]
fn representative_bodies_decode_for_every_matrix_row() {
    use serde_json::json;
    let cases: Vec<(&str, &str, Value)> = vec![
        (
            "POST",
            "/collections/docs/points/query",
            json!({"query": {"nearest": [0.1, 0.2]}, "using": "dense", "limit": 5,
                   "filter": {"must": [{"key": "a", "match": {"value": 1}}]},
                   "with_payload": {"include": ["title"]}, "with_vector": ["dense"]}),
        ),
        (
            "POST",
            "/collections/docs/points/query/groups",
            json!({"query": {"nearest": [0.1]}, "group_by": "city", "group_size": 3,
                   "limit": 10, "with_lookup": "cities"}),
        ),
        (
            "POST",
            "/collections/docs/points",
            json!({"ids": [1, "pt-2"], "with_payload": true, "with_vector": false}),
        ),
        (
            "POST",
            "/collections/docs/facet",
            json!({"key": "city", "limit": 5, "exact": true,
                   "filter": {"must": [{"key": "a", "match": {"value": 1}}]}}),
        ),
        (
            "POST",
            "/collections/docs/points/scroll",
            json!({"limit": 10, "offset": 7,
                   "filter": {"must": [{"key": "a", "range": {"gte": 1}}]},
                   "with_vector": ["dense"]}),
        ),
        (
            "POST",
            "/collections/docs/points/count",
            json!({"filter": {"must": [{"key": "a", "match": {"value": 1}}]}, "exact": false}),
        ),
        (
            "PUT",
            "/collections/docs/points",
            json!({"points": [{"id": 1, "vector": [0.1], "payload": {"title": "x"}}]}),
        ),
        (
            "POST",
            "/collections/docs/points/delete",
            json!({"points": [1, 2]}),
        ),
        (
            "POST",
            "/collections/docs/points/payload/clear",
            json!({"filter": {"must": [{"key": "a", "match": {"value": 1}}]}}),
        ),
        (
            "POST",
            "/collections/docs/points/payload/delete",
            json!({"keys": ["a"], "points": [1]}),
        ),
        (
            "POST",
            "/collections/docs/points/vectors/delete",
            json!({"vector": ["dense"], "filter": {"must": [{"key": "a", "match": {"value": 1}}]}}),
        ),
        (
            "PUT",
            "/collections/docs/points/vectors",
            json!({"points": [{"id": 1, "vector": {"dense": [0.1]}}]}),
        ),
        (
            "POST",
            "/collections/docs/points/payload",
            json!({"payload": {"a": 1}, "points": [1]}),
        ),
        (
            "PUT",
            "/collections/docs",
            json!({"vectors": {"dense": {"size": 4, "distance": "Cosine"}},
                   "sparse_vectors": {"sparse": {"modifier": "idf"}},
                   "hnsw_config": {"m": 16},
                   "optimizers_config": {"indexing_threshold": 1000},
                   "quantization_config": {"scalar": {"type": "int8"}},
                   "shard_number": 2, "sharding_method": "custom",
                   "replication_factor": 1, "write_consistency_factor": 1,
                   "on_disk_payload": false,
                   "payload": {"memory": "cached"}}),
        ),
        (
            "PATCH",
            "/collections/docs",
            json!({"vectors": {"dense": {"hnsw_config": {"m": 8}}},
                   "optimizers_config": {"indexing_threshold": 500},
                   "params": {"replication_factor": 2, "read_fan_out_factor": 1},
                   "hnsw_config": {"m": 16},
                   "quantization_config": "Disabled",
                   "sparse_vectors": {"sparse": {"modifier": "none"}}}),
        ),
        ("DELETE", "/collections/docs", json!({})),
        (
            "PUT",
            "/collections/docs/index",
            json!({"field_name": "body",
                   "field_schema": {"type": "text", "tokenizer": "word", "lowercase": true,
                                    "stemmer": {"type": "snowball", "language": "english"},
                                    "stopwords": {"custom": ["the"]}}}),
        ),
        ("DELETE", "/collections/docs/index/city", json!({})),
        (
            "PUT",
            "/collections/docs/shards",
            json!({"shard_key": "acme", "shards_number": 3, "replication_factor": 2}),
        ),
        (
            "POST",
            "/collections/docs/shards/delete",
            json!({"shard_key": 101}),
        ),
        ("GET", "/collections/docs/shards", json!({})),
        ("GET", "/collections", json!({})),
        ("GET", "/collections/docs", json!({})),
        ("GET", "/quotas", json!({})),
        (
            "PUT",
            "/quotas",
            json!({"enabled": true, "max_disk_usage_percent": 90, "release_margin_percent": 5}),
        ),
    ];

    for (method, path, body) in cases {
        let wrapped = serde_json::json!({"method": method, "path": path, "body": body}).to_string();
        let statements = convert(&wrapped, Some("caller"))
            .unwrap_or_else(|e| panic!("{method} {path} did not decode: {e}"));
        assert!(
            !statements.is_empty(),
            "{method} {path} produced no statements"
        );
        for statement in &statements {
            let parsed = Parser::parse(&format!("{statement};"))
                .unwrap_or_else(|e| panic!("{method} {path} emitted unparseable QQL: {e}"));
            assert_eq!(
                &format_stmt(&parsed),
                statement,
                "{method} {path} emitted non-canonical QQL"
            );
        }
    }
}

#[test]
fn alias_helper_is_not_a_qql_statement() {
    let wrapped =
        serde_json::json!({"method": "POST", "path": "/collections/aliases", "body": {"actions": []}})
            .to_string();
    assert!(matches!(
        convert(&wrapped, Some("docs")).unwrap_err(),
        ConvertError::UnsupportedEndpoint(_)
    ));
}
