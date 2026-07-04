use std::borrow::Cow;
use qql_core::ast::{self, Value};
use qql_core::parser::Parser;

struct User { tenant: &'static str, role: &'static str }

fn enforce(user: &str, query: &str) -> String {
    let users = std::collections::HashMap::from([
        ("alice", User { tenant: "acme", role: "admin" }),
        ("bob",   User { tenant: "acme", role: "viewer" }),
        ("charlie", User { tenant: "globex", role: "viewer" }),
    ]);
    let ctx = users.get(user).unwrap();
    let mut stmt = Parser::parse(query).unwrap();
    ast::inject_filter(&mut stmt, "tenant_id", "=", &Value::Str(Cow::Owned(ctx.tenant.to_string())));
    if ctx.role == "viewer" {
        ast::inject_filter(&mut stmt, "status", "!=", &Value::Str(Cow::Borrowed("confidential")));
    }
    format!("{:#?}", stmt)
}

fn main() {
    let requests = [
        ("alice", "QUERY 'sales data' FROM analytics LIMIT 10"),
        ("bob", "QUERY 'sales data' FROM analytics LIMIT 10"),
        ("charlie", "QUERY 'engineering docs' FROM docs LIMIT 5"),
    ];
    println!("=== QQL Query Gateway ===");
    for (user, raw) in &requests {
        let safe = enforce(user, raw);
        println!("\n  raw:  {}", raw);
        println!("  safe: {}...", &safe[..safe.len().min(130)]);
    }
}
