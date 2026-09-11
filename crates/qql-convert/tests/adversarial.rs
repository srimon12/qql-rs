//! Adversarial conversion matrix: upstream edge shapes must either decode to
//! canonical, plan-reparseable QQL or fail closed with a typed error.
//!
//! Regression coverage for the empty-list family found during review: Qdrant
//! treats empty `must` / `must_not` / `exclude` / `with_vector` lists as
//! no-ops but an empty `should` as match-nothing, and QQL rejects empty
//! vector literals, empty selectors, and `PARAMS ()`. Emitting those shapes
//! verbatim produced statements that did not re-parse.

mod common;

use common::{canon, convert, convert_err, expect_one, wrapped};
use qql_convert::{ConvertError, json_to_qql};
use qql_core::parser::Parser;
use qql_plan::{plan, to_rest_route};

/// Re-plan an emitted statement and project it back to its REST body.
fn replanned_body(statement: &str) -> serde_json::Value {
    let parsed = Parser::parse(&format!("{statement};")).unwrap_or_else(|e| panic!("reparse: {e}"));
    let op = plan(&parsed).unwrap_or_else(|e| panic!("plan {statement}: {e}"));
    to_rest_route(&op)
        .unwrap_or_else(|e| panic!("route {statement}: {e:?}"))
        .body_json()
        .expect("statement has a request body")
}

