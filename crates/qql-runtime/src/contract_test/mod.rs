//! OpenAPI contract tests + REST/gRPC parity checks for planned query bodies.
//!
//! Wire authority: `openapi.json` (REST body schemas) and `proto/points.proto`
//! (gRPC field mapping via `grpc_route` converters).

mod ddl;
mod embeddings;
mod extra;
mod formula;
mod parity;
mod query;

use std::fs;

/// Load the vendored OpenAPI document, or `None` if it is missing.
pub(super) fn load_openapi_json() -> Option<serde_json::Value> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("openapi.json");
    let content = fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Load OpenAPI or skip the calling test when the file is absent.
pub(super) fn openapi_or_skip() -> Option<serde_json::Value> {
    let openapi = load_openapi_json();
    if openapi.is_none() {
        eprintln!("openapi.json not found, skipping contract test");
    }
    openapi
}

/// Assert `value` matches `#/components/schemas/{schema_name}`.
pub(super) fn validate_ref(
    openapi: &serde_json::Value,
    schema_name: &str,
    value: &serde_json::Value,
) {
    let validator = jsonschema::validator_for(&serde_json::json!({
        "$ref": format!("#/components/schemas/{schema_name}"),
        "components": openapi["components"]
    }))
    .unwrap_or_else(|e| panic!("failed to compile {schema_name} schema: {e}"));
    let errors: Vec<_> = validator.iter_errors(value).collect();
    assert!(
        errors.is_empty(),
        "OpenAPI {schema_name} violation: {errors:?}\nJSON: {value}"
    );
}
