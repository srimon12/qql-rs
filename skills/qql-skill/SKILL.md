---
name: qql-skill
description: "Use QQL (Qdrant Query Language) to manage collections, upsert documents, search, filter, rerank, recommend, and execute multi-stage retrieval workflows."
---

# QQL Skill

Turn retrieval intent into **valid, current QQL** and correct SDK usage.

## Proposition (read this first)

QQL is the **typed language + plan IR** for Qdrant:

1. **One grammar** for search, hybrid, multivector, mutations, DDL, multitenancy.
2. **One plan** (`PlannedOperation`) — gRPC and REST are equal projections, not REST-first.
3. **Host isolation** via `inject_filter` (AST); **routing** via `SHARD '…'` or `stmt.shard_key`.
4. **Never invent** syntax listed as open in [qql-gaps.md](references/qql-gaps.md).

Human docs (product-facing): [`docs/`](../../docs/). This skill is for agents writing QQL/SDK code.

## Reference Wiki

| Doc | When to open it |
|-----|-----------------|
| [qql-examples.md](references/qql-examples.md) | Golden QQL patterns (CTE, hybrid, rerank, formula, geo) |
| [qql-multitenancy.md](references/qql-multitenancy.md) | `SHARD KEY` DDL vs `SHARD` routing vs `inject_filter` |
| [inject-filter.md](references/inject-filter.md) | Fail-closed tenant / policy injection |
| [qql-gaps.md](references/qql-gaps.md) | Open vs closed — **do not invent open syntax** |
| [qql-install.md](references/qql-install.md) | Install pyqql / nqql / CLI / edge |
| [python-sdk.md](references/python-sdk.md) | `pyqql` |
| [node-sdk.md](references/node-sdk.md) | `@veristamp/nqql` |
| [wasm-sdk.md](references/wasm-sdk.md) | `qql-wasm` |
| [rust-sdk.md](references/rust-sdk.md) | `qql-core` / `qql-plan` / `qql` |

Runnable demos: `scripts/demo_*.py`. Repo examples: `examples/` (Berlin, SEC 10-K, medical, edge).

## Intent Mapping

Translate user intent directly into QQL syntax:

