//! Qdrant 1.20 request shapes fail closed with one recognizable diagnostic.
//!
//! QQL cannot spell these yet (`nested` payload indexes, `substring` match,
//! `turbo8` / `turbo16`, `bits8`, `id_tracker`). BM25 `text` queries,
//! text-index `scoring`, and the `code` tokenizer convert now; see
//! `qdrant_1_20_bm25.rs`. Each case asserts a typed
//! [`ConvertError::InvalidField`] whose detail starts with `Qdrant 1.20` at
//! the exact offending path. When core grammar support lands, flip the case
//! into a round-trip test in `roundtrip.rs`.

use serde_json::{Value, json};

use qql_convert::{ConvertError, convert};

fn wrapped(method: &str, path: &str, body: Value) -> String {
    json!({"method": method, "path": path, "body": body}).to_string()
}

#[track_caller]
fn assert_pending(input: &str, want_path: &str, want_feature: &str) {
    match convert(input, None) {
        Err(ConvertError::InvalidField { path, detail }) => {
            assert_eq!(path, want_path, "detail: {detail}");
            assert!(
                detail.starts_with("Qdrant 1.20 "),
                "expected a pending-1.20 diagnostic, got: {detail}"
            );
            assert!(
                detail.contains(want_feature),
                "expected '{want_feature}' in: {detail}"
            );
        }
        other => panic!("expected InvalidField for {input}, got {other:?}"),
    }
}

#[test]
fn pending_nested_index() {
    let input = wrapped(
        "PUT",
        "/collections/docs/index",
        json!({"field_name": "offers[].price", "field_schema": {"type": "float", "nested": "offers"}}),
    );
    assert_pending(
        &input,
        "body.field_schema.nested",
        "per-element payload index",
    );
}

#[test]
fn pending_substring_match() {
    let input = wrapped(
        "POST",
        "/collections/docs/points/scroll",
        json!({"filter": {"must": [{"key": "path", "match": {"substring": "src/"}}]}, "limit": 10}),
    );
    match convert(&input, None) {
        Err(ConvertError::InvalidField { path, detail }) => {
            assert!(path.ends_with("match.substring"), "path: {path}");
            assert!(
                detail.starts_with("Qdrant 1.20 substring match"),
                "detail: {detail}"
            );
        }
        other => panic!("expected InvalidField, got {other:?}"),
    }
}

#[test]
fn pending_turbo8_and_turbo16_datatypes() {
    for dt in ["turbo8", "turbo16"] {
        let input = wrapped(
            "PUT",
            "/collections/docs",
            json!({"vectors": {"dense": {"size": 384, "distance": "Cosine", "datatype": dt}}}),
        );
        assert_pending(
            &input,
            "body.vectors.dense.datatype",
            &format!("datatype '{dt}'"),
        );
    }
}

#[test]
fn pending_turbo_bits8() {
    let input = wrapped(
        "PUT",
        "/collections/docs",
        json!({
            "vectors": {"size": 384, "distance": "Cosine"},
            "quantization_config": {"turbo": {"bits": "bits8"}}
        }),
    );
    assert_pending(
        &input,
        "body.quantization_config.turbo.bits",
        "TurboQuant bit size 'bits8'",
    );
}

#[test]
fn pending_id_tracker_on_create_and_update() {
    let input = wrapped(
        "PUT",
        "/collections/docs",
        json!({"vectors": {"size": 4, "distance": "Dot"}, "id_tracker": {"memory": "cold"}}),
    );
    assert_pending(&input, "body.id_tracker", "id-tracker memory tier");

    let input = wrapped(
        "PATCH",
        "/collections/docs",
        json!({"params": {"id_tracker": {"memory": "cold"}}}),
    );
    assert_pending(&input, "body.params.id_tracker", "id-tracker memory tier");
}

#[test]
fn pre_1_20_values_keep_their_generic_errors() {
    let input = wrapped(
        "PUT",
        "/collections/docs",
        json!({"vectors": {"size": 4, "distance": "Dot", "datatype": "turbo3"}}),
    );
    match convert(&input, None) {
        Err(ConvertError::InvalidField { detail, .. }) => {
            assert!(
                detail.contains("unknown vector datatype"),
                "detail: {detail}"
            );
        }
        other => panic!("expected InvalidField, got {other:?}"),
    }
}

/// Every 1.20 enum value the vendored spec advertises must hit the pending
/// path (no-op while the spec is still pinned to 1.19; enforced once the
/// API sync lands).
#[test]
fn spec_1_20_enums_are_covered() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../qql-runtime/openapi.json");
    let spec: Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("read openapi.json"))
            .expect("openapi.json is valid JSON");
    let schemas = &spec["components"]["schemas"];
    let enum_has = |schema: &str, value: &str| {
        schemas[schema]["enum"]
            .as_array()
            .is_some_and(|items| items.iter().any(|v| v == value))
    };
    for dt in ["turbo8", "turbo16"] {
        if enum_has("Datatype", dt) {
            let input = wrapped(
                "PUT",
                "/collections/docs",
                json!({"vectors": {"size": 4, "distance": "Dot", "datatype": dt}}),
            );
            assert_pending(&input, "body.vectors.datatype", dt);
        }
    }
    if enum_has("TurboQuantBitSize", "bits8") {
        let input = wrapped(
            "PUT",
            "/collections/docs",
            json!({"vectors": {"size": 4, "distance": "Dot"}, "quantization_config": {"turbo": {"bits": "bits8"}}}),
        );
        assert_pending(&input, "body.quantization_config.turbo.bits", "bits8");
    }
}
