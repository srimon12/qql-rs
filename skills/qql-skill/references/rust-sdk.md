# Rust SDK (`qql-core`, `qql-plan`, `qql`) Reference & Examples

Three crates, three responsibilities. Use only what you need.

## Dependencies

```toml
[dependencies]
qql-core = "0.1"    # parser + inject_filter (no I/O, no networking)
qql-plan = "0.1"    # AST -> typed Route { method, path, body }
qql = "0.1"         # runtime executor (REST, gRPC, embedding)
tokio = { version = "1", features = ["full"] }
serde_json = "1"
```

## Crate Features

| Feature | Description | Default |
|---------|-------------|---------|
| `rest` | HTTP REST client (reqwest) | yes |
| `grpc` | gRPC client (tonic) | no |
| `edge` | In-process execution via qdrant-edge | no |

---

## 1. Multi-Tenant Filter Injection + Route Compilation

Parse a user query, inject tenant isolation, lower to a typed REST route -- zero network I/O.

```rust
use qql_core::parser::Parser;
use qql_core::ast::{self, ComparisonOp, Value};
use qql_plan::routing::try_route;

fn tenant_route(user_query: &str, tenant: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut stmt = Parser::parse(user_query)?;

    // Inject tenant filter -- recurses into CTEs and prefetches
    ast::inject_filter(&mut stmt, "tenant_id", ComparisonOp::Eq,
                       Value::Str(tenant.to_string()))?;

    // Lower to typed REST route (no Qdrant connection needed)
    let r = try_route(&stmt)?;
    assert_eq!(r.method.as_str(), "POST");

    Ok(())
}
```

---

## 2. Execute with REST or gRPC Client

Full runtime: parse → **schema vector kind resolution** → embeddings → plan →
dispatch. `USING dense` / `USING sparse` / multivector names work without `AS`
when the collection exists; kinds and multivector flags come from schema.
`USING name` alone without schema fails closed (`QQL-VECTOR-KIND`). Multivector
TEXT needs a host `Embedder::embed_multi` (or precomputed `VECTOR [[...]]`).

```rust
use qql::executor::{Executor, OnError};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Convenience constructors -- REST or gRPC with optional API key
    let exec = Executor::rest("http://localhost:6333", Some("my-api-key".into()))?;

    exec.execute(
        "QUERY 'supply chain risks' FROM sec10k SHARD 'honeywell' LIMIT 10",
        OnError::Stop,
    ).await?;

    // Schema-driven sparse + multivector (no AS required when collection exists)
    // exec.execute("QUERY TEXT 'q' FROM docs USING sparse LIMIT 10", OnError::Stop).await?;
    // exec.execute("QUERY TEXT 'q' FROM docs USING colbert LIMIT 10", OnError::Stop).await?;

    Ok(())
}
```

Prefer `Executor::rest()` or `Executor::grpc()` over manual construction. If you need a custom HTTP client, use the four-argument constructor:

```rust
use qql::executor::{Executor, OnError};
use qql::rest::RestQdrant;

let client = RestQdrant::with_client(
    "http://localhost:6333".into(),
    None,
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()?,
);
let exec = Executor::new(Box::new(client), None);
```

`RestQdrant::with_timeout(url, api_key, timeout)` constructs with an explicit duration.

**Request-level params (QQL 1.2+):** `PARAMS (timeout = 30, consistency = majority)`
lower to REST query string / gRPC fields on the request (not body `SearchParams`).
Client builder timeouts remain a separate HTTP-layer budget.

**Routing:** prefer `SHARD 'tenant'` in the query string. After parse, use
`stmt.set_shard_key(Some(tenant.into()))` — there is no `inject_shard_key`.

**Compilation:** use `compile_statement` / `try_route`. The deprecated
`route()` (which panicked on client-side-only ops such as bare `CROSS RERANK`)
has been removed; `try_route` returns `Err` — never panics — for those cases.

---

## 3. Batch Execution

`execute_batch` and `execute_batch_nodes` execute multiple queries. Same-collection QUERY and mutation statements are automatically grouped into a single network call.

```rust
use qql::executor::{Executor, OnError};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let exec = Executor::rest("http://localhost:6333", None)?;

    // Batch from strings
    let results = exec.execute_batch(&[
        "QUERY 'a' FROM docs USING dense LIMIT 10",
        "QUERY 'b' FROM docs USING dense LIMIT 10",
        "QUERY 'c' FROM docs USING dense LIMIT 10",
    ], OnError::Stop).await?;
    // -> 3 queries, 1 network call (auto-grouped by collection)

    // Batch from pre-parsed Stmts
    let stmts = Parser::parse_all("COUNT FROM docs; COUNT FROM sec10k")?;
    let results = exec.execute_batch_nodes(stmts, OnError::Stop).await?;

    Ok(())
}
```

`OnError::Stop` halts on the first failure. `OnError::Continue` collects per-statement errors.

---

## 4. Offline Statement Compilation (no I/O)

`compile_statement` lowers a parsed statement to a typed IR with optional REST route --
useful for offline validation or proxy layers. `try_route` gives a fallible
`Result<Route, _>` suitable for library code.

```rust
use qql_core::parser::Parser;
use qql_plan::{compile_statement, try_route};

let stmt = Parser::parse("QUERY 'a' FROM docs USING dense LIMIT 1;")?;

// Full compilation (stmt_type + optional route)
let compiled = compile_statement(&stmt)?;
println!("type={} method={:?}", compiled.stmt_type, compiled.route.as_ref().map(|r| &r.method));

// Or get just the REST route directly (falls back for client-side ops)
let route = try_route(&stmt)?;
println!("{} {}", route.method.as_str(), route.path);
```

---

## 5. Schema-as-Code

`execute()` auto-detects semicolons -- one call to deploy a complete schema. Same-collection QUERY statements are automatically batch-grouped.

```rust
use qql::executor::Executor;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let exec = Executor::rest("http://localhost:6333", None)?;

    // Multi-statement string -- auto-detected, batch-executed
    exec.execute(r#"
        CREATE COLLECTION docs HYBRID (dense VECTOR(768, COSINE), sparse SPARSE)
          WITH HNSW (m = 16)
          WITH PARAMS (replication_factor = 3, shard_number = 4);

        CREATE INDEX ON COLLECTION docs FOR title TYPE text;
        CREATE INDEX ON COLLECTION docs FOR tenant_id TYPE keyword WITH (is_tenant = true);
        CREATE SHARD KEY 'acme' ON COLLECTION docs WITH (shards_number = 2);
    "#, OnError::Stop).await?;

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
// exec.execute_batch_nodes(stmts, OnError::Stop).await?;
```
