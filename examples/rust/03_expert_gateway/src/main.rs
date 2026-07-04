use qql_core::ast::{self, Value};
use qql_core::parser::Parser;
use std::borrow::Cow;

struct UserCtx {
    tenant: &'static str,
    role: &'static str,
}

fn enforce(user: &str, query: &str) -> Result<String, String> {
    let users = std::collections::HashMap::from([
        ("alice", UserCtx { tenant: "acme", role: "admin" }),
        ("bob", UserCtx { tenant: "acme", role: "viewer" }),
        ("charlie", UserCtx { tenant: "globex", role: "viewer" }),
    ]);

    let ctx = users.get(user).ok_or_else(|| format!("unknown user: {}", user))?;
    Parser::parse(query).map_err(|e| e.to_string())?;

    let mut stmt = Parser::parse(query).map_err(|e| e.to_string())?;
    ast::inject_filter(&mut stmt, "tenant_id", "=", &Value::Str(Cow::Owned(ctx.tenant.to_string())));

    // For viewer role, also hide confidential records
    if ctx.role == "viewer" {
        ast::inject_filter(&mut stmt, "status", "!=", &Value::Str(Cow::Borrowed("confidential")));
    }

    Ok(format!("{:#?}", stmt))
}

fn main() {
    let requests = [
        ("alice", "QUERY 'sales data' FROM analytics LIMIT 10"),
        ("bob", "QUERY 'sales data' FROM analytics LIMIT 10"),
        ("charlie", "QUERY 'engineering docs' FROM docs LIMIT 5"),
    ];

    println!("=== QQL Query Gateway (Rust) ===");
    for (user, raw) in &requests {
        let safe = enforce(user, raw).unwrap();
        println!("\n  raw:  {}", raw);
        println!("  safe: {}...", &safe[..safe.len().min(130)]);
    }
    println!("\n  → Rust SDK allows chaining multiple inject_filter calls");
    println!("    because it operates on the AST, not string round-trips.");
}
