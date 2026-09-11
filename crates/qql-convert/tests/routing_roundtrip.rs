//! Wrapped-request routing, bare-body detection, and typed errors.

mod common;

use common::{convert, convert_err, wrapped};
use qql_convert::{ConvertError, convert as convert_json};

// ── Wrapped requests / bodyless routes ──────────────────────────

#[test]
fn wrapped_requests_cover_every_matrix_row() {
    let cases: Vec<(&str, &str, &str, &str)> = vec![
        (
            "POST",
            "/collections/docs/points/query",
            r#"{"query": {"nearest": [0.1]}, "limit": 2}"#,
            "QUERY [0.1] FROM docs LIMIT 2",
        ),
        (
            "POST",
            "/collections/docs/points/query/groups",
            r#"{"query": {"nearest": [0.1]}, "group_by": "city", "limit": 2}"#,
            "QUERY [0.1] FROM docs GROUP BY city LIMIT 2",
        ),
        (
            "POST",
            "/collections/docs/points",
            r#"{"ids": [1]}"#,
            "QUERY POINTS (1) FROM docs",
        ),
        (
            "POST",
            "/collections/docs/facet",
            r#"{"key": "city"}"#,
            "FACET city FROM docs",
        ),
        (
            "POST",
            "/collections/docs/points/scroll",
            r#"{"limit": 2}"#,
            "SCROLL FROM docs LIMIT 2",
        ),
        (
            "POST",
            "/collections/docs/points/count",
            r#"{"exact": true}"#,
            "COUNT FROM docs WITH (exact = true)",
        ),
        (
            "PUT",
            "/collections/docs/points",
            r#"{"points": [{"id": 1, "vector": [0.1]}]}"#,
            "UPSERT INTO docs VALUES {id: 1, vector: [0.1]}",
        ),
        (
            "POST",
            "/collections/docs/points/delete",
            r#"{"points": [1]}"#,
            "DELETE FROM docs WHERE id IN (1)",
        ),
        (
            "POST",
            "/collections/docs/points/payload/clear",
            r#"{"points": [1]}"#,
            "CLEAR PAYLOAD FROM docs WHERE id IN (1)",
        ),
        (
            "POST",
            "/collections/docs/points/payload/delete",
            r#"{"keys": ["a"], "points": [1]}"#,
            "DELETE PAYLOAD a FROM docs WHERE id IN (1)",
        ),
        (
            "POST",
            "/collections/docs/points/vectors/delete",
            r#"{"vector": ["dense"], "points": [1]}"#,
            "DELETE VECTOR dense FROM docs WHERE id IN (1)",
        ),
        (
            "PUT",
            "/collections/docs/points/vectors",
            r#"{"points": [{"id": 1, "vector": [0.1]}]}"#,
            "UPDATE docs SET VECTOR = [0.1] WHERE id = 1",
        ),
        (
            "POST",
            "/collections/docs/points/payload",
            r#"{"payload": {"a": 1}, "points": [1]}"#,
            "UPDATE docs SET PAYLOAD = {a: 1} WHERE id IN (1)",
        ),
        (
            "PUT",
            "/collections/docs",
            r#"{"vectors": {}}"#,
            "CREATE COLLECTION docs",
        ),
        (
            "PATCH",
            "/collections/docs",
            r#"{"optimizers_config": {"indexing_threshold": 1}}"#,
            "ALTER COLLECTION docs WITH OPTIMIZERS (indexing_threshold = 1)",
        ),
        (
            "DELETE",
            "/collections/docs",
            r#"{}"#,
            "DROP COLLECTION docs",
        ),
        (
            "PUT",
            "/collections/docs/index",
            r#"{"field_name": "city", "field_schema": "keyword"}"#,
            "CREATE INDEX ON COLLECTION docs FOR city TYPE keyword",
        ),
        (
            "DELETE",
            "/collections/docs/index/city",
            r#"{}"#,
            "DROP INDEX ON COLLECTION docs FOR city",
        ),
        (
            "PUT",
            "/collections/docs/shards",
            r#"{"shard_key": "acme"}"#,
            "CREATE SHARD KEY 'acme' ON COLLECTION docs",
        ),
        (
            "POST",
            "/collections/docs/shards/delete",
            r#"{"shard_key": "acme"}"#,
            "DROP SHARD KEY 'acme' ON COLLECTION docs",
        ),
        (
            "GET",
            "/collections/docs/shards",
            r#"{}"#,
            "SHOW SHARD KEYS ON COLLECTION docs",
        ),
        ("GET", "/collections", r#"{}"#, "SHOW COLLECTIONS"),
        ("GET", "/collections/docs", r#"{}"#, "SHOW COLLECTION docs"),
        ("GET", "/quotas", r#"{}"#, "SHOW QUOTAS"),
        (
            "PUT",
            "/quotas",
            r#"{"enabled": true}"#,
            "SET QUOTA (enabled = true)",
        ),
    ];
    for (method, path, body, expected) in cases {
        let input = wrapped(method, path, serde_json::from_str(body).expect("case body"));
        let stmts = convert(&input);
        assert_eq!(stmts, [expected], "{method} {path}");
    }
}

#[test]
fn wrapped_collection_overrides_caller() {
    let input = wrapped(
        "POST",
        "/collections/docs/points",
        serde_json::json!({"ids": [1]}),
    );
    let stmts = convert_json(&input, Some("other")).expect("wrapped conversion");
    assert_eq!(stmts, ["QUERY POINTS (1) FROM docs"]);
}

#[test]
fn unsupported_endpoints_fail_closed() {
    for (method, path) in [
        ("POST", "/collections/docs/aliases"),
        ("GET", "/collections/docs/snapshots"),
        ("PUT", "/collections/docs/points/payload"),
        ("POST", "/collections/docs/points/search"),
        ("POST", "/collections/docs/points/query/batch"),
        ("PUT", "/collections/docs/vectors/dense"),
    ] {
        let input = wrapped(method, path, serde_json::json!({}));
        assert!(
            matches!(
                convert_json(&input, None).unwrap_err(),
                ConvertError::UnsupportedEndpoint(_)
            ),
            "{method} {path} must be unsupported"
        );
    }
}

#[test]
fn typed_errors_cover_invalid_inputs() {
    assert!(matches!(
        convert_json("not json", None).unwrap_err(),
        ConvertError::InvalidJson(_)
    ));
    assert!(matches!(
        convert_json("[1, 2]", None).unwrap_err(),
        ConvertError::UndecodableBody { .. }
    ));
    assert!(matches!(
        convert_json(r#"{"limit": 1}"#, Some("docs")).unwrap_err(),
        ConvertError::UndecodableBody { .. }
    ));
    assert!(matches!(
        convert_json(r#"{"unrelated": true}"#, Some("docs")).unwrap_err(),
        ConvertError::UndecodableBody { .. }
    ));

    // min_should is a documented typed error, not a silent drop.
    let err = convert_err(
        r#"{"query": {"nearest": [0.1]}, "filter": {"min_should": {"conditions": [{"key": "a", "match": {"value": 1}}], "min_count": 1}}}"#,
    );
    assert!(
        matches!(err, ConvertError::InvalidField { ref path, .. } if path == "body.filter.min_should"),
        "{err:?}"
    );

    // match.except has no QQL representation.
    let err = convert_err(
        r#"{"query": {"nearest": [0.1]}, "filter": {"must": [{"key": "a", "match": {"except": ["x"]}}]}}"#,
    );
    assert!(matches!(err, ConvertError::InvalidField { .. }), "{err:?}");
}

/// Shapes the QQL AST cannot express fail closed with a typed error naming
/// the offending path — never a silent drop or a placeholder statement.
#[test]
fn unrepresentable_fields_are_typed_errors() {
    let cases: Vec<(&str, &str, &str)> = vec![
        // (wrapped method, wrapped path, body)
        (
            "POST",
            "/collections/docs/points/scroll",
            r#"{"order_by": {"key": "created_at"}}"#,
        ),
        (
            "POST",
            "/collections/docs/points/payload",
            r#"{"payload": {"a": 1}, "points": [1], "key": "nested.path"}"#,
        ),
        (
            "PUT",
            "/collections/docs/points/vectors",
            r#"{"points": [{"id": 1, "vector": [0.1]}], "update_filter": {"must": [{"key": "a", "match": {"value": 1}}]}}"#,
        ),
        (
            "PUT",
            "/collections/docs/points",
            r#"{"points": [{"id": 1, "vector": [0.1]}], "update_mode": "insert_only"}"#,
        ),
        (
            "PUT",
            "/collections/docs",
            r#"{"vectors": {"dense": {"size": 4, "distance": "Cosine"}}, "wal_config": {"wal_capacity_mb": 8}}"#,
        ),
        (
            "PUT",
            "/collections/docs",
            r#"{"vectors": {"dense": {"size": 4, "distance": "Cosine"}}, "metadata": {"a": 1}}"#,
        ),
        (
            "PATCH",
            "/collections/docs",
            r#"{"strict_mode_config": {"enabled": true}}"#,
        ),
        (
            "PUT",
            "/collections/docs/shards",
            r#"{"shard_key": "acme", "placement": [1]}"#,
        ),
        (
            "POST",
            "/collections/docs/points/query",
            r#"{"query": {"nearest": {"text": "x", "model": "m", "options": {"a": 1}}}}"#,
        ),
        (
            "POST",
            "/collections/docs/points/query",
            r#"{"query": {"nearest": {"object": {"a": 1}, "model": "m"}}}"#,
        ),
    ];
    for (method, path, body) in cases {
        let wrapped = wrapped(method, path, serde_json::from_str(body).expect("case body"));
        match convert_json(&wrapped, None).unwrap_err() {
            ConvertError::InvalidField { path: field, .. } => {
                assert!(
                    !field.is_empty(),
                    "{method} {path} error must carry a field path"
                );
            }
            other => panic!("{method} {path} must fail with InvalidField, got {other:?}"),
        }
    }
}

// ── Bare bodies ─────────────────────────────────────────────────

#[test]
fn bare_body_collection_passthrough() {
    let stmts = convert_json(r#"{"ids": [1]}"#, Some("docs")).expect("conversion");
    assert_eq!(stmts, ["QUERY POINTS (1) FROM docs"]);

    assert!(matches!(
        convert_json(r#"{"ids": [1]}"#, None).unwrap_err(),
        ConvertError::MissingCollection
    ));
    assert!(matches!(
        convert_json(r#"{"ids": [1]}"#, Some("")).unwrap_err(),
        ConvertError::MissingCollection
    ));
}

#[test]
fn bare_body_detection_table() {
    // Upsert (points of objects) vs delete (points of ids).
    assert!(convert(r#"{"points": [{"id": 1, "vector": [0.1]}]}"#)[0].starts_with("UPSERT"));
    assert!(convert(r#"{"points": [1, 2]}"#)[0].starts_with("DELETE"));
    // SetPayload / DeletePayload / DeleteVectors discriminations.
    assert!(
        convert(r#"{"payload": {"a": 1}, "points": [1]}"#)[0]
            .starts_with("UPDATE docs SET PAYLOAD")
    );
    assert!(convert(r#"{"keys": ["a"], "points": [1]}"#)[0].starts_with("DELETE PAYLOAD"));
    assert!(convert(r#"{"vector": ["dense"], "points": [1]}"#)[0].starts_with("DELETE VECTOR"));
    // Legacy search body -> nearest query.
    assert_eq!(
        convert(r#"{"vector": [0.1], "limit": 5}"#),
        ["QUERY [0.1] FROM docs LIMIT 5"]
    );
    // Query with query/prefetch.
    assert_eq!(
        convert(r#"{"query": {"nearest": [0.1]}, "limit": 5}"#),
        ["QUERY [0.1] FROM docs LIMIT 5"]
    );
    // Order-by / fusion / sample.
    assert_eq!(
        convert(r#"{"order_by": "created_at", "limit": 3}"#),
        ["QUERY ORDER BY created_at ASC FROM docs LIMIT 3"]
    );
    assert_eq!(
        convert(
            r#"{"fusion": "rrf", "prefetch": [{"query": {"nearest": [0.1]}, "limit": 10}], "limit": 3}"#
        ),
        ["QUERY FUSION RRF FROM docs PREFETCH (QUERY [0.1] LIMIT 10) LIMIT 3"]
    );
    // DDL detections.
    assert_eq!(convert(r#"{"vectors": {}}"#), ["CREATE COLLECTION docs"]);
    assert_eq!(
        convert(r#"{"field_name": "city", "field_schema": "keyword"}"#),
        ["CREATE INDEX ON COLLECTION docs FOR city TYPE keyword"]
    );
    assert!(
        convert(r#"{"shard_key": "acme", "shards_number": 2}"#)[0].starts_with("CREATE SHARD KEY")
    );
    assert!(convert(r#"{"enabled": true}"#)[0].starts_with("SET QUOTA"));
    // filter-only bodies are ambiguous; +limit is SCROLL; +exact is COUNT.
    assert!(matches!(
        convert_err(r#"{"filter": {"must": [{"key": "a", "match": {"value": 1}}]}}"#),
        ConvertError::UndecodableBody { .. }
    ));
    assert!(
        convert(r#"{"filter": {"must": [{"key": "a", "match": {"value": 1}}]}, "limit": 3}"#)[0]
            .starts_with("SCROLL")
    );
    assert!(
        convert(r#"{"filter": {"must": [{"key": "a", "match": {"value": 1}}]}, "exact": true}"#)[0]
            .starts_with("COUNT")
    );
}

#[test]
fn bare_ambiguities_fail_closed() {
    for input in [
        r#"{"points": []}"#,
        r#"{"shard_key": "acme"}"#,
        r#"{"limit": 5}"#,
    ] {
        assert!(
            matches!(
                convert_json(input, Some("docs")).unwrap_err(),
                ConvertError::UndecodableBody { .. }
            ),
            "{input}"
        );
    }
}

#[test]
fn wait_and_read_opts_round_trip_from_query() {
    let upsert = wrapped_query(
        "PUT",
        "/collections/docs/points",
        serde_json::json!({"wait": "true"}),
        serde_json::json!({"points": [{"id": 1, "vector": [0.1]}]}),
    );
    assert_eq!(
        convert_json(&upsert, None).expect("wait"),
        ["UPSERT INTO docs VALUES {id: 1, vector: [0.1]} WAIT true"]
    );

    let query = wrapped_query(
        "POST",
        "/collections/docs/points/query",
        serde_json::json!({"timeout": "30", "consistency": "majority"}),
        serde_json::json!({"query": {"nearest": [0.1]}, "limit": 5}),
    );
    assert_eq!(
        convert_json(&query, None).expect("timeout"),
        ["QUERY [0.1] FROM docs PARAMS (timeout = 30, consistency = majority) LIMIT 5"]
    );

    let path_qs = serde_json::json!({
        "method": "POST",
        "path": "/collections/docs/points/delete?wait=false",
        "body": {"points": [1]},
    })
    .to_string();
    assert_eq!(
        convert_json(&path_qs, None).expect("path query"),
        ["DELETE FROM docs WHERE id IN (1) WAIT false"]
    );
}

fn wrapped_query(
    method: &str,
    path: &str,
    query: serde_json::Value,
    body: serde_json::Value,
) -> String {
    serde_json::json!({"method": method, "path": path, "query": query, "body": body}).to_string()
}

#[test]
fn unknown_request_fields_fail_closed() {
    let err = convert_err(r#"{"query": {"nearest": [0.1]}, "not_a_field": 1}"#);
    assert!(
        matches!(err, ConvertError::InvalidField { ref path, .. } if path == "body.not_a_field"),
        "{err:?}"
    );
    let err =
        convert_err(r#"{"query": {"nearest": [0.1]}, "params": {"hnsw_ef": 32, "foo": true}}"#);
    assert!(
        matches!(err, ConvertError::InvalidField { ref path, .. } if path == "body.params.foo"),
        "{err:?}"
    );
}
