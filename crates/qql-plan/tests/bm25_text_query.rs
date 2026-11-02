//! QQL 1.9 server-side BM25 (Qdrant >= 1.20): `QUERY BM25 … USING <field>`,
//! `CREATE INDEX … WITH (scoring = 'bm25', tokenizer = 'code')`.
//! Parse → format round-trip → plan → REST body.

use qql_core::parser::Parser;
use qql_plan::{plan, to_rest_route};
use serde_json::json;

fn route(source: &str) -> (String, serde_json::Value) {
    let stmt = Parser::parse(source).unwrap_or_else(|e| panic!("parse {source}: {e}"));
    let op = plan(&stmt).unwrap_or_else(|e| panic!("plan {source}: {e}"));
    let route = to_rest_route(&op).unwrap_or_else(|e| panic!("route {source}: {e:?}"));
    (route.path.to_string(), route.body_json().expect("body"))
}

fn body(source: &str) -> serde_json::Value {
    route(source).1
}

fn parse_err(source: &str) -> String {
    match Parser::parse(source) {
        Ok(stmt) => panic!("expected parse error for {source}, got {stmt:?}"),
        Err(e) => e.code.to_string(),
    }
}

fn plan_err(source: &str) -> String {
    let stmt = Parser::parse(source).unwrap_or_else(|e| panic!("parse {source}: {e}"));
    match plan(&stmt) {
        Ok(_) => panic!("expected plan error for {source}"),
        Err(e) => e.code.to_string(),
    }
}

fn roundtrip(source: &str) -> String {
    let stmt = Parser::parse(source).unwrap_or_else(|e| panic!("parse {source}: {e}"));
    let formatted = qql_core::fmt::format_stmt(&stmt);
    let reparsed = Parser::parse(&formatted).unwrap_or_else(|e| panic!("reparse {formatted}: {e}"));
    // Spans shift with layout, so compare the canonical text (idempotence)
    // plus the parsed query expression / params.
    assert_eq!(
        formatted,
        qql_core::fmt::format_stmt(&reparsed),
        "fmt not idempotent"
    );
    if let (qql_core::ast::Stmt::Query(a), qql_core::ast::Stmt::Query(b)) = (&stmt, &reparsed) {
        assert_eq!(a.expression, b.expression, "{formatted}");
        assert_eq!(a.params, b.params, "{formatted}");
    }
    formatted
}

#[test]
fn bm25_query_lowers_to_text_query_using_payload_field() {
    let (path, json) = route("QUERY BM25 'time travel' FROM books USING title LIMIT 10;");
    assert_eq!(path, "/collections/books/points/query");
    assert_eq!(json["query"], json!({"text": "time travel"}));
    assert_eq!(json["using"], "title");
    assert_eq!(json["limit"], 10);
    assert!(json.get("params").is_none(), "{json}");
}

#[test]
fn bm25_params_lower_into_text_scoring_not_search_params() {
    let json = body(
        "QUERY BM25 'time travel' FROM books USING title PARAMS (k = 1.2, b = 0.75) LIMIT 10;",
    );
    assert_eq!(
        json["query"],
        json!({"text": {"query": "time travel", "scoring": {"bm25": {"k": 1.2, "b": 0.75}}}})
    );
    assert_eq!(json["using"], "title");
    assert!(
        json.get("params").is_none(),
        "k/b must not leak into params: {json}"
    );

    let only_b = body("QUERY BM25 'x' FROM books USING title PARAMS (b = 0) LIMIT 5;");
    assert_eq!(
        only_b["query"]["text"]["scoring"]["bm25"],
        json!({"b": 0.0})
    );

    // Body search params still lower next to BM25 scoring.
    let mixed =
        body("QUERY BM25 'x' FROM books USING title PARAMS (k = 2, indexed_only = true) LIMIT 5;");
    assert_eq!(mixed["query"]["text"]["scoring"]["bm25"]["k"], 2.0);
    assert_eq!(mixed["params"]["indexed_only"], true);
}

#[test]
fn bm25_query_keeps_filter_and_threshold() {
    let json = body(
        "QUERY BM25 'rust async' FROM books USING title WHERE lang = 'en' SCORE THRESHOLD 0.5 LIMIT 3;",
    );
    assert_eq!(json["query"]["text"], "rust async");
    assert_eq!(json["filter"]["must"][0]["key"], "lang");
    assert_eq!(json["score_threshold"], 0.5);
}

