//! Inference inputs and collection/index config extras (w4-inference-config).
//!
//! Vertical coverage for the shapes this slice closes: inference `OPTIONS`,
//! `OBJECT` inputs, per-point inference in upserts, text-index stopword
//! languages, `WITH WAL` / `WITH STRICT_MODE` / `WITH METADATA`, shard-key
//! `placement` / `initial_state`, create-time `read_fan_out_*`, and strict
//! quantization sub-field validation. Each item asserts the full vertical
//! (parse → fmt → plan → convert) plus the fail-closed edges.

mod common;

use common::{canon, convert, wrapped};
use qql_convert::ConvertError;
use qql_core::fmt::format_stmt;
use qql_core::parser::Parser;
use qql_plan::{plan, to_rest_route};

/// Canonical QQL rendering of one source statement.
fn fmt(source: &str) -> String {
    let stmt = Parser::parse(source).unwrap_or_else(|e| panic!("parse {source}: {e}"));
    format_stmt(&stmt)
}

/// REST body JSON for one source statement.
fn body_of(source: &str) -> serde_json::Value {
    let stmt = Parser::parse(source).unwrap_or_else(|e| panic!("parse {source}: {e}"));
    let op = plan(&stmt).unwrap_or_else(|e| panic!("plan {source}: {e}"));
    let route = to_rest_route(&op).unwrap_or_else(|e| panic!("route {source}: {e:?}"));
    route.body_json().expect("route has a body")
}

// ── 1. Inference OPTIONS ─────────────────────────────────────────

#[test]
fn text_and_image_options_round_trip() {
    // Canonical rendering is idempotent (layout may wrap long statements).
    for source in [
        "QUERY TEXT 'hi' MODEL 'm' OPTIONS {temperature: 0.5, mode: 'x'} FROM docs USING dense LIMIT 1;",
        "QUERY IMAGE 'http://x/y.jpg' MODEL 'clip' OPTIONS {size: 512} FROM docs USING img LIMIT 1;",
    ] {
        assert_eq!(fmt(source), canon(source), "{source}");
    }
    // Options survive planning onto the wire Document / Image objects.
    assert_eq!(
        body_of(
            "QUERY TEXT 'hi' MODEL 'm' OPTIONS {temperature: 0.5} FROM docs USING dense LIMIT 1;"
        )["query"]["nearest"],
        serde_json::json!({"text": "hi", "model": "m", "options": {"temperature": 0.5}})
    );
    assert_eq!(
        body_of(
            "QUERY IMAGE 'http://x/y.jpg' MODEL 'clip' OPTIONS {size: 512} FROM docs USING img LIMIT 1;"
        )["query"]["nearest"],
        serde_json::json!({"image": "http://x/y.jpg", "model": "clip", "options": {"size": 512}})
    );
    // Absent options stay absent (no `"options": {}` / null on the wire).
    assert_eq!(
        body_of("QUERY TEXT 'hi' MODEL 'm' FROM docs USING dense LIMIT 1;")["query"]["nearest"],
        serde_json::json!({"text": "hi", "model": "m"})
    );
}

