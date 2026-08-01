//! Formatter round-trip property tests over the conformance fixture corpus.
//!
//! For every valid fixture the formatter must satisfy:
//! 1. `parse(format(fixture))` produces the identical AST as `parse(fixture)`.
//! 2. `format(format(fixture)) == format(fixture)` (idempotent / stable).

use qql_core::fmt;
use qql_core::parser::Parser;

fn fixture_dir() -> std::path::PathBuf {
    // `cargo test` runs from the crate root (`crates/qql-core`).
    std::path::Path::new("../../language/v1/fixtures/valid")
        .canonicalize()
        .expect("valid fixture directory should exist")
}

fn fixtures() -> Vec<(String, String)> {
    let mut files: Vec<_> = std::fs::read_dir(fixture_dir())
        .expect("read fixture dir")
        .map(|entry| entry.expect("entry"))
        .collect();
    files.sort_by_key(|e| e.file_name());
    files
        .into_iter()
        .filter(|e| e.path().extension().map(|x| x == "qql").unwrap_or(false))
        .map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let source = std::fs::read_to_string(e.path()).expect("read fixture");
            (name, source)
        })
        .collect()
}

#[test]
fn every_valid_fixture_round_trips() {
    let corpus = fixtures();
    assert!(!corpus.is_empty(), "fixture corpus should not be empty");
    for (name, source) in &corpus {
        let expected = Parser::parse_all(source)
            .unwrap_or_else(|e| panic!("fixture {} failed to parse: {e}", name));
        let formatted = fmt::format(source)
            .unwrap_or_else(|e| panic!("fixture {} failed to format: {e}", name));
        let reparsed = Parser::parse_all(&formatted).unwrap_or_else(|e| {
            panic!(
                "formatted {} failed to re-parse: {e}\n---\n{}",
                name, formatted
            )
        });
        assert_eq!(reparsed, expected, "AST mismatch for {name}");
    }
}

#[test]
fn every_valid_fixture_formats_idempotently() {
    for (name, source) in &fixtures() {
        let once = fmt::format(source)
            .unwrap_or_else(|e| panic!("fixture {} failed to format: {e}", name));
        let twice = fmt::format(&once)
            .unwrap_or_else(|e| panic!("fixture {} failed to re-format: {e}", name));
        assert_eq!(once, twice, "formatting not stable for {name}");
    }
}

#[test]
fn canonical_output_reparses_for_all_fixtures() {
    // The formatter output itself is also a valid fixture corpus.
    for (name, source) in &fixtures() {
        let formatted = fmt::format(source).expect("format");
        assert!(
            Parser::parse_all(&formatted).is_ok(),
            "formatted output for {name} should parse"
        );
    }
}
