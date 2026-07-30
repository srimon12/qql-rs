//! Medium → Expert (Rust / qql-core)
//!
//! Multi-tenant query gateway: inject_filter + inject_shard_key at one call site.
//! Viewers get an extra status filter; admins do not.
//!
//! Offline only — no Qdrant required.

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

    // Logical isolation — always
    ast::inject_filter(
        &mut stmt,
        "tenant_id",
        ComparisonOp::Eq,
        Value::Str(ctx.tenant.into()),
    )
    .unwrap();

    // Physical shard routing — always
    ast::inject_shard_key(&mut stmt, ctx.tenant).unwrap();

    // Role gate — viewers only see published docs
    // Note: inject_filter does not support != ; use positive equality.
    if ctx.role == "viewer" {
        ast::inject_filter(
            &mut stmt,
            "status",
            ComparisonOp::Eq,
            Value::Str("published".into()),
        )
        .unwrap();
    }

    format!(
        "shard={:?}  stmt={:#?}",
        stmt.shard_key(),
        stmt
    )
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
            r#"
                QUERY TEXT 'engineering docs'
                FROM docs
                USING HYBRID DENSE dense SPARSE sparse FUSION RRF
                LIMIT 5
            "#,
        ),
    ];

    println!("=== QQL Multi-Tenant Query Gateway ===\n");
    for (user, raw) in &requests {
        let safe = enforce(user, raw);
        println!("user: {user}");
        println!("  raw:  {}", raw.trim());
        // Truncate debug dump for readability
        let preview: String = safe.chars().take(180).collect();
        println!("  safe: {preview}…\n");
    }

    // Fail-closed demo: DDL rejects inject_shard_key
    let mut ddl = Parser::parse("SHOW COLLECTIONS").unwrap();
    match ast::inject_shard_key(&mut ddl, "acme") {
        Ok(()) => println!("unexpected: DDL accepted shard key"),
        Err(e) => println!("fail-closed on DDL: {e}"),
    }
}