- Semantic similarity -> `QUERY 'text' FROM <collection> USING dense LIMIT <n>` (schema resolves dense; or `AS DENSE` offline)
- Keyword / sparse retrieval -> `QUERY 'text' FROM <collection> USING sparse LIMIT <n>` (schema resolves sparse; or `AS SPARSE` offline)
- Hybrid retrieval (dense + sparse) -> `QUERY TEXT 'text' FROM <collection> USING HYBRID DENSE dense SPARSE sparse FUSION RRF LIMIT <n>` (or front-form `QUERY HYBRID TEXT 'text' DENSE dense SPARSE sparse FUSION RRF FROM <collection> LIMIT <n>`)
- Hybrid retrieval with DBSF fusion -> `QUERY TEXT 'text' FROM <collection> USING HYBRID DENSE dense SPARSE sparse FUSION DBSF LIMIT <n>`
- Multivector / ColBERT nearest -> `QUERY TEXT 't' FROM <collection> USING colbert LIMIT <n>` when collection has multivector config; offline use `USING colbert AS MULTI`
- Late-interaction rerank (ColBERT MaxSim) -> `WITH c AS (QUERY 't' USING dense LIMIT 50) QUERY RERANK TEXT 't' MODEL 'answerai-colbert-small-v1' FROM <collection> USING colbert PREFETCH (c) LIMIT <n>`
- Cross-encoder pair rerank -> `WITH c AS (QUERY 't' USING dense LIMIT 50) QUERY CROSS RERANK TEXT 't' MODEL 'bge-reranker-base' ON FIELD text FROM <collection> PREFETCH (c) LIMIT <n>`
- Direct point retrieval by ID -> `QUERY POINTS (id1, id2, 'id3') FROM <collection>`
- Recommendation by example -> `QUERY RECOMMEND POSITIVE (id1, id2) NEGATIVE (id3) STRATEGY average_vector FROM <collection> USING dense LIMIT <n>`
- Context search -> `QUERY CONTEXT (POSITIVE POINT id1 NEGATIVE POINT id2) FROM <collection> USING dense LIMIT <n>`
- Discovery search -> `QUERY DISCOVER TARGET POINT id1 CONTEXT (POSITIVE POINT id2 NEGATIVE POINT id3) FROM <collection> USING dense LIMIT <n>`
- Relevance feedback search -> `QUERY RELEVANCE FEEDBACK TARGET 'query_text' FEEDBACK ((1, 0.9), (2, 0.1)) STRATEGY NAIVE (a=1.0, b=0.75, c=0.25) FROM <collection> USING dense LIMIT <n>`
- Random sampling -> `QUERY SAMPLE RANDOM FROM <collection> LIMIT <n>`
- Browse by payload field -> `QUERY ORDER BY <field> [ASC|DESC] FROM <collection> LIMIT <n>`
- Multi-stage retrieval -> `WITH c1 AS (QUERY 't' USING dense LIMIT 100), c2 AS (QUERY 't' USING sparse LIMIT 100) QUERY FUSION RRF FROM <collection> PREFETCH (c1, c2) LIMIT <n>`
- CLIP text→image -> `QUERY TEXT '…' MODEL 'Qdrant/clip-ViT-B-32-text' FROM <coll> USING image LIMIT <n>`
- CLIP image query -> `QUERY IMAGE '/path.jpg' MODEL 'Qdrant/clip-ViT-B-32-vision' FROM <coll> USING image LIMIT <n>`
- CLIP image upsert -> `UPSERT … USING IMAGE MODEL 'clip-vision' ON FIELD image INTO image`
- MMR diversification -> `QUERY MMR 'query_text' DIVERSITY 0.5 CANDIDATES 100 FROM <collection> USING dense LIMIT <n>`
- Formula / Score shaping -> `QUERY FORMULA score + 0.3 * popularity DEFAULTS (popularity = 1.0) FROM <collection> USING dense LIMIT <n>`
- Grouped results -> add `GROUP BY <field> SIZE <m> LOOKUP FROM <collection>`
- Browse points -> `SCROLL FROM <collection> [AFTER <id>] LIMIT <n>`
- Batch ingest -> `UPSERT INTO <collection> VALUES {id: 1, text: '...'}, {id: 2, text: '...'}`
- Delete points -> `DELETE FROM <collection> WHERE <filter>`
- Clear payload -> `CLEAR PAYLOAD FROM <collection> WHERE <filter>`
- Delete payload keys -> `DELETE PAYLOAD <key1, key2> FROM <collection> WHERE <filter>`
- Delete vectors -> `DELETE VECTOR <name> FROM <collection> WHERE id = N`
- Count points -> `COUNT FROM <collection> WHERE <filter>` (or `COUNT FROM <collection> WITH (exact = true)` for exact count)
- Create shard key -> `CREATE SHARD KEY '<key>' ON COLLECTION <name> [WITH (shards_number = N, replication_factor = M)]`
- Drop shard key -> `DROP SHARD KEY '<key>' ON COLLECTION <name>`
- Show shard keys -> `SHOW SHARD KEYS ON COLLECTION <name>`
- Multi-tenant isolation -> `QUERY 'text' FROM <collection> WHERE tenant_id = 'honeywell' SHARD 'honeywell' LIMIT 10`
- Keyword prefix filter -> `WHERE title MATCH PREFIX 'Comp'` (keyword index with `prefix = true`)
- Deterministic ID-space sampling -> `WHERE SLICE (total, index)` e.g. `SLICE (4, 1)`
- Sparse IDF corpus (global / tenant) -> `PARAMS (idf = 'global')` or `PARAMS (idf = WHERE tenant_id = 'acme')`
- Cluster quotas (REST) -> `SHOW QUOTAS;` / `SET QUOTA (enabled = true, max_resident_memory_percent = 80) WAIT true;`
- Memory placement + TurboQuant -> `WITH VECTOR (memory = 'cached', datatype = 'turbo4')`, `WITH HNSW (memory = 'cold')`, `payload_memory = 'cold'`
- Read affinity (host SDKs) -> `RestQdrant` / `GrpcQdrant` `.with_route_affinity(…)`, `pyqql.Client(route_affinity=…)`, `nqql` `{ routeAffinity }`, wasm `client.setRouteAffinity(key)` → `X-Qdrant-Route-Affinity` (not QQL syntax)

