# qql-plan

Typed lowering from QQL AST to transport-neutral [`PlannedOperation`] — the canonical
source of truth for every supported statement. REST routes are a **projection**
([`to_rest_route`]) of a `PlannedOperation`; gRPC converts the same typed
operation without reverse-engineering JSON shapes.

```
AST (qql-core::ast::Stmt)
    │
    └─plan()─→ PlannedOperation (canonical; BatchFamily, collection, shard_key)
    │
    ├─to_rest_route()─→ Route { method, path, query, body }   (REST projection)
    ├─gRPC: match PlannedOperation → protobuf via grpc_route   (direct typed → proto)
    └─mutation batch: lower_update_operation() → UpdateOperation
```

## Architecture

### plan() — the single planner entry point

```rust
use qql_core::parser::Parser;
use qql_plan::plan::{plan, to_rest_route, PlannedOperation};

let stmt = Parser::parse("QUERY 'hello' FROM docs LIMIT 5;")?;
let op = plan(&stmt)?;               // → PlannedOperation::Query { .. }
let route = to_rest_route(&op);      // → Route (REST projection)
```

The [`plan`] function returns a [`PlannedOperation`] enum with 21 variants
covering all query, mutation, and DDL operations. Each variant carries typed
request parameters (not raw JSON).

### PlannedOperation variants

| Variant | Statement | Transport | Batch family |
|---------|-----------|-----------|-------------|
| `Query` / `QueryGroups` / `GetPoints` | QUERY | POST /collections/{c}/points/query[/groups] | Query (when batchable) |
| `Scroll` | SCROLL | POST /collections/{c}/points/scroll | Single |
| `Count` | COUNT | POST /collections/{c}/points/count | Single |
| `Upsert` | UPSERT | PUT /collections/{c}/points | Mutation |
| `Delete` | DELETE | POST /collections/{c}/points/delete | Mutation |
| `ClearPayload` | CLEAR PAYLOAD | POST /collections/{c}/points/payload/clear | Mutation |
| `UpdateVectors` | UPDATE SET VECTOR | PUT /collections/{c}/points/vectors | Mutation |
| `DeleteVectors` | DELETE VECTOR | POST /collections/{c}/points/vectors/delete | Mutation |
| `UpdatePayload` | UPDATE SET PAYLOAD | POST /collections/{c}/points/payload | Mutation |
| `CreateCollection` | CREATE COLLECTION | PUT /collections/{c} | Single |
| `UpdateCollection` | ALTER COLLECTION | PATCH /collections/{c} | Single |
| `DropCollection` | DROP COLLECTION | DELETE /collections/{c} | Single |
| `CreateIndex` | CREATE INDEX | PUT /collections/{c}/index | Single |
| `DropIndex` | DROP INDEX | DELETE /collections/{c}/index/{field} | Single |
| `CreateShardKey` / `DropShardKey` | CREATE/DROP SHARD KEY | PUT /collections/{c}/shards | Single |
| `ListShardKeys` | SHOW SHARD KEYS | GET /collections/{c}/shards | Single |
| `ListCollections` | SHOW COLLECTIONS | GET /collections | Single |
| `GetCollection` | SHOW COLLECTION | GET /collections/{c} | Single |

### Route — REST projection

`Route` is a transport-agnostic HTTP descriptor `{ method, path, query, body }`.
The body is an untagged `RequestBody` enum whose `#[serde(untagged)]`
serialization matches Qdrant's OpenAPI wire format exactly.

```rust
pub struct Route {
    pub method: Method,         // Get | Post | Put | Patch | Delete
    pub path: String,           // e.g. "/collections/docs/points/query"
    pub query: Vec<(String, String)>,  // e.g. [("wait", "true")]
    pub body: Option<RequestBody>,
}
```

REST routes are produced by `to_rest_route()`. New code should use
`plan() + to_rest_route()` or `try_route()` (fallible combination).

### gRPC conversion

