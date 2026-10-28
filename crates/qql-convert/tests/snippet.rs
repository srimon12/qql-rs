//! HTTP-snippet and curl input shapes — pasting from docs/blogs/curl history.
//!
//! Docs show `METHOD /path` plus a JSON body (or a full `curl` command), not
//! the wrapped `{"method","path","body"}` envelope the recorder writes. These
//! tests pin that both paste shapes convert to the same QQL as the envelope.

mod common;

use common::assert_canonical;
use qql_convert::{ConvertError, ConvertOptions, convert, convert_with};

fn var_pairs(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

fn convert_files(input: &str, files: &[(&str, &str)]) -> Result<Vec<String>, ConvertError> {
    let read = |path: &str| {
        files
            .iter()
            .find(|(name, _)| *name == path)
            .map(|(_, body)| (*body).to_string())
            .ok_or_else(|| format!("{path}: no such file"))
    };
    convert_with(input, None, &ConvertOptions::new().with_read_file(&read))
}

fn convert_vars(input: &str, vars: &[(String, String)]) -> Result<Vec<String>, ConvertError> {
    convert_with(input, None, &ConvertOptions::new().with_vars(vars))
}

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
        matches!(&error, ConvertError::UndecodableBody { detail } if detail.contains("paste the JSON body inline")),
        "got {error:?}"
    );
}

