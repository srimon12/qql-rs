---
name: qql-skill
description: "Use QQL (Qdrant Query Language) to manage collections, upsert documents, search, filter, rerank, recommend, and execute multi-stage retrieval workflows."
---

# QQL Skill

Use this skill to turn vector retrieval and database intent into valid, canonical QQL statements.
Treat QQL as a typed query language and execution surface for vector databases.

## Reference Wiki

Read these reference documents when you need details on specific topics:
- [references/qql-examples.md](references/qql-examples.md) — Canonical QQL query examples (CTEs, RRF/DBSF, MMR, Discover, Rerank, Formulas, Filters).
- [references/python-sdk.md](references/python-sdk.md) — Python SDK (`pyqql`) client, AST parsing, and filter injection.
- [references/node-sdk.md](references/node-sdk.md) — Node.js SDK (`nqql`) client and N-API methods.
- [references/wasm-sdk.md](references/wasm-sdk.md) — WebAssembly SDK (`qql-wasm`) browser & edge client.
- [references/rust-sdk.md](references/rust-sdk.md) — Rust SDK (`qql`, `qql-core`, `qql-plan`) runtime & executor.
- [references/qql-gaps.md](references/qql-gaps.md) — Read for feature mapping guidelines.
- [references/qql-install.md](references/qql-install.md) — Read for installation and setup instructions across Python, Rust, Node.js, and CLI.
- [references/qql-multitenancy.md](references/qql-multitenancy.md) — Complete multi-tenant guide: shard routing, filter injection, and tenant isolation.

For runnable demo scripts, see `scripts/demo_retrieval_modes.py`, `scripts/demo_medical_records.py`, `scripts/demo_kitchen_sink.py`, and `scripts/demo_multivector.py`.

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
- Delete vectors -> `DELETE VECTOR <name> FROM <collection> WHERE id = N`
- Count points -> `COUNT FROM <collection> WHERE <filter>`
- Create shard key -> `CREATE SHARD KEY '<key>' ON COLLECTION <name> [WITH (shards_number = N, replication_factor = M)]`
- Drop shard key -> `DROP SHARD KEY '<key>' ON COLLECTION <name>`
- Show shard keys -> `SHOW SHARD KEYS ON COLLECTION <name>`
- Multi-tenant isolation -> `QUERY 'text' FROM <collection> WHERE tenant_id = 'honeywell' SHARD 'honeywell' LIMIT 10`

## Canonical Grammar & Capabilities

### Collection Management (DDL)
```sql
CREATE COLLECTION docs (
  dense VECTOR(384, COSINE),
  sparse SPARSE,
  colbert VECTOR(128, COSINE) WITH MULTIVECTOR (comparator = 'max_sim')
) WITH HNSW (m = 16, ef_construct = 100);

ALTER COLLECTION docs WITH VECTOR (on_disk = true);
ALTER COLLECTION docs WITH PARAMS (replication_factor = 3);
ALTER COLLECTION docs WITH QUANTIZATION (type = 'scalar', always_ram = true);

CREATE INDEX ON COLLECTION docs FOR title TYPE text WITH (lowercase = true);
CREATE INDEX ON COLLECTION docs FOR tenant_id TYPE keyword WITH (is_tenant = true);
CREATE INDEX ON COLLECTION docs FOR rating TYPE integer WITH (range = true);

DROP INDEX ON COLLECTION docs FOR title;
SHOW COLLECTIONS;
SHOW COLLECTION docs;

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
[PARAMS (hnsw_ef = <n>, exact = <bool>, acorn = <bool>, indexed_only = <bool>)]
[SCORE THRESHOLD <number>]
[GROUP BY <field> [SIZE <n>] [LOOKUP FROM <collection>]]
[WITH PAYLOAD [true | false | INCLUDE (...) | EXCLUDE (...)]]
[WITH VECTOR [true | false | (...)]]
[LIMIT <n>]
[OFFSET <n>];
```

`SHARD` appears after `WHERE` and before `PARAMS`. Clause order violations produce parse errors.

**Vector roles (critical for embedding):**

| Form | Behavior |
|---|---|
| `USING name` | Runtime looks up `name` on collection schema (dense / sparse / multivector). Names are **not** special-cased by spelling. |
| `USING name AS DENSE` | Single dense embed (MiniLM, CLIP text, …) — one `Vec<f32>` |
| `USING name AS SPARSE` | Sparse BM25-style embed |
| `USING name AS MULTI` | Multivector / ColBERT bag → `[[f32,…],…]` via `embed_multi` (BGE-M3 ColBERT, not CLIP) |
| `USING HYBRID …` | Expand text nearest → dense+sparse fusion (same AST as `QUERY HYBRID`) |
| No `USING` | Schema must have exactly one compatible vector |

Offline/embed-only paths without schema require an explicit `AS …`. Leaving kind unknown fails with `QQL-VECTOR-KIND` (never silent dense default for named targets).

### Shard Routing & Multi-Tenancy

For collections using custom sharding, append `SHARD '<key>'` to route queries and mutations to a specific tenant's shard group:

```sql
-- Create a multi-tenant collection with custom sharding
CREATE COLLECTION sec10k HYBRID (dense VECTOR(768, COSINE), sparse SPARSE)
WITH PARAMS (
  replication_factor = 2,
  shard_number = 8,
  sharding_method = 'custom',
  shard_keys = ['honeywell', 'ge', '3m', 'rtx']
);

-- Query isolated to one tenant
QUERY 'supply chain risks' FROM sec10k
WHERE tenant_id = 'honeywell'
SHARD 'honeywell'
LIMIT 10;

-- Upsert with shard routing
UPSERT INTO sec10k VALUES {id: 1, text: '...', tenant_id: 'honeywell'}
SHARD 'honeywell';

-- Scroll with shard routing
SCROLL FROM sec10k WHERE tenant_id = 'honeywell' SHARD 'honeywell' LIMIT 100;

-- Delete with shard routing
DELETE FROM sec10k WHERE tenant_id = 'honeywell' SHARD 'honeywell';
```

Shard routing is optional — omit `SHARD` for auto-sharded collections. Shard key is supported on `QUERY`, `COUNT`, `SCROLL`, `UPSERT`, and `DELETE`.

### Filters (`WHERE` Clause)
Supports standard comparison operators and predicates:
- Comparisons: `=`, `!=`, `>`, `>=`, `<`, `<=`
- Range: `BETWEEN <min> AND <max>`
- Sets: `IN ('a', 'b')`, `NOT IN ('c', 'd')`
- Null/Empty: `IS NULL`, `IS NOT NULL`, `IS EMPTY`, `IS NOT EMPTY`
- Text Match: `MATCH 'term'`, `MATCH ANY ('term1', 'term2')`, `MATCH PHRASE 'exact phrase'`
- Array / Vector: `HAS_VECTOR 'dense'`, `tags VALUES_COUNT >= 2`
- Geo: `location GEO_BBOX { top_left: {lat: 52.5, lon: 13.4}, bottom_right: {lat: 52.4, lon: 13.5} }`
- Geo radius: `location GEO_RADIUS { center: {lat: 48.85, lon: 2.35}, radius: 5000 }`
- Nested: `NESTED('reviews', rating > 4)`
- Logical: `AND`, `OR`, `NOT`

## Query Planning & Execution Architecture

QQL uses a prepare → plan → dispatch pipeline shared by all SDKs and the CLI:

```
Phase 1: Parse (qql-core)
  QQL string -> AST (Stmt enum)
  USING name without AS keeps kind: null (source fidelity)

Phase 2: Prepare (qql-runtime / WASM Client)
  1. Schema topology: resolve USING kinds + multivector flags
     (dense / sparse names + multivector_config → multi)
  2. Embeddings (qql-embed): text → Dense | Sparse | MultiDense
     - kind unknown without schema → QQL-VECTOR-KIND (fail closed)
     - multi targets call Embedder::embed_multi

Phase 3: Plan (qql-plan)
  AST -> PlannedOperation (canonical, transport-neutral)
  MultiDense serializes as array-of-arrays on REST / multi_dense on gRPC
  to_rest_route() -> Route { method, path, body }

Phase 4: Dispatch
  Smart batching: same-collection QUERYs -> /points/query/batch,
                   same-collection mutations -> /points/batch
  REST / gRPC / Edge backends
```

DDL (`CREATE COLLECTION`, `ALTER`, `CREATE INDEX`, etc.) and DML (`QUERY`, `UPSERT`, `DELETE`, etc.) all flow through the same plan-then-dispatch path.


### Backend Limitations

| Backend | Limitations |
|---------|-------------|
| REST | None |
| gRPC | Requires `--features grpc` at build time |
| Edge (`qdrant-edge`) | No shard key management (CreateShardKey returns error); in-process HNSW; no persistence unless `--on-disk` |

## CLI Reference

```text
qql exec <query> [--json] [--quiet]          Execute a single QQL query
qql execute <file.qql> [--stop-on-error]     Execute statements from file
qql explain <query> [--json] [--quiet]       Show execution plan (no Qdrant needed)
qql connect                                   Interactive REPL
qql convert [file.json]                       Convert REST JSON to QQL
qql dump <collection> <output.qql> [options]  Dump collection to QQL script
qql doctor [--json] [--quiet]                 Check Qdrant connection health
qql edge <query> [options]                    Execute against local qdrant-edge
qql version                                   Show version

Global: --url <URL> (overrides QDRANT_URL env, default http://localhost:6333)
```

## Execution via SDKs

### Python (`pyqql`)
```python
import pyqql

embedder = pyqql.HttpEmbedder("http://localhost:11434/v1/embeddings", "all-minilm:l6-v2", 384)
client = pyqql.Client("http://localhost:6333", embedder=embedder)

result = client.execute("QUERY 'semantic search' FROM docs USING dense LIMIT 5")
```

### Rust (`qql`)
```rust
use qql::executor::{Executor, OnError};

let exec = Executor::rest("http://localhost:6333", None).unwrap();
let res = exec.execute(
    "QUERY 'search' FROM docs USING dense LIMIT 5",
    OnError::Stop,
).await.unwrap();
```

### Node.js (`nqql`)
```js
const { Client } = require('@veristamp/nqql');
const client = new Client({ url: "http://localhost:6333" });
const result = await client.execute("QUERY 'search' FROM docs USING dense LIMIT 5");
```

### WebAssembly (`qql-wasm`)
```js
import init, { Client } from 'qql-wasm';
await init();
const client = new Client("http://localhost:6333", null);
const result = await client.execute("QUERY 'search' FROM docs USING dense LIMIT 5");
```