#[test]
fn bm25_inside_prefetch_and_rrf_fusion() {
    let json = body(
        "WITH dense AS (QUERY VECTOR [0.1, 0.2] FROM books USING dense LIMIT 50), \
         lexical AS (QUERY BM25 'time travel' FROM books USING title PARAMS (k = 1.2, b = 0.75) LIMIT 50) \
         QUERY FUSION RRF FROM books PREFETCH (dense, lexical) LIMIT 10;",
    );
    assert_eq!(json["query"], json!({"fusion": "rrf"}));
    let prefetch = json["prefetch"].as_array().expect("prefetch array");
    assert_eq!(prefetch.len(), 2);
    assert_eq!(prefetch[0]["using"], "dense");
    assert_eq!(prefetch[1]["using"], "title");
    assert_eq!(
        prefetch[1]["query"],
        json!({"text": {"query": "time travel", "scoring": {"bm25": {"k": 1.2, "b": 0.75}}}})
    );
    assert_eq!(prefetch[1]["limit"], 50);
    assert!(prefetch[1].get("params").is_none(), "{json}");

    // Inline PREFETCH source.
    let inline = body(
        "QUERY FUSION RRF FROM books PREFETCH (\
         QUERY BM25 'time travel' FROM books USING title LIMIT 20, \
         QUERY VECTOR [0.1, 0.2] FROM books USING dense LIMIT 20) LIMIT 10;",
    );
    assert_eq!(
        inline["prefetch"][0]["query"],
        json!({"text": "time travel"})
    );
    assert_eq!(inline["prefetch"][0]["using"], "title");
}

#[test]
fn bm25_outer_query_over_prefetch_candidates() {
    let json = body(
        "QUERY BM25 'time travel' FROM books USING title \
         PREFETCH (QUERY VECTOR [0.1, 0.2] FROM books USING dense LIMIT 100) LIMIT 10;",
    );
    assert_eq!(json["query"], json!({"text": "time travel"}));
    assert_eq!(json["using"], "title");
    assert_eq!(json["prefetch"][0]["using"], "dense");
}

#[test]
fn bm25_formats_and_round_trips() {
    assert_eq!(
        roundtrip(
            "query bm25 'time travel' from books using title params (k = 1.2, b = 0.75) limit 10;"
        ),
        "QUERY BM25 'time travel'\nFROM books\nUSING title\nPARAMS (k = 1.2, b = 0.75)\nLIMIT 10"
    );
    roundtrip("QUERY BM25 :q FROM books USING title LIMIT 10;");
    roundtrip(
        "WITH lexical AS (QUERY BM25 'a' FROM books USING title LIMIT 50) \
         QUERY FUSION RRF FROM books PREFETCH (lexical) LIMIT 10;",
    );
}

#[test]
fn bm25_text_placeholder_binds() {
    let source =
        qql_core::params::bind_named("QUERY BM25 :q FROM books USING title LIMIT 10;", |name| {
            (name == "q").then(|| qql_core::ast::Value::Str("time travel".into()))
        })
        .expect("bind");
    let bound = Parser::parse(&source).expect("parse bound");
    let op = plan(&bound).expect("plan");
    let json = to_rest_route(&op)
        .expect("route")
        .body_json()
        .expect("body");
    assert_eq!(json["query"], json!({"text": "time travel"}));
}

#[test]
fn query_text_embed_path_is_unchanged() {
    // `QUERY TEXT … USING <vector>` stays the client-side embed / inference path.
    let stmt =
        Parser::parse("QUERY TEXT 'time travel' FROM books USING dense LIMIT 10;").expect("parse");
    match &stmt {
        qql_core::ast::Stmt::Query(q) => assert!(
            matches!(q.expression, qql_core::ast::QueryExpr::Nearest { .. }),
            "{:?}",
            q.expression
        ),
        other => panic!("{other:?}"),
    }
    // A vector literally named `bm25` (client sparse BM25) still parses as a target.
    let stmt =
        Parser::parse("QUERY TEXT 'time travel' FROM books USING bm25 LIMIT 10;").expect("parse");
    match &stmt {
        qql_core::ast::Stmt::Query(q) => assert!(matches!(
            q.expression,
            qql_core::ast::QueryExpr::Nearest { .. }
        )),
        other => panic!("{other:?}"),
    }
    // `bm25` stays usable as an identifier (not a reserved keyword).
    Parser::parse("QUERY VECTOR [0.1] FROM bm25 USING bm25 LIMIT 1;").expect("bm25 identifier");
}

