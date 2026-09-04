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
| REST | 6333 | OpenAPI **1.19.0** JSON body (`openapi.json`) |
| gRPC | 6334 | tonic + public protos in `proto/` (1.19.0 pin) |
| Edge | n/a | qdrant-edge **0.8** (IDF yes; quotas / SHARD / GROUP BY / ACORN no) |

API key: REST header `api-key`; gRPC `ApiKeyInterceptor`.

### Route affinity (client API, Qdrant ≥ 1.19)

Not part of QQL grammar — transport metadata only. Pins reads to a stable replica:

```rust
use qql::rest::RestQdrant;
// use qql::grpc::GrpcQdrant;

let ops = Box::new(
    RestQdrant::new("http://localhost:6333", None)
        .with_route_affinity("session-42"), // X-Qdrant-Route-Affinity
);
// GrpcQdrant::with_route_affinity sends gRPC metadata x-qdrant-route-affinity
```

Empty strings are treated as unset. Host bindings expose the same option:
`pyqql.Client(..., route_affinity=...)`, `nqql` `new Client({ routeAffinity })`,
and the WASM `client.setRouteAffinity(key)`.

### Quotas (REST only)

```sql
SHOW QUOTAS;
SET QUOTA (enabled = true, max_resident_memory_percent = 80,
           max_disk_usage_percent = 90, release_margin_percent = 5) WAIT true;
```

| Backend | Result |
|---------|--------|
| REST | `GET|PUT /quotas` |
| gRPC | `QQL-GRPC-QUOTA` — no public gRPC quota service |
| Edge | `QQL-EDGE-UNSUPPORTED-QUOTA` |

`SET QUOTA` is a **full replace** of the cluster config (omitted/`null` keys clear
limits in the replacement body).

Other 1.19 body features (`memory`, `MATCH PREFIX`, `SLICE`, `idf`, `turbo4`,
keyword `prefix`) flow through the shared plan IR on all backends that support
the corresponding Qdrant capability.

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

- [AGENTS.md](../../AGENTS.md) · [qql-plan](../qql-plan/README.md) · [qql-embed](../qql-embed/README.md) · [Syntax](../../docs/syntax.md)

## Test

```bash
cargo test -p qql --lib
# live Qdrant:
cargo test -p qql --test live_integration_test -- --test-threads=1
```