#[test]
fn document_options_convert_accepts() {
    assert_eq!(
        convert(r#"{"field_name": "x", "field_schema": "keyword"}"#).len(),
        1,
        "sanity"
    );
    let input = wrapped(
        "POST",
        "/collections/docs/points/query",
        serde_json::json!({"query": {"nearest": {"text": "x", "model": "m", "options": {"a": 1}}}, "limit": 1}),
    );
    let stmts = qql_convert::convert(&input, None).expect("document options convert");
    assert_eq!(stmts.len(), 1);
    assert!(
        stmts[0].contains("OPTIONS {a: 1}"),
        "options survive conversion: {}",
        stmts[0]
    );
}

// ── 2. InferenceObject ───────────────────────────────────────────

#[test]
fn object_input_parses_formats_and_plans() {
    // Parenthesized and bare objects are both accepted; bare is canonical.
    assert_eq!(
        fmt("QUERY OBJECT ({a: 1}) MODEL 'm' FROM docs USING dense LIMIT 1;"),
        canon("QUERY OBJECT {a: 1} MODEL 'm' FROM docs USING dense LIMIT 1;")
    );
    assert_eq!(
        fmt("QUERY OBJECT {a: 1} MODEL 'm' OPTIONS {k: true} FROM docs USING dense LIMIT 1;"),
        canon("QUERY OBJECT {a: 1} MODEL 'm' OPTIONS {k: true} FROM docs USING dense LIMIT 1;")
    );
    assert_eq!(
        body_of("QUERY OBJECT {a: 1} MODEL 'm' OPTIONS {k: true} FROM docs USING dense LIMIT 1;")["query"]
            ["nearest"],
        serde_json::json!({"object": {"a": 1}, "model": "m", "options": {"k": true}})
    );
    // Model-less objects serialize the `""` placeholder like Documents.
    assert_eq!(
        body_of("QUERY OBJECT {a: 1} FROM docs USING dense LIMIT 1;")["query"]["nearest"],
        serde_json::json!({"object": {"a": 1}, "model": ""})
    );
}

#[test]
fn inference_object_convert_accepts() {
    let input = wrapped(
        "POST",
        "/collections/docs/points/query",
        serde_json::json!({"query": {"nearest": {"object": {"a": [1, 2]}, "model": "m"}}, "limit": 1}),
    );
    let stmts = qql_convert::convert(&input, None).expect("inference object convert");
    assert_eq!(stmts.len(), 1);
    assert!(
        stmts[0].contains("OBJECT {a: [1, 2]}"),
        "object survives conversion: {}",
        stmts[0]
    );
    // Unknown members on the wire input still fail closed with a path.
    let bad = wrapped(
        "POST",
        "/collections/docs/points/query",
        serde_json::json!({"query": {"nearest": {"object": {"a": 1}, "model": "m", "bogus": 1}}, "limit": 1}),
    );
    assert!(matches!(
        qql_convert::convert(&bad, None).unwrap_err(),
        ConvertError::InvalidField { .. }
    ));
}

// ── 3. Per-point inference in upserts ────────────────────────────

#[test]
fn per_point_inference_upsert_round_trip() {
    for source in [
        "UPSERT INTO docs VALUES {id: 1, vector: {text: 'hello', model: 'm'}};",
        "UPSERT INTO docs VALUES {id: 1, vector: {image: 'https://x/y.jpg', model: 'c', options: {size: 224}}};",
        "UPSERT INTO docs VALUES {id: 1, vector: {object: {a: [1, 2]}, model: 'm'}};",
        "UPSERT INTO docs VALUES {id: 1, vector: {dense: {text: 'hello', model: 'm'}, sparse: {indices: [1], values: [0.5]}}};",
        "UPDATE docs SET VECTOR dense = {text: 'hello', model: 'm'} WHERE id = 1;",
        "UPDATE docs SET VECTOR VALUES {id: 1, vector: {text: 'hello', model: 'm'}};",
    ] {
        let stmt = Parser::parse(source).unwrap_or_else(|e| panic!("parse {source}: {e}"));
        let rendered = format_stmt(&stmt);
        let op = plan(&stmt).unwrap_or_else(|e| panic!("plan {source}: {e}"));
        let route = to_rest_route(&op).unwrap_or_else(|e| panic!("route {source}: {e:?}"));
        let body = body_of(source);
        let body_str = body.to_string().replace(' ', "");
        assert!(
            body_str.contains("\"text\":")
                || body_str.contains("\"image\":")
                || body_str.contains("\"object\":"),
            "{source} must carry the inference object on the wire: {body}"
        );
        // Convert the wire body back and replan to the same body.
        let wrapped = serde_json::json!({
            "method": route.method.as_str(), "path": route.path, "body": body,
        })
        .to_string();
        let emitted = qql_convert::convert(&wrapped, None)
            .unwrap_or_else(|e| panic!("convert {source}: {e}"));
        assert_eq!(emitted.len(), 1);
        let reparsed = Parser::parse(&format!("{};", emitted[0])).expect("reparse");
        assert_eq!(format_stmt(&reparsed), emitted[0], "not canonical");
        let reroute = to_rest_route(&plan(&reparsed).expect("replan")).expect("reroute");
        assert_eq!(reroute.body_json(), Some(body), "{source}");
        let _ = rendered;
    }
}

#[test]
fn per_point_inference_mixed_shapes_fail_closed() {
    // Inference keys mixed with sparse keys have no meaning.
    assert!(
        Parser::parse(
            "UPSERT INTO docs VALUES {id: 1, vector: {text: 'hi', indices: [1], values: [0.5]}};"
        )
        .is_err()
    );
    // Two inference kinds in one dict have no meaning.
    assert!(
        Parser::parse(
            "UPSERT INTO docs VALUES {id: 1, vector: {text: 'hi', image: 'x', model: 'm'}};"
        )
        .is_err()
    );
    // A vector literally named `text` with a numeric value still parses.
    let stmt = Parser::parse("UPSERT INTO docs VALUES {id: 1, vector: {text: [0.1, 0.2]}};")
        .expect("named vector `text`");
    assert_eq!(
        format_stmt(&stmt),
        "UPSERT INTO docs VALUES {id: 1, vector: {text: [0.1, 0.2]}}"
    );
}

// ── 4. Text-index stopwords / languages ──────────────────────────

#[test]
fn stopwords_shapes_round_trip() {
    assert_eq!(
        fmt("CREATE INDEX ON COLLECTION docs FOR body TYPE text WITH (stopwords = ['the', 'a']);"),
        canon(
            "CREATE INDEX ON COLLECTION docs FOR body TYPE text WITH (stopwords = ['the', 'a']);"
        )
    );
    assert_eq!(
        fmt("CREATE INDEX ON COLLECTION docs FOR body TYPE text WITH (stopwords = 'english');"),
        canon("CREATE INDEX ON COLLECTION docs FOR body TYPE text WITH (stopwords = 'english');")
    );
    assert_eq!(
        fmt(
            "CREATE INDEX ON COLLECTION docs FOR body TYPE text WITH (stopwords = {languages: ['english'], custom: ['foo']});"
        ),
        canon(
            "CREATE INDEX ON COLLECTION docs FOR body TYPE text WITH (stopwords = {languages: ['english'], custom: ['foo']});"
        )
    );
    // Bare language plans to the `languages` set; the set plans verbatim.
    let body =
        body_of("CREATE INDEX ON COLLECTION docs FOR b TYPE text WITH (stopwords = 'english');");
    assert_eq!(
        body["field_schema"]["stopwords"],
        serde_json::json!({"languages": ["english"]})
    );
    let body = body_of(
        "CREATE INDEX ON COLLECTION docs FOR b TYPE text WITH (stopwords = {languages: ['english', 'german'], custom: ['foo']});",
    );
    assert_eq!(
        body["field_schema"]["stopwords"],
        serde_json::json!({"languages": ["english", "german"], "custom": ["foo"]})
    );
    // Unknown languages fail closed at plan time.
    let stmt = Parser::parse(
        "CREATE INDEX ON COLLECTION docs FOR b TYPE text WITH (stopwords = 'klingon');",
    )
    .expect("parse");
    assert!(plan(&stmt).is_err());
    // Unknown set keys fail closed at parse time.
    assert!(
        Parser::parse(
            "CREATE INDEX ON COLLECTION docs FOR b TYPE text WITH (stopwords = {dialect: ['x']});"
        )
        .is_err()
    );
}

#[test]
fn stopwords_convert_shapes() {
    // Bare language string converts to the bare form.
    let input = wrapped(
        "PUT",
        "/collections/docs/index",
        serde_json::json!({"field_name": "b", "field_schema": {"type": "text", "stopwords": "english"}}),
    );
    assert_eq!(
        qql_convert::convert(&input, None).expect("convert"),
        [canon(
            "CREATE INDEX ON COLLECTION docs FOR b TYPE text WITH (stopwords = 'english');"
        )]
    );
    // Language + custom set keeps the object spelling.
    let input = wrapped(
        "PUT",
        "/collections/docs/index",
        serde_json::json!({"field_name": "b", "field_schema": {"type": "text", "stopwords": {"languages": ["english"], "custom": ["x"]}}}),
    );
    assert_eq!(
        qql_convert::convert(&input, None).expect("convert"),
        [canon(
            "CREATE INDEX ON COLLECTION docs FOR b TYPE text WITH (stopwords = {languages: ['english'], custom: ['x']});"
        )]
    );
    // Unknown languages fail closed with a path.
    let bad = wrapped(
        "PUT",
        "/collections/docs/index",
        serde_json::json!({"field_name": "b", "field_schema": {"type": "text", "stopwords": "klingon"}}),
    );
    assert!(matches!(
        qql_convert::convert(&bad, None).unwrap_err(),
        ConvertError::InvalidField { .. }
    ));
}

// ── 5. Collection config extras ──────────────────────────────────

#[test]
fn wal_strict_metadata_create_round_trip() {
    assert_eq!(
        fmt("CREATE COLLECTION docs (v VECTOR(8, COSINE)) WITH WAL (wal_capacity_mb = 32);"),
        canon("CREATE COLLECTION docs (v VECTOR(8, COSINE)) WITH WAL (wal_capacity_mb = 32);")
    );
    let body = body_of(
        "CREATE COLLECTION docs (v VECTOR(8, COSINE)) WITH WAL (wal_capacity_mb = 32, wal_segments_ahead = 2, wal_retain_closed = 1);",
    );
    assert_eq!(
        body["wal_config"],
        serde_json::json!({"wal_capacity_mb": 32, "wal_segments_ahead": 2, "wal_retain_closed": 1})
    );
    let body = body_of(
        "CREATE COLLECTION docs (v VECTOR(8, COSINE)) WITH STRICT_MODE (enabled = true, max_query_limit = 100, search_allow_exact = false, search_max_oversampling = 2.5, multivector_config = {colbert: {max_vectors: 8}}, sparse_config = {bm25: {max_length: 64}});",
    );
    assert_eq!(
        body["strict_mode_config"]["enabled"],
        serde_json::json!(true)
    );
    assert_eq!(
        body["strict_mode_config"]["max_query_limit"],
        serde_json::json!(100)
    );
    assert_eq!(
        body["strict_mode_config"]["search_max_oversampling"],
        serde_json::json!(2.5)
    );
    assert_eq!(
        body["strict_mode_config"]["multivector_config"],
        serde_json::json!({"colbert": {"max_vectors": 8}})
    );
    assert_eq!(
        body["strict_mode_config"]["sparse_config"],
        serde_json::json!({"bm25": {"max_length": 64}})
    );
    let body = body_of(
        "CREATE COLLECTION docs (v VECTOR(8, COSINE)) WITH METADATA (owner = 'team', version = 3, flags = {a: true});",
    );
    assert_eq!(
        body["metadata"],
        serde_json::json!({"owner": "team", "version": 3, "flags": {"a": true}})
    );
    // ALTER carries strict/metadata but never WAL.
    let body = body_of("ALTER COLLECTION docs WITH STRICT_MODE (enabled = true);");
    assert_eq!(
        body["strict_mode_config"],
        serde_json::json!({"enabled": true})
    );
    let body = body_of("ALTER COLLECTION docs WITH METADATA (owner = 'team');");
    assert_eq!(body["metadata"], serde_json::json!({"owner": "team"}));
    assert!(Parser::parse("ALTER COLLECTION docs WITH WAL (wal_capacity_mb = 8);").is_err());
}

#[test]
fn collection_config_fail_closed_edges() {
    // Unknown keys fail at parse time.
    assert!(
        Parser::parse("CREATE COLLECTION docs (v VECTOR(8, COSINE)) WITH WAL (bogus = 1);")
            .is_err()
    );
    assert!(
        Parser::parse("CREATE COLLECTION docs (v VECTOR(8, COSINE)) WITH STRICT_MODE (bogus = 1);")
            .is_err()
    );
    // Wrong value types fail at plan time (hand-built ASTs fail there too).
    let stmt = Parser::parse(
        "CREATE COLLECTION docs (v VECTOR(8, COSINE)) WITH WAL (wal_capacity_mb = 'big');",
    );
    assert!(stmt.is_err(), "wal values are type-checked at parse");
    let stmt = Parser::parse(
        "CREATE COLLECTION docs (v VECTOR(8, COSINE)) WITH STRICT_MODE (max_query_limit = 0);",
    )
    .expect("parse");
    assert!(plan(&stmt).is_err(), "ranges are enforced at plan time");
    // Create-time fan-out is accepted (applied via deferred PATCH, so the
    // PUT body itself never carries it).
    let stmt = Parser::parse(
        "CREATE COLLECTION docs (v VECTOR(8, COSINE)) WITH PARAMS (read_fan_out_factor = 2);",
    )
    .expect("parse");
    let op = plan(&stmt).expect("plan");
    let route = to_rest_route(&op).expect("route");
    assert!(
        route
            .body_json()
            .expect("body")
            .get("read_fan_out_factor")
            .is_none()
    );
}

#[test]
fn collection_extras_convert_accepts() {
    let input = wrapped(
        "PUT",
        "/collections/docs",
        serde_json::json!({
            "vectors": {"v": {"size": 8, "distance": "Cosine"}},
            "wal_config": {"wal_capacity_mb": 8},
            "strict_mode_config": {"enabled": true},
            "metadata": {"a": 1},
        }),
    );
    let stmts = qql_convert::convert(&input, None).expect("convert");
    assert_eq!(stmts.len(), 1);
    assert!(
        stmts[0].contains("WITH WAL (wal_capacity_mb = 8)"),
        "{}",
        stmts[0]
    );
    assert!(
        stmts[0].contains("WITH STRICT_MODE (enabled = true)"),
        "{}",
        stmts[0]
    );
    assert!(stmts[0].contains("WITH METADATA (a = 1)"), "{}", stmts[0]);
    // ALTER strict/metadata decode.
    let input = wrapped(
        "PATCH",
        "/collections/docs",
        serde_json::json!({"strict_mode_config": {"enabled": true}, "metadata": {"a": 1}}),
    );
    let stmts = qql_convert::convert(&input, None).expect("convert");
    assert_eq!(stmts.len(), 1);
    assert!(stmts[0].contains("STRICT_MODE"), "{}", stmts[0]);
    assert!(stmts[0].contains("METADATA"), "{}", stmts[0]);
}

// ── 5b. Shard placement / initial_state ──────────────────────────

#[test]
fn shard_key_placement_and_state_round_trip() {
    assert_eq!(
        fmt(
            "CREATE SHARD KEY 'acme' ON COLLECTION docs WITH (placement = [1, 2], initial_state = 'active');"
        ),
        canon(
            "CREATE SHARD KEY 'acme' ON COLLECTION docs WITH (placement = [1, 2], initial_state = 'Active');"
        )
    );
    let body = body_of(
        "CREATE SHARD KEY 'acme' ON COLLECTION docs WITH (shards_number = 2, placement = [1, 2], initial_state = 'Active');",
    );
    assert_eq!(body["shard_key"], serde_json::json!("acme"));
    assert_eq!(body["placement"], serde_json::json!([1, 2]));
    assert_eq!(body["initial_state"], serde_json::json!("Active"));
    // Unknown states fail closed.
    assert!(
        Parser::parse("CREATE SHARD KEY 'a' ON COLLECTION docs WITH (initial_state = 'bogus');")
            .is_err()
    );
    assert!(
        Parser::parse("CREATE SHARD KEY 'a' ON COLLECTION docs WITH (placement = []);").is_err()
    );
    // Convert accepts both.
    let input = wrapped(
        "PUT",
        "/collections/docs/shards",
        serde_json::json!({"shard_key": "acme", "placement": [1], "initial_state": "Active"}),
    );
    assert_eq!(
        qql_convert::convert(&input, None).expect("convert"),
        [canon(
            "CREATE SHARD KEY 'acme' ON COLLECTION docs WITH (placement = [1], initial_state = 'Active');"
        )]
    );
}

// ── 6. Quantization extras ───────────────────────────────────────

#[test]
fn quantization_misplaced_and_unknown_fields_fail_closed() {
    // Unknown sub-fields fail in convert (previously silently dropped).
    for body in [
        serde_json::json!({"scalar": {"type": "int8", "bogus": 1}}),
        serde_json::json!({"scalar": {"type": "int8", "compression": "x4"}}),
        serde_json::json!({"binary": {"quantile": 0.5}}),
        serde_json::json!({"binary": {"encoding": "three_bits"}}),
        serde_json::json!({"binary": {"query_encoding": "weird"}}),
        serde_json::json!({"product": {"compression": "x7"}}),
        serde_json::json!({"turbo": {"bits": "bits3"}}),
    ] {
        let input = wrapped(
            "PUT",
            "/collections/docs",
            serde_json::json!({"vectors": {"v": {"size": 8, "distance": "Cosine", "quantization_config": body}}}),
        );
        assert!(
            qql_convert::convert(&input, None).is_err(),
            "quantization body must fail closed: {body}"
        );
    }
    // Valid configs still convert.
    let input = wrapped(
        "PUT",
        "/collections/docs",
        serde_json::json!({"vectors": {"v": {"size": 8, "distance": "Cosine", "quantization_config": {"binary": {"encoding": "two_bits", "query_encoding": "scalar4bits"}}}}}),
    );
    let stmts = qql_convert::convert(&input, None).expect("convert");
    assert!(stmts[0].contains("encoding = 'two_bits'"), "{}", stmts[0]);
    // Misplaced QQL keys fail at parse time.
    assert!(
        Parser::parse(
            "CREATE COLLECTION docs (v VECTOR(8, COSINE) WITH QUANTIZATION (type = 'scalar', compression = 'x4'));"
        )
        .is_err(),
        "misplaced quantization keys fail closed"
    );
}

// ── Hybrid / OPTIONS interaction ─────────────────────────────────

#[test]
fn hybrid_rejects_options() {
    assert!(
        Parser::parse("QUERY HYBRID TEXT 'x' MODEL 'm' OPTIONS {a: 1} DENSE d SPARSE s FUSION RRF FROM docs LIMIT 1;")
            .is_err()
    );
    assert!(
        Parser::parse("QUERY TEXT 'x' OPTIONS {a: 1} FROM docs USING HYBRID LIMIT 1;").is_err()
    );
}

#[test]
fn options_reject_duplicate_keys() {
    assert!(
        Parser::parse(
            "QUERY TEXT 'x' MODEL 'm' OPTIONS {a: 1, a: 2} FROM docs USING dense LIMIT 1;"
        )
        .is_err()
    );
}