gRPC routes bypass `Route` entirely. The runtime's `grpc_route::execute_grpc_route`
reads `Route.body` (which carries the same typed request data produced by the planner)
and maps directly to protobuf. All 21 operation variants are supported.

### Batch compatibility

`PlannedOperation::batch_family()` determines adjacency-based batching:

- **Query**: contiguous same-collection `Query` → `QueryBatchRequest`
- **Mutation**: contiguous same-collection Upsert/Delete/ClearPayload/DeleteVectors/UpdateVectors/UpdatePayload → `UpdateBatchRequest`
- **Single**: everything else (DDL, Scroll, Count) — never batched

## Key types

All typed request types live in [`crate::types`]:

| Type | Purpose |
|------|---------|
| `QueryRequest` / `QueryGroupsRequest` | Search with variant, filters, params |
| `PointsRequest` | Point ID lookup (QUERY POINTS) |
| `ScrollRequest` | Scrolling with optional filter |
| `UpsertRequest` / `UpsertPointRequest` | Point insertion with optional vectors |
| `DeleteRequest` | By ID list or filter |
| `UpdateVectorRequest` / `UpdatePayloadRequest` | Point-level vector/payload mutation |
| `ClearPayloadRequest` / `DeleteVectorRequest` | Payload/vector removal |
| `CountRequest` | Count with filter and shard key |
| `CreateCollectionRequest` | Collection configuration |
| `CreateIndexRequest` | Field index with per-schema params |
| `CreateShardKeyRequest` / `DropShardKeyRequest` | Shard key management |
| `QueryBatchRequest` / `UpdateBatchRequest` | Batch wire formats |
| `UpdateOperation` | Single mutation in an `UpdateBatchRequest` |
| `FilterExpression` / `FilterClause` | Lowered filter AST |
| `SearchParamsRequest` / `AcornSearchParams` | Search parameters |
| `QueryVariant` | Typed query expression (nearest/recommend/context/fusion/…) |

### Semantic types (typed until transport)

These types preserve semantic distinctions that JSON shape inference cannot recover:

- `PlanPointId`: `Number(u64)` or `String` — serialized as bare number or string
- `PlanVectorValue`: `Dense`, `Sparse { indices, values }`, or `MultiDense`
- `PlanQueryInput`: `Point`, `Vector`, or `Document { text, model }`
- `PlanPointVectors`: `Unnamed(PlanVectorValue)` or `Named(Vec<(String, PlanVectorValue)>)`
- `PlanFormula`: typed formula tree — REST uses snake_case OpenAPI keys via custom serialization

## Plan lowering modules

| Module | Lowers |
|--------|--------|
| `ddl` | CreateCollection, AlterCollection, CreateIndex |
| `mutation` | Upsert, Delete, Scroll, ClearPayload, DeleteVector, UpdateVector, UpdatePayload |
| `query` | Query expressions, prefetch, formula, search params |
| `filter` | Filter expressions: comparison, range, match, geo, nested, has_id, has_vector |
| `embedding` | Embedding job extraction for embedder dispatch |

## Features

- `serde` (default): JSON serialization for all request/response types
- `std` (default): `std::error::Error` impls
- `alloc` (always): used for `Vec`, `String`

## Verification

```rust
#[test]
fn plan_agrees_with_route() {
    use qql_core::parser::Parser;
    use qql_plan::plan::try_route;

    let stmt = Parser::parse("QUERY 'search' FROM docs LIMIT 10;").unwrap();
    let route = try_route(&stmt).unwrap();
    assert_eq!(route.method, Method::Post);
    assert_eq!(route.path, "/collections/docs/points/query");
}

#[test]
fn gddl_create_via_plan() {
    use qql_core::parser::Parser;
    use qql_plan::plan::plan;

    let stmt = Parser::parse("CREATE COLLECTION docs (dense VECTOR(384, COSINE));").unwrap();
    let op = plan(&stmt).unwrap();
    assert!(matches!(op, PlannedOperation::CreateCollection { .. }));
}
```
