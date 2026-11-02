//! Qdrant 1.20 server-side BM25 converts to QQL 1.9 syntax.
//!
//! - `{"text": …}` query (no `model`) → `QUERY BM25 '…' USING <field>`, with
//!   `scoring.bm25.{k, b}` → `PARAMS (k, b)`. Never the embed `QUERY TEXT`.
//! - text-index `scoring` → `WITH (scoring = 'bm25')`.
//! - `tokenizer: "code"` → `WITH (tokenizer = 'code')`.
//!
//! Client sparse BM25 (`Document` with `model`) keeps converting to
//! `QUERY TEXT … MODEL …`. Full plan-back parity lives in `route_parity.rs`.

mod common;

use common::{expect_one, wrapped};
use qql_convert::{ConvertError, convert};
use serde_json::json;

fn query(body: serde_json::Value) -> String {
    wrapped("POST", "/collections/docs/points/query", body)
}

fn index(body: serde_json::Value) -> String {
    wrapped("PUT", "/collections/docs/index", body)
}

#[test]
fn text_query_plain_string() {
    expect_one(
        &query(json!({"query": {"text": "time travel"}, "using": "title", "limit": 10})),
        "QUERY BM25 'time travel' FROM docs USING title LIMIT 10",
    );
}

#[test]
fn text_query_with_bm25_params() {
    expect_one(
        &query(json!({
            "query": {"text": {"query": "time travel", "scoring": {"bm25": {"k": 1.2, "b": 0.75}}}},
            "using": "title",
            "limit": 10
        })),
        "QUERY BM25 'time travel' FROM docs USING title PARAMS (k = 1.2, b = 0.75) LIMIT 10",
    );
    // Partial / empty scoring: absent params stay absent (server defaults).
    expect_one(
        &query(json!({
            "query": {"text": {"query": "q", "scoring": {"bm25": {"b": 0.3}}}},
            "using": "title"
        })),
        "QUERY BM25 'q' FROM docs USING title PARAMS (b = 0.3)",
    );
    expect_one(
        &query(json!({
            "query": {"text": {"query": "q", "scoring": null}},
            "using": "title"
        })),
        "QUERY BM25 'q' FROM docs USING title",
    );
}

#[test]
fn text_query_keeps_filter_and_quotes() {
    expect_one(
        &query(json!({
            "query": {"text": "it's"},
            "using": "title",
            "filter": {"must": [{"key": "lang", "match": {"value": "en"}}]},
            "limit": 5
        })),
        "QUERY BM25 'it''s' FROM docs USING title WHERE lang = 'en' LIMIT 5",
    );
}

#[test]
fn text_query_inside_prefetch_hybrid() {
    expect_one(
        &query(json!({
            "prefetch": [
                {"query": {"text": "time travel", "model": "e5"}, "using": "dense", "limit": 50},
                {"query": {"text": "time travel"}, "using": "title", "limit": 50}
            ],
            "query": {"fusion": "rrf"},
            "limit": 10
        })),
        "QUERY FUSION RRF FROM docs PREFETCH (\
           QUERY TEXT 'time travel' MODEL 'e5' USING dense LIMIT 50, \
           QUERY BM25 'time travel' USING title LIMIT 50\
         ) LIMIT 10",
    );
}

#[test]
fn text_query_rescoring_prefetch() {
    expect_one(
        &query(json!({
            "prefetch": {"query": [0.1, 0.2], "using": "dense", "limit": 100},
            "query": {"text": "q"},
            "using": "title",
            "limit": 10
        })),
        "QUERY BM25 'q' FROM docs USING title PREFETCH (QUERY [0.1, 0.2] USING dense LIMIT 100) LIMIT 10",
    );
}

#[test]
fn document_query_with_model_stays_client_side() {
    // Client sparse BM25 / inference Documents never become QUERY BM25.
    expect_one(
        &query(
            json!({"query": {"text": "time travel", "model": "bm25"}, "using": "sparse", "limit": 5}),
        ),
        "QUERY TEXT 'time travel' MODEL 'bm25' FROM docs USING sparse LIMIT 5",
    );
}

#[test]
fn text_query_rejections() {
    let invalid_at = |input: String, want: &str| match convert(&input, Some("docs")) {
        Err(ConvertError::InvalidField { path, detail }) => {
            assert!(path.ends_with(want), "path {path} (detail: {detail})");
        }
        other => panic!("expected InvalidField at {want}, got {other:?}"),
    };
    // USING is mandatory: it names the BM25-scored payload field.
    invalid_at(query(json!({"query": {"text": "q"}})), "using");
    invalid_at(
        query(json!({"query": {"text": ""}, "using": "title"})),
        "text",
    );
    invalid_at(
        query(
            json!({"query": {"text": {"query": "q", "scoring": {"bm25": {"k": -1}}}}, "using": "t"}),
        ),
        "bm25.k",
    );
    invalid_at(
        query(
            json!({"query": {"text": {"query": "q", "scoring": {"bm25": {"b": 1.5}}}}, "using": "t"}),
        ),
        "bm25.b",
    );
    invalid_at(
        query(json!({"query": {"text": {"query": "q", "scoring": {"tfidf": {}}}}, "using": "t"})),
        "tfidf",
    );
    // A model-less text object is a Query, not a VectorInput.
    invalid_at(
        query(json!({"query": {"nearest": {"text": "q"}}, "using": "t"})),
        "nearest",
    );
}

#[test]
fn text_index_bm25_scoring() {
    let want = "CREATE INDEX ON COLLECTION docs FOR title TYPE text WITH (scoring = 'bm25')";
    expect_one(
        &index(
            json!({"field_name": "title", "field_schema": {"type": "text", "scoring": {"type": "bm25"}}}),
        ),
        want,
    );
    expect_one(
        &index(json!({"field_name": "title", "field_schema": {"type": "text", "scoring": true}})),
        want,
    );
    // `false` is the disabled default: nothing to emit.
    expect_one(
        &index(json!({"field_name": "title", "field_schema": {"type": "text", "scoring": false}})),
        "CREATE INDEX ON COLLECTION docs FOR title TYPE text",
    );
    match convert(
        &index(
            json!({"field_name": "t", "field_schema": {"type": "text", "scoring": {"type": "tfidf"}}}),
        ),
        Some("docs"),
    ) {
        Err(ConvertError::InvalidField { path, .. }) => {
            assert_eq!(path, "body.field_schema.scoring.type");
        }
        other => panic!("expected InvalidField, got {other:?}"),
    }
}

#[test]
fn code_tokenizer_with_scoring() {
    expect_one(
        &index(json!({
            "field_name": "title",
            "field_schema": {"type": "text", "scoring": {"type": "bm25"}, "tokenizer": "code", "lowercase": true}
        })),
        "CREATE INDEX ON COLLECTION docs FOR title TYPE text \
         WITH (lowercase = true, scoring = 'bm25', tokenizer = 'code')",
    );
    expect_one(
        &index(json!({"field_name": "src", "field_schema": {"type": "text", "tokenizer": "code"}})),
        "CREATE INDEX ON COLLECTION docs FOR src TYPE text WITH (tokenizer = 'code')",
    );
}
