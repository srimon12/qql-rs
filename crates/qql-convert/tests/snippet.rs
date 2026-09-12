//! HTTP-snippet and curl input shapes — pasting from docs/blogs/curl history.
//!
//! Docs show `METHOD /path` plus a JSON body (or a full `curl` command), not
//! the wrapped `{"method","path","body"}` envelope the recorder writes. These
//! tests pin that both paste shapes convert to the same QQL as the envelope.

mod common;

use common::assert_canonical;
use qql_convert::{ConvertError, convert};

// ── HTTP snippet: `METHOD path` line + JSON body ────────────────────────────

#[test]
fn http_snippet_query_converts() {
    let input = "POST /collections/docs/points/query\n{\n  \"query\": {\"nearest\": [0.1, 0.2]},\n  \"using\": \"dense\",\n  \"limit\": 5\n}";
    let statements = convert(input, None).expect("snippet converts");
    assert_canonical(&statements, input);
    assert_eq!(statements.len(), 1);
    assert!(
        statements[0].starts_with("QUERY [0.1, 0.2]"),
        "{statements:?}"
    );
}

#[test]
fn http_snippet_accepts_full_url_and_query_string() {
    let input = "PUT http://localhost:6333/collections/docs/points?wait=true\n{\"points\": [{\"id\": 1, \"vector\": [0.1, 0.2]}]}";
    let statements = convert(input, None).expect("snippet converts");
    assert_canonical(&statements, input);
    assert!(
        statements[0].starts_with("UPSERT INTO docs"),
        "{statements:?}"
    );
    assert!(statements[0].contains("WAIT true"), "{statements:?}");
}

#[test]
fn http_snippet_bodyless_get_converts() {
    let statements = convert("GET /collections", None).expect("snippet converts");
    assert_eq!(statements, ["SHOW COLLECTIONS"]);
}

#[test]
fn http_snippet_ignores_http_version_suffix() {
    let input = "POST /collections/docs/points/count HTTP/1.1\n{\"exact\": true}";
    let statements = convert(input, None).expect("snippet converts");
    assert_eq!(statements, ["COUNT FROM docs WITH (exact = true)"]);
}

#[test]
fn http_snippet_placeholder_collection_fails_closed() {
    let input = "POST /collections/<your-collection>/points/count\n{\"exact\": true}";
    let error = convert(input, None).expect_err("placeholder must fail");
    match &error {
        ConvertError::InvalidField { path, .. } => assert_eq!(path, "collection"),
        other => panic!("expected InvalidField, got {other:?}"),
    }
}

#[test]
fn multiple_snippets_in_one_file_convert() {
    let input = "POST /collections/docs/points/count\n{\"exact\": true}\nPOST /collections/docs/points/count\n{\"exact\": false}\n";
    let statements = convert(input, None).expect("snippets convert");
    assert_canonical(&statements, input);
    assert_eq!(statements.len(), 2);
}

// ── curl commands ────────────────────────────────────────────────────────────

#[test]
fn curl_basic_post_converts() {
    let input = "curl -X POST http://localhost:6333/collections/docs/points/count -H 'Content-Type: application/json' -d '{\"exact\": true}'";
    let statements = convert(input, None).expect("curl converts");
    assert_canonical(&statements, input);
    assert_eq!(statements, ["COUNT FROM docs WITH (exact = true)"]);
}

#[test]
fn curl_implies_post_when_data_present() {
    let input = "curl http://localhost:6333/collections/docs/points/count -d '{\"exact\": true}'";
    let statements = convert(input, None).expect("curl converts");
    assert_eq!(statements, ["COUNT FROM docs WITH (exact = true)"]);
}

#[test]
fn curl_get_without_data_converts() {
    let statements =
        convert("curl http://localhost:6333/collections", None).expect("curl converts");
    assert_eq!(statements, ["SHOW COLLECTIONS"]);
}

#[test]
fn curl_long_flags_and_double_quotes_convert() {
    let input = "curl --request POST --header \"Content-Type: application/json\" --data-raw \"{\\\"exact\\\": true}\" http://localhost:6333/collections/docs/points/count";
    let statements = convert(input, None).expect("curl converts");
    assert_eq!(statements, ["COUNT FROM docs WITH (exact = true)"]);
}

#[test]
fn curl_multiline_continuation_converts() {
    let input = "curl -X POST \\\n  http://localhost:6333/collections/docs/points/count \\\n  -H 'Content-Type: application/json' \\\n  -d '{\"exact\": true}'";
    let statements = convert(input, None).expect("curl converts");
    assert_eq!(statements, ["COUNT FROM docs WITH (exact = true)"]);
}

#[test]
fn curl_multiple_commands_convert() {
    let input = "curl http://localhost:6333/collections/docs/points/count -d '{\"exact\": true}'\ncurl http://localhost:6333/collections/docs/points/count -d '{\"exact\": false}'";
    let statements = convert(input, None).expect("curl converts");
    assert_eq!(statements.len(), 2);

    let input = "curl http://localhost:6333/collections; curl http://localhost:6333/quotas";
    let statements = convert(input, None).expect("curl converts");
    assert_eq!(statements, ["SHOW COLLECTIONS", "SHOW QUOTAS"]);
}

#[test]
fn curl_file_body_fails_closed() {
    let input = "curl -X POST http://localhost:6333/collections/docs/points/count -d @body.json";
    let error = convert(input, None).expect_err("@file must fail");
    assert!(
        matches!(error, ConvertError::UndecodableBody { .. }),
        "got {error:?}"
    );
}

#[test]
fn curl_unresolved_variable_fails_closed() {
    let input = "curl -X POST $QDRANT_URL/collections/docs/points/count -d '{\"exact\": true}'";
    let error = convert(input, None).expect_err("$VAR must fail");
    assert!(
        matches!(error, ConvertError::UndecodableBody { .. }),
        "got {error:?}"
    );
}

#[test]
fn curl_placeholder_collection_fails_closed() {
    let input = "curl -X POST http://localhost:6333/collections/<your-collection>/points/count -d '{\"exact\": true}'";
    let error = convert(input, None).expect_err("placeholder must fail");
    assert!(
        matches!(error, ConvertError::InvalidField { .. }),
        "got {error:?}"
    );
}

#[test]
fn fenced_snippet_converts() {
    let input = "```http\nPOST /collections/docs/points/count\n{\"exact\": true}\n```";
    let statements = convert(input, None).expect("fenced snippet converts");
    assert_eq!(statements, ["COUNT FROM docs WITH (exact = true)"]);
}

// ── existing behavior is preserved ───────────────────────────────────────────

#[test]
fn bare_body_still_needs_collection() {
    let error = convert("{\"ids\": [1]}", None).expect_err("must fail");
    assert!(matches!(error, ConvertError::MissingCollection));
}

#[test]
fn single_line_garbage_still_invalid_json() {
    let error = convert("{oops", None).expect_err("must fail");
    assert!(matches!(error, ConvertError::InvalidJson(_)));
}
