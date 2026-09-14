<div align="center">

  <img src="https://raw.githubusercontent.com/srimon12/qql-rs/main/docs/assets/qql-banner.png" alt="QQL Banner" width="600" />

  # QQL — Declarative SQL for Qdrant & Vector Search

  **QQL is to Qdrant what SQL is to PostgreSQL.**
  One language for dense, sparse, and hybrid vector search, filtering, multitenancy, and analytics.

  [![CI](https://github.com/srimon12/qql-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/srimon12/qql-rs/actions/workflows/ci.yml)
  [![Release](https://img.shields.io/github/v/release/srimon12/qql-rs?color=blue)](https://github.com/srimon12/qql-rs/releases)
  [![Crates.io](https://img.shields.io/crates/v/qql.svg)](https://crates.io/crates/qql)
  [![PyPI](https://img.shields.io/pypi/v/pyqql.svg)](https://pypi.org/project/pyqql/)
  [![npm](https://img.shields.io/npm/v/@veristamp/nqql.svg)](https://www.npmjs.com/package/@veristamp/nqql)
  [![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

  [Documentation](docs/README.md) • [Syntax Guide](docs/syntax.md) • [Interactive Playground](https://qql.veristamp.com/playground/) • [VS Code Extension](https://marketplace.visualstudio.com/items?itemName=srimon12.qql-lang) • [Python SDK](crates/pyqql) • [Node.js SDK](crates/nqql)

</div>

---

## Contents

- [Why QQL?](#why-qql)
- [Quickstart](#quickstart)
- [Query Patterns](#query-patterns)
- [Migration Suite](#migration-suite)
- [Tools](#tools)
- [Documentation](#documentation)
- [Compatibility](#compatibility)

---

## Why QQL?

The official SDKs make you spell out every search as nested builder objects. QQL says the same thing in 4 lines:

```python
# qdrant-client: builder soup for a single search
client.query_points(
    collection_name="articles",
    query=[0.12, 0.45, 0.78, 0.03],
    using="dense",
    query_filter=Filter(must=[
        FieldCondition(key="category", match=MatchValue(value="tech")),
        FieldCondition(key="year", range=Range(gte=2024)),
    ]),
    limit=5,
)
```

```sql
-- In QQL: Clean, readable, and intuitive
QUERY [0.12, 0.45, 0.78, 0.03] FROM articles
USING dense
WHERE category = 'tech' AND year >= 2024
LIMIT 5;
```

- **Less boilerplate**: nested builder objects become self-documenting SQL.
- **Automatic vector and text detection**: pass float arrays `[0.1, 0.2, ...]` or string literals `'search text'` directly — `VECTOR` and `TEXT` keywords are optional.
- **One-line hybrid and rerank**: Dense + BM25 via `QUERY HYBRID '...'`, two-stage scoring with `QUERY RERANK`.
- **Batching**: multiple queries or mutations in one network roundtrip with `BATCH { ... }`.
- **In-process embeddings**: dense vectors and wire-compatible BM25 sparse vectors resolved on the fly, no glue code.
- **Multitenancy**: `SHARD 'tenant_1'` partition routing plus AST-level `inject_filter` for tenant isolation.
- **Two transports**: compiles to Qdrant REST routes or typed gRPC protobuf (`tonic`).
- **Embedded mode**: in-process vector search with no server (`qdrant-edge` + `fastembed-rs`).
- **Standalone runtimes**: no dependency on official SDK wrappers. PyO3 (`pyqql`), N-API (`nqql`), WebAssembly (`qql-wasm`), Rust (`qql`).

---

## Quickstart

<details>
<summary><strong>CLI</strong> — query any Qdrant instance, interactive REPL</summary>

```bash
# Linux & macOS
curl -fsSL https://raw.githubusercontent.com/srimon12/qql-rs/main/scripts/install.sh | sh

# Windows (PowerShell)
irm https://raw.githubusercontent.com/srimon12/qql-rs/main/scripts/install.ps1 | iex
```

```bash
# Raw vector floats
qql --url http://localhost:6333 "QUERY [0.12, 0.45, 0.78, 0.03] FROM articles LIMIT 5"

# Text (embedded on the fly via BM25 / dense model)
qql --url http://localhost:6333 "QUERY 'machine learning' FROM papers LIMIT 5"

# Hybrid search (Dense + BM25 RRF fusion)
qql --url http://localhost:6333 "QUERY HYBRID 'latency optimization' FROM docs LIMIT 5"

# Interactive REPL
qql repl --url http://localhost:6333
```

</details>

<details>
<summary><strong>Python</strong> — <code>pip install pyqql</code></summary>

```python
import pyqql

# REST: 6333 or gRPC: 6334
client = pyqql.Client("http://localhost:6333")

report = client.execute("""
    QUERY [0.12, 0.45, 0.78, 0.03] FROM articles
    WHERE category = 'tech' AND year >= 2024
    LIMIT 5
""")

for hit in report.hits():
    print(f"ID: {hit.id} | Score: {hit.score:.3f} | Title: {hit.payload.get('title')}")
```

</details>

<details>
<summary><strong>Node.js</strong> — <code>npm install @veristamp/nqql</code></summary>

```typescript
import { Client } from "@veristamp/nqql";

const client = new Client("http://localhost:6333");

const report = await client.execute(`
  QUERY [0.12, 0.45, 0.78, 0.03] FROM articles
  WHERE category = 'tech' AND year >= 2024
  LIMIT 5
`);

for (const hit of report.hits()) {
  console.log(`[${hit.score.toFixed(3)}] ${hit.id} - ${hit.payload.title}`);
}
```

</details>

<details>
<summary><strong>Edge</strong> — embedded, no server, no network</summary>

```bash
pip install pyqql-edge
# or: npm install @veristamp/nqql-edge
```

```python
import pyqql_edge

# In-process HNSW storage + local FastEmbed ONNX model
client = pyqql_edge.local_executor("./qdrant_data", model="BGESmallENV15")

report = client.execute("QUERY 'embedded search' FROM notes LIMIT 5")
for hit in report.hits():
    print(hit.id, hit.score, hit.payload)
```

</details>

<details>
<summary><strong>Rust</strong> — <code>cargo add qql</code></summary>

```rust
use qql::client::Client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::connect_grpc("http://localhost:6334").await?;

    let report = client
        .execute("QUERY [0.12, 0.45, 0.78, 0.03] FROM crates WHERE downloads > 1000 LIMIT 5")
        .await?;

    for hit in report.hits(0) {
        println!("Hit: id={:?} score={:.3}", hit.id, hit.score);
    }
    Ok(())
}
```

</details>

---

## Query Patterns

<details>
<summary><strong>Vector and text search</strong> — <code>VECTOR</code> / <code>TEXT</code> keywords optional</summary>

```sql
-- Float vector search with filters
QUERY [0.12, 0.45, 0.78, 0.03] FROM articles
WHERE category = 'ai' AND rating >= 4.5
LIMIT 10;

-- Text search (embedded on the fly via BM25 / dense model)
QUERY 'distributed consensus protocols' FROM papers
WHERE year >= 2023
LIMIT 5;

-- Named vector index
QUERY [0.12, 0.45, 0.78, 0.03] FROM articles
USING dense
LIMIT 10;
```

</details>

<details>
<summary><strong>Hybrid search and reranking</strong></summary>

```sql
-- Hybrid: dense semantic + BM25 sparse with RRF fusion
QUERY HYBRID 'latency optimization' FROM docs LIMIT 10;

-- Hybrid with explicit vector targets
QUERY HYBRID 'latency optimization' DENSE dense SPARSE bm25 FUSION RRF FROM docs LIMIT 10;

-- Two-stage neural reranking
QUERY RERANK 'neural ranking' MODEL 'bge-reranker-base' FROM docs LIMIT 10;

-- Client-side cross-encoder reranking
QUERY CROSS RERANK 'query' MODEL 'bge-reranker-large' ON FIELD content FROM docs LIMIT 10;
```

</details>

<details>
<summary><strong>Fusion with prefetch CTEs</strong> — RRF or DBSF over sub-queries</summary>

```sql
WITH
  v_dense  AS (QUERY 'latency optimization' FROM docs USING dense  LIMIT 50),
  v_sparse AS (QUERY 'latency optimization' FROM docs USING sparse LIMIT 50)
QUERY FUSION RRF FROM docs PREFETCH (v_dense, v_sparse)
LIMIT 10;
```

</details>

<details>
<summary><strong>Batching</strong> — one roundtrip (<code>/points/query/batch</code>, <code>/points/batch</code>)</summary>

```sql
BATCH {
  QUERY [0.1, 0.2, 0.3] FROM products WHERE category = 'tech' LIMIT 5;
  QUERY [0.8, 0.9, 0.1] FROM products WHERE category = 'books' LIMIT 5;
}

BATCH {
  UPSERT INTO users (id, vector, role) VALUES (1, [0.1, 0.2], 'admin');
  DELETE FROM users WHERE last_login < '2023-01-01T00:00:00Z';
}
```

</details>

<details>
<summary><strong>Multitenancy</strong> — filter plus direct partition routing</summary>

```sql
QUERY [0.12, 0.45, 0.78, 0.03] FROM financial_records
WHERE tenant_id = 'tenant_corp_99' AND status = 'audited'
SHARD 'tenant_corp_99'
LIMIT 10;
```

</details>

<details>
<summary><strong>Facets</strong> — in-database categorical aggregation</summary>

```sql
FACET category FROM products
WHERE in_stock = true AND price <= 500.0
LIMIT 10
EXACT true;
```

</details>

<details>
<summary><strong>Parameterized queries</strong> — injection-safe, plan reuse</summary>

```sql
QUERY :query_vector FROM articles
WHERE author = :author AND category IN :categories
LIMIT :limit;
```

</details>

<details>
<summary><strong>Ingestion</strong> — upserts and filtered deletes</summary>

```sql
UPSERT INTO products (id, vector, title, price) VALUES
  (1, [0.12, 0.45, 0.78], 'Mechanical Keyboard', 129.99),
  (2, [0.89, 0.22, 0.05], 'Wireless Mouse', 49.99);

DELETE FROM products WHERE in_stock = false AND updated_at < '2024-01-01T00:00:00Z';
```

</details>

<details>
<summary><strong>DDL</strong> — collections and payload indexes</summary>

```sql
CREATE COLLECTION articles WITH (
  vectors = (dense = (size = 384, distance = 'Cosine')),
  sparse_vectors = (bm25 = ())
);

CREATE INDEX ON articles (category) TYPE 'keyword';
```

</details>

---

## Migration Suite

<details>
<summary><strong><code>qql migrate</code></strong> — logical collection migration across clusters</summary>

Streaming migration across clusters, versions, or configurations. Unlike binary snapshots, endpoints may differ in minor version and schema.

```bash
# Same-cluster copy with on-the-fly TurboQuant 4-bit quantization
qql --url grpc://localhost:6334 migrate articles \
  --to articles_quantized \
  --quantize turbo4 \
  --workers 4

# Cross-cluster migration with tenant shard promotion and cutover
qql --url grpc://old-cluster:6334 migrate docs \
  --target-url grpc://new-cluster:6334 \
  --to docs \
  --shard-key-field tenant_id \
  --cutover docs \
  --recreate
```

- Cross-version: non-adjacent versions (e.g. `1.15` → `1.19.1`).
- In-flight quantization: `scalar`, `binary`, or `turbo4` during stream.
- Resharding: new shard counts or custom shard keys.
- Checkpoints: resume interrupted migrations without re-transferring points.
- Full guide: [Migration Guide](crates/qql-cli/src/migrate/README.md), [Quickstart](crates/qql-cli/src/migrate/QUICKSTART.md).

</details>

<details>
<summary><strong><code>qql convert</code></strong> — REST JSON / curl to QQL</summary>

```bash
# Pasted curl command to QQL
qql convert "curl -X POST http://localhost:6333/collections/docs/points/query -d '{\"query\": [0.1, 0.2], \"limit\": 5}'"

# JSON payload file
cat search.json | qql convert --collection docs

# Multiple queries from a JSONL capture
qql convert --collection docs capture.jsonl
```

</details>

<details>
<summary><strong><code>qql record</code></strong> — transparent recording proxy</summary>

Captures queries from running applications (Python, TypeScript, Go, Java) with no code changes. Point the app at the proxy port; requests forward to Qdrant unchanged while `.qql` files are logged.

```bash
qql record --listen 127.0.0.1:6334 --target http://127.0.0.1:6333 --qql-out app_queries.qql
```

</details>

---

## Tools

- **[VS Code & Cursor Extension](https://marketplace.visualstudio.com/items?itemName=srimon12.qql-lang)**: syntax highlighting, live WASM diagnostics, plan hovers, CodeLens, completions for `.qql` files.
- **[Web Playground](https://qql.veristamp.com/playground/)**: run queries and inspect compiled plans in the browser (WebAssembly).
- **[AI Agent Skill](skills/qql-skill/README.md)**: MCP / agent skill for Cursor, Claude Code, Codex (`npx skills add srimon12/qql-rs --skill qql-skill`).

---

## Documentation

| Guide | Covers |
|---|---|
| [Syntax Reference](docs/syntax.md) | `QUERY`, `HYBRID`, `RERANK`, `BATCH`, `FUSION`, `FACET`, DML, DDL |
| [Filters & Operators](docs/filters.md) | `WHERE`, comparisons, ranges, geo predicates, `MATCH PREFIX`, `SLICE` |
| [Multitenancy & Security](docs/inject_filter.md) | Sharding, isolation, AST filter injection |
| [Cluster Migration](crates/qql-cli/src/migrate/README.md) | `qql migrate`, in-flight quantization, resharding |
| [Conversion & Recording](skills/qql-skill/references/convert-migration.md) | curl / REST JSON conversion, `qql record` proxy |
| [Python SDK](crates/pyqql/README.md) | PyO3 client, async usage, offline compilation |
| [Node.js SDK](crates/nqql/README.md) | N-API client, TypeScript types, streaming |
| [Rust Crate](crates/qql-runtime/README.md) | Runtime, gRPC tonic client, custom embedders |
| [Edge Mode](crates/qql-edge/README.md) | In-process database, zero network |

---

## Compatibility

- **Qdrant Server**: `≥ 1.19.0` (Quotas, `memory` tiering, `MATCH PREFIX`, `SLICE`, sparse `idf`, `turbo4`)
- **Python**: `3.10+`
- **Node.js**: `18+`
- **Rust**: `1.98+` (2024 edition)

---

<div align="center">
  <sub>QQL is open-source under the MIT License. Not affiliated with Qdrant.</sub>
</div>