#[test]
fn curl_data_flags_resolve_file_bodies() {
    for flag in ["-d", "--data", "--data-ascii", "--data-binary", "--json"] {
        let input = format!(
            "curl -X POST http://localhost:6333/collections/docs/points/count {flag} @body.json"
        );
        let statements = convert_files(&input, &[("body.json", r#"{"exact": true}"#)]).expect(flag);
        assert_eq!(
            statements,
            ["COUNT FROM docs WITH (exact = true)"],
            "{flag}"
        );
    }
}

#[test]
fn curl_attached_short_data_resolves_file_body() {
    let input = "curl -X POST http://localhost:6333/collections/docs/points/count -d@body.json";
    let statements =
        convert_files(input, &[("body.json", r#"{"exact": true}"#)]).expect("attached -d@file");
    assert_eq!(statements, ["COUNT FROM docs WITH (exact = true)"]);
}

#[test]
fn curl_data_raw_keeps_at_literal() {
    let read = |_: &str| -> Result<String, String> { panic!("--data-raw must never read a file") };
    let input =
        "curl -X POST http://localhost:6333/collections/docs/points/count --data-raw @literal";
    let error = convert_with(input, None, &ConvertOptions::new().with_read_file(&read))
        .expect_err("@literal is not JSON");
    assert!(
        matches!(error, ConvertError::InvalidJson(_)),
        "got {error:?}"
    );
}

#[test]
fn curl_stdin_body_fails_closed() {
    let input = "curl -X POST http://localhost:6333/collections/docs/points/count -d @-";
    let error = convert_files(input, &[]).expect_err("@- must fail");
    match &error {
        ConvertError::UndecodableBody { detail } => {
            assert!(detail.contains("stdin"), "got {detail}");
        }
        other => panic!("expected UndecodableBody, got {other:?}"),
    }
}

#[test]
fn curl_missing_file_error_names_the_path() {
    let input =
        "curl -X POST http://localhost:6333/collections/docs/points/count -d @bodies/missing.json";
    let error = convert_files(input, &[]).expect_err("missing file must fail");
    match &error {
        ConvertError::UndecodableBody { detail } => {
            assert!(detail.contains("bodies/missing.json"), "got {detail}");
        }
        other => panic!("expected UndecodableBody, got {other:?}"),
    }
}

#[test]
fn curl_multiline_continuation_resolves_quoted_path_with_spaces() {
    let input = "curl -X POST \\\n  http://localhost:6333/collections/docs/points/count \\\n  -H 'Content-Type: application/json' \\\n  -d @'body file.json'";
    let statements = convert_files(input, &[("body file.json", r#"{"exact": true}"#)])
        .expect("quoted path with spaces");
    assert_eq!(statements, ["COUNT FROM docs WITH (exact = true)"]);
}

#[test]
fn curl_data_urlencode_still_rejected() {
    let input = "curl -X POST http://localhost:6333/collections/docs/points/count --data-urlencode 'exact=true'";
    let error = convert_files(input, &[]).expect_err("--data-urlencode must fail");
    match &error {
        ConvertError::UndecodableBody { detail } => {
            assert!(detail.contains("no QQL mapping"), "got {detail}");
        }
        other => panic!("expected UndecodableBody, got {other:?}"),
    }
}

#[test]
fn vars_expand_dollar_braced_and_angle_forms() {
    let input = "curl -X POST http://localhost:6333/collections/<COLLECTION>/points/count -d '{\"filter\": {\"must\": [{\"key\": \"tenant\", \"match\": {\"value\": \"${TENANT}\"}}, {\"key\": \"id\", \"match\": {\"value\": $ID}}]}}'";
    let vars = var_pairs(&[("COLLECTION", "docs"), ("TENANT", "acme"), ("ID", "7")]);
    let statements = convert_vars(input, &vars).expect("vars expand");
    assert_eq!(
        statements,
        ["COUNT FROM docs WHERE tenant = 'acme' AND id = 7"]
    );
}

#[test]
fn missing_vars_list_every_unresolved_name() {
    let input = "curl http://localhost:6333/collections/$COLL/points/count -d '{\"a\": \"${TENANT}\", \"b\": \"$ID\", \"c\": \"<WAREHOUSE>\"}'";
    let vars = var_pairs(&[("OTHER", "unused")]);
    let error = convert_vars(input, &vars).expect_err("missing vars fail");
    match &error {
        ConvertError::UnresolvedVariables { names } => {
            assert_eq!(names, &["COLL", "ID", "TENANT", "WAREHOUSE"]);
        }
        other => panic!("expected UnresolvedVariables, got {other:?}"),
    }
    assert_eq!(
        error.to_string(),
        "unresolved variables: COLL, ID, TENANT, WAREHOUSE (pass --var NAME=value)"
    );
}

#[test]
fn non_identifier_angle_placeholder_keeps_the_collection_error() {
    let input = "curl -X POST http://localhost:6333/collections/<your-collection>/points/count -d '{\"exact\": true}'";
    let vars = var_pairs(&[("OTHER", "unused")]);
    let error = convert_vars(input, &vars).expect_err("placeholder must fail");
    assert!(
        matches!(&error, ConvertError::InvalidField { path, .. } if path == "collection"),
        "got {error:?}"
    );
}

#[test]
fn unsafe_var_value_is_rejected_before_parsing() {
    let input = "curl -X POST http://localhost:6333/collections/docs/points/count -d '{\"filter\": {\"must\": [{\"key\": \"tenant\", \"match\": {\"value\": \"$TENANT\"}}]}}'";
    let vars = var_pairs(&[("TENANT", "ac\"me")]);
    let error = convert_vars(input, &vars).expect_err("quote must fail");
    match &error {
        ConvertError::UnsafeVariableValue { name, reason } => {
            assert_eq!(name, "TENANT");
            assert!(reason.contains('"'), "got {reason}");
        }
        other => panic!("expected UnsafeVariableValue, got {other:?}"),
    }
    assert!(
        error.to_string().starts_with("--var TENANT:"),
        "got {error}"
    );
}

#[test]
fn var_value_cannot_inject_a_second_filter() {
    let input = "curl -X POST http://localhost:6333/collections/docs/points/count -d '{\"filter\": {\"must\": [{\"key\": \"tenant\", \"match\": {\"value\": \"$TENANT\"}}]}}'";
    let vars = var_pairs(&[(
        "TENANT",
        "acme\"}},{\"key\":\"role\",\"match\":{\"value\":\"admin",
    )]);
    let error = convert_vars(input, &vars).expect_err("injection must fail");
    assert!(
        matches!(error, ConvertError::UnsafeVariableValue { ref name, .. } if name == "TENANT"),
        "got {error:?}"
    );
}

#[test]
fn var_value_cannot_inject_curl_flags_or_paths() {
    let input =
        "curl -X POST http://localhost:6333/collections/$COLL/points/count -d '{\"exact\": true}'";
    let vars = var_pairs(&[("COLL", "docs -d @/etc/hostname")]);
    let error = convert_vars(input, &vars).expect_err("flags must fail");
    assert!(
        matches!(error, ConvertError::UnsafeVariableValue { ref name, .. } if name == "COLL"),
        "got {error:?}"
    );
}

#[test]
fn var_value_cannot_start_a_flag_or_at() {
    for value in ["@body.json", "-s"] {
        let vars = var_pairs(&[("V", value)]);
        let error = convert_vars(
            "curl http://localhost:6333/collections/docs/points/count -d $V",
            &vars,
        )
        .expect_err("leading flag/file marker must fail");
        assert!(
            matches!(error, ConvertError::UnsafeVariableValue { ref name, .. } if name == "V"),
            "got {error:?}"
        );
    }
}

#[test]
fn var_value_cannot_be_empty_or_a_dot_segment() {
    for value in ["", ".."] {
        let vars = var_pairs(&[("V", value)]);
        let error = convert_vars(
            "curl http://localhost:6333/collections/$V/points/count -d '{\"exact\": true}'",
            &vars,
        )
        .expect_err("empty/dot value must fail");
        assert!(
            matches!(error, ConvertError::UnsafeVariableValue { ref name, .. } if name == "V"),
            "got {error:?}"
        );
    }
}

#[test]
fn html_angle_tag_passes_through_unresolved() {
    let input = "curl -X PUT http://localhost:6333/collections/docs/points -d '{\"points\":[{\"id\":1,\"vector\":[0.1,0.2],\"payload\":{\"html\":\"<b>x</b>\"}}]}'";
    let vars = var_pairs(&[("OTHER", "unused")]);
    let statements = convert_vars(input, &vars).expect("<b> is not a variable");
    assert!(statements[0].contains("<b>x</b>"), "got {statements:?}");
}

#[test]
fn double_dollar_escapes_a_literal_dollar() {
    let input = "curl -X PUT http://localhost:6333/collections/docs/points -d '{\"points\":[{\"id\":1,\"vector\":[0.1,0.2],\"payload\":{\"price\":\"costs $$USD\"}}]}'";
    let vars = var_pairs(&[("OTHER", "unused")]);
    let statements = convert_vars(input, &vars).expect("$$ escapes the dollar");
    assert!(statements[0].contains("costs $USD"), "got {statements:?}");
}

#[test]
fn unknown_dollar_variable_still_fails_closed() {
    let input = "curl -X PUT http://localhost:6333/collections/docs/points -d '{\"points\":[{\"id\":1,\"vector\":[0.1,0.2],\"payload\":{\"price\":\"costs $USD\"}}]}'";
    let vars = var_pairs(&[("OTHER", "unused")]);
    let error = convert_vars(input, &vars).expect_err("$USD must fail without --var");
    assert!(
        matches!(error, ConvertError::UnresolvedVariables { ref names } if names == &["USD".to_string()]),
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
