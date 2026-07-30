//! Basic → Medium (Rust / qql-core)
//! Offline: parse, inject_filter, SHARD clause, set_shard_key.

use qql_core::ast::{self, ComparisonOp, Value};
use qql_core::parser::Parser;

fn main() {
    let q = "QUERY TEXT 'machine learning transformer' FROM papers USING dense LIMIT 20";

    let mut stmt = Parser::parse(q).expect("valid QQL");
    println!("=== parse OK ===\n{stmt:#?}\n");

    ast::inject_filter(
        &mut stmt,
        "tenant_id",
        ComparisonOp::Eq,
        Value::Str("acme-corp".into()),
    )
    .unwrap();
    println!("=== after inject_filter(tenant_id) ===\n{stmt:#?}\n");

    // Preferred: SHARD in the language
    let with_shard = Parser::parse(
        "QUERY TEXT 'vector database latency' FROM papers \
         USING HYBRID DENSE dense SPARSE sparse FUSION RRF \
         SHARD 'acme-corp' LIMIT 10",
    )
    .unwrap();
    println!("=== SHARD in QQL → shard_key = {:?} ===\n", with_shard.shard_key());

    // Host path after parse
    let mut host = Parser::parse(q).unwrap();
    assert!(host.set_shard_key(Some("acme-corp".into())));
    println!("=== set_shard_key → {:?} ===", host.shard_key());
}
