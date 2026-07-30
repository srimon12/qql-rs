//! Medium → Expert (Rust) — multi-tenant gateway.
//! inject_filter always; SHARD in QQL or set_shard_key after parse.

use qql_core::ast::{self, ComparisonOp, Value};
use qql_core::parser::Parser;

struct User {
    tenant: &'static str,
    role: &'static str,
}

fn enforce(user: &str, query: &str) -> String {
    let users = std::collections::HashMap::from([
        ("alice", User { tenant: "acme", role: "admin" }),
        ("bob", User { tenant: "acme", role: "viewer" }),
        ("charlie", User { tenant: "globex", role: "viewer" }),
    ]);
    let ctx = users.get(user).expect("known user");

    let mut stmt = Parser::parse(query).expect("valid QQL");
    ast::inject_filter(
        &mut stmt,
        "tenant_id",
        ComparisonOp::Eq,
        Value::Str(ctx.tenant.into()),
    )
    .unwrap();
    // Physical routing — same field as QQL `SHARD '…'`
    assert!(stmt.set_shard_key(Some(ctx.tenant.into())));

    if ctx.role == "viewer" {
        ast::inject_filter(
            &mut stmt,
            "status",
            ComparisonOp::Eq,
            Value::Str("published".into()),
        )
        .unwrap();
    }

    format!("shard={:?}  stmt={:#?}", stmt.shard_key(), stmt)
}

fn main() {
    let requests = [
        (
            "alice",
            "QUERY TEXT 'sales data' FROM analytics USING dense LIMIT 10",
        ),
        (
            "bob",
            "QUERY TEXT 'sales data' FROM analytics USING dense LIMIT 10",
        ),
        (
            "charlie",
            // SHARD written in the language when tenant is known up front
            "QUERY TEXT 'engineering docs' FROM docs \
             USING HYBRID DENSE dense SPARSE sparse FUSION RRF \
             SHARD 'globex' LIMIT 5",
        ),
    ];

    println!("=== QQL Multi-Tenant Query Gateway ===\n");
    for (user, raw) in &requests {
        let safe = enforce(user, raw);
        println!("user: {user}");
        println!("  raw:  {}", raw.trim());
        let preview: String = safe.chars().take(180).collect();
        println!("  safe: {preview}…\n");
    }

    // DDL has no request-level shard routing
    let mut ddl = Parser::parse("SHOW COLLECTIONS").unwrap();
    assert!(!ddl.set_shard_key(Some("acme".into())));
    println!("DDL set_shard_key → false (expected)");
}
