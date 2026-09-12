---
name: qql-skill
description: "Use QQL (Qdrant Query Language) to manage collections, upsert documents, search, filter, rerank, recommend, and execute multi-stage retrieval workflows."
---

# QQL Skill

Turn retrieval intent into valid, current QQL and correct SDK usage.

QQL is the typed language plus plan IR for Qdrant.

1. One grammar for search, hybrid, multivector, mutations, DDL, multitenancy.
2. One plan (`PlannedOperation`). gRPC and REST are equal projections, not REST-first.
3. Host isolation via `inject_filter` (AST). Routing via `SHARD '...'` or `stmt.shard_key`.
4. Never invent syntax listed as open in `references/qql-gaps.md`.

Human docs (product-facing): `docs/`. This skill is for agents writing QQL and SDK code.

## Reference wiki

| Doc | When to open it |
|-----|-----------------|
| [qql-query.md](references/qql-query.md) | All 13 `QUERY` forms, prefetch, fusion, rerank, formula, hybrid |
| [qql-filters.md](references/qql-filters.md) | All 20 `FilterExpr` forms, logic, geo, text match |
| [qql-mutations.md](references/qql-mutations.md) | `UPSERT`, `DELETE`, payload and vector mutations, conditional writes |
| [qql-read.md](references/qql-read.md) | `SCROLL`, `COUNT`, `FACET`, `GROUP BY`, `BATCH`, ordering and paging |
| [qql-ddl.md](references/qql-ddl.md) | Collections, indexes, shard keys, quotas, memory and quantization |
| [qql-params.md](references/qql-params.md) | Placeholders, binding, prepared statements, inference options |
| [qql-embeddings.md](references/qql-embeddings.md) | `USING` roles, dense and sparse and multi, BM25, CLIP, ColBERT |
| [qql-multitenancy.md](references/qql-multitenancy.md) | `SHARD KEY` DDL versus `SHARD` routing versus `inject_filter` |
| [inject-filter.md](references/inject-filter.md) | Fail-closed tenant and policy injection |
| [convert-migration.md](references/convert-migration.md) | `qql convert` and `qql record`, cluster and edge migration, dump |
| [cli.md](references/cli.md) | `qql` CLI, REPL, `exec` and `execute`, `explain` and `fmt`, `doctor` |
| [qql-install.md](references/qql-install.md) | Install CLI and SDKs, backend version matrix |
| [qql-gaps.md](references/qql-gaps.md) | Open versus closed. Do not invent open syntax |
| [python-sdk.md](references/python-sdk.md) | `pyqql` full guide |
| [node-sdk.md](references/node-sdk.md) | `@veristamp/nqql` full guide |
| [wasm-sdk.md](references/wasm-sdk.md) | `qql-wasm` full guide |
| [rust-sdk.md](references/rust-sdk.md) | `qql-core` and `qql-plan` and `qql` full guide |

Runnable QQL: `examples/`. Runnable Python demos: `scripts/demo_*.py`.

## Intent map

Translate intent directly into QQL. Full forms live in the references above.

