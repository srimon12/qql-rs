# Rust SDK (`qql-core`, `qql-plan`, `qql`) Reference & Examples

Three crates, three responsibilities. Use only what you need.

## Dependencies

```toml
[dependencies]
qql-core = "0.1"    # parser + inject_filter (no I/O, no networking)
qql-plan = "0.1"    # AST → typed Route { method, path, body }
qql = "0.1"         # runtime executor (REST, gRPC, embedding)
tokio = { version = "1", features = ["full"] }
serde_json = "1"
```

---

## 1. Multi-Tenant Filter Injection + Route Compilation

Parse a user query, inject tenant isolation, lower to a typed REST route — zero network I/O.

```rust
use qql_core::parser::Parser;
use qql_core::ast::{self, ComparisonOp, Value};
use qql_plan::routing::route;

fn tenant_route(user_query: &str, tenant: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut stmt = Parser::parse(user_query)?;

    // Inject tenant filter — recurses into CTEs and prefetches
    ast::inject_filter(&mut stmt, "tenant_id", ComparisonOp::Eq,
                       Value::Str(tenant.to_string()))?;

    // Lower to typed REST route (no Qdrant connection needed)
    let r = route(&stmt);
    assert_eq!(r.method.as_str(), "POST");

    Ok(())
}
```

---

## 2. Execute with REST Client

Full runtime: parse, optionally resolve embeddings, execute against Qdrant.

```rust
use qql::executor::Executor;
use qql::rest::RestQdrant;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ops = Box::new(RestQdrant::new("http://localhost:6333", None));
    let exec = Executor::new(ops, None);

    exec.execute("QUERY 'supply chain risks' FROM sec10k SHARD 'honeywell' LIMIT 10").await?;

    Ok(())
}
```

---

## 3. Schema-as-Code

Parse a `.qql` file and execute each statement — infrastructure defined in a version-controlled, language-agnostic format.

```rust
use qql_core::parser::Parser;
use qql::executor::Executor;
use qql::rest::RestQdrant;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ops = Box::new(RestQdrant::new("http://localhost:6333", None));
    let exec = Executor::new(ops, None);

    let schema = r#"
        CREATE COLLECTION docs HYBRID (dense VECTOR(768, COSINE), sparse SPARSE)
          WITH HNSW (m = 16)
          WITH PARAMS (replication_factor = 3, shard_number = 4);

        CREATE INDEX ON COLLECTION docs FOR title TYPE text;
        CREATE INDEX ON COLLECTION docs FOR tenant_id TYPE keyword WITH (is_tenant = true);
    "#;

    for stmt in Parser::parse_all(schema)? {
        exec.execute_node(stmt).await?;
    }

    Ok(())
}
```
