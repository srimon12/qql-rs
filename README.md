<div align="center">

  <img src="https://raw.githubusercontent.com/srimon12/qql-rs/main/docs/assets/qql-banner.png" alt="QQL banner: the QQL wordmark, the words Query Language for Qdrant, a sample search.qql query, and the host list Rust, Python, Node, WASM, and Edge" width="600" />

  # QQL: SQL for Qdrant and vector search

  [![CI](https://github.com/srimon12/qql-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/srimon12/qql-rs/actions/workflows/ci.yml)
  [![Release](https://img.shields.io/github/v/release/srimon12/qql-rs?color=blue)](https://github.com/srimon12/qql-rs/releases)
  [![Crates.io](https://img.shields.io/crates/v/qql.svg)](https://crates.io/crates/qql)
  [![PyPI](https://img.shields.io/pypi/v/pyqql.svg)](https://pypi.org/project/pyqql/)
  [![npm](https://img.shields.io/npm/v/@veristamp/nqql.svg)](https://www.npmjs.com/package/@veristamp/nqql)
  [![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

  [Documentation](https://qql.veristamp.in/docs/) • [Syntax](docs/syntax.md) • [Playground](https://qql.veristamp.in/playground/) • [VS Code extension](https://marketplace.visualstudio.com/items?itemName=srimon12.qql-lang) • [Python SDK](crates/pyqql/README.md) • [Node.js SDK](crates/nqql/README.md)

</div>

QQL is the Qdrant Query Language, a SQL-style language and toolchain for the Qdrant vector database. It covers vector search, hybrid search, semantic search, filtering, multitenancy, and DDL. One grammar and one typed plan run from the CLI, Python, Node.js, Rust, WebAssembly, and an in-process edge engine, over REST or gRPC, against the same Qdrant server or no server at all.

## One query, every host

The same statement runs on every host. Each client below sends it as the `sql` string:

```sql
QUERY [0.12, 0.45, 0.78, 0.03] FROM articles USING dense LIMIT 10;
```

```python
import pyqql

report = pyqql.Client("http://localhost:6333").execute(sql)
```

```javascript
const { Client } = require("@veristamp/nqql");

const report = await new Client({ url: "http://localhost:6333" }).execute(sql);
```

```rust
use qql::executor::{Executor, OnError};

// in an async fn
let report = Executor::grpc("http://localhost:6334", None)?.execute(sql, OnError::Stop).await?;
```

```javascript
import init, { Client } from "qql-wasm";

await init();
const report = await new Client("http://localhost:6333", null).execute(sql);
```

The CLI runs the same text with `qql run`. The edge SDKs run it with no server at all: `pyqql-edge` and `@veristamp/nqql-edge` ship local executors backed by an in-process qdrant-edge store with FastEmbed models, and the Rust `qql-edge` crate exposes the same executor over its `EdgeQdrant` backend.

```python
import pyqql_edge

client = pyqql_edge.local_executor("./qdrant_data", model="BGESmallENV15")
report = client.execute("QUERY 'offline search' FROM notes USING dense LIMIT 5")
```

## Why QQL

Qdrant's official Python client is the reference client for building requests, and it stays the right choice for Python applications. QQL adds a language layer over the same operations, so one search can be written either way:

```python
from qdrant_client import QdrantClient
from qdrant_client.models import FieldCondition, Filter, MatchValue, Range

client = QdrantClient("http://localhost:6333")
points = client.query_points(
    collection_name="articles",
    query=[0.12, 0.45, 0.78, 0.03],
    using="dense",
    query_filter=Filter(must=[
        FieldCondition(key="category", match=MatchValue(value="tech")),
        FieldCondition(key="year", range=Range(gte=2024)),
    ]),
    limit=5,
).points
```

```sql
-- The same search in QQL
QUERY [0.12, 0.45, 0.78, 0.03]
FROM articles
USING dense
WHERE category = 'tech' AND year >= 2024
LIMIT 5;
```

What the language layer adds:

- **Search.** Dense, sparse, and hybrid retrieval with RRF or DBSF fusion, reranking, faceting, grouped search, scroll, and count.
- **Batching.** Several queries or mutations from one collection travel in a single request. `BATCH { ... }` maps to `/points/query/batch` or `/points/batch`.
- **Embeddings in the pipeline.** Text resolves to dense vectors or wire-compatible BM25 sparse vectors during execution. Application code does not call an embedding service separately.
- **Shard routing.** `SHARD 'tenant'` becomes a request-level shard key on REST and gRPC, so a tenant query does not fan out across partitions.
- **Typed plan.** The planner emits one transport-neutral `PlannedOperation`. REST, gRPC, and the edge engine are projections of it, so every transport executes the same plan.
- **Standalone runtimes.** PyO3, N-API, and wasm-bindgen bindings, plus the Rust crate, talk to Qdrant directly.
- **Fail-closed isolation.** `inject_filter` rewrites the AST with a tenant predicate and rejects statements it cannot rewrite.
- **Qdrant 1.19 surface.** Quotas, memory placement, `MATCH PREFIX`, `SLICE`, sparse `idf`, and `turbo4`, on the backends that support each.

## Quickstart

```bash
# Linux and macOS (Standard)
curl -fsSL https://qql.veristamp.in/install.sh | bash

# Linux and macOS (Full: with ONNX + embedded edge)
curl -fsSL https://qql.veristamp.in/install.sh | bash -s -- --full

# Windows (PowerShell - Standard)
irm https://qql.veristamp.in/install.ps1 | iex

# Windows (PowerShell - Full: with ONNX + embedded edge)
& ([scriptblock]::Create((irm https://qql.veristamp.in/install.ps1))) -Full

# or install from crates.io
cargo install qql-cli --locked
cargo install qql-cli --locked --features full

# Run a query, run a script, or open the REPL
qql run "QUERY [0.12, 0.45, 0.78, 0.03] FROM articles LIMIT 5"
qql --url http://localhost:6333 run "QUERY 'machine learning' FROM papers LIMIT 5"
qql repl --url http://localhost:6333
```

Sparse text embeds locally with BM25. Dense text needs an embedding endpoint (`EMBED_URL`) or the edge backend with its FastEmbed models.

| Host | Install | Reference |
|------|---------|-----------|
| Python | `pip install pyqql` | [Python SDK](crates/pyqql/README.md) |
| Node.js | `npm install @veristamp/nqql` | [Node.js SDK](crates/nqql/README.md) |
| Rust | `cargo add qql` | [Rust crate](crates/qql-runtime/README.md) |
| WASM | `npm install qql-wasm` | [WASM SDK](crates/qql-wasm/README.md) |
| Python edge | `pip install pyqql-edge` | [Edge SDK](crates/pyqql-edge/README.md) |
| Node.js edge | `npm install @veristamp/nqql-edge` | [Edge SDK](crates/nqql-edge/README.md) |

Python:

```python
import pyqql

client = pyqql.Client("http://localhost:6333")  # use_grpc=True for gRPC
report = client.execute(
    "QUERY [0.12, 0.45, 0.78, 0.03] FROM articles WHERE category = 'tech' LIMIT 5"
)
for hit in report.hits():
    print(hit.id, hit.score, hit.get("title"))
```

Node.js:

```javascript
const { Client } = require("@veristamp/nqql");

const client = new Client({ url: "http://localhost:6333" }); // useGrpc: true for gRPC
const report = await client.execute(
  "QUERY [0.12, 0.45, 0.78, 0.03] FROM articles WHERE category = 'tech' LIMIT 5",
);
for (const hit of report.hits()) console.log(hit.id, hit.score, hit.payload);
```

Rust:

```rust
use qql::executor::{Executor, OnError};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let exec = Executor::grpc("http://localhost:6334", None)?; // Executor::rest for REST
    let report = exec.execute("QUERY [0.12, 0.45, 0.78, 0.03] FROM articles LIMIT 5", OnError::Stop).await?;
    for hit in report.hits(0).unwrap_or_default() {
        println!("{} {:.3}", hit.id, hit.score);
    }
    Ok(())
}
```

## Query patterns

### Vector and text search

The `VECTOR` and `TEXT` keywords are optional.

```sql
QUERY [0.12, 0.45, 0.78, 0.03]
FROM articles
USING dense
WHERE category = 'tech' AND year >= 2024
LIMIT 5;

QUERY 'distributed consensus protocols' FROM papers WHERE year >= 2023 LIMIT 5;
```

### Hybrid search

Dense plus BM25 in one statement, fused with RRF (the default) or DBSF.

```sql
QUERY HYBRID TEXT 'latency optimization' FUSION RRF FROM docs LIMIT 10;

QUERY HYBRID TEXT 'latency optimization' DENSE dense SPARSE bm25 FUSION RRF
FROM docs
LIMIT 10;
```

### Reranking

Late-interaction rerank over prefetch candidates, or a client-side cross-encoder pass.

```sql
WITH
  candidates AS (QUERY 'vector database latency' FROM docs USING dense LIMIT 100)
QUERY RERANK TEXT 'vector database latency' MODEL 'answerai-colbert-small-v1'
FROM docs
USING colbert AS MULTI
PREFETCH (candidates)
LIMIT 10;

WITH
  candidates AS (QUERY 'vector database latency' FROM docs USING dense LIMIT 50)
QUERY CROSS RERANK TEXT 'vector database latency' MODEL 'bge-reranker-base' ON FIELD text
FROM docs
PREFETCH (candidates)
LIMIT 10;
```

### Fusion with prefetch CTEs

Compose sub-queries and fuse their rankings. Swap RRF for DBSF when score scales differ.

```sql
WITH
  v_dense AS (QUERY 'latency optimization' FROM docs USING dense LIMIT 50),
  v_sparse AS (QUERY 'latency optimization' FROM docs USING sparse LIMIT 50)
QUERY FUSION RRF
FROM docs
PREFETCH (v_dense, v_sparse)
LIMIT 10;
```

### Batching

Each block is one roundtrip to `/points/query/batch` or `/points/batch`.

```sql
BATCH {
  QUERY [0.1, 0.2, 0.3] FROM products WHERE category = 'tech' LIMIT 5;
  QUERY [0.8, 0.9, 0.1] FROM products WHERE category = 'books' LIMIT 5;
};

BATCH {
  UPSERT INTO users VALUES {id: 1, vector: [0.1, 0.2], role: 'admin'};
  DELETE FROM users WHERE last_login < '2023-01-01T00:00:00Z';
};
```

### Multitenancy

`SHARD` routes the request to a tenant partition. Filter injection isolates the data. Routing alone is not a security boundary.

```sql
QUERY [0.12, 0.45, 0.78, 0.03]
FROM financial_records
WHERE tenant_id = 'tenant_corp_99' AND status = 'audited'
SHARD 'tenant_corp_99'
LIMIT 10;
```

```python
stmt = pyqql.parse("QUERY [0.12, 0.45, 0.78, 0.03] FROM financial_records LIMIT 10")[0]
pyqql.inject_filter(stmt, "tenant_id", "=", "tenant_corp_99")
client.execute(stmt)
```

### Facets

Value counts computed inside Qdrant.

```sql
FACET category FROM products WHERE in_stock = true AND price <= 500.0 LIMIT 10 EXACT true;
```

### Parameters

Bind `:name` from a dict or object, `?` from a list or array. Values are escaped at bind time.

```sql
QUERY :query
FROM articles
WHERE author = :author AND category = :category
LIMIT :limit;
```

### Ingestion

Points are data, so vectors or text travel with the payload.

```sql
UPSERT INTO products VALUES
  {id: 1, vector: [0.12, 0.45, 0.78], title: 'Mechanical Keyboard', price: 129.99},
  {id: 2, vector: [0.89, 0.22, 0.05], title: 'Wireless Mouse', price: 49.99};

UPSERT INTO docs VALUES {id: 3, title: 'portable keyboard cover'} USING DENSE MODEL 'all-minilm:l6-v2';

DELETE FROM products WHERE in_stock = false AND updated_at < '2024-01-01T00:00:00Z';
```

### Reads

Fetch points by ID or count a filtered subset.

```sql
QUERY POINTS (1, 2, 'point-a') FROM docs;

COUNT FROM products WHERE in_stock = true;
```

### DDL

Collections, payload indexes, and tenant partitions are statements too.

```sql
CREATE COLLECTION articles (
  dense VECTOR(384, COSINE),
  sparse SPARSE
);

CREATE INDEX ON COLLECTION articles FOR category TYPE keyword;

CREATE INDEX ON COLLECTION articles FOR tenant_id TYPE keyword WITH (is_tenant = true);

CREATE SHARD KEY 'acme' ON COLLECTION articles WITH (shards_number = 2);
```

## Migration and tooling

### qql migrate

`qql migrate` copies a collection as schema plus points, so the target can differ in version, shard count, sharding method, or quantization. Checkpoints land under `.qql-migrate/` and resume an interrupted run without re-sending points. Verification compares exact counts by default.

```bash
# Cross-cluster migration with tenant shard promotion and cutover
qql --url grpc://old-cluster:6334 migrate docs \
  --target-url grpc://new-cluster:6334 \
  --to docs \
  --shard-key-field tenant_id \
  --cutover docs \
  --recreate

# Re-quantize to 4-bit TurboQuant during the stream
qql --url grpc://localhost:6334 migrate articles \
  --to articles_turbo4 \
  --quantize turbo --quantize-bits 4 \
  --workers 4
```

Full guides: [migration guide](crates/qql-cli/src/migrate/README.md), [quickstart](crates/qql-cli/src/migrate/QUICKSTART.md).

### qql convert

`qql convert` turns Qdrant REST JSON, HTTP snippets, or curl command text into canonical QQL. It reads from a file or stdin.

```bash
qql convert search.json                 # wrapped request
qql convert --collection docs body.json # bare REST body
qql convert curl.txt                    # a pasted curl command
```

### qql record
 
`qql record` is a transparent proxy built directly into the CLI that captures live traffic from any application and writes `.qql` files.

```bash
qql record --listen 127.0.0.1:6334 --target http://127.0.0.1:6333 --qql-out app_queries.qql
```

### Editor and agent tooling

- [VS Code and Cursor extension](https://marketplace.visualstudio.com/items?itemName=srimon12.qql-lang): syntax highlighting, live WASM diagnostics, plan hovers, CodeLens, and completions for `.qql` files.
- [Web playground](https://qql.veristamp.in/playground/): run queries and inspect compiled plans in the browser, using the WASM build.
- [Agent skill](skills/qql-skill/README.md): reference map for coding agents (`npx skills add srimon12/qql-rs --skill qql-skill`).

## Documentation

| Guide | Covers |
|-------|--------|
| [Documentation site](https://qql.veristamp.in/docs/) | Rendered guides for the language, SDKs, and operations |
| [Documentation index](docs/README.md) | Architecture, typed pipeline, Qdrant 1.19 language highlights |
| [Syntax reference](docs/syntax.md) | `QUERY`, `HYBRID`, `RERANK`, `BATCH`, `FUSION`, `FACET`, DML, DDL |
| [Filters and operators](docs/filters.md) | `WHERE`, `IN`, ranges, geo predicates, `MATCH PREFIX`, `SLICE` |
| [Parameters](docs/parameters.md) | `:name` and `?` binding, prepared statements |
| [Multitenancy](docs/inject_filter.md) | Shard routing, isolation, AST filter injection |
| [CLI reference](crates/qql-cli/README.md) | Commands, configuration, edge backend, script format |
| [Cluster migration](crates/qql-cli/src/migrate/README.md) | `qql migrate`, in-flight quantization, resharding |
| [Convert and record](skills/qql-skill/references/convert-migration.md) | curl and REST JSON conversion, `qql record` proxy |
| [Python SDK](crates/pyqql/README.md) | PyO3 client, async usage, offline compilation |
| [Node.js SDK](crates/nqql/README.md) | N-API client, TypeScript types, streaming scroll |
| [Rust crate](crates/qql-runtime/README.md) | Executor, REST and gRPC backends, custom embedders |
| [Edge mode](crates/qql-edge/README.md) | In-process database, zero network |
| [Examples](examples/README.md) | Multi-tenant RAG over SEC 10-K filings, Berlin geo search, and more |

## Compatibility

- Qdrant server: `>= 1.19.0` (quotas, `memory` tiers, `MATCH PREFIX`, `SLICE`, sparse `idf`, `turbo4`)
- Python: `3.10+`
- Node.js: `18+`
- Rust: `1.98+` (2024 edition)
- Edge: qdrant-edge `0.8`, no server. Cluster-only features such as custom `SHARD`, quotas, and `GROUP BY ... LOOKUP FROM` return `QQL-EDGE-UNSUPPORTED-*` codes; use remote Qdrant for those.

---

<div align="center">
  <sub>QQL is open source under the MIT License. Not affiliated with Qdrant.</sub>
</div>
