# QQL Documentation

**QQL** is a typed query language for [Qdrant](https://qdrant.tech): one grammar, one plan IR, three backends (REST, gRPC, edge). The language surface tracks **Qdrant ≥ 1.19.0** (OpenAPI / public protos pinned in `qql-runtime`).

## Proposition

| Without QQL | With QQL |
|-------------|----------|
| Nested filter JSON, manual embed, per-SDK clients | One SQL-like surface for search, hybrid, mutations, DDL |
| Tenant filters pasted into every code path | `inject_filter` on the AST — recursive, fail-closed |
| Custom sharding as ad-hoc wire fields | First-class `SHARD '…'` + `CREATE SHARD KEY` |
| REST-only or gRPC-only apps | Plan once → REST **or** gRPC **or** in-process edge |

**Pipeline:** parse (`qql-core`) → prepare/embed (`qql-runtime` + `qql-embed`) → plan (`qql-plan` → `PlannedOperation`) → dispatch (REST / gRPC / edge).

### Qdrant 1.19 language highlights

| Feature | Notes |
|---------|--------|
| `SHOW QUOTAS` / `SET QUOTA (…)` | Cluster REST `GET|PUT /quotas` only; gRPC → `QQL-GRPC-QUOTA`; edge → `QQL-EDGE-UNSUPPORTED-QUOTA` |
| `FACET field FROM col` | In-database categorical value counts via REST `/collections/{col}/facet` |
| `QUERY [...] FROM col` | Implicit vector array literals without requiring `VECTOR` keyword |
| `WITH PAYLOAD` default | Queries default to returning all payload fields (`true`) when omitted |
| `memory = 'cold'\|'cached'\|'pinned'` | HNSW / VECTOR / SPARSE / QUANTIZATION / indexes; `payload_memory` is cold\|cached only |
| `WHERE field MATCH PREFIX '…'` | Keyword prefix match (`prefix=true` index) |
| `WHERE SLICE (total, index)` | Deterministic id-space slice (`total ≥ 1`, `index < total`) |
| `PARAMS (idf = …)` | Sparse IDF corpus: `'global'` or `WHERE <filter>` |
| `datatype = 'turbo4'` | Dense TurboQuant 4-bit storage |
| Route affinity | **Client API only** (`RestQdrant` / `GrpcQdrant::with_route_affinity`, `pyqql.Client(route_affinity=…)`, `nqql` `{ routeAffinity }`, WASM `setRouteAffinity`) — not QQL grammar |

See [syntax.md](syntax.md) and [filters.md](filters.md) for examples.

## Guides in this folder

| Doc | What it covers |
|-----|----------------|
| [syntax.md](syntax.md) | Full language: QUERY, DML, DDL, quotas, memory/datatype, `SHARD` vs `SHARD KEY`, params, embeddings |
| [filters.md](filters.md) | `WHERE` predicates (including `MATCH PREFIX` / `SLICE`) and how they lower to Qdrant `Filter` |
| [inject_filter.md](inject_filter.md) | Host isolation: AST injection (not routing) |
| [parameters.md](parameters.md) | Prepared statements and parameter binding (`:name`, `?`) |
| [STORY.md](STORY.md) | Product history (Python → Go → Rust) |

## Companion material

- **Agent skill:** [`skills/qql-skill/`](../skills/qql-skill/) — intent maps, SDK refs, multitenancy, gaps
- **Language contract:** [`language/v1/`](../language/v1/) — grammar, fixtures, semantics
- **Examples:** [`examples/`](../examples/) — Berlin geo, SEC 10-K, medical, edge, bindings
- **Wire contracts:** `crates/qql-runtime/openapi.json`, `crates/qql-runtime/proto/`

## Two rules that never change

1. **Isolation ≠ routing.**  
   - Isolation: `WHERE tenant_id = '…'` / `inject_filter` → `Filter`  
   - Routing: `SHARD '…'` / `stmt.shard_key` → request `shard_key` / `ShardKeySelector`  
   Never put routing inside a filter.

2. **Plan is transport-neutral.**  
   `PlannedOperation` is the source of truth. REST and gRPC are projections of the same IR.