- Semantic search becomes `QUERY 'text' FROM c USING dense LIMIT n`.
- Keyword search becomes `QUERY 'text' FROM c USING sparse LIMIT n`.
- Hybrid becomes `QUERY TEXT 'text' FROM c USING HYBRID DENSE dense SPARSE sparse FUSION RRF LIMIT n`.
- Multivector nearest becomes `QUERY TEXT 't' FROM c USING colbert LIMIT n`. Offline use `USING colbert AS MULTI`.
- ColBERT rerank becomes `WITH c AS (QUERY 't' USING dense LIMIT 50) QUERY RERANK TEXT 't' MODEL 'answerai-colbert-small-v1' FROM c USING colbert PREFETCH (c) LIMIT n`.
- Cross-encoder rerank becomes `WITH c AS (QUERY 't' USING dense LIMIT 50) QUERY CROSS RERANK TEXT 't' MODEL 'bge-reranker-base' ON FIELD text FROM c PREFETCH (c) LIMIT n`.
- Point fetch becomes `QUERY POINTS (id1, id2) FROM c`.
- Recommend becomes `QUERY RECOMMEND POSITIVE (id1) NEGATIVE (id2) STRATEGY average_vector FROM c USING dense LIMIT n`.
- Context becomes `QUERY CONTEXT (POSITIVE POINT id1 NEGATIVE POINT id2) FROM c USING dense LIMIT n`.
- Discover becomes `QUERY DISCOVER TARGET POINT id1 CONTEXT (POSITIVE POINT id2 NEGATIVE POINT id3) FROM c USING dense LIMIT n`.
- Relevance feedback becomes `QUERY RELEVANCE FEEDBACK TARGET TEXT 'text' FEEDBACK ((POINT 1, 0.9)) STRATEGY NAIVE (a=1.0, b=0.75, c=0.25) FROM c USING dense LIMIT n`.
- Random sample becomes `QUERY SAMPLE RANDOM FROM c LIMIT n`.
- Ordered browse becomes `QUERY ORDER BY field DESC FROM c LIMIT n`.
- Multi-stage becomes `WITH a AS (...), b AS (...) QUERY FUSION RRF FROM c PREFETCH (a, b) LIMIT n`.
- MMR becomes `QUERY MMR 'text' DIVERSITY 0.5 CANDIDATES 100 FROM c USING dense LIMIT n`.
- Vector literal becomes `QUERY [0.1, 0.2] FROM c USING dense LIMIT n`. `VECTOR` keyword is optional.
- Formula becomes `QUERY FORMULA score + 0.3 * EXP_DECAY(published_at, TARGET = "2024-01-01T00:00:00Z", SCALE = 630720000) FROM c USING dense LIMIT n`.
- Facet becomes `FACET field FROM c WHERE filter LIMIT n EXACT true`.
- Batch becomes `BATCH { stmt; stmt; }`. Members share one collection and one family.
- Scroll becomes `SCROLL FROM c WHERE filter LIMIT n`.
- Count becomes `COUNT FROM c WHERE filter`.
- Upsert becomes `UPSERT INTO c VALUES {id: 1, text: '...'}, {id: 2, text: '...'}`.
- Conditional upsert becomes `UPSERT INTO c VALUES {...} UPDATE FILTER filter UPDATE MODE insert_only|update_only|upsert`.
- Payload write becomes `UPDATE c SET PAYLOAD = {...} KEY 'a.b' OVERWRITE WHERE filter`. Lone `OVERWRITE` runs only inside `BATCH`.
- Delete becomes `DELETE FROM c WHERE filter`.
- Clear payload becomes `CLEAR PAYLOAD FROM c WHERE filter`.
- Delete payload keys becomes `DELETE PAYLOAD k1, k2 FROM c WHERE filter`.
- Delete vectors becomes `DELETE VECTOR name FROM c WHERE id = N`.
- Grouped results append `GROUP BY field SIZE m LOOKUP FROM other WITH PAYLOAD INCLUDE (...) LIMIT n`.
- Prefetch routing uses `PREFETCH (c LOOKUP FROM other VECTOR name SHARD 'key')`.

Payloads are included by default. `QUERY` returns payloads unless `WITH PAYLOAD false` strips them. Do not add redundant `WITH PAYLOAD true`.

## Clause order

Clauses must appear in this order. Parse fails otherwise.

```sql
[WITH c AS (QUERY ...), ...]
QUERY <expression>
FROM <collection>
[USING HYBRID [DENSE <vector>] [SPARSE <vector>] [FUSION RRF|DBSF]
 | USING <name> [AS DENSE | AS SPARSE | AS MULTI | AS MULTIVECTOR]]
[PREFETCH (ref [WHERE <filter>] [SCORE THRESHOLD <n>] [LOOKUP FROM <c> [VECTOR <v>] [SHARD <key>]], ...)]
[WHERE <filter>]
[SHARD '<key>' | SHARD <int>]
[PARAMS (...)]
[SCORE THRESHOLD <n>]
[GROUP BY <field> [SIZE <n>] [LOOKUP FROM <c> [WITH PAYLOAD ...] [WITH VECTOR ...]]]
[WITH PAYLOAD [true | false | INCLUDE (...) | EXCLUDE (...)]]
[WITH VECTOR [true | false | (...)]]
[LIMIT <n>]
[OFFSET <n>]
```

`SHARD` sits after `WHERE` and before `PARAMS`. `OFFSET` works with `GROUP BY` as `group_offset`.

## Vector roles

| Form | Behavior |
|---|---|
| `USING name` | Runtime resolves `name` from collection schema. Names are never special-cased by spelling |
| `USING name AS DENSE` | One dense vector. MiniLM, CLIP text, precomputed `VECTOR [...]` |
| `USING name AS SPARSE` | Sparse vector. Wire-compatible BM25. Unit-weight queries, tf-saturated documents |
| `USING name AS MULTI` | Multivector bag `[[f32]]` via `embed_multi`. BGE-M3 ColBERT, not CLIP |
| `USING HYBRID ...` | Text expands to dense plus sparse fusion. Same AST as `QUERY HYBRID` |
| No `USING` | Schema must hold exactly one compatible vector |

