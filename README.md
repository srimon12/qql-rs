<div align="center">

  <img src="https://raw.githubusercontent.com/srimon12/qql-rs/main/docs/assets/qql-banner.png" alt="QQL Banner" width="600" />

  # QQL — Declarative SQL for Qdrant & Vector Search

  **QQL is to Qdrant what SQL is to PostgreSQL.**  
  Write expressive, declarative queries for dense, sparse, and hybrid vector search, filtering, multitenancy, and analytics — in one universal language.

  [![CI](https://github.com/srimon12/qql-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/srimon12/qql-rs/actions/workflows/ci.yml)
  [![Release](https://img.shields.io/github/v/release/srimon12/qql-rs?color=blue)](https://github.com/srimon12/qql-rs/releases)
  [![Crates.io](https://img.shields.io/crates/v/qql.svg)](https://crates.io/crates/qql)
  [![PyPI](https://img.shields.io/pypi/v/pyqql.svg)](https://pypi.org/project/pyqql/)
  [![npm](https://img.shields.io/npm/v/@veristamp/nqql.svg)](https://www.npmjs.com/package/@veristamp/nqql)
  [![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

  [Documentation](docs/README.md) • [Syntax Guide](docs/syntax.md) • [Interactive Playground](https://qql.veristamp.com/playground/) • [VS Code Extension](https://marketplace.visualstudio.com/items?itemName=srimon12.qql-lang) • [Python SDK](crates/pyqql) • [Node.js SDK](crates/nqql)

</div>

---

## Why QQL?

Why write 40+ lines of nested JSON payloads or complex SDK builder chains for a single vector search?

```json
// Raw Qdrant REST Request (40+ lines of nested JSON)
POST /collections/articles/points/query
{
  "query": { "nearest": [0.12, 0.45, 0.78, 0.03] },
  "using": "dense",
  "filter": {
    "must": [
      { "key": "category", "match": { "value": "tech" } },
      { "key": "year", "range": { "gte": 2024 } }
    ]
  },
  "limit": 5
}
```

```sql
-- In QQL: Clean, readable, and intuitive
QUERY [0.12, 0.45, 0.78, 0.03] FROM articles
USING dense
WHERE category = 'tech' AND year >= 2024
LIMIT 5;
```

- ⚡ **80% Less Boilerplate**: Replace sprawling dictionary hierarchies and builder objects with clean, self-documenting SQL.
- 🎯 **Automatic Vector & Text Detection**: Pass raw float arrays `[0.1, 0.2, ...]` or string literals `'search text'` directly — `VECTOR` and `TEXT` keywords are completely optional.
- 🔀 **One-Line Hybrid & Rerank**: Run full Dense + BM25 search via `QUERY HYBRID '...'` or two-stage neural scoring with `QUERY RERANK`.
- 📦 **High-Throughput Batching**: Run multiple queries or mutations in a single network roundtrip using `BATCH { ... }`.
- 🧠 **In-Process Embeddings**: QQL resolves dense vectors and wire-compatible BM25 sparse vectors on the fly in-process — zero out-of-band glue code.
- 🏢 **First-Class Multitenancy**: Explicit `SHARD 'tenant_1'` partition routing eliminates cross-shard network broadcast, while AST-level `inject_filter` prevents data leaks.
- 🚀 **Zero-Overhead Compilation**: Compiles directly to optimized Qdrant REST routes or high-performance typed gRPC protobuf (`tonic`).
- 💻 **Offline & Embedded Mode**: Run full vector search locally in-process without spinning up a Qdrant server (`qdrant-edge` + `fastembed-rs`).
- 🔌 **Standalone Runtimes**: Zero dependency on official SDK wrappers. Native PyO3 (`pyqql`), N-API (`nqql`), WebAssembly (`qql-wasm`), and Rust (`qql`).

---

## 30-Second Quickstart

### 1. CLI (Instant Querying & REPL)

Install the standalone binary (Linux, macOS, Windows):

```bash
# Linux & macOS
curl -fsSL https://raw.githubusercontent.com/srimon12/qql-rs/main/scripts/install.sh | sh

# Windows (PowerShell)
irm https://raw.githubusercontent.com/srimon12/qql-rs/main/scripts/install.ps1 | iex
```

Run queries against any Qdrant instance:

```bash
# Query with raw vector floats
qql --url http://localhost:6333 "QUERY [0.12, 0.45, 0.78, 0.03] FROM articles LIMIT 5"

# Query with natural text (auto-detected string, embedded via BM25 / dense model)
qql --url http://localhost:6333 "QUERY 'machine learning' FROM papers LIMIT 5"

# Single-line hybrid search (Dense + BM25 RRF fusion)
qql --url http://localhost:6333 "QUERY HYBRID 'latency optimization' FROM docs LIMIT 5"

# Start the interactive REPL
qql repl --url http://localhost:6333
```

---

### 2. Python (`pyqql`)

```bash
pip install pyqql
```

```python
import pyqql

# Connect to Qdrant (REST: 6333 or gRPC: 6334)
client = pyqql.Client("http://localhost:6333")

# Query using raw vector floats or auto-detected text strings
report = client.execute("""
    QUERY [0.12, 0.45, 0.78, 0.03] FROM articles
    WHERE category = 'tech' AND year >= 2024
    LIMIT 5
""")

for hit in report.hits():
    print(f"ID: {hit.id} | Score: {hit.score:.3f} | Title: {hit.payload.get('title')}")
```

---

### 3. TypeScript / Node.js (`@veristamp/nqql`)

```bash
npm install @veristamp/nqql
```

```typescript
import { Client } from "@veristamp/nqql";

const client = new Client("http://localhost:6333");

// Query with raw vector floats or auto-detected text strings
const report = await client.execute(`
  QUERY [0.12, 0.45, 0.78, 0.03] FROM articles
  WHERE category = 'tech' AND year >= 2024
  LIMIT 5
`);

for (const hit of report.hits()) {
  console.log(`[${hit.score.toFixed(3)}] ${hit.id} - ${hit.payload.title}`);
}
```

---

### 4. Local In-Process Edge Engine (Zero Server, Zero Network)

Need embedded vector search without running Docker or an external Qdrant instance?

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

---

### 5. Native Rust (`qql`)

```bash
cargo add qql
```

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

---

## Cheat Sheet: Common Query Patterns

### 1. Vector & Text Search (Auto-Detection)
Pass raw vector array literals or text strings directly — `VECTOR` and `TEXT` keywords are optional:

```sql
-- Raw float vector search with filters
QUERY [0.12, 0.45, 0.78, 0.03] FROM articles
WHERE category = 'ai' AND rating >= 4.5
LIMIT 10;

-- Auto-detected text search (embedded on the fly via BM25 / dense model)
QUERY 'distributed consensus protocols' FROM papers
WHERE year >= 2023
LIMIT 5;

-- Target a specific named vector index
QUERY [0.12, 0.45, 0.78, 0.03] FROM articles
USING dense
LIMIT 10;
```

### 2. Native Hybrid Search & Reranking
Run multi-modal vector search and re-scoring with concise native statements:

```sql
-- Single-line hybrid search (Dense semantic + BM25 sparse with automatic RRF fusion)
QUERY HYBRID 'latency optimization' FROM docs LIMIT 10;

-- Hybrid search with explicit vector targets
QUERY HYBRID 'latency optimization' DENSE dense SPARSE bm25 FUSION RRF FROM docs LIMIT 10;

-- Two-stage neural reranking
QUERY RERANK 'neural ranking' MODEL 'bge-reranker-base' FROM docs LIMIT 10;

-- Client-side cross-encoder reranking
QUERY CROSS RERANK 'query' MODEL 'bge-reranker-large' ON FIELD content FROM docs LIMIT 10;
```

### 3. Advanced Fusion with Prefetch CTEs
Combine arbitrary sub-queries with Reciprocal Rank Fusion (RRF) or Distribution-Based Score Fusion (DBSF):

```sql
WITH
  v_dense  AS (QUERY 'latency optimization' FROM docs USING dense  LIMIT 50),
  v_sparse AS (QUERY 'latency optimization' FROM docs USING sparse LIMIT 50)
QUERY FUSION RRF FROM docs PREFETCH (v_dense, v_sparse)
LIMIT 10;
```

### 4. High-Throughput Batching (`BATCH { ... }`)
Execute multiple queries or mutations in a single network roundtrip (routes to `/points/query/batch` or `/points/batch`):

```sql
-- Batch multiple queries in a single network roundtrip
BATCH {
  QUERY [0.1, 0.2, 0.3] FROM products WHERE category = 'tech' LIMIT 5;
  QUERY [0.8, 0.9, 0.1] FROM products WHERE category = 'books' LIMIT 5;
}

-- Batch atomic mutations in one call
BATCH {
  UPSERT INTO users (id, vector, role) VALUES (1, [0.1, 0.2], 'admin');
  DELETE FROM users WHERE last_login < '2023-01-01T00:00:00Z';
}
```

### 5. Multi-Tenancy & Partition Routing
Direct queries to specific tenant partitions for zero-broadcast efficiency:

```sql
-- Filter tenant data AND route directly to partition 'tenant_corp_99'
QUERY [0.12, 0.45, 0.78, 0.03] FROM financial_records
WHERE tenant_id = 'tenant_corp_99' AND status = 'audited'
SHARD 'tenant_corp_99'
LIMIT 10;
```

### 6. In-Database Categorical Facets (Aggregations)
Aggregate value distributions in Qdrant without pulling point payloads over the wire:

```sql
FACET category FROM products
WHERE in_stock = true AND price <= 500.0
LIMIT 10
EXACT true;
```

### 7. Safe Parameterized Queries
Prevent injection and reuse compiled query plans across calls:

```sql
QUERY :query_vector FROM articles
WHERE author = :author AND category IN :categories
LIMIT :limit;
```

### 8. Ingestion & Bulk Upserts
```sql
-- Upsert records with vectors and JSON payloads
UPSERT INTO products (id, vector, title, price) VALUES
  (1, [0.12, 0.45, 0.78], 'Mechanical Keyboard', 129.99),
  (2, [0.89, 0.22, 0.05], 'Wireless Mouse', 49.99);

-- Delete by filter
DELETE FROM products WHERE in_stock = false AND updated_at < '2024-01-01T00:00:00Z';
```

### 9. Schema & DDL Management
```sql
-- Create collection with dense and sparse vectors
CREATE COLLECTION articles WITH (
  vectors = (dense = (size = 384, distance = 'Cosine')),
  sparse_vectors = (bm25 = ())
);

-- Create payload index for fast filtering
CREATE INDEX ON articles (category) TYPE 'keyword';
```

---

## Migration Suite: From Qdrant to QQL

Whether migrating cluster data, legacy application code, or running services, QQL provides dedicated migration tools.

### 1. Logical Cluster & Data Migration (`qql migrate`)

[`qql migrate`](crates/qql-cli/src/migrate/README.md) performs high-speed streaming migration of collections across clusters, minor versions, or configurations (unlike binary snapshots which require identical minor versions and immutable schemas):

```bash
# Same-cluster copy with on-the-fly TurboQuant 4-bit quantization
qql --url grpc://localhost:6334 migrate articles \
  --to articles_quantized \
  --quantize turbo4 \
  --workers 4

# Cross-cluster migration with tenant shard promotion and zero-downtime cutover
qql --url grpc://old-cluster:6334 migrate docs \
  --target-url grpc://new-cluster:6334 \
  --to docs \
  --shard-key-field tenant_id \
  --cutover docs \
  --recreate
```

- **Cross-version leap**: Migrate directly between non-adjacent versions (e.g. `1.15` → `1.19.1`).
- **In-flight quantization**: Re-quantize to `scalar`, `binary`, or `turbo4` during stream.
- **Resharding & Multitenancy**: Dynamically re-hash across new shard counts or custom shard keys.
- **Crash-resilient checkpoints**: Resumes interrupted migrations without re-transferring points.
- See the full [Migration Guide](crates/qql-cli/src/migrate/README.md) and [Quickstart](crates/qql-cli/src/migrate/QUICKSTART.md).

### 2. Instant Query & Code Translation (`qql convert`)

Convert raw Qdrant REST JSON, HTTP snippets, or pasted `curl` commands directly into canonical QQL:

```bash
# Convert a pasted curl command directly to QQL
qql convert "curl -X POST http://localhost:6333/collections/docs/points/query -d '{\"query\": [0.1, 0.2], \"limit\": 5}'"

# Convert an existing Qdrant JSON payload file
cat search.json | qql convert --collection docs

# Convert multiple queries from a JSONL capture
qql convert --collection docs capture.jsonl
```

### 3. Zero-Code App Migration via Transparent Proxy (`qql record`)

Capture queries from running applications in any language (Python, TypeScript, Go, Java) with zero code changes:

```bash
# Start the transparent recording proxy (proxy on 6334 -> Qdrant on 6333)
qql record --listen 127.0.0.1:6334 --target http://127.0.0.1:6333 --qql-out app_queries.qql
```

Point your existing application to port `6334`. Requests forward to Qdrant unchanged while automatically logging clean, production-ready `.qql` query files.

---

## Ecosystem & Tools

- 🖥️ **[VS Code & Cursor Extension](https://marketplace.visualstudio.com/items?itemName=srimon12.qql-lang)**: Syntax highlighting, live WASM diagnostics, plan hovers, CodeLens, and completions for `.qql` files.
- 🌐 **[Interactive Web Playground](https://qql.veristamp.com/playground/)**: Test QQL queries and inspect compiled execution plans in real time in your browser (compiled via WebAssembly).
- 🤖 **[AI Agent Skill](skills/qql-skill/README.md)**: Drop-in MCP / Agent skill for Cursor, Claude Code, and Codex (`npx skills add srimon12/qql-rs --skill qql-skill`).
- 🚚 **[Cluster Migrator (`qql migrate`)](crates/qql-cli/src/migrate/README.md)**: High-speed logical streaming collection migration across clusters and versions.
- 🔄 **CLI REST Converter (`qql convert`)**: Automatically translate raw Qdrant REST JSON, curl commands, or HTTP snippets into clean, canonical QQL queries.
- ⏺️ **Transparent Recorder (`qql record`)**: Intercept existing application traffic to generate `.qql` scripts with zero code changes.

---

## Documentation Index

| Guide | Description |
|---|---|
| 📖 [Syntax Reference](docs/syntax.md) | Complete grammar: `QUERY`, `HYBRID`, `RERANK`, `BATCH`, `FUSION`, `FACET`, DML, DDL |
| 🔍 [Filters & Operators](docs/filters.md) | `WHERE` clauses, comparison ops, ranges, geo predicates, `MATCH PREFIX`, `SLICE` |
| 🏢 [Multitenancy & Security](docs/inject_filter.md) | Partition sharding vs isolation, AST filter injection security |
| 🚚 [Cluster Migration Guide](crates/qql-cli/src/migrate/README.md) | Logical streaming migration (`qql migrate`), in-flight quantization, resharding |
| 🔁 [Query Conversion & Recording](skills/qql-skill/references/convert-migration.md) | Convert curl / REST JSON, zero-code proxy recording with `qql record` |
| 🐍 [Python SDK (`pyqql`)](crates/pyqql/README.md) | PyO3 client reference, async usage, offline compilation |
| 🟨 [Node.js SDK (`nqql`)](crates/nqql/README.md) | N-API client reference, TypeScript types, streaming |
| 🦀 [Rust Crate (`qql`)](crates/qql-runtime/README.md) | Direct Rust runtime, gRPC tonic client, custom embedder integration |
| ⚙️ [Edge Embedded Mode](crates/qql-edge/README.md) | In-process vector database with zero network footprint |

---

## Compatibility

- **Qdrant Server**: `≥ 1.19.0` (supports Quotas, `memory` tiering, `MATCH PREFIX`, `SLICE`, sparse `idf`, `turbo4`)
- **Python**: `3.10+`
- **Node.js**: `18+`
- **Rust**: `1.98+` (2024 edition)

---

<div align="center">
  <sub>QQL is an open-source project licensed under the MIT License. Not officially affiliated with Qdrant.</sub>
</div>
