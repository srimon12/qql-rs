//! Round-trip invariant plus behavior-fix coverage for `qql-convert`.
//!
//! [`assert_roundtrip`] conversions must emit statements that all parse with
//! `qql_core::parser::Parser`. [`documents_preexisting_drift`] pins the exact
//! output of emitters that predate the current QQL grammar (stale clause
//! order, stale recommend/discover/fusion/prefetch shapes, JSON-quoted DDL
//! literals). Those pins lock byte-identical behavior; grammar alignment is
//! out of scope (only the three specified behavior fixes may change output).

use qql_convert::{ConvertError, json_to_qql, json_to_qql_with_collection};
use qql_core::parser::Parser;

/// Convert `input` with collection `"docs"`, assert every emitted statement
/// parses as QQL, and return the statements for exact-match checks.
fn assert_roundtrip(input: &str) -> Vec<String> {
    let stmts = json_to_qql_with_collection(input, "docs")
        .unwrap_or_else(|e| panic!("conversion failed for {input}: {e}"));
    assert_parses(&stmts, input);
    stmts
}

fn assert_parses(stmts: &[String], input: &str) {
    assert!(
        !stmts.is_empty(),
        "conversion produced no statements for {input}"
    );
    for stmt in stmts {
        Parser::parse(&format!("{stmt};"))
            .unwrap_or_else(|e| panic!("emitted statement failed to parse: {stmt} ({e})"));
    }
}

