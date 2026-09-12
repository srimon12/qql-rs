//! Fail-closed mutation gaps: upsert `UPDATE FILTER` / `UPDATE MODE`,
//! `SetPayload.key`, and batch-only `OVERWRITE`.
//!
//! Vertical coverage per item: parse + fmt (qql-core), lowering (qql-plan),
//! decode (qql-convert), and the plan → route → convert → replan identity.

use qql_convert::convert;
use qql_core::fmt::format_stmt;
use qql_core::parser::Parser;
use qql_plan::{plan, to_rest_route};

/// Parse one statement or panic.
fn parse(source: &str) -> qql_core::ast::Stmt {
    Parser::parse(source).unwrap_or_else(|e| panic!("parse {source}: {e}"))
}

/// Canonical fmt of one statement.
fn fmt(source: &str) -> String {
    format_stmt(&parse(source))
}

/// Planned REST body JSON for one statement.
fn route_body(source: &str) -> serde_json::Value {
    let op = plan(&parse(source)).unwrap_or_else(|e| panic!("plan {source}: {e}"));
    let route = to_rest_route(&op).unwrap_or_else(|e| panic!("route {source}: {e:?}"));
    route.body_json().expect("mutation route has a body")
}

/// Wrapped `{method, path, body}` request for convert.
fn wrapped(method: &str, path: &str, body: &serde_json::Value) -> String {
    serde_json::json!({"method": method, "path": path, "body": body}).to_string()
}

// ── 1. update_filter + update_mode on upserts ────────────────────

#[test]
fn upsert_guards_parse_and_format() {
    assert_eq!(
        fmt("UPSERT INTO docs VALUES {id: 1, vector: [0.1]} UPDATE FILTER status = 'active';"),
        "UPSERT INTO docs VALUES {id: 1, vector: [0.1]} UPDATE FILTER status = 'active'"
    );
    assert_eq!(
        fmt("UPSERT INTO docs VALUES {id: 1, vector: [0.1]} UPDATE MODE insert_only;"),
        "UPSERT INTO docs VALUES {id: 1, vector: [0.1]} UPDATE MODE insert_only"
    );
    assert_eq!(
        fmt("UPSERT INTO docs VALUES {id: 1, vector: [0.1]} UPDATE MODE update_only;"),
        "UPSERT INTO docs VALUES {id: 1, vector: [0.1]} UPDATE MODE update_only"
    );
    assert_eq!(
        fmt("UPSERT INTO docs VALUES {id: 1, vector: [0.1]} UPDATE MODE upsert;"),
        "UPSERT INTO docs VALUES {id: 1, vector: [0.1]} UPDATE MODE upsert"
    );
    // Either order is accepted; fmt normalizes to FILTER-then-MODE.
    assert_eq!(
        fmt(
            "UPSERT INTO docs VALUES {id: 1, vector: [0.1]} UPDATE MODE insert_only UPDATE FILTER k = 1;"
        ),
        "UPSERT INTO docs VALUES {id: 1, vector: [0.1]} UPDATE FILTER k = 1 UPDATE MODE insert_only"
    );
    // Case-insensitive mode words.
    assert_eq!(
        fmt("UPSERT INTO docs VALUES {id: 1, vector: [0.1]} UPDATE MODE INSERT_ONLY;"),
        "UPSERT INTO docs VALUES {id: 1, vector: [0.1]} UPDATE MODE insert_only"
    );
}

#[test]
fn upsert_guards_reject_duplicates_and_bad_modes() {
    for (source, code) in [
        (
            "UPSERT INTO docs VALUES {id: 1} UPDATE FILTER a = 1 UPDATE FILTER b = 2;",
            "QQL-PARSE-DUPLICATE-CLAUSE",
        ),
        (
            "UPSERT INTO docs VALUES {id: 1} UPDATE MODE insert_only UPDATE MODE update_only;",
            "QQL-PARSE-DUPLICATE-CLAUSE",
        ),
        (
            "UPSERT INTO docs VALUES {id: 1} UPDATE MODE merge;",
            "QQL-PARSE-UPSERT-MODE",
        ),
        (
            "UPSERT INTO docs VALUES {id: 1} UPDATE MODE 'insert_only';",
            "QQL-PARSE-UPSERT-MODE",
        ),
    ] {
        let err = Parser::parse(source).expect_err(&format!("must reject: {source}"));
        assert_eq!(err.code, code, "{source}: {err:?}");
    }
}

