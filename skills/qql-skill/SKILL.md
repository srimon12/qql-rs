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

- Semantic similarity -> `QUERY 'text' FROM <collection> USING dense LIMIT <n>`
- Keyword / sparse retrieval -> `QUERY 'text' FROM <collection> USING sparse LIMIT <n>`
- Hybrid retrieval (dense + sparse) -> `QUERY HYBRID TEXT 'text' DENSE dense SPARSE sparse FUSION RRF FROM <collection> LIMIT <n>`
- Hybrid retrieval with DBSF fusion -> `QUERY HYBRID TEXT 'text' DENSE dense SPARSE sparse FUSION DBSF FROM <collection> LIMIT <n>`
- Direct point retrieval by ID -> `QUERY POINTS (id1, id2, 'id3') FROM <collection>`
- Recommendation by example -> `QUERY RECOMMEND POSITIVE (id1, id2) NEGATIVE (id3) STRATEGY average_vector FROM <collection> USING dense LIMIT <n>`
- Context search -> `QUERY CONTEXT (POSITIVE POINT id1 NEGATIVE POINT id2) FROM <collection> USING dense LIMIT <n>`
- Exploration / Discovery search -> `QUERY DISCOVER TARGET POINT id1 CONTEXT (POSITIVE POINT id2 NEGATIVE POINT id3) FROM <collection> USING dense LIMIT <n>`
- Relevance feedback search -> `QUERY RELEVANCE FEEDBACK TARGET 'query_text' FEEDBACK ((1, 0.9), (2, 0.1)) STRATEGY NAIVE (a=1.0, b=0.75, c=0.25) FROM <collection> USING dense LIMIT <n>`
- Random sampling -> `QUERY SAMPLE RANDOM FROM <collection> LIMIT <n>`
- Browse by payload field -> `QUERY ORDER BY <field> [ASC|DESC] FROM <collection> LIMIT <n>`
- Multi-stage retrieval -> `WITH c1 AS (QUERY 't' USING dense LIMIT 100), c2 AS (QUERY 't' USING sparse LIMIT 100) QUERY FUSION RRF FROM <collection> PREFETCH (c1, c2) LIMIT <n>`
- Rerank search -> `WITH c AS (QUERY 't' USING dense LIMIT 50) QUERY RERANK TEXT 't' MODEL 'bge-reranker' FROM <collection> USING colbert PREFETCH (c) LIMIT <n>`
- MMR diversification -> `QUERY MMR 'query_text' DIVERSITY 0.5 CANDIDATES 100 FROM <collection> USING dense LIMIT <n>`
- Formula / Score shaping -> `QUERY FORMULA score + 0.3 * popularity DEFAULTS (popularity = 1.0) FROM <collection> USING dense LIMIT <n>`
- Grouped results -> add `GROUP BY <field> SIZE <m> LOOKUP FROM <collection>`
- Browse points -> `SCROLL FROM <collection> [AFTER <id>] LIMIT <n>`
- Batch ingest -> `UPSERT INTO <collection> VALUES {id: 1, text: '...'}, {id: 2, text: '...'}`
- Delete points -> `DELETE FROM <collection> WHERE <filter>`
- Count points -> `COUNT FROM <collection> WHERE <filter>`
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
CREATE INDEX ON COLLECTION docs FOR title TYPE text WITH (lowercase = true);
DROP INDEX ON COLLECTION docs FOR title;
SHOW COLLECTIONS;
SHOW COLLECTION docs;
DROP COLLECTION docs;
```

### Data Manipulation (DML)
```sql
-- Upsert points with automated text embedding inference
UPSERT INTO docs VALUES
  {id: 1, text: 'Qdrant vector database', category: 'tech'},
  {id: 2, text: 'Rust programming language', category: 'programming'}
  USING DENSE MODEL 'all-minilm:l6-v2';

-- Update vector by point ID
UPDATE docs SET VECTOR dense = [0.1, 0.2, 0.3] WHERE id = 1;

-- Update payload metadata
UPDATE docs SET PAYLOAD = {status: 'reviewed'} WHERE category = 'tech';

-- Delete points
DELETE FROM docs WHERE category = 'obsolete';

-- Count points with filter
COUNT FROM docs WHERE status = 'active';
```

### Universal Query Syntax
Clauses must appear in the exact required order:

```sql
[WITH cte_name AS (QUERY ...), ...]
QUERY <expression>
FROM <collection>
[USING <vector_name>]
[PREFETCH (cte_ref [WHERE <filter>] [SCORE THRESHOLD <number>], ...)]
[WHERE <filter_expression>]
[PARAMS (hnsw_ef = <n>, exact = <bool>, acorn = <bool>)]
[SCORE THRESHOLD <number>]
[GROUP BY <field> [SIZE <n>] [LOOKUP FROM <collection>]]
[WITH PAYLOAD [true | false | INCLUDE (...) | EXCLUDE (...)]]
[WITH VECTOR [true | false | (...)]]
[LIMIT <n>]
[OFFSET <n>];

-- Optional shard routing for multi-tenant collections
SHARD '<tenant_key>'    -- appears after WHERE, before PARAMS
```

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

Shard routing is optional — omit `SHARD` for auto-sharded collections.

### Filters (`WHERE` Clause)
Supports standard comparison operators and predicates:
- Comparisons: `=`, `!=`, `>`, `>=`, `<`, `<=`
- Range: `BETWEEN <min> AND <max>`
- Sets: `IN ('a', 'b')`, `NOT IN ('c', 'd')`
- Null/Empty: `IS NULL`, `IS NOT NULL`, `IS EMPTY`, `IS NOT EMPTY`
- Text Match: `MATCH 'term'`, `MATCH ANY ('term1', 'term2')`, `MATCH PHRASE 'exact phrase'`
- Array / Vector: `HAS_VECTOR 'dense'`, `tags VALUES_COUNT >= 2`
- Geo: `location GEO_BBOX { top_left: {lat: 52.5, lon: 13.4}, bottom_right: {lat: 52.4, lon: 13.5} }`
- Nested: `NESTED('reviews', rating > 4)`
- Logical: `AND`, `OR`, `NOT`

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
use qql::executor::Executor;
use qql::rest::RestQdrant;

let ops = Box::new(RestQdrant::new("http://localhost:6333", None));
let exec = Executor::new(ops, None);
let res = exec.execute("QUERY 'search' FROM docs USING dense LIMIT 5").await?;
```