## Canonical Grammar & Capabilities

Language surface targets **Qdrant 1.19** / **QQL 1.5** features (quotas, memory placement,
`MATCH PREFIX`, `SLICE`, IDF corpus, `turbo4`). Prefer forms below over legacy dual-write
keys (`on_disk` / `always_ram`) for new scripts.

### Collection Management (DDL)
```sql
CREATE COLLECTION docs (
  dense VECTOR(384, COSINE),
  sparse SPARSE,
  colbert VECTOR(128, COSINE) WITH MULTIVECTOR (comparator = 'max_sim')
) WITH HNSW (m = 16, ef_construct = 100);

-- Memory tiers + TurboQuant 4-bit dense (Qdrant 1.19 / QQL 1.5)
CREATE COLLECTION docs_tiered (
  dense VECTOR(384, COSINE) WITH VECTOR (memory = 'cached', datatype = 'turbo4')
    WITH HNSW (memory = 'cold')
) WITH PARAMS (payload_memory = 'cold')
  WITH QUANTIZATION (type = 'scalar', memory = 'cached');

ALTER COLLECTION docs WITH VECTOR (on_disk = true);
ALTER COLLECTION docs WITH PARAMS (replication_factor = 3);
ALTER COLLECTION docs WITH QUANTIZATION (type = 'scalar', always_ram = true);

CREATE INDEX ON COLLECTION docs FOR title TYPE text WITH (lowercase = true);
CREATE INDEX ON COLLECTION docs FOR tenant_id TYPE keyword WITH (is_tenant = true);
CREATE INDEX ON COLLECTION docs FOR rating TYPE integer WITH (range = true);
-- Keyword prefix index for MATCH PREFIX filters
CREATE INDEX ON COLLECTION docs FOR title TYPE keyword WITH (prefix = true, memory = 'cached');

DROP INDEX ON COLLECTION docs FOR title;
SHOW COLLECTIONS;
SHOW COLLECTION docs;

-- Cluster-wide resource quotas (REST /quotas only — not gRPC, not edge)
SHOW QUOTAS;
SET QUOTA (enabled = true, max_resident_memory_percent = 80) WAIT true;

-- Shard key lifecycle for multi-tenant custom sharding
CREATE SHARD KEY 'acme' ON COLLECTION docs WITH (shards_number = 2);
SHOW SHARD KEYS ON COLLECTION docs;
DROP SHARD KEY 'acme' ON COLLECTION docs;

DROP COLLECTION docs;
```

### Data Manipulation (DML)
```sql
-- Upsert points with automated text embedding inference
UPSERT INTO docs VALUES
  {id: 1, text: 'Qdrant vector database', category: 'tech'},
  {id: 2, text: 'Rust programming language', category: 'programming'}
  USING DENSE MODEL 'all-minilm:l6-v2';

-- Explicit target payload field and named destination vector
UPSERT INTO docs VALUES
  {id: 1, text: 'primary text', title: 'Qdrant Overview', category: 'tech'}
  USING DENSE MODEL 'all-minilm' ON FIELD title INTO title_vec;

-- Multiple target fields mapped to distinct named vectors
UPSERT INTO docs VALUES
  {id: 1, text: 'primary text', title: 'Qdrant Overview'}
  USING
    DENSE MODEL 'all-minilm' ON FIELD text INTO dense,
    DENSE MODEL 'all-minilm' ON FIELD title INTO title_vec;

-- Update vector by point ID
UPDATE docs SET VECTOR dense = [0.1, 0.2, 0.3] WHERE id = 1;

-- Update payload metadata
UPDATE docs SET PAYLOAD = {status: 'reviewed'} WHERE category = 'tech';

-- Delete points
DELETE FROM docs WHERE category = 'obsolete';

-- Clear payload from points
CLEAR PAYLOAD FROM docs WHERE status = 'archived';

-- Delete specific vectors from points
DELETE VECTOR colbert FROM docs WHERE id = 42;

-- Count points with filter
COUNT FROM docs WHERE status = 'active';
```