#[test]
fn upsert_guards_lower_to_the_wire_body() {
    let body = route_body("UPSERT INTO docs VALUES {id: 1, vector: [0.1]};");
    assert!(body.get("update_filter").is_none(), "default omits filter");
    assert!(body.get("update_mode").is_none(), "default omits mode");

    let body = route_body(
        "UPSERT INTO docs VALUES {id: 1, vector: [0.1]} UPDATE FILTER status = 'active' UPDATE MODE update_only;",
    );
    assert_eq!(
        body["update_filter"],
        serde_json::json!({"must": [{"key": "status", "match": {"value": "active"}}]}),
        "{body}"
    );
    assert_eq!(body["update_mode"], "update_only");
}

#[test]
fn upsert_guards_decode_from_wire() {
    let body = serde_json::json!({
        "points": [{"id": 1, "vector": [0.1]}],
        "update_filter": {"must": [{"key": "status", "match": {"value": "active"}}]},
        "update_mode": "insert_only",
    });
    let emitted = convert(&wrapped("PUT", "/collections/docs/points", &body), None)
        .expect("convert upsert guards");
    assert_eq!(emitted.len(), 1);
    assert_eq!(
        emitted[0],
        "UPSERT INTO docs VALUES {id: 1, vector: [0.1]} UPDATE FILTER status = 'active' UPDATE MODE insert_only",
        "{emitted:?}"
    );
    // Explicit `upsert` mode survives the decode (default still omits it).
    let body = serde_json::json!({"points": [{"id": 1}], "update_mode": "upsert"});
    let emitted = convert(&wrapped("PUT", "/collections/docs/points", &body), None)
        .expect("convert explicit upsert mode");
    assert!(emitted[0].ends_with("UPDATE MODE upsert"), "{emitted:?}");
}

#[test]
fn upsert_guard_decode_fails_closed() {
    // Unknown mode string.
    let body = serde_json::json!({"points": [{"id": 1}], "update_mode": "merge"});
    assert!(convert(&wrapped("PUT", "/collections/docs/points", &body), None).is_err());
    // Non-string mode.
    let body = serde_json::json!({"points": [{"id": 1}], "update_mode": 7});
    assert!(convert(&wrapped("PUT", "/collections/docs/points", &body), None).is_err());
    // Empty update_filter object has no QQL representation here.
    let body = serde_json::json!({"points": [{"id": 1}], "update_filter": {}});
    let emitted = convert(&wrapped("PUT", "/collections/docs/points", &body), None)
        .expect("empty filter decodes to no guard");
    assert_eq!(emitted[0], "UPSERT INTO docs VALUES {id: 1}");
}

#[test]
fn upsert_guard_params_bind() {
    use qql_core::params_json::bind_stmt_with_params;
    let mut stmt =
        parse("UPSERT INTO docs VALUES {id: 1, vector: [0.1]} UPDATE FILTER status = :s;");
    assert!(plan(&stmt).is_err(), "unbound guard param must not plan");
    let params = serde_json::json!({"s": "active"});
    bind_stmt_with_params(&mut stmt, &params).expect("bind guard param");
    let op = plan(&stmt).expect("bound guard plans");
    let route = to_rest_route(&op).expect("route");
    let body = route.body_json().unwrap();
    assert_eq!(body["update_filter"]["must"][0]["match"]["value"], "active");
}

// ── 2. SetPayload key ────────────────────────────────────────────

#[test]
fn set_payload_key_parses_and_formats() {
    assert_eq!(
        fmt("UPDATE docs SET PAYLOAD = {a: 1} KEY 'a.b' WHERE id = 1;"),
        "UPDATE docs SET PAYLOAD = {a: 1} KEY 'a.b' WHERE id = 1"
    );
    // KEY + OVERWRITE commute; fmt normalizes to KEY-then-OVERWRITE.
    assert_eq!(
        fmt("UPDATE docs SET PAYLOAD = {a: 1} OVERWRITE KEY 'a.b' WHERE id = 1;"),
        "UPDATE docs SET PAYLOAD = {a: 1} KEY 'a.b' OVERWRITE WHERE id = 1"
    );
    for (source, code) in [
        (
            "UPDATE docs SET PAYLOAD = {a: 1} KEY 'x' KEY 'y' WHERE id = 1;",
            "QQL-PARSE-DUPLICATE-CLAUSE",
        ),
        (
            "UPDATE docs SET PAYLOAD = {a: 1} OVERWRITE OVERWRITE WHERE id = 1;",
            "QQL-PARSE-DUPLICATE-CLAUSE",
        ),
    ] {
        let err = Parser::parse(source).expect_err(&format!("must reject: {source}"));
        assert_eq!(err.code, code, "{source}: {err:?}");
    }
}

