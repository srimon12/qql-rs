//! Basic → Medium (Rust / qql-core)
//!
//! Offline showcase of parse, inject_filter, inject_shard_key, and explain.
//! No network I/O — pure language layer.

use qql_core::ast::{self, ComparisonOp, Value};
use qql_core::parser::Parser;

fn main() {
    let q = "QUERY TEXT 'machine learning transformer' FROM papers USING dense LIMIT 20";

    // ── Parse ───────────────────────────────────────────────────────
    let mut stmt = Parser::parse(q).expect("valid QQL");
    println!("=== parse OK ===\n{stmt:#?}\n");

    // ── inject_filter (string / numeric / bool) ─────────────────────
    ast::inject_filter(
        &mut stmt,
        "tenant_id",
        ComparisonOp::Eq,
        Value::Str("acme-corp".into()),
    )
    .unwrap();
    println!("=== after inject_filter(tenant_id = acme-corp) ===");
    println!("{stmt:#?}\n");

    let mut stmt = Parser::parse(q).unwrap();
    ast::inject_filter(&mut stmt, "impact_factor", ComparisonOp::Gte, Value::Float(5.0)).unwrap();
    println!("=== after inject_filter(impact_factor >= 5.0) ===");
    println!("{stmt:#?}\n");

    let mut stmt = Parser::parse(q).unwrap();
    ast::inject_filter(&mut stmt, "is_published", ComparisonOp::Eq, Value::Bool(true)).unwrap();
    println!("=== after inject_filter(is_published = true) ===");
    println!("{stmt:#?}\n");

    // ── inject_shard_key ────────────────────────────────────────────
    let mut stmt = Parser::parse(q).unwrap();
    ast::inject_filter(
        &mut stmt,
        "tenant_id",
        ComparisonOp::Eq,
        Value::Str("acme-corp".into()),
    )
    .unwrap();
    ast::inject_shard_key(&mut stmt, "acme-corp").unwrap();
    println!("=== after inject_shard_key('acme-corp') ===");
    println!("shard_key = {:?}\n", stmt.shard_key());

    // ── Hybrid shorthand (QQL 1.2) ──────────────────────────────────
    let hybrid = r#"
        QUERY TEXT 'vector database latency'
        FROM papers
        USING HYBRID DENSE dense SPARSE sparse FUSION RRF
        LIMIT 10
    "#;
    let hybrid_stmt = Parser::parse(hybrid).expect("hybrid shorthand");
    println!("=== USING HYBRID shorthand ===");
    println!("{hybrid_stmt:#?}");
}
