//! JSONL capture conversion — the `qql record --out` replay path.

mod common;

use common::{assert_canonical, wrapped};
use qql_convert::{ConvertError, convert};
use serde_json::json;

#[test]
fn two_line_capture_converts_every_line() {
    let capture = format!(
        "{}\n{}\n",
        wrapped(
            "POST",
            "/collections/docs/points/query",
            json!({"query": {"nearest": [0.1, 0.2]}, "using": "dense", "limit": 5})
        ),
        wrapped(
            "POST",
            "/collections/docs/points/count",
            json!({"exact": true})
        ),
    );

    let statements = convert(&capture, Some("docs")).expect("capture converts");
    assert_canonical(&statements, &capture);
    assert_eq!(statements.len(), 2);
    assert!(
        statements[0].starts_with("QUERY [0.1, 0.2]"),
        "{statements:?}"
    );
    assert_eq!(statements[1], "COUNT FROM docs WITH (exact = true)");
}

#[test]
fn blank_lines_are_skipped() {
    let capture = format!(
        "\n{}\n\n{}\n\n",
        wrapped(
            "POST",
            "/collections/docs/points/scroll",
            json!({"limit": 10})
        ),
        wrapped(
            "POST",
            "/collections/docs/points/count",
            json!({"exact": false})
        ),
    );

    let statements = convert(&capture, Some("docs")).expect("capture converts");
    assert_eq!(statements.len(), 2);
}

#[test]
fn bare_lines_use_the_fallback_collection() {
    let capture = "{\"ids\": [1]}\n{\"ids\": [2]}\n";
    let statements = convert(capture, Some("docs")).expect("capture converts");
    assert_eq!(
        statements,
        ["QUERY POINTS (1) FROM docs", "QUERY POINTS (2) FROM docs"]
    );
}

#[test]
fn failing_line_reports_its_number() {
    let capture = format!(
        "{}\n{}\n",
        wrapped(
            "POST",
            "/collections/docs/points/count",
            json!({"exact": true})
        ),
        wrapped("POST", "/collections/docs/aliases", json!({})),
    );

    let error = convert(&capture, Some("docs")).expect_err("line 2 must fail");
    match &error {
        ConvertError::InvalidLine { line, source } => {
            assert_eq!(*line, 2);
            assert!(matches!(**source, ConvertError::UnsupportedEndpoint(_)));
        }
        other => panic!("expected InvalidLine, got {other:?}"),
    }
    assert!(error.to_string().starts_with("line 2: "), "{error}");
}

#[test]
fn empty_capture_is_a_typed_error() {
    let error = convert("\n  \n", None).expect_err("empty capture must fail");
    assert!(matches!(error, ConvertError::UndecodableBody { .. }));
}
