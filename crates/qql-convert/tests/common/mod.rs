//! Shared helpers for the converter integration tests.
//!
//! Not every test binary uses every helper; the allow keeps `-D warnings`
//! green for the shared module.
#![allow(dead_code)]

use qql_convert::{ConvertError, convert as convert_json};
use qql_core::fmt::format_stmt;
use qql_core::parser::Parser;

pub(crate) fn convert_with(input: &str, collection: &str) -> Vec<String> {
    let stmts = convert_json(input, Some(collection))
        .unwrap_or_else(|e| panic!("conversion failed for {input}: {e}"));
    assert_canonical(&stmts, input);
    stmts
}

pub(crate) fn convert(input: &str) -> Vec<String> {
    convert_with(input, "docs")
}

pub(crate) fn assert_canonical(stmts: &[String], input: &str) {
    assert!(
        !stmts.is_empty(),
        "conversion produced no statements for {input}"
    );
    for stmt in stmts {
        let parsed = Parser::parse(&format!("{stmt};"))
            .unwrap_or_else(|e| panic!("emitted statement failed to parse: {stmt} ({e})"));
        let again = format_stmt(&parsed);
        assert_eq!(&again, stmt, "not canonical for input {input}");
    }
}

pub(crate) fn wrapped(method: &str, path: &str, body: serde_json::Value) -> String {
    serde_json::json!({"method": method, "path": path, "body": body}).to_string()
}

pub(crate) fn wrapped_query(
    method: &str,
    path: &str,
    query: serde_json::Value,
    body: serde_json::Value,
) -> String {
    serde_json::json!({"method": method, "path": path, "query": query, "body": body}).to_string()
}

/// Canonical form of an expected QQL statement.
pub(crate) fn canon(source: &str) -> String {
    let parsed =
        Parser::parse(source).unwrap_or_else(|e| panic!("expected QQL must parse: {source} ({e})"));
    format_stmt(&parsed)
}

/// Assert the converter emits exactly the canonical form of `expected`.
pub(crate) fn expect_one(input: &str, expected: &str) {
    assert_eq!(convert(input), [canon(expected)], "input: {input}");
}

pub(crate) fn convert_err(input: &str) -> ConvertError {
    convert_json(input, Some("docs")).expect_err("expected a typed error")
}
