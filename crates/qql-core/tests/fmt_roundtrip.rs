//! Formatter round-trip property tests over the conformance fixture corpus.
//!
//! For every valid fixture the formatter must satisfy:
//! 1. `parse(format(fixture))` produces the identical AST as `parse(fixture)`.
//! 2. `format(format(fixture)) == format(fixture)` (idempotent / stable).

use qql_core::ast::{PrefetchSource, QueryExpr, QueryStmt, Stmt};
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
        let mut expected = Parser::parse_all(source)
            .unwrap_or_else(|e| panic!("fixture {} failed to parse: {e}", name));
        let formatted = fmt::format(source)
            .unwrap_or_else(|e| panic!("fixture {} failed to format: {e}", name));
        let mut reparsed = Parser::parse_all(&formatted).unwrap_or_else(|e| {
            panic!(
                "formatted {} failed to re-parse: {e}\n---\n{}",
                name, formatted
            )
        });
        // Source locations shift whenever the formatter rewrites preceding
        // text, so they are not part of round-trip semantics: strip them
        // before comparing (mirrors `transform`'s CTE/prefetch recursion).
        for stmts in [&mut expected, &mut reparsed] {
            for stmt in stmts {
                strip_spans(stmt);
            }
        }
        assert_eq!(reparsed, expected, "AST mismatch for {name}");
    }
}

/// Clear stored source spans recursively (see `strip_spans` rationale above).
fn strip_spans(stmt: &mut Stmt) {
    match stmt {
        Stmt::Query(q) => strip_query_spans(q),
        Stmt::Batch(batch) => {
            for member in &mut batch.statements {
                strip_spans(member);
            }
        }
        _ => {}
    }
}

fn strip_query_spans(q: &mut QueryStmt) {
    q.collection_span = None;
    if let Some(group) = q.group.as_mut() {
        group.field_span = None;
    }
    for cte in &mut q.ctes {
        strip_query_spans(&mut cte.query);
    }
    // Mirrors `transform`'s prefetch recursion; deliberately exhaustive so a
    // new prefetch-carrying variant fails to compile here until covered.
    let prefetch = match &mut q.expression {
        QueryExpr::Nearest { prefetch, .. }
        | QueryExpr::Recommend { prefetch, .. }
        | QueryExpr::Context { prefetch, .. }
        | QueryExpr::Discover { prefetch, .. }
        | QueryExpr::Fusion { prefetch, .. }
        | QueryExpr::Formula { prefetch, .. }
        | QueryExpr::RelevanceFeedback { prefetch, .. }
        | QueryExpr::Rerank { prefetch, .. }
        | QueryExpr::CrossRerank { prefetch, .. } => Some(prefetch),
        QueryExpr::Points { .. }
        | QueryExpr::OrderBy { .. }
        | QueryExpr::SampleRandom
        | QueryExpr::Hybrid { .. } => None,
    };
    if let Some(prefetches) = prefetch {
        for stage in prefetches {
            if let PrefetchSource::Query(nested) = &mut stage.source {
                strip_query_spans(nested);
            }
        }
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