### Universal Query Syntax
Clauses must appear in the exact required order (enforced at parse time):

```sql
[WITH cte_name AS (QUERY ...), ...]
QUERY <expression>
FROM <collection>
[USING HYBRID [DENSE <vector>] [SPARSE <vector>] [FUSION RRF|DBSF]
 | USING <vector_name> [AS DENSE | AS SPARSE | AS MULTI | AS MULTIVECTOR]]
[PREFETCH (cte_ref [WHERE <filter>] [SCORE THRESHOLD <number>], ...)]
[WHERE <filter_expression>]
[SHARD '<tenant_key>']
[PARAMS (hnsw_ef = <n>, exact = <bool>, acorn = <bool>, max_selectivity = <0–1>,
         indexed_only = <bool>, timeout = <seconds>, consistency = majority|quorum|all|<n>,
         idf = 'global' | WHERE <filter>)]
[SCORE THRESHOLD <number>]
[GROUP BY <field> [SIZE <n>] [LOOKUP FROM <collection>]]
[WITH PAYLOAD [true | false | INCLUDE (...) | EXCLUDE (...)]]
[WITH VECTOR [true | false | (...)]]
[LIMIT <n>]
[OFFSET <n>];
```

`SHARD` appears after `WHERE` and before `PARAMS`. Clause order violations produce parse errors.

**Limits (see [qql-gaps.md](references/qql-gaps.md)):**

