//! Query / retrieval round-trip invariants.
//!
//! Every emitted statement must parse with `qql_core::parser::Parser` and be
//! canonical: `format_stmt(parse(emit)) == emit`.

mod common;

use common::{convert, convert_err, expect_one, wrapped};
use qql_convert::{ConvertError, convert as convert_json};

// ── Queries ─────────────────────────────────────────────────────

#[test]
fn query_nearest_variants() {
    assert_eq!(
        convert(r#"{"query": {"nearest": [0.1, 0.2]}, "limit": 5}"#),
        ["QUERY [0.1, 0.2] FROM docs LIMIT 5"]
    );
    assert_eq!(
        convert(r#"{"query": {"nearest": 42}, "limit": 3}"#),
        ["QUERY POINT 42 FROM docs LIMIT 3"]
    );
    assert_eq!(
        convert(r#"{"query": {"nearest": "pt-1"}}"#),
        ["QUERY POINT 'pt-1' FROM docs"]
    );
    assert_eq!(
        convert(r#"{"query": {"nearest": {"indices": [1, 5], "values": [0.5, 0.25]}}}"#),
        ["QUERY {indices: [1, 5], values: [0.5, 0.25]} FROM docs"]
    );
    assert_eq!(
        convert(r#"{"query": {"nearest": [[0.1, 0.2], [0.3, 0.4]]}}"#),
        ["QUERY [[0.1, 0.2], [0.3, 0.4]] FROM docs"]
    );
    assert_eq!(
        convert(r#"{"query": {"nearest": {"text": "hello", "model": "e5"}}, "limit": 2}"#),
        ["QUERY TEXT 'hello' MODEL 'e5' FROM docs LIMIT 2"]
    );
    assert_eq!(
        convert(r#"{"query": {"nearest": {"text": "hello", "model": ""}}, "limit": 2}"#),
        ["QUERY 'hello' FROM docs LIMIT 2"]
    );
    assert_eq!(
        convert(
            r#"{"query": {"nearest": {"image": "https://x/y.jpg", "model": "clip"}}, "limit": 2}"#
        ),
        ["QUERY IMAGE 'https://x/y.jpg' MODEL 'clip' FROM docs LIMIT 2"]
    );
    assert_eq!(
        convert(r#"{"query": {"nearest": [0.1]}, "using": "dense", "limit": 5}"#),
        ["QUERY [0.1] FROM docs USING dense LIMIT 5"]
    );
}

#[test]
fn query_controls() {
    expect_one(
        r#"{"query": {"nearest": [0.1]}, "limit": 5, "offset": 2, "score_threshold": 0.5,
            "filter": {"must": [{"key": "age", "range": {"gte": 18, "lte": 65}}]},
            "with_payload": {"include": ["title", "url"]}, "with_vector": ["dense"]}"#,
        "QUERY [0.1] FROM docs WHERE age BETWEEN 18 AND 65 SCORE THRESHOLD 0.5 \
         WITH PAYLOAD INCLUDE (title, url) WITH VECTOR (dense) LIMIT 5 OFFSET 2",
    );
    expect_one(
        r#"{"query": {"nearest": [0.1]}, "with_payload": false, "with_vector": true}"#,
        "QUERY [0.1] FROM docs WITH PAYLOAD false WITH VECTOR true",
    );
    expect_one(
        r#"{"query": {"nearest": [0.1]}, "with_payload": ["a"]}"#,
        "QUERY [0.1] FROM docs WITH PAYLOAD INCLUDE (a)",
    );
    expect_one(
        r#"{"query": {"nearest": [0.1]}, "with_payload": {"exclude": ["b"]}}"#,
        "QUERY [0.1] FROM docs WITH PAYLOAD EXCLUDE (b)",
    );
}

#[test]
fn query_params_decode() {
    expect_one(
        r#"{"query": {"nearest": [0.1]}, "params": {"hnsw_ef": 64, "exact": false, "indexed_only": true, "quantization": {"ignore": true, "oversampling": 2.0}, "acorn": {"enable": true, "max_selectivity": 0.4}}, "limit": 5}"#,
        "QUERY [0.1] FROM docs PARAMS (hnsw_ef = 64, exact = false, acorn = true, \
         max_selectivity = 0.4, indexed_only = true, quantization = {ignore: true, oversampling: 2.0}) LIMIT 5",
    );
    expect_one(
        r#"{"query": {"nearest": [0.1]}, "params": {"idf": "global"}}"#,
        "QUERY [0.1] FROM docs PARAMS (idf = 'global')",
    );
    expect_one(
        r#"{"query": {"nearest": [0.1]}, "params": {"idf": {"corpus": {"must": [{"key": "tenant", "match": {"value": "acme"}}]}}}}"#,
        "QUERY [0.1] FROM docs PARAMS (idf = WHERE tenant = 'acme')",
    );
}

#[test]
fn query_recommend_context_discover() {
    expect_one(
        r#"{"query": {"recommend": {"positive": [1, 2], "negative": [3], "strategy": "best_score"}}}"#,
        "QUERY RECOMMEND POSITIVE (1, 2) NEGATIVE (3) STRATEGY best_score FROM docs",
    );
    expect_one(
        r#"{"query": {"recommend": {"positive": [[0.1, 0.2]]}}}"#,
        "QUERY RECOMMEND POSITIVE (VECTOR [0.1, 0.2]) FROM docs",
    );
    expect_one(
        r#"{"query": {"context": [{"positive": 1, "negative": 2}, {"positive": 3, "negative": 4}]}}"#,
        "QUERY CONTEXT (POSITIVE POINT 1 NEGATIVE POINT 2, POSITIVE POINT 3 NEGATIVE POINT 4) FROM docs",
    );
    expect_one(
        r#"{"query": {"discover": {"target": 7, "context": [{"positive": 1, "negative": 2}]}}}"#,
        "QUERY DISCOVER TARGET POINT 7 CONTEXT (POSITIVE POINT 1 NEGATIVE POINT 2) FROM docs",
    );
}

#[test]
fn query_order_sample_fusion_formula_rrf_feedback() {
    expect_one(
        r#"{"query": {"order_by": "created_at"}, "limit": 20}"#,
        "QUERY ORDER BY created_at ASC FROM docs LIMIT 20",
    );
    expect_one(
        r#"{"query": {"order_by": {"key": "created_at", "direction": "desc"}}, "limit": 20}"#,
        "QUERY ORDER BY created_at DESC FROM docs LIMIT 20",
    );
    expect_one(
        r#"{"query": {"sample": "random"}, "limit": 5}"#,
        "QUERY SAMPLE RANDOM FROM docs LIMIT 5",
    );
    expect_one(
        r#"{"query": {"fusion": "rrf"}, "prefetch": [{"query": {"nearest": [0.1]}, "using": "dense", "limit": 100}], "limit": 5}"#,
        "QUERY FUSION RRF FROM docs PREFETCH (QUERY [0.1] USING dense LIMIT 100) LIMIT 5",
    );
    expect_one(
        r#"{"query": {"rrf": {"k": 60, "weights": [0.5, 0.5]}}, "params": {"hnsw_ef": 32}, "prefetch": [{"query": {"nearest": [0.1]}, "limit": 100}, {"query": {"nearest": [0.2]}, "limit": 100}], "limit": 5}"#,
        "QUERY FUSION RRF FROM docs PREFETCH (QUERY [0.1] LIMIT 100, QUERY [0.2] LIMIT 100) \
         PARAMS (hnsw_ef = 32, rrf_k = 60, rrf_weights = [0.5, 0.5]) LIMIT 5",
    );
    expect_one(
        r#"{"query": {"formula": {"sum": ["$score", 2.0]}, "defaults": {}}, "limit": 5}"#,
        "QUERY FORMULA score + 2.0 FROM docs LIMIT 5",
    );
    expect_one(
        r#"{"query": {"formula": {"mult": ["$score", 2.0]}, "defaults": {"score": 0.0}}, "limit": 5}"#,
        "QUERY FORMULA score * 2.0 DEFAULTS (score = 0.0) FROM docs LIMIT 5",
    );
    expect_one(
        r#"{"query": {"relevance_feedback": {"target": 42, "feedback": [{"example": 43, "score": 0.5}], "strategy": {"naive": {"a": 1.0, "b": 0.5, "c": 0.5}}}}}"#,
        "QUERY RELEVANCE FEEDBACK TARGET POINT 42 FEEDBACK ((POINT 43, 0.5)) \
         STRATEGY NAIVE (a = 1.0, b = 0.5, c = 0.5) FROM docs",
    );
}

#[test]
fn query_nested_prefetch_and_lookup() {
    // A prefetch LOOKUP FROM is reproduced at both stage and request level.
    expect_one(
        r#"{"query": {"nearest": [0.1]},
            "prefetch": [{"query": {"nearest": {"text": "warm", "model": "e5"}}, "using": "dense", "limit": 50, "lookup_from": {"collection": "other", "vector": "image"}}],
            "lookup_from": {"collection": "other", "vector": "image"},
            "limit": 3}"#,
        "QUERY [0.1] FROM docs \
         PREFETCH (QUERY TEXT 'warm' MODEL 'e5' USING dense LIMIT 50 LOOKUP FROM other VECTOR image) \
         LIMIT 3",
    );
    // A top-level lookup_from without a matching prefetch has no QQL form.
    let err = convert_err(
        r#"{"query": {"nearest": [0.1]}, "lookup_from": {"collection": "other"}, "limit": 3}"#,
    );
    assert!(
        matches!(err, ConvertError::InvalidField { ref path, .. } if path == "body.lookup_from"),
        "{err:?}"
    );
}

#[test]
fn query_groups() {
    assert_eq!(
        convert(
            r#"{"query": {"nearest": [0.1]}, "group_by": "city", "group_size": 3, "limit": 10, "with_lookup": "cities"}"#
        ),
        ["QUERY [0.1] FROM docs GROUP BY city SIZE 3 LOOKUP FROM cities LIMIT 10"]
    );
}

#[test]
fn point_request_scroll_count_facet() {
    assert_eq!(
        convert(r#"{"ids": [1, "pt-2"], "with_payload": false, "with_vector": ["dense"]}"#),
        ["QUERY POINTS (1, 'pt-2') FROM docs WITH PAYLOAD false WITH VECTOR (dense)"]
    );
    assert_eq!(
        convert(
            r#"{"filter": {"must": [{"key": "city", "match": {"value": "berlin"}}]}, "limit": 10, "offset": 7}"#
        ),
        ["SCROLL FROM docs WHERE city = 'berlin' AFTER 6 LIMIT 10"]
    );
    let scroll = wrapped(
        "POST",
        "/collections/docs/points/scroll",
        serde_json::json!({"limit": 2, "with_vector": true}),
    );
    assert_eq!(
        convert_json(&scroll, None).expect("wrapped scroll"),
        ["SCROLL FROM docs WITH VECTOR true LIMIT 2"]
    );
    assert_eq!(
        convert(
            r#"{"filter": {"must_not": [{"key": "city", "match": {"keyword": "x"}}], "should": [{"key": "a", "match": {"value": 1}}, {"key": "b", "match": {"value": 2}}]}, "limit": 1}"#
        ),
        ["SCROLL FROM docs WHERE (a = 1 OR b = 2) AND NOT city = 'x' LIMIT 1"]
    );
    assert_eq!(
        convert(r#"{"filter": {"must": [{"key": "age", "range": {"gt": 18}}]}, "exact": false}"#),
        ["COUNT FROM docs WHERE age > 18 WITH (exact = false)"]
    );
    assert_eq!(
        convert(r#"{"key": "city", "limit": 5, "exact": true}"#),
        ["FACET city FROM docs LIMIT 5 EXACT true"]
    );
}
