# Rust SDK (`qql-core`, `qql-plan`, `qql`) Reference & Examples

Native Rust libraries for parsing (`qql-core`), lowering/routing (`qql-plan`), and execution (`qql`).

## Dependencies

```toml
[dependencies]
qql-core = "0.1"
qql-plan = "0.1"
qql = "0.1"        # package name for qql-runtime
```

## Quick Start & Executor Setup

```rust
use std::sync::Arc;
use qql::executor::Executor;
use qql::rest::RestQdrant;
use qql::grpc::GrpcQdrant;
use qql::embedder::HttpEmbedder;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Connect over REST or gRPC
    let rest_ops = Box::new(RestQdrant::new("http://localhost:6333", None));
    
    // Optional: Connect embedder for automatic text -> vector resolution
    let embedder = Arc::new(HttpEmbedder::new(
        "http://localhost:11434/v1/embeddings".to_string(),
        "".to_string(),
        "all-minilm:l6-v2".to_string(),
        384,
    )?);

    let executor = Executor::with_embedder(rest_ops, None, Some(embedder));

    // 2. Execute QQL statements
    let res = executor.execute("QUERY 'semantic search' FROM docs USING dense LIMIT 5").await?;
    println!("Operation: {}, Message: {}", res.operation, res.message);

    // 3. gRPC Client example
    let grpc_ops = Box::new(GrpcQdrant::from_url("http://localhost:6334", None)?);
    let grpc_exec = Executor::new(grpc_ops, None);
    let grpc_res = grpc_exec.execute("QUERY POINTS (1, 2) FROM docs").await?;
    println!("gRPC Points lookup: {:?}", grpc_res);

    Ok(())
}
```

## Pure AST Parsing, Filter Injection & Route Lowering

```rust
use qql_core::parser::Parser;
use qql_core::ast::{self, ComparisonOp, Value};
use qql_plan::routing::route;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Parse statement string to Stmt
    let mut stmt = Parser::parse("QUERY 'search' FROM docs USING dense LIMIT 10")?;

    // 2. Inject security filter into AST
    ast::inject_filter(&mut stmt, "tenant_id", ComparisonOp::Eq, Value::Str("acme".to_string()))?;

    // 3. Lower AST statement to typed Qdrant Route
    let route = route(&stmt);
    println!("Route Method: {:?}", route.method);
    println!("Route Path: {}", route.path);
    println!("Route Body JSON: {}", route.body_json().unwrap());

    Ok(())
}
```
