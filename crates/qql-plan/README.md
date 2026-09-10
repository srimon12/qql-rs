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
// Prefer try_route(&stmt) when you want plan + projection in one call.
```

## PlannedOperation (selected)

| Family | Variants |
|--------|----------|
| Search | `Query`, `QueryGroups`, `GetPoints`, `Scroll`, `Count`, `Facet` |
| Mutations | `Upsert`, `Delete`, `ClearPayload`, `DeletePayload`, `UpdateVectors`, `DeleteVectors`, `UpdatePayload` |
| DDL | `CreateCollection`, `UpdateCollection`, indexes, **`CreateShardKey` / `DropShardKey` / `ListShardKeys`** |
| Quotas | **`GetQuotas`** / **`SetQuotas`** → REST `GET|PUT /quotas` (Qdrant ≥ 1.19) |
| Client-side | `CrossRerank` (no single Qdrant route) |

### Batch families

- **Query** — contiguous same-collection searches → `/points/query/batch` (`CrossRerank` is Single)
- **Mutation** — contiguous same-collection mutations, including `DELETE PAYLOAD` → `/points/batch`
- **Single** — DDL, scroll, count, facet, quotas, `CrossRerank`, …

### Semantic primitives

`PlanPointId`, `PlanVectorValue` (Dense / Sparse / MultiDense), `PlanQueryInput`,
typed formula trees — stay typed until a transport boundary.
`MemoryPlacement` / `VectorDatatype` re-exported from `qql-core`.

### Qdrant 1.19 lowering notes

| Surface | Plan behavior |
|---------|----------------|
| `SET QUOTA (…)` | **Full replace** body (`SetQuotaRequest`). Omitted keys / `key = null` are unset in the PUT body — not a merge of the previous config. Invalid keys/ranges → `QQL-PLAN-QUOTA`. Optional `WAIT` → query `?wait=`. |
| `PARAMS (idf = …)` | `'global'` or `WHERE <filter>`; empty lowered corpus → `QQL-PLAN-IDF`. |
| `MATCH PREFIX` / `SLICE` | `MatchValue::Prefix` / `SliceCondition` on the filter IR. |
| `memory` / `payload_memory` / `datatype` / keyword `prefix` | Forwarded on collection, vector, HNSW, quantization, and index REST bodies. |

```rust
let op = plan(&Parser::parse(
    "SET QUOTA (enabled = true, max_resident_memory_percent = 80) WAIT true;"
)?)?;
let route = to_rest_route(&op)?; // PUT /quotas?wait=true
```

## Modules

Public crate surface. REST projection lives in `routing`; `plan` re-exports
`to_rest_route` / `try_route` so existing `qql_plan::plan::*` paths stay valid.

| Module | Role |
|--------|------|
| `plan` | `plan` / `plan_template` → `PlannedOperation`. Re-exports `to_rest_route`, `try_route`. |
| `routing` | `Route`, `to_rest_route`, `try_route`, `compile_statement` |
| `query` / `mutation` / `ddl` | Lowering (including quotas / IDF / memory) |
| `filter` | `FilterExpression` only (no routing fields) |
| `formula_types` | `PlanFormula` OpenAPI `Expression` tree, `FormulaDefault` bindings |
| `types` | Request IR façade (`SetQuotaRequest`, `IdfSearchParams`, …) |
| `batch` | `BatchGrouper`, `BatchKey`, query/update batch builders |
| `semantic` | `PlanPointId`, `PlanVectorValue`, `PlanQueryInput` |

## Docs

- [AGENTS.md](../../AGENTS.md) pipeline · [Syntax](../../docs/syntax.md) · [Multitenancy](../../skills/qql-skill/references/qql-multitenancy.md)

## Test

```bash
cargo test -p qql-plan
```
