# qql-plan

Transport-neutral lowering: **AST → `PlannedOperation`**.

## Proposition

One plan for every backend. REST is a **projection** (`to_rest_route`); gRPC
maps the **same** typed structs to protobuf. Never invent wire shapes in
bindings — plan here once.

```
Stmt (qql-core)
    │ plan()
    ▼
PlannedOperation          ← source of truth
    ├── filter            → REST filter / gRPC Filter
    ├── shard_key         → REST shard_key / gRPC ShardKeySelector
    ├── to_rest_route()   → Route { method, path, query, body }
    └── gRPC match arm    → typed protobuf (runtime)
```

**Filter never carries `shard_key`.** Isolation and routing are siblings on the
request (OpenAPI + proto both model it that way).

## Usage

```rust
use qql_core::parser::Parser;
use qql_plan::plan::{plan, to_rest_route, try_route};

let stmt = Parser::parse(
    "QUERY TEXT 'hello' FROM docs USING dense SHARD 'acme' LIMIT 5"
)?;
let op = plan(&stmt)?;
let route = to_rest_route(&op)?;   // fallible REST projection
// Prefer try_route(&stmt) for libraries; avoid panic-prone route()
```

## PlannedOperation (selected)

| Family | Variants |
|--------|----------|
| Search | `Query`, `QueryGroups`, `GetPoints`, `Scroll`, `Count` |
| Mutations | `Upsert`, `Delete`, `ClearPayload`, `UpdateVectors`, `DeleteVectors`, `UpdatePayload`, … |
| DDL | `CreateCollection`, `UpdateCollection`, indexes, **`CreateShardKey` / `DropShardKey` / `ListShardKeys`** |
| Client-side | `CrossRerank` (no single Qdrant route) |

### Batch families

- **Query** — contiguous same-collection queries → `/points/query/batch`
- **Mutation** — contiguous mutations → `/points/batch`
- **Single** — DDL, scroll, count, …

### Semantic primitives

`PlanPointId`, `PlanVectorValue` (Dense / Sparse / MultiDense), `PlanQueryInput`,
typed formula trees — stay typed until a transport boundary.

## Modules

| Module | Role |
|--------|------|
| `plan` | `plan`, `to_rest_route`, `try_route`, `compile_statement` |
| `query` / `mutation` / `ddl` | Lowering |
| `filter` | `FilterExpression` only (no routing fields) |
| `types` | Request IR |

## Docs

- [AGENT.md](../../AGENT.md) pipeline · [Syntax](../../docs/syntax.md) · [Multitenancy](../../skills/qql-skill/references/qql-multitenancy.md)

## Test

```bash
cargo test -p qql-plan
```
