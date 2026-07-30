# qql-runtime (crate name **`qql`**)

Execute QQL: parse → prepare/embed → plan → batch → REST **or** gRPC **or** edge.

## Proposition

One executor API. Backends implement `QdrantOps`. Plan IR is shared; gRPC is
**not** a second-class path — `execute_grpc_route` converts typed plan structs
to protobuf (query vectors / IDs without a JSON detour).

```
Stmt
  → prepare (schema USING kinds + embeddings)
  → plan() → PlannedOperation
  → REST: to_rest_route → JSON
  → gRPC: PlannedOperation → protobuf
  → Edge: in-process HNSW (qql-edge)
```

## Quick start

```rust
use std::sync::Arc;
use qql::executor::{Executor, OnError};
use qql::rest::RestQdrant;
use qql::embedder::HttpEmbedder;

let ops = Box::new(RestQdrant::new("http://localhost:6333", None));
let embedder = Arc::new(HttpEmbedder::new(
    "http://localhost:11434/v1/embeddings",
    "",
    "all-minilm:l6-v2",
    384,
)?);
let exec = Executor::with_embedder(ops, None, Some(embedder));

exec.execute(
    "QUERY TEXT 'semantic search' FROM docs USING dense LIMIT 5",
    OnError::Stop,
).await?;
```

Convenience: `Executor::rest(url, api_key)` / `Executor::grpc(url, api_key)`.

## QdrantOps

Unified backend trait (collections, indexes, `execute_planned`, query/update
batches). Implementations: `RestQdrant`, `GrpcQdrant`, `EdgeQdrant` (other crate).

| Backend | Port (typical) | Notes |
|---------|----------------|--------|
| REST | 6333 | OpenAPI JSON body |
| gRPC | 6334 | tonic + protos in `proto/` |
| Edge | n/a | No cluster SHARD admin / GROUP BY / ACORN |

API key: REST header `api-key`; gRPC `ApiKeyInterceptor`.

## Prepare order

1. Schema topology → fill `USING` dense/sparse/multi  
2. Embeddings (`qql-embed`) — fail closed on unknown kind (`QQL-VECTOR-KIND`)  
3. Upsert collection prep when needed  

## Multitenancy at execute time

```sql
-- Isolation in language + routing
QUERY TEXT 'q' FROM t USING dense
WHERE tenant_id = 'acme' SHARD 'acme' LIMIT 10;
```

Hosts should still `inject_filter` untrusted QQL. Routing is request-level
`shard_key` / `ShardKeySelector` on both transports.

## Features

`default = ["grpc", "rest"]` — either can be disabled.

## Docs

- [AGENT.md](../../AGENT.md) · [qql-plan](../qql-plan/README.md) · [qql-embed](../qql-embed/README.md) · [Syntax](../../docs/syntax.md)

## Test

```bash
cargo test -p qql --lib
# live Qdrant:
cargo test -p qql --test live_integration_test -- --test-threads=1
```