Offline or embed-only paths without schema require explicit `AS ...`. Unknown kind fails closed with `QQL-VECTOR-KIND`. There is no silent dense default.

## Shard routing and multitenancy

| Keyword | Kind | Meaning |
|---------|------|---------|
| `CREATE` and `DROP` and `SHOW SHARD KEY` | DDL | Define and list custom partition names |
| `SHARD 'key'` on a statement | DML routing | Route this request via `shard_key` and `ShardKeySelector` |

```sql
QUERY TEXT 'supply chain risks' FROM sec10k USING dense
WHERE tenant_id = 'honeywell'
SHARD 'honeywell'
LIMIT 10;
```

Security is `inject_filter(..., "tenant_id", "=", tenant)` on untrusted QQL. Routing is `SHARD '...'` in QQL or `stmt.shard_key = tenant` after parse. There is no `inject_shard_key`. Full guide is `references/qql-multitenancy.md`.

Route affinity is not QQL syntax. It is a client transport option. Rust uses `with_route_affinity`, Python uses `Client(route_affinity=...)`, Node uses `{ routeAffinity }`, WASM uses `setRouteAffinity`.

## Planning and execution

```text
source or host AST
  -> qql-core parses and validates into Stmt
  -> runtime prepares: schema topology fills USING kinds, qql-embed resolves vectors
  -> qql-plan plans into PlannedOperation
  -> REST via to_rest_route, gRPC via typed proto conversion, edge in-process
```

`filter` and `shard_key` are siblings on the request. Neither nests routing inside the filter object.

| Backend | Notes |
|---------|-------|
| REST | Full matrix including `SHOW QUOTAS` and `SET QUOTA` |
| gRPC | Typed plan to proto. No public quota service (`QQL-GRPC-QUOTA`) |
| Edge | No quotas, no shard-key admin, no `GROUP BY ... LOOKUP FROM`. `GROUP BY` and ACORN and IDF work on edge 0.8 plus |

## Parameters

Named placeholders use `:name`. Positional placeholders use `?`. Dotted paths bind nested dicts. `{"loc": {"lat": 1.0}}` binds `:loc.lat`. Batch binds one param entry per statement. `bind(query, params, truncate_vectors=True)` keeps logs readable. `$name` is an identifier, never a placeholder. RRF tuning lives in `PARAMS (rrf_k = 60, rrf_weights = [...])`, never in `WITH (...)`. `USING bm25` defaults model to `Qdrant/bm25`. Full rules live in `references/qql-params.md`.

## CLI

```text
qql exec <query> [--json]              Execute one QQL statement
qql execute <file.qql>                 Execute a script file
qql explain <query> [--json]           Print the plan tree, no I/O
qql convert [file.json]                REST JSON to QQL
qql record [--listen A] [--target B]   Capture live traffic plus QQL
qql fmt [file.qql] [--check] [--write] Canonical formatter
qql dump <coll> <out.qql>              Dump collection to QQL
qql migrate <coll> [--to X]            Copy schema plus points, with verify and cutover
qql doctor [--json]                    Connection health and model hosts
qql connect | qql repl                 Interactive REPL
qql edge bootstrap | qql --edge ...    Edge seed and local execution
qql version                            Version
```

Global `--url` overrides `QDRANT_URL`. Default is `http://localhost:6333`. Full flags live in `references/cli.md`.

## SDK quickstart

Python, Node, WASM, and Rust share one binding contract. Dict and object bind named params. List and array bind positional params. WASM takes a JS object or array, never a JSON string. Rust keeps typed `bind_named` and `bind_positional`.

```python
import pyqql
client = pyqql.Client("http://localhost:6333")
report = client.execute("QUERY 'search' FROM docs USING dense LIMIT 5")
hits = report.hits()
```

```js
const { Client } = require('@veristamp/nqql');
const client = new Client({ url: "http://localhost:6333" });
const report = await client.execute("QUERY 'search' FROM docs USING dense LIMIT 10");
```

```js
import init, { Client } from 'qql-wasm';
await init();
const client = new Client("http://localhost:6333", null);
const result = await client.execute("QUERY 'search' FROM docs USING dense LIMIT 10");
```

```rust
use qql::executor::{Executor, OnError};
let exec = Executor::rest("http://localhost:6333", None).unwrap();
let res = exec.execute("QUERY 'search' FROM docs USING dense LIMIT 5", OnError::Stop).await.unwrap();
```

Each SDK reference is self-contained. Open only the one you need. Shared language rules live in `qql-query.md` and `qql-filters.md` and `qql-params.md`, repeated in SDK guides only where the guide needs them to run alone.