#[test]
fn upsert_and_point_lookup_roundtrip() {
    let stmts = assert_roundtrip(
        r#"{"points": [{"id": 1, "vector": [0.1, 0.2], "payload": {"title": "hello"}}]}"#,
    );
    assert_eq!(stmts.len(), 1);
    assert!(stmts[0].starts_with("UPSERT INTO docs VALUES "));

    let stmts = assert_roundtrip(r#"{"ids": [1, "point-2"]}"#);
    assert_eq!(
        stmts,
        [
            "QUERY POINTS (1) FROM docs",
            "QUERY POINTS ('point-2') FROM docs"
        ]
    );
}

#[test]
fn vector_search_and_formula_queries_roundtrip() {
    assert_eq!(
        assert_roundtrip(r#"{"vector": [0.1, 0.2], "limit": 5}"#),
        ["QUERY [0.1, 0.2] FROM docs LIMIT 5"]
    );
    assert_eq!(
        assert_roundtrip(r#"{"query": {"nearest": [0.1, 0.2]}, "limit": 5}"#),
        ["QUERY [0.1, 0.2] FROM docs LIMIT 5"]
    );
    assert_eq!(
        assert_roundtrip(r#"{"query": {"text": "hello"}, "limit": 5}"#),
        ["QUERY 'hello' FROM docs LIMIT 5"]
    );
    assert_eq!(
        assert_roundtrip(r#"{"query": {"nearest": {"text": "q"}}, "limit": 2}"#),
        ["QUERY 'q' FROM docs LIMIT 2"]
    );
    assert_eq!(
        assert_roundtrip(r#"{"searches": [{"vector": [0.1], "limit": 2}]}"#),
        ["QUERY [0.1] FROM docs LIMIT 2"]
    );
}

#[test]
fn scroll_deletes_and_payload_writes_roundtrip() {
    assert_eq!(
        assert_roundtrip(
            r#"{"filter": {"must": [{"key": "city", "match": {"value": "berlin"}}]}, "limit": 10, "offset": 7}"#,
        ),
        ["SCROLL FROM docs WHERE city = 'berlin' AFTER 7 LIMIT 10"]
    );
    // Multi-clause filters: `should` groups with OR, `must_not` negates.
    assert_eq!(
        assert_roundtrip(
            r#"{"filter": {"must_not": [{"key": "city", "match": {"keyword": "x"}}], "should": [{"key": "a", "match": {"value": 1}}, {"key": "b", "match": {"value": 2}}]}, "limit": 1}"#,
        ),
        ["SCROLL FROM docs WHERE ((a = 1 OR b = 2) AND NOT (city = 'x')) LIMIT 1"]
    );
    // A single `should` condition needs no OR group.
    assert_eq!(
        assert_roundtrip(
            r#"{"filter": {"should": [{"key": "a", "match": {"value": 1}}]}, "limit": 1}"#
        ),
        ["SCROLL FROM docs WHERE a = 1 LIMIT 1"]
    );
    assert_eq!(
        assert_roundtrip(r#"{"points": [1, 2]}"#),
        [
            "DELETE FROM docs WHERE id = 1",
            "DELETE FROM docs WHERE id = 2"
        ]
    );
    assert_eq!(
        assert_roundtrip(r#"{"filter": {"must": [{"key": "age", "range": {"gt": 18}}]}}"#),
        ["DELETE FROM docs WHERE age > 18"]
    );
    assert_eq!(
        assert_roundtrip(r#"{"payload": {"a": 1}, "points": [1, 2]}"#),
        [
            "UPDATE docs SET PAYLOAD = {'a': 1} WHERE id = 1",
            "UPDATE docs SET PAYLOAD = {'a': 1} WHERE id = 2"
        ]
    );
    assert_eq!(
        assert_roundtrip(
            r#"{"payload": {"a": "x"}, "filter": {"must": [{"key": "k", "match": {"value": 1}}]}}"#,
        ),
        ["UPDATE docs SET PAYLOAD = {'a': 'x'} WHERE k = 1"]
    );
}

#[test]
fn ddl_keyword_shapes_roundtrip() {
    assert_eq!(
        assert_roundtrip(r#"{"vectors": {}}"#),
        ["CREATE COLLECTION docs"]
    );
    assert_eq!(
        assert_roundtrip(r#"{"field_name": "city", "field_schema": "keyword"}"#),
        ["CREATE INDEX ON COLLECTION docs FOR \"city\" TYPE keyword"]
    );
}

#[test]
fn wrapped_requests_roundtrip() {
    for (method, path, body, expected) in [
        (
            "PUT",
            "/collections/docs/points",
            r#"{"points": [{"id": 1, "vector": [0.1]}]}"#,
            vec!["UPSERT INTO docs VALUES {'id': 1, 'vector': [0.1]}"],
        ),
        (
            "POST",
            "/collections/docs/points/search",
            r#"{"vector": [0.1], "limit": 1}"#,
            vec!["QUERY [0.1] FROM docs LIMIT 1"],
        ),
        (
            "POST",
            "/collections/docs/points/scroll",
            r#"{"limit": 2}"#,
            vec!["SCROLL FROM docs LIMIT 2"],
        ),
        (
            "POST",
            "/collections/docs/points",
            r#"{"ids": [1]}"#,
            vec!["QUERY POINTS (1) FROM docs"],
        ),
        (
            "POST",
            "/collections/docs/points/delete",
            r#"{"points": [1]}"#,
            vec!["DELETE FROM docs WHERE id = 1"],
        ),
        (
            "POST",
            "/collections/docs/points/payload",
            r#"{"payload": {"a": 1}, "points": [1]}"#,
            vec!["UPDATE docs SET PAYLOAD = {'a': 1} WHERE id = 1"],
        ),
        (
            "DELETE",
            "/collections/docs",
            r#"{}"#,
            vec!["DROP COLLECTION docs"],
        ),
        (
            "PUT",
            "/collections/docs/index",
            r#"{"field_name": "city", "field_schema": "keyword"}"#,
            vec!["CREATE INDEX ON COLLECTION docs FOR \"city\" TYPE keyword"],
        ),
        (
            "POST",
            "/collections/docs/points/query",
            r#"{"query": {"nearest": [0.1]}, "limit": 2}"#,
            vec!["QUERY [0.1] FROM docs LIMIT 2"],
        ),
    ] {
        let input = format!(r#"{{"method": "{method}", "path": "{path}", "body": {body}}}"#);
        let stmts = json_to_qql(&input)
            .unwrap_or_else(|e| panic!("wrapped conversion failed for {method} {path}: {e}"));
        assert_eq!(stmts, expected);
        assert_parses(&stmts, &input);
    }

    // A bodiless formula-query request converts to zero statements.
    let stmts = json_to_qql(r#"{"method": "POST", "path": "/collections/docs/points/query"}"#)
        .expect("bodiless query");
    assert!(stmts.is_empty());

    // Wrapped requests derive the collection from the path, ignoring the
    // caller-supplied collection.
    let stmts = json_to_qql_with_collection(
        r#"{"method": "POST", "path": "/collections/docs/points", "body": {"ids": [1]}}"#,
        "other",
    )
    .expect("wrapped conversion");
    assert_eq!(stmts, ["QUERY POINTS (1) FROM docs"]);
}

/// Exact-output pins for emitters that predate the current QQL grammar.
///
/// These do NOT parse today (stale `QUERY RECOMMEND WITH`, `TARGET <id>`,
/// leading `FUSION`, `PREFETCH`-before-`QUERY`, `LIMIT`-before-`WHERE`,
/// `MATCH ANY 'text'`, JSON-quoted DDL literals, dropped sample/sparse
/// shapes). The pins lock byte-identical output until the grammar is
/// aligned; they must not be "fixed" by editing expectations.
#[test]
fn documents_preexisting_drift() {
    let convert = |input: &str| {
        json_to_qql_with_collection(input, "docs")
            .unwrap_or_else(|e| panic!("conversion failed for {input}: {e}"))
    };
    // Stale recommend shape: the grammar expects `QUERY RECOMMEND POSITIVE ...`.
    assert_eq!(
        convert(r#"{"positive": [1, 2], "negative": [3], "limit": 5}"#),
        ["QUERY RECOMMEND WITH (positive = (1, 2), negative = (3)) FROM docs LIMIT 5"]
    );
    assert_eq!(
        convert(
            r#"{"positive": [1], "strategy": "best_score", "using": "sparse", "lookup_from": {"collection": "docs"}}"#
        ),
        [
            "QUERY RECOMMEND WITH (positive = (1)) FROM docs STRATEGY 'best_score' USING SPARSE LOOKUP FROM docs"
        ]
    );
    // Stale discover shape: the grammar needs a typed target (`POINT (1)`).
    assert_eq!(
        convert(r#"{"target": 1, "context": [{"positive": 1, "negative": 2}], "limit": 5}"#),
        ["QUERY DISCOVER TARGET 1 CONTEXT PAIRS (1, 2) FROM docs LIMIT 5"]
    );
    // Fusion-only and prefetch emitters lead with non-QUERY keywords.
    assert_eq!(
        convert(r#"{"query": {"fusion": "rrf"}, "limit": 5}"#),
        ["FUSION RRF FROM docs LIMIT 5"]
    );
    assert_eq!(
        convert(
            r#"{"prefetch": [{"query": "warm", "limit": 5}], "query": {"nearest": [0.1]}, "limit": 3}"#
        ),
        ["WITH _pf0 AS (QUERY 'warm' LIMIT 5) PREFETCH (_pf0) QUERY [0.1] FROM docs LIMIT 3"]
    );
    assert_eq!(
        convert(
            r#"{"prefetch": [{"vector": [0.1, 0.2], "limit": 4}], "query": {"nearest": [0.1]}, "limit": 3}"#
        ),
        ["WITH _pf0 AS (QUERY [0.1, 0.2] LIMIT 4) PREFETCH (_pf0) QUERY [0.1] FROM docs LIMIT 3"]
    );
    // Search emits LIMIT/OFFSET before WHERE; the grammar wants WHERE first.
    assert_eq!(
        convert(
            r#"{"vector": [0.1], "limit": 5, "offset": 2, "filter": {"must": [{"key": "age", "range": {"gte": 18, "lte": 65}}]}, "score_threshold": 0.5}"#
        ),
        ["QUERY [0.1] FROM docs LIMIT 5 OFFSET 2 WHERE age BETWEEN 18 AND 65 SCORE THRESHOLD 0.5"]
    );
    assert_eq!(
        convert(r#"{"query": {"text": "hello", "model": "m"}, "limit": 3, "using": "dense"}"#),
        ["QUERY 'hello' FROM docs LIMIT 3 USING 'dense'"]
    );
    // Unknown search extras: GROUP_SIZE token, hybrid/text validation.
    assert_eq!(
        convert(
            r#"{"vector": [0.1], "group_by": "city", "group_size": 3, "lookup_from": {"collection": "docs", "vector": "image"}}"#
        ),
        ["QUERY [0.1] FROM docs GROUP BY 'city' GROUP_SIZE 3 LOOKUP FROM docs VECTOR 'image'"]
    );
    assert_eq!(
        convert(r#"{"vector": [0.1], "using": "hybrid"}"#),
        ["QUERY [0.1] FROM docs USING HYBRID"]
    );
    // MATCH ANY needs a parenthesized list in the grammar.
    assert_eq!(
        convert(
            r#"{"filter": {"must": [{"key": "ta", "match": {"text_any": "solo"}}]}, "limit": 1}"#
        ),
        ["SCROLL FROM docs WHERE ta MATCH ANY 'solo' LIMIT 1"]
    );
    // DDL literals carry JSON quoting (`"Cosine"`, `TYPE "geo"`).
    assert_eq!(
        convert(r#"{"vectors": {"size": 4, "distance": "Cosine"}}"#),
        ["CREATE COLLECTION docs (\n    'dense' VECTOR(4, \"Cosine\")\n)"]
    );
    assert_eq!(
        convert(
            r#"{"vectors": {"image": {"size": 512, "distance": "Cosine", "hnsw_config": {"m": 16}}}}"#
        ),
        ["CREATE COLLECTION docs (\n    'image' VECTOR(512, \"Cosine\") WITH HNSW (m = 16)\n)"]
    );
    assert_eq!(
        convert(r#"{"field_name": "loc", "field_schema": {"type": "geo"}}"#),
        ["CREATE INDEX ON COLLECTION docs FOR \"loc\" TYPE \"geo\""]
    );
    // Bare `{"query": {...}}` bodies route to the formula converter, which
    // drops sample/sparse/recommend/discover/context shapes.
    assert_eq!(
        convert(r#"{"query": {"sample": {}}, "limit": 3}"#),
        ["FROM docs LIMIT 3"]
    );
    assert_eq!(
        convert(r#"{"query": {"indices": [1], "values": [0.5]}}"#),
        ["FROM docs"]
    );
    assert_eq!(
        convert(r#"{"query": {"recommend": {"positive": [1]}}}"#),
        ["FROM docs"]
    );
    assert_eq!(
        convert(r#"{"query": {"discover": {"target": 7}}}"#),
        ["FROM docs"]
    );
}

#[test]
fn bare_body_detection_gaps_stay_undetectable() {
    // Bodies with no recognizable operation shape fail closed.
    for input in [
        r#"{"unrelated": true}"#,
        r#"{"points": []}"#,
        r#"{"limit": 1}"#,
        r#"{"limit": 5}"#,
        r#"{"query": "hello", "limit": 3}"#,
        r#"{"context": [{"positive": 1, "negative": 2}]}"#,
    ] {
        assert_eq!(
            json_to_qql(input).unwrap_err(),
            ConvertError::UndetectableOperation,
            "input: {input}"
        );
    }
    assert_eq!(
        json_to_qql(r#"[1, 2]"#).unwrap_err(),
        ConvertError::InvalidPayload("expected a JSON object")
    );
}

#[test]
fn bare_body_collection_passthrough() {
    // `--collection docs` replaces the old hardcoded `FROM unknown`.
    let stmts = json_to_qql_with_collection(r#"{"ids": [1]}"#, "docs").expect("conversion");
    assert_eq!(stmts, ["QUERY POINTS (1) FROM docs"]);

    // Default and empty collections still fall back to `unknown`.
    let stmts = json_to_qql(r#"{"ids": [1]}"#).expect("conversion");
    assert_eq!(stmts, ["QUERY POINTS (1) FROM unknown"]);
    let stmts = json_to_qql_with_collection(r#"{"ids": [1]}"#, "").expect("conversion");
    assert_eq!(stmts, ["QUERY POINTS (1) FROM unknown"]);

    // Batch searches honor the caller collection too.
    let stmts =
        json_to_qql_with_collection(r#"{"searches": [{"vector": [0.1], "limit": 2}]}"#, "docs")
            .expect("conversion");
    assert_eq!(stmts, ["QUERY [0.1] FROM docs LIMIT 2"]);
    assert_parses(&stmts, "batch");

    // Unsafe characters are stripped from the collection name.
    assert_eq!(
        json_to_qql_with_collection(r#"{"ids": [1]}"#, "docs!!!").expect("conversion"),
        ["QUERY POINTS (1) FROM docs"]
    );
}

#[test]
fn geo_predicates_error_with_predicate_name() {
    for pred in ["geo_bounding_box", "geo_radius", "geo_polygon"] {
        let input = format!(r#"{{"key": "loc", "{pred}": {{}}}}"#);
        let filter = format!(r#"{{"filter": {{"must": [{input}] }}, "limit": 1}}"#);
        let err = json_to_qql(&filter).unwrap_err();
        assert_eq!(err, ConvertError::GeoUnsupported(pred));
        assert!(err.to_string().contains(pred), "Display names {pred}");

        // Geo inside a multi-`should` clause must propagate, not vanish.
        let should = format!(
            r#"{{"filter": {{"should": [{{"key": "a", "match": {{"value": 1}}}}, {input}] }}, "limit": 1}}"#
        );
        let err = json_to_qql(&should).unwrap_err();
        assert_eq!(err, ConvertError::GeoUnsupported(pred));
    }
}

#[test]
fn typed_errors_cover_invalid_inputs() {
    assert!(matches!(
        json_to_qql("not json").unwrap_err(),
        ConvertError::InvalidJson(_)
    ));
    assert_eq!(
        json_to_qql(r#"{"method": "GET", "path": "/collections", "body": {}}"#).unwrap_err(),
        ConvertError::UnsupportedEndpoint("GET collections".to_string())
    );
    assert_eq!(
        ConvertError::UndetectableOperation.to_string(),
        "cannot detect operation from JSON structure"
    );
}
