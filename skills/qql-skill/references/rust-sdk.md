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

## 3. Batch Execution

`execute_batch` and `execute_batch_nodes` execute multiple queries. Same-collection
QUERY statements are automatically grouped into a single network call.

```rust
use qql_core::parser::Parser;
use qql::executor::Executor;
use qql::rest::RestQdrant;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ops = Box::new(RestQdrant::new("http://localhost:6333", None));
    let exec = Executor::new(ops, None);

    // Batch from strings
    let results = exec.execute_batch(&[
        "QUERY 'a' FROM docs USING dense LIMIT 10",
        "QUERY 'b' FROM docs USING dense LIMIT 10",
        "QUERY 'c' FROM docs USING dense LIMIT 10",
    ], true).await?;
    // → 3 queries, 1 network call (auto-grouped by collection)

    // Batch from pre-parsed Stmts
    let stmts = Parser::parse_all("Q1; Q2; Q3;")?;
    let results = exec.execute_batch_nodes(stmts, true).await?;

    Ok(())
}
```

---

## 4. Batch Route Compilation (no I/O)

`route_query_batch` groups query statements by collection and produces batch
request payloads — useful for offline compilation or proxy layers.

```rust
use qql_core::parser::Parser;
use qql_plan::routing::route_query_batch;

let stmts = Parser::parse_all(
    "QUERY 'a' FROM docs USING dense LIMIT 1;\
     QUERY 'b' FROM docs USING dense LIMIT 1;\
     QUERY 'c' FROM docs USING dense LIMIT 1;"
)?;

let stmt_refs: Vec<_> = stmts.iter().filter_map(|s| match s {
    qql_core::ast::Stmt::Query(q) => Some(&**q),
    _ => None,
}).collect();

let batches = route_query_batch(&stmt_refs);
for (collection, batch) in batches {
    println!("{} → {} searches batched", collection, batch.searches.len());
    // → "docs → 3 searches batched"
}
```

---

## 5. Schema-as-Code

`execute()` auto-detects semicolons — one call to deploy a complete schema.
Same-collection QUERY statements are automatically batch-grouped.

```rust
use qql::executor::Executor;
use qql::rest::RestQdrant;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ops = Box::new(RestQdrant::new("http://localhost:6333", None));
    let exec = Executor::new(ops, None);

    // Multi-statement string — auto-detected, batch-executed
    exec.execute(r#"
        CREATE COLLECTION docs HYBRID (dense VECTOR(768, COSINE), sparse SPARSE)
          WITH HNSW (m = 16)
          WITH PARAMS (replication_factor = 3, shard_number = 4);

        CREATE INDEX ON COLLECTION docs FOR title TYPE text;
        CREATE INDEX ON COLLECTION docs FOR tenant_id TYPE keyword WITH (is_tenant = true);
        CREATE SHARD KEY 'acme' ON COLLECTION docs WITH (shards_number = 2);
    "#).await?;

    Ok(())
}
```

For programmatic manipulation (inspect before executing), use `parse_all` + `execute_batch_nodes`:

```rust
use qql_core::parser::Parser;

let stmts = Parser::parse_all(r#"
    QUERY 'a' FROM docs USING dense LIMIT 1;
    QUERY 'b' FROM docs USING dense LIMIT 1;
"#)?;

// Inspect, inject filters, set shard keys...
// exec.execute_batch_nodes(stmts, true).await?;
```