#[test]
fn empty_bool_lists_follow_upstream_semantics() {
    // qdrant-edge `check_must` / `check_must_not` are `all` over the list, so
    // an empty list is vacuously true = no constraint, like an empty filter.
    assert_eq!(
        convert(r#"{"query": {"nearest": [0.1]}, "filter": {"must": []}}"#),
        ["QUERY [0.1] FROM docs"]
    );
    assert_eq!(
        convert(r#"{"query": {"nearest": [0.1]}, "filter": {}}"#),
        ["QUERY [0.1] FROM docs"]
    );
    assert_eq!(
        convert(r#"{"query": {"nearest": [0.1]}, "filter": {"must_not": []}}"#),
        ["QUERY [0.1] FROM docs"]
    );
    // `check_should` is `any` over the list: empty means match nothing, which
    // QQL cannot express, so it must fail closed rather than widen the filter.
    let err = convert_err(r#"{"query": {"nearest": [0.1]}, "filter": {"should": []}}"#);
    assert!(
        matches!(err, ConvertError::InvalidField { ref path, .. } if path == "body.filter.should"),
        "{err:?}"
    );
}

#[test]
fn empty_selectors_and_params_do_not_emit_empty_clauses() {
    // `PARAMS ()`, `WITH PAYLOAD INCLUDE ()`, `WITH VECTOR ()`, and
    // `MATCH ANY ()` are all rejected by the runtime parser.
    expect_one(
        r#"{"query": {"nearest": [0.1]}, "params": {}}"#,
        "QUERY [0.1] FROM docs",
    );
    expect_one(
        r#"{"query": {"nearest": [0.1]}, "with_payload": {"exclude": []}}"#,
        "QUERY [0.1] FROM docs WITH PAYLOAD true",
    );
    expect_one(
        r#"{"query": {"nearest": [0.1]}, "with_vector": []}"#,
        "QUERY [0.1] FROM docs WITH VECTOR false",
    );
    // Nested boolean filters keep grouping and negation.
    assert_eq!(
        convert(
            r#"{"query": {"nearest": [0.1]}, "filter": {"must": [{"should": [{"key": "a", "match": {"value": 1}}, {"key": "b", "match": {"value": 2}}]}, {"must_not": [{"key": "c", "match": {"value": 3}}]}]}}"#
        ),
        ["QUERY [0.1] FROM docs WHERE (a = 1 OR b = 2) AND NOT c = 3"]
    );

    for (input, path) in [
        (
            r#"{"query": {"nearest": [0.1]}, "with_payload": []}"#,
            "body.with_payload",
        ),
        (
            r#"{"query": {"nearest": [0.1]}, "with_payload": {"include": []}}"#,
            "body.with_payload.include",
        ),
        (
            r#"{"query": {"nearest": [0.1]}, "filter": {"must": [{"key": "a", "match": {"any": []}}]}}"#,
            "body.filter.must[0].match.any",
        ),
    ] {
        let err = convert_err(input);
        assert!(
            matches!(err, ConvertError::InvalidField { path: ref got, .. } if got == path),
            "{input}: {err:?}"
        );
    }
}

#[test]
fn empty_and_mismatched_vector_literals_fail_closed() {
    // Regression: these used to emit `vector: []`, `{indices: [], …}`, … which
    // the runtime parser rejects, breaking the crate's re-parse contract.
    for (input, path) in [
        (r#"{"query": {"nearest": []}}"#, "body.query.nearest"),
        (
            r#"{"points": [{"id": 1, "vector": []}]}"#,
            "body.points[0].vector",
        ),
        (
            r#"{"points": [{"id": 1, "vector": {"dense": []}}]}"#,
            "body.points[0].vector.dense",
        ),
        (
            r#"{"points": [{"id": 1, "vector": {"sparse": []}}]}"#,
            "body.points[0].vector.sparse",
        ),
        (
            r#"{"points": [{"id": 1, "vector": {"indices": [], "values": []}}]}"#,
            "body.points[0].vector",
        ),
        (
            r#"{"points": [{"id": 1, "vector": {"indices": [1], "values": [0.5, 0.6]}}]}"#,
            "body.points[0].vector",
        ),
        (
            r#"{"points": [{"id": 1, "vector": [[], [0.1]]}]}"#,
            "body.points[0].vector[0]",
        ),
    ] {
        let err = convert_err(input);
        assert!(
            matches!(err, ConvertError::InvalidField { path: ref got, .. } if got == path),
            "{input}: {err:?}"
        );
    }
}

#[test]
fn geo_filters_are_real_ast_and_replan() {
    for (condition, geo_key) in [
        (
            r#"{"key": "loc", "geo_bounding_box": {"top_left": {"lat": 52.5, "lon": 13.4}, "bottom_right": {"lat": 52.4, "lon": 13.5}}}"#,
            "geo_bounding_box",
        ),
        (
            r#"{"key": "loc", "geo_radius": {"center": {"lat": 52.5, "lon": 13.4}, "radius": 1000.0}}"#,
            "geo_radius",
        ),
        (
            r#"{"key": "loc", "geo_polygon": {"exterior": {"points": [{"lat": 0.0, "lon": 0.0}, {"lat": 1.0, "lon": 0.0}, {"lat": 1.0, "lon": 1.0}]}, "interiors": [{"points": [{"lat": 0.1, "lon": 0.1}, {"lat": 0.2, "lon": 0.1}, {"lat": 0.2, "lon": 0.2}]}]}}"#,
            "geo_polygon",
        ),
    ] {
        let input =
            format!(r#"{{"query": {{"nearest": [0.1]}}, "filter": {{"must": [{condition}]}}}}"#);
        let statements = convert(&input);
        assert_eq!(statements.len(), 1, "{input}");
        // The predicate survives as a real filter in the re-planned route.
        let body = replanned_body(&statements[0]);
        assert!(
            body["filter"]["must"][0][geo_key].is_object(),
            "{input} -> {} -> {body}",
            statements[0]
        );
    }
}

#[test]
fn scroll_offset_inverts_the_exclusive_after_cursor() {
    // `SCROLL … AFTER n` plans to wire `offset = n + 1`, so the converter
    // must subtract one (UUIDs included) to stay lossless.
    let scroll = |offset: serde_json::Value| {
        wrapped(
            "POST",
            "/collections/docs/points/scroll",
            serde_json::json!({"limit": 10, "offset": offset}),
        )
    };
    let zero = json_to_qql(&scroll(serde_json::json!(0))).expect("offset 0");
    assert_eq!(zero, ["SCROLL FROM docs LIMIT 10"]);
    assert!(replanned_body(&zero[0]).get("offset").is_none());

    let seven = json_to_qql(&scroll(serde_json::json!(7))).expect("offset 7");
    assert_eq!(seven, ["SCROLL FROM docs AFTER 6 LIMIT 10"]);
    assert_eq!(replanned_body(&seven[0])["offset"], 7);

    let uuid = "550e8400-e29b-41d4-a716-446655440000";
    let uuid_scroll = json_to_qql(&scroll(serde_json::json!(uuid))).expect("offset uuid");
    let uuid_after = "550e8400-e29b-41d4-a716-44665543ffff";
    assert_eq!(
        uuid_scroll,
        [format!("SCROLL FROM docs AFTER '{uuid_after}' LIMIT 10")]
    );
    assert_eq!(replanned_body(&uuid_scroll[0])["offset"], uuid);
}

#[test]
fn shard_key_types_survive_the_round_trip() {
    for (input, expected) in [
        (
            serde_json::json!(101),
            "DELETE FROM docs WHERE id IN (1) SHARD 101",
        ),
        (
            serde_json::json!("acme"),
            "DELETE FROM docs WHERE id IN (1) SHARD 'acme'",
        ),
    ] {
        let wrapped = wrapped(
            "POST",
            "/collections/docs/points/delete",
            serde_json::json!({"points": [1], "shard_key": input}),
        );
        let statements = json_to_qql(&wrapped).expect("shard key");
        assert_eq!(statements, [expected]);
        assert_eq!(replanned_body(&statements[0])["shard_key"], input);
    }
}

#[test]
fn batch_map_vectors_decode_like_per_point_vectors() {
    let input = r#"{"batch": {"ids": [1, 2], "vectors": {"dense": [[0.1], [0.2]], "sparse": [{"indices": [1], "values": [0.5]}, {"indices": [2], "values": [0.6]}]}}}"#;
    assert_eq!(
        convert(input),
        [canon(
            "UPSERT INTO docs VALUES\n  {id: 1, vector: {dense: [0.1], sparse: {indices: [1], values: [0.5]}}},\n  {id: 2, vector: {dense: [0.2], sparse: {indices: [2], values: [0.6]}}}"
        )]
    );
}

#[test]
fn uuid_ids_decode_and_replan_as_strings() {
    let uuid = "550e8400-e29b-41d4-a716-446655440000";
    let input =
        format!(r#"{{"points": [{{"id": "{uuid}", "vector": [0.1], "payload": {{"a": 1}}}}]}}"#);
    let statements = convert(&input);
    assert_eq!(
        statements,
        [format!(
            "UPSERT INTO docs VALUES {{id: '{uuid}', vector: [0.1], a: 1}}"
        )]
    );
    let body = replanned_body(&statements[0]);
    assert_eq!(body["points"][0]["id"], uuid);
}

#[test]
fn wrong_shapes_are_typed_errors_never_junk() {
    // Bare scalar `query` is a legal VectorInput (point id), not an error.
    expect_one(r#"{"query": "nope"}"#, "QUERY POINT 'nope' FROM docs");

    for (input, path) in [
        (r#"{"query": {"nearest": {}}}"#, "body.query.nearest"),
        (
            r#"{"query": {"nearest": {"text": 5, "model": "m"}}}"#,
            "body.query.nearest.text",
        ),
        (
            r#"{"points": [{"id": 1.5, "vector": [0.1]}]}"#,
            "body.points[0].id",
        ),
        (
            r#"{"points": [{"id": "x", "vector": 3}]}"#,
            "body.points[0].vector",
        ),
    ] {
        let err = convert_err(input);
        assert!(
            matches!(err, ConvertError::InvalidField { path: ref got, .. } if got == path),
            "{input}: {err:?}"
        );
    }

    // Wrapped requests with a missing/undecodable body.
    let bodyless = serde_json::json!({"method": "PUT", "path": "/collections/docs"}).to_string();
    assert!(matches!(
        json_to_qql(&bodyless).unwrap_err(),
        ConvertError::UndecodableBody { .. }
    ));
    let null_body =
        serde_json::json!({"method": "POST", "path": "/collections/docs/points/delete", "body": null})
            .to_string();
    assert!(matches!(
        json_to_qql(&null_body).unwrap_err(),
        ConvertError::InvalidField { .. }
    ));
}