- `OFFSET` **is** now supported with `GROUP BY` (maps to Qdrant's `group_offset`).
- `MMR` now supports sparse vectors (`USING … AS SPARSE` with MMR is supported).
- `max_selectivity` requires `acorn = true` (remote Qdrant; not edge).
- `timeout` / `consistency` are request-level (OpenAPI query params / gRPC fields); not on edge.
- `idf` is a search param for sparse IDF corpus scoping (remote + edge 0.8+).
- Edge has **no** `GROUP BY` — use remote Qdrant or filter + `LIMIT`.
- Edge / gRPC have **no** quotas (`SHOW QUOTAS` / `SET QUOTA` are REST-only).
- Dynamic shard: write `SHARD 'tenant'` in QQL, or set `stmt.shard_key = tenant` after parse (no `$bind` syntax).
- Route affinity is **not** QQL syntax — a client transport option: Rust `with_route_affinity`, `pyqql.Client(route_affinity=…)`, `nqql` `{ routeAffinity }`, wasm `setRouteAffinity` (see [rust-sdk.md](references/rust-sdk.md)).

**Vector roles (critical for embedding):**

| Form | Behavior |
|---|---|
| `USING name` | Runtime looks up `name` on collection schema (dense / sparse / multivector). Names are **not** special-cased by spelling. |
| `USING name AS DENSE` | Single dense embed (MiniLM, CLIP text, …) — one `Vec<f32>` |
| `USING name AS SPARSE` | Sparse embed — wire-compatible BM25 (Qdrant `qdrant/bm25` token IDs; unit-weight queries, tf-saturated documents) |
| `USING name AS MULTI` | Multivector / ColBERT bag → `[[f32,…],…]` via `embed_multi` (BGE-M3 ColBERT, not CLIP) |
| `USING HYBRID …` | Expand text nearest → dense+sparse fusion (same AST as `QUERY HYBRID`) |
| No `USING` | Schema must have exactly one compatible vector |

Offline/embed-only paths without schema require an explicit `AS …`. Leaving kind unknown fails with `QQL-VECTOR-KIND` (never silent dense default for named targets).

### Shard routing & multi-tenancy (two keywords)

| Keyword | Kind | Meaning |
|---------|------|---------|
| `CREATE/DROP/SHOW SHARD KEY` | DDL | Define / list custom partition names |
| `SHARD 'key'` on a statement | DML routing | Route **this** request (`shard_key` / `ShardKeySelector`) |

```sql
CREATE COLLECTION sec10k HYBRID (dense VECTOR(384, COSINE), sparse SPARSE)
WITH PARAMS (
  shard_number = 8,
  sharding_method = 'custom',
  shard_keys = ['honeywell', 'ge', '3m', 'rtx']
);
CREATE INDEX ON COLLECTION sec10k FOR tenant_id TYPE keyword WITH (is_tenant = true);

-- Isolation (filter) + routing (SHARD) together
QUERY TEXT 'supply chain risks' FROM sec10k USING dense
WHERE tenant_id = 'honeywell'
SHARD 'honeywell'
LIMIT 10;

UPSERT INTO sec10k VALUES {id: 1, text: '…', tenant_id: 'honeywell'} SHARD 'honeywell';
```

- **Security:** host `inject_filter(…, "tenant_id", "=", tenant)` on untrusted QQL.  
- **Routing:** prefer `SHARD '…'` in the query; or `stmt.shard_key = tenant` after parse.  
- **No** `inject_shard_key` API. Full guide: [qql-multitenancy.md](references/qql-multitenancy.md).

### Filters (`WHERE` Clause)
Supports standard comparison operators and predicates:
- Comparisons: `=`, `!=`, `>`, `>=`, `<`, `<=`
- Range: `BETWEEN <min> AND <max>`
- Sets: `IN ('a', 'b')`, `NOT IN ('c', 'd')`
- Null/Empty: `IS NULL`, `IS NOT NULL`, `IS EMPTY`, `IS NOT EMPTY`
- Text Match: `MATCH 'term'`, `MATCH ANY ('term1', 'term2')`, `MATCH PHRASE 'exact phrase'`
- Keyword prefix: `title MATCH PREFIX 'Comp'` (pair with keyword index `prefix = true`)
- Deterministic slice: `SLICE (total, index)` e.g. `WHERE SLICE (4, 1)` — hash buckets over point IDs
- Array / Vector: `HAS_VECTOR 'dense'`, `tags VALUES_COUNT >= 2`
- Geo: `location GEO_BBOX { top_left: {lat: 52.5, lon: 13.4}, bottom_right: {lat: 52.4, lon: 13.5} }`
- Geo radius: `location GEO_RADIUS { center: {lat: 48.85, lon: 2.35}, radius: 5000 }`
- Nested: `NESTED('reviews', rating > 4)`
- Logical: `AND`, `OR`, `NOT`

## Query planning & execution

```
source / host AST
    │
    ▼
qql-core: parse + validation  →  Stmt
    │
    ▼
prepare (runtime / WASM Client)
  · schema topology → USING dense/sparse/multi
  · embeddings (qql-embed) → Dense | Sparse | MultiDense
    │
    ▼
qql-plan: plan() → PlannedOperation   ← single source of truth
    │
    ├── to_rest_route()     → REST JSON
    ├── execute_grpc_route  → typed protobuf (no JSON for query vectors / IDs)
    └── EdgeQdrant          → in-process
```

`filter` and `shard_key` are **siblings** on the request. gRPC uses `Filter` +
`ShardKeySelector`; REST uses body `filter` + body `shard_key`. Neither puts
routing inside the filter object.

### Backend limits

| Backend | Notes |
|---------|--------|
| REST | Full matrix including `SHOW QUOTAS` / `SET QUOTA` |
| gRPC | Typed plan → proto; **no** public quota service (`QQL-GRPC-QUOTA`) |
| Edge | No quotas; no custom shard-key admin; no `GROUP BY` / ACORN; **IDF** on search params (edge 0.8+); optional multi/image/rerank hosts |
| Route affinity | Client transport option on remote SDKs (`RestQdrant` / `GrpcQdrant`, `pyqql.Client(route_affinity=…)`, `nqql` `{ routeAffinity }`, wasm `setRouteAffinity`); not on edge |

## Parameter Binding & Prepared Queries

QQL provides type-safe parameter binding across all SDKs:
- **Named Placeholders**: `:name` (e.g. `:category`, `:lim`)
- **Positional Placeholders**: `?` (sequential 1-to-1 mapping)
- **Host DX**: one `bind(query, params)` plus `Client.execute(..., params=...)`. Dict/object → named; list/array → positional. WASM takes a JS object/array, not a JSON string. Rust keeps typed `bind_named` / `bind_positional`.
- **Grammar Rule**: `$` is an identifier character in QQL (`$category`, `$score`). **Never** use `$name` or `$1` as placeholders — only `:name` and `?`.
- **Token Boundaries**: Colons in compact dicts (`{a:b}`, `{'a':b}`) are preserved as key-value separators. Write `{key: :val}` to bind dict values.

## CLI Reference

```text
qql [repl | connect]                         Interactive REPL (multiline, \f fmt, \d doctor, \e script)
qql exec <query> [--json] [--quiet]          Execute a single QQL query
qql execute <file.qql> [--stop-on-error]     Execute statements from file
qql explain <query> [--json] [--quiet]       Show hierarchical ASCII tree execution plan
qql convert [file.json]                       Convert REST JSON to QQL
qql fmt [file.qql] [--check] [--write]        Format QQL source into canonical form
qql dump <collection> <output.qql> [options]  Dump collection to QQL script
qql doctor [--json] [--quiet]                 Check Qdrant connection health & model hosts
qql --edge exec <query> [options]           Execute against local qdrant-edge
qql config edge [options]                    Configure local qdrant-edge backend
qql version                                   Show version

Global: --url <URL> (overrides QDRANT_URL env, default http://localhost:6333)
```

## Execution via SDKs

### Python (`pyqql`)
```python
import pyqql

embedder = pyqql.HttpEmbedder("http://localhost:11434/v1/embeddings", "all-minilm:l6-v2", 384)
client = pyqql.Client("http://localhost:6333", embedder=embedder)

# Standard execution
result = client.execute("QUERY 'semantic search' FROM docs USING dense LIMIT 5")

# Parameterized execution (named or positional)
result = client.execute(
    "QUERY TEXT :q FROM docs WHERE category = :cat LIMIT :lim",
    params={"q": "chest pain", "cat": "medical", "lim": 10},
)
```

### Rust (`qql`)
```rust
use std::collections::HashMap;
use qql::executor::{Executor, OnError};
use qql_core::ast::Value;

let exec = Executor::rest("http://localhost:6333", None).unwrap();

// Parameterized execution with named parameters
let mut params = HashMap::new();
params.insert("q".into(), Value::Str("chest pain".into()));
params.insert("lim".into(), Value::Int(10));
let res = exec.execute_with_params(
    "QUERY TEXT :q FROM docs LIMIT :lim",
    &params,
    OnError::Stop,
).await.unwrap();
```

### Node.js (`nqql`)
```js
const { Client, bind } = require('@veristamp/nqql');
const client = new Client({ url: "http://localhost:6333" });

// Parameterized execution
const result = await client.execute(
  "QUERY TEXT :q FROM docs WHERE category = :cat LIMIT :lim",
  { params: { q: "chest pain", cat: "medical", lim: 10 } }
);
```

### WebAssembly (`qql-wasm`)
```js
import init, { Client, bind, explain } from 'qql-wasm';
await init();
const client = new Client("http://localhost:6333", null);

// Offline binding & tree explanation
const bound = bind("QUERY TEXT :q FROM docs LIMIT :lim", { q: "chest pain", lim: 10 });
const plan = explain(bound);
const result = await client.execute("QUERY TEXT :q FROM docs LIMIT :lim", {
  params: { q: "chest pain", lim: 10 },
});
```