#[test]
fn bm25_parse_errors_are_typed() {
    assert_eq!(
        parse_err("QUERY BM25 'x' FROM books LIMIT 10;"),
        "QQL-VALIDATION-BM25-USING"
    );
    assert_eq!(
        parse_err("QUERY BM25 'x' FROM books USING title AS SPARSE LIMIT 10;"),
        "QQL-VALIDATION-BM25-USING"
    );
    assert_eq!(
        parse_err("QUERY TEXT 'x' FROM books USING dense PARAMS (k = 1.2) LIMIT 10;"),
        "QQL-VALIDATION-SEARCH-PARAM"
    );
    assert_eq!(
        parse_err("QUERY BM25 'x' FROM books USING title PARAMS (b = 1.5) LIMIT 10;"),
        "QQL-VALIDATION-BM25-PARAMS"
    );
    assert_eq!(
        parse_err("QUERY BM25 'x' FROM books USING title PARAMS (k = -1) LIMIT 10;"),
        "QQL-VALIDATION-BM25-PARAMS"
    );
    assert_eq!(
        plan_err("QUERY BM25 '  ' FROM books USING title LIMIT 10;"),
        "QQL-PLAN-BM25"
    );
}

#[test]
fn create_text_index_with_bm25_scoring_and_code_tokenizer() {
    let source = "CREATE INDEX ON COLLECTION books FOR title TYPE text \
                  WITH (scoring = 'bm25', tokenizer = 'code', lowercase = true);";
    let (path, json) = route(source);
    assert_eq!(path, "/collections/books/index");
    assert_eq!(json["field_name"], "title");
    assert_eq!(
        json["field_schema"],
        json!({"type": "text", "tokenizer": "code", "lowercase": true, "scoring": {"type": "bm25"}})
    );
    roundtrip(source);
}

#[test]
fn bm25_scoring_requires_text_index() {
    assert_eq!(
        parse_err(
            "CREATE INDEX ON COLLECTION books FOR title TYPE keyword WITH (scoring = 'bm25');"
        ),
        "QQL-VALIDATION-TEXT-INDEX"
    );
    assert_eq!(
        parse_err("CREATE INDEX ON COLLECTION books FOR title TYPE text WITH (scoring = 'tfidf');"),
        "QQL-VALIDATION-TEXT-INDEX"
    );
    assert_eq!(
        plan_err("CREATE INDEX ON COLLECTION books FOR title TYPE text WITH (tokenizer = 'kode');"),
        "QQL-PLAN-INDEX-OPTION"
    );
}

#[test]
fn qdrant_1_20_feature_detection_covers_queries_prefetch_and_indexes() {
    let feature = |source: &str| {
        let stmt = Parser::parse(source).unwrap_or_else(|e| panic!("parse {source}: {e}"));
        plan(&stmt).expect("plan").qdrant_1_20_feature()
    };
    assert_eq!(
        feature("QUERY BM25 'x' FROM books USING title LIMIT 1;"),
        Some("QUERY BM25")
    );
    assert_eq!(
        feature(
            "QUERY FUSION RRF FROM books PREFETCH (QUERY BM25 'x' FROM books USING title LIMIT 5) LIMIT 1;"
        ),
        Some("QUERY BM25")
    );
    assert_eq!(
        feature("CREATE INDEX ON COLLECTION books FOR title TYPE text WITH (scoring = 'bm25');"),
        Some("text index scoring = 'bm25'")
    );
    assert_eq!(
        feature("CREATE INDEX ON COLLECTION books FOR title TYPE text WITH (tokenizer = 'code');"),
        Some("text index tokenizer = 'code'")
    );
    assert_eq!(
        feature("QUERY VECTOR [0.1] FROM books USING dense LIMIT 1;"),
        None
    );
    assert_eq!(
        feature("CREATE INDEX ON COLLECTION books FOR title TYPE text WITH (tokenizer = 'word');"),
        None
    );
}