#[test]
fn set_payload_key_lowers_and_decodes() {
    let body = route_body("UPDATE docs SET PAYLOAD = {a: 1} WHERE id = 1;");
    assert!(body.get("key").is_none(), "default omits key: {body}");
    let body = route_body("UPDATE docs SET PAYLOAD = {a: 1} KEY 'a.b' WHERE id = 1;");
    assert_eq!(body["key"], "a.b", "{body}");

    let wire = serde_json::json!({"payload": {"a": 1}, "points": [1], "key": "a.b"});
    let emitted = convert(
        &wrapped("POST", "/collections/docs/points/payload", &wire),
        None,
    )
    .expect("convert keyed SetPayload");
    assert_eq!(
        emitted,
        ["UPDATE docs SET PAYLOAD = {a: 1} KEY 'a.b' WHERE id IN (1)"]
    );
}

// ── 3. overwrite_payload ─────────────────────────────────────────

#[test]
fn overwrite_parses_formats_and_plans_batch_only() {
    assert_eq!(
        fmt("UPDATE docs SET PAYLOAD = {a: 1} OVERWRITE WHERE id = 1;"),
        "UPDATE docs SET PAYLOAD = {a: 1} OVERWRITE WHERE id = 1"
    );
    let stmt = parse("UPDATE docs SET PAYLOAD = {a: 1} OVERWRITE WHERE id = 1;");
    let op = plan(&stmt).expect("overwrite plans");
    assert_eq!(op.operation_label(), "OVERWRITE_PAYLOAD");
    assert_eq!(op.compile_stmt_type(), "overwrite_payload");
    match to_rest_route(&op) {
        Err(qql_plan::RestProjectionError::OverwriteRequiresBatch) => {}
        other => panic!("single OVERWRITE must fail closed, got {other:?}"),
    }
    let err = qql_plan::try_route(&stmt).expect_err("try_route must fail");
    assert_eq!(err.code, "QQL-REST-OVERWRITE-BATCH-ONLY", "{err:?}");
}

#[test]
fn overwrite_batch_members_plan_and_round_trip() {
    let source = "BATCH { UPDATE docs SET PAYLOAD = {a: 1} OVERWRITE WHERE id = 1; UPDATE docs SET PAYLOAD = {b: 2} WHERE id = 2; }";
    let stmt = parse(&format!("{source};"));
    let op = plan(&stmt).expect("overwrite batch plans");
    let route = to_rest_route(&op).expect("batch route");
    assert_eq!(route.path, "/collections/docs/points/batch");
    let body = route.body_json().unwrap();
    assert!(
        body["operations"][0].get("overwrite_payload").is_some(),
        "first op must be overwrite_payload: {body}"
    );
    assert_eq!(
        body["operations"][0]["overwrite_payload"]["payload"],
        serde_json::json!({"a": 1})
    );
    assert!(
        body["operations"][1].get("set_payload").is_some(),
        "second op stays set_payload: {body}"
    );
    // Labels follow the batch in order.
    let qql_plan::PlannedOperation::Batch { operations, .. } = &op else {
        panic!("expected Batch, got {op:?}");
    };
    let (_, labels, _) = qql_plan::build_update_batch(operations).expect("build batch");
    assert_eq!(labels, vec!["OVERWRITE_PAYLOAD", "UPDATE_PAYLOAD"]);

    // Convert decodes the batch back into the overwrite statement.
    let emitted = convert(
        &wrapped("POST", "/collections/docs/points/batch", &body),
        None,
    )
    .expect("convert overwrite batch");
    assert_eq!(emitted.len(), 1, "{emitted:?}");
    let reparsed = parse(&format!("{};", emitted[0]));
    assert_eq!(format_stmt(&reparsed), emitted[0], "not canonical");
    let replanned = plan(&reparsed).expect("replan");
    let reroute = to_rest_route(&replanned).expect("reroute");
    assert_eq!(reroute.body_json().unwrap(), body);
}
