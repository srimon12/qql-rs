# QQL — The Query Language for Vector Search

**QQL is to Qdrant what SQL is to Postgres.** Write queries that search,
filter, rerank, recommend, and transform — in one language, across every
language.

```python
parse("QUERY 'chest pain' FROM medical USING dense LIMIT 5 WHERE department = 'cardio'")
# → inspectable, injectable, transformable AST
```

Available as native parser and execution libraries for Python, Node.js, Rust, and WASM,
with zero-copy compilation to Qdrant REST/gRPC wire schemas. No gateway. No YAML policies. No server sidecars.

---

## Quickstart

### Python
```bash
pip install pyqql
```
```python
from pyqql import parse, tokenize, is_valid, inject_filter, Client, HttpEmbedder

# Parse any QQL statement into an AST
stmt = parse("QUERY 'machine learning' FROM papers USING dense LIMIT 20 WHERE year >= 2024")

# Check if a query is valid without returning the AST
if is_valid("CREATE COLLECTION docs (dense VECTOR(384, COSINE))"):
    print("valid QQL")

# Inject security filters programmatically
safe_stmt = inject_filter(
    "QUERY 'papers' FROM docs LIMIT 50",
    "org_id", "=", "acme",
)

# Tokenize for syntax highlighting or analysis
for t in tokenize("QUERY 'search' FROM docs WHERE id = 1"):
    print(t['kind'], t['text'])
```

### Node.js
```bash
npm install nqql
```
```js
import { parse, injectFilter, isValid, Client } from 'nqql';

const ast = parse("QUERY 'search' FROM docs USING dense LIMIT 10");
const safe = injectFilter("QUERY 'x' FROM docs LIMIT 5", "tenant_id", "=", "acme");
```

### Rust
```toml
qql-core = "0.1"    # parser only
qql-plan = "0.1"    # typed lowering layer
qql = "0.1"         # full runtime + executor (package name `qql`)
```
```rust
use qql_core::parser::Parser;
use qql_core::ast;

let stmt = Parser::parse("QUERY 'search' FROM docs USING dense LIMIT 10").unwrap();
if let ast::Stmt::Query(q) = &stmt {
    println!("querying collection {:?} with expr {:?}", q.collection, q.expression);
}
```

### WASM (Browser)
```js
import init, { Client, parse, tokenize, isValid } from 'qql-wasm';

const ast = parse("QUERY 'hello' FROM docs LIMIT 5");
const tokens = tokenize("CREATE COLLECTION docs (dense VECTOR(384, COSINE))");
```

---

## API Surface

Every language binding exposes the same core set of functions:

| Function | Returns | Description |
|----------|---------|-------------|
| `parse(input)` | AST (`Stmt` object or dictionary) | Parse a single QQL statement |
| `parse_all(input)` | `Vec<Stmt>` | Parse a semicolon-delimited script |
| `parse_batch(queries)` | `Vec<Stmt>` | Batch-parse multiple queries (minimises FFI overhead) |
| `tokenize(input)` | `Vec<Token>` | Tokenize for highlighting, validation, or analysis |
| `is_valid(input)` / `isValid` | `bool` | Lightweight syntax validation |
| `inject_filter(query, field, op, value)` | AST | Programmatically inject a WHERE clause into statement AST |
| `compile(query)` / `compile_query` | Route object | Lower QQL statement into a transport-neutral route. Python returns `{ method, path, payload }`; Node/WASM return `{ stmt_type, payload }`. |

---

## What Makes QQL Different

Most search interfaces are opaque — you build a JSON object, send it,
and hope it works. QQL gives you **programmatic access to the query itself**:

| Pattern | Without QQL | With QQL |
|---------|-------------|----------|
| **Validate** | Round-trip to Qdrant | `is_valid()` — instant, no network |
| **Inspect** | Read JSON manually | `parse()` → typed AST |
| **Transform** | String concatenation | `inject_filter()` — safe, recursive |
| **Audit** | Log raw SDK calls | `tokenize()` → structured tokens |
| **Batch** | Sequential network calls | `execute("Q1; Q2; Q3;")` — auto-detected, single wire-level batch call |

---

## Language Status

| Crate / SDK | Language | parse | tokenize | is_valid | inject_filter | parse_all | parse_batch | Runtime |
|---|---|---|---|---|---|---|---|---|
| **pyqql** | Python (PyO3) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **qql-core** | Rust parser | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | — |
| **qql-plan** | Rust lowering | — | — | — | — | — | — | — |
| **qql** | Rust runtime | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **nqql** | Node.js (N-API) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **qql-wasm** | WebAssembly | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |

> **Known limitation**: `qql-wasm` re-exports the parser and routing functions
> but the WASM Client does not include the full executor pipeline
> (`resolve_embeddings` is not bound via `#[wasm_bindgen]`). Runtime execution
> in WASM delegates embedding to JS-side HTTP providers.

---

## CLI

The Rust runtime includes a CLI for execution, debugging, and data migration:

```bash
cargo install qql-cli

# Execute a single query against Qdrant
qql exec "QUERY 'search' FROM docs USING dense LIMIT 10"

# Execute multiple statements (semicolons auto-detected)
qql exec "CREATE COLLECTION docs (dense VECTOR(384, COSINE)); \
          CREATE INDEX ON COLLECTION docs FOR title TYPE text"

# Explain the query plan — works on multi-statement too
qql explain "QUERY POINTS (1) FROM docs; COUNT FROM docs"

# Convert REST JSON payloads to QQL
qql convert payload.json

# Dump a collection as a .qql script
qql dump medical backup.qql
```

---

## Architecture

```
                        ┌─────────────────────────────────────────────────┐
                        │          qql-core (parser + AST)                │
                        │  parse / tokenize / is_valid / inject_filter    │
                        │  ErrorKind: Lex, Parse, Validation              │
                        └──────────────────┬──────────────────────────────┘
                                           │ Stmt
                                           ▼
                        ┌─────────────────────────────────────────────────┐
                        │          qql-plan (lowering layer)              │
                        │  plan() → PlannedOperation (canonical enum)     │
                        │  to_rest_route() → Route (REST projection)      │
                        │  try_route() = plan + to_rest_route             │
                        │  Lowering: ddl, mutation, query, filter, embed  │
                        │  Typed until transport: PlanPointId,            │
                        │  PlanVectorValue, PlanQueryInput, PlanFormula   │
                        └──────────────────┬──────────────────────────────┘
                                           │ PlannedOperation / Route
                                           ▼
          ┌─────────────────────────────────┬──────────────────────────────┐
          │                                 │                              │
          ▼                                 ▼                              ▼
┌─────────────────────┐   ┌──────────────────────────┐   ┌────────────────┐
│  qql-runtime (qql)  │   │     qql-edge              │   │  qql-wasm      │
│                     │   │  qdrant-edge (in-process)  │   │  (parse only)  │
│  RestQdrant (reqwest)│   │  FastEmbedder (ONNX,CPU)  │   │                │
│  GrpcQdrant (tonic)  │   │  HttpEmbedder (opt.)     │   │                │
│  HttpEmbedder       │   │  local/http/custom exec   │   │                │
└─────────────────────┘   └──────────────────────────┘   └────────────────┘
          │                          │
          ▼                          ▼
    ┌──────────┐             In-process HNSW
    │  Qdrant  │             (no network)
    │ REST/gRPC│
    └──────────┘
```

### Canonical execution flow: prepare → plan → batch → dispatch

```
Statement string
    │
    ▼
1. Parse (qql-core Parser → Stmt)
    │
    ▼
2. Prepare (executor: embeddings + schema validation)
   ├─ resolve_embeddings: text → dense/sparse vectors (if embedder registered)
   ├─ ensure_vector_name: validate USING against known named vectors
   └─ ensure_collection_for_upsert: auto-create default dense/hybrid on first upsert
    │
    ▼
3. Plan (qql-plan plan() → PlannedOperation)
   └─ (or route() → Route for direct REST dispatch)
    │
    ▼
4. Batch classify: group adjacent same-collection operations
   ├─ Same-collection QUERY × 2+ → QueryBatchRequest → /points/query/batch
   ├─ Same-collection UPSERT/DELETE/UPDATE× 2+ → UpdateBatchRequest → /points/batch
   └─ Individual ops → dispatch each separately
    │
    ▼
5. Dispatch (QdrantOps::execute_route / execute_query_batch / execute_update_batch)
   ├─ REST: Route body → JSON → POST/PUT/DELETE
   ├─ gRPC: PlannedOperation → typed protobuf → tonic
   └─ Edge: PlannedOperation → qdrant-edge in-process API
    │
    ▼
6. Response normalization
   ├─ REST: extract result from {"result": ..., "status": "ok", "time": ...}
   ├─ gRPC: synthesize same envelope from protobuf response
   ├─ Edge: synthesize same envelope from in-process result
   └─ Batch: strict cardinality check — response count must match operation count
```

Batch execution is automatic: multi-statement scripts and list inputs are
smart-batched without grammar changes or separate API calls. Order is preserved.

### Route is a REST projection

`Route { method, path, query, body }` is derived from `PlannedOperation` via
`to_rest_route()`. It is a REST projection, not the canonical representation.
New code should prefer `plan()` for operation logic and use `to_rest_route()`
only when serializing to the REST wire format.

### gRPC DDL mapping — complete

All DDL operations are mapped to typed protobuf:

| Operation | gRPC RPC |
|-----------|----------|
| `CreateCollection` | `CreateCollection` (with deferred params + shard keys) |
| `UpdateCollection` | `UpdateCollection` |
| `DropCollection` | `DeleteCollection` |
| `CreateIndex` | `CreateFieldIndexCollection` |
| `DropIndex` | `DeleteFieldIndexCollection` |
| `CreateShardKey` | `CreateShardKey` |
| `DropShardKey` | `DeleteShardKey` |
| `ListCollections` | `ListCollections` (raw) |
| `GetCollection` | `CollectionInfo` (raw) |
| `ListShardKeys` | `ListShardKeys` |

gRPC uses JSON-intermediate extraction from the planner's typed request data
(not raw Qdrant protobuf), converting `serde_json::Value` fields to typed
protobuf oneofs. Both REST and gRPC produce the same response envelope.

### Embedding resolution — shared across targets

The embedding rewrite is shared (`qql-embed`) so Python, Node, Rust, Edge,
and WASM all batch dense texts the same way. The `Embedder` trait is
host-agnostic; implementations differ per target:

- `HttpEmbedder` — OpenAI-compatible REST endpoint (Ollama, OpenAI, vLLM, TEI)
- `SparseEmbedder` — local BM25 (hash-based, no dependencies)
- `FastEmbedder` — ONNX inference via fastembed-rs (edge only)

### Response envelope normalization

All three backends normalize to:
```json
{ "result": { ... }, "status": "ok", "time": 0.001 }
```

- **REST**: `validate_success_envelope()` checks `result` present + `status == "ok"`
- **gRPC**: `execute_grpc_route()` wraps each protobuf response in the same envelope
- **Edge**: `backend/mod.rs` `mutation_response()` and query results follow same pattern
- **Batch**: cardinality mismatch returns `QQL-BATCH-CARDINALITY` error

---

## Syntax Highlights

Full reference at [`docs/syntax.md`](docs/syntax.md).

### Search modes
```sql
QUERY 'semantic search' FROM docs USING dense LIMIT 10;
QUERY HYBRID TEXT 'hybrid search' DENSE dense SPARSE sparse FUSION RRF FROM docs LIMIT 10;
QUERY TEXT 'keyword search' FROM docs USING sparse LIMIT 10;
```

### Recsys modes
```sql
QUERY RECOMMEND POSITIVE (1) NEGATIVE (2) STRATEGY average_vector FROM docs USING dense LIMIT 10;
QUERY CONTEXT (POSITIVE POINT 1 NEGATIVE POINT 2) FROM docs USING dense LIMIT 10;
QUERY DISCOVER TARGET 'target_text' CONTEXT (POSITIVE POINT 1 NEGATIVE POINT 2) FROM docs USING dense LIMIT 10;
QUERY RELEVANCE FEEDBACK TARGET POINT 42 FEEDBACK ((POINT 43, 0.5), (POINT 44, -0.2)) STRATEGY NAIVE (a=1.0, b=0.75, c=0.25) FROM docs USING dense LIMIT 10;
```

### Multi-stage retrieval (CTE + Prefetch + Fusion)
```sql
WITH dense AS (QUERY TEXT 'search' USING dense LIMIT 100),
     sparse AS (QUERY TEXT 'search' USING sparse LIMIT 100)
QUERY FUSION RRF FROM docs
  PREFETCH (dense WHERE priority = 'high', sparse)
  LIMIT 10;
```

### Score shaping (Formula scoring)
```sql
QUERY FORMULA score + 0.3 * popularity DEFAULTS (popularity = 1.0) FROM docs LIMIT 10;
```

### Filters
```sql
WHERE tenant_id = 'acme'
  AND status IN ('active', 'pending')
  AND score >= 0.5
  AND created_at BETWEEN 1700000000 AND 1800000000
  AND tags IS NOT EMPTY
  AND content MATCH ANY ('hello', 'world')
```

### DDL & Point Operations
```sql
-- Count points matching a filter
COUNT FROM docs WHERE status = 'active';

-- Manage payload indexes
CREATE INDEX ON COLLECTION docs FOR title TYPE text WITH (lowercase = true);
DROP INDEX ON COLLECTION docs FOR title;

-- Clear payload fields
CLEAR PAYLOAD FROM docs WHERE status = 'archived';

-- Delete specific named vectors
DELETE VECTOR colbert FROM docs WHERE id = 42;

-- Create, list, and drop custom shard keys for multi-tenant isolation
CREATE SHARD KEY 'acme' ON COLLECTION docs WITH (shards_number = 2);
SHOW SHARD KEYS ON COLLECTION docs;
DROP SHARD KEY 'acme' ON COLLECTION docs;
```

### Multi-tenancy

```sql
-- One collection, many tenants, zero cross-tenant leaks
CREATE COLLECTION sec10k HYBRID (dense VECTOR(768, COSINE), sparse SPARSE)
WITH PARAMS (
  replication_factor = 2, shard_number = 8,
  sharding_method = 'custom',
  shard_keys = ['honeywell', 'ge', '3m', 'rtx']
);

CREATE INDEX ON COLLECTION sec10k FOR tenant_id
  TYPE keyword WITH (is_tenant = true);

QUERY 'supply chain risks' FROM sec10k
  WHERE tenant_id = 'honeywell' SHARD 'honeywell' LIMIT 10;
```

Programmatic isolation via `inject_filter()` — a single call that recursively injects
tenant filters into every sub-query, CTE, and prefetch across Python, Rust, Node, and WASM.
[Full guide →](skills/qql-skill/references/qql-multitenancy.md)

---

## API key & timeout behavior

Both REST and gRPC clients accept an optional API key and configurable timeout:

- `RestQdrant::new(url, api_key)` — 30s default timeout
- `RestQdrant::with_timeout(url, api_key, timeout)` — explicit timeout
- `GrpcQdrant::from_url(url, api_key)` — tonic default timeout
- `GrpcQdrant::from_url_with_timeout(url, api_key, timeout)` — optional `Duration` timeout (None = tonic default)

API keys are sent via:
- REST: `api-key` header
- gRPC: `ApiKeyInterceptor` (tonic interceptor, attaches `api-key` metadata)

Pass `None` / `""` for unauthenticated local Qdrant instances.

---

## Typed vectors and formulas

The planner preserves semantic distinctions until the transport boundary:

- **`PlanVectorValue`**: `Dense(Vec<f32>)`, `Sparse { indices, values }`, `MultiDense(Vec<Vec<f32>>)`
- **`PlanQueryInput`**: `Point(PlanPointId)`, `Vector(PlanVectorValue)`, `Document { text, model }`
- **`PlanPointVectors`**: `Unnamed(PlanVectorValue)` or `Named(Vec<(String, PlanVectorValue)>)`
- **`PlanFormula`**: typed formula tree (Constant/Variable/Sum/Sub/Mul/Div/Neg/Abs/Sqrt/Log/Ln/Exp/Pow/GeoDistance/Decay/Case/Datetime)

REST serialization uses snake_case OpenAPI expression keys (`sum`, `mult`, `div`, `neg`, `geo_distance`, `exp_decay`, `gauss_decay`, `lin_decay`). gRPC converts the same typed formula tree to Qdrant's protobuf `Expression` oneofs.

---

## Collection preparation

When embedding via `USING DENSE MODEL` or `USING HYBRID`, the executor
auto-creates the target collection with a default schema if it does not exist:

- **Dense only**: single `dense` vector with model-dimension inference
- **Hybrid**: `dense` vector + `sparse` vector (BM25-based, no model)

This applies only to UPSERT paths — `QUERY` against a non-existent collection
returns a Qdrant error as expected.

---

## Batch cardinality

Contiguous same-collection operations that share a batch family are sent as
a single wire-level batch. The executor **strictly verifies** that the response
count matches the operation count:

```
[UPSERT a, UPSERT b, DELETE c]  → 3 operations → must return 3 results
Mismatch → QQL-BATCH-CARDINALITY error
```

This replaces the previous silent padding behavior. Single-statement paths
and non-batchable operations are unaffected.

---

## Contributing

```bash
# Build everything
PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 cargo build --workspace --all-targets

# Run all tests
PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 cargo test --workspace --all-targets

# Check clippy
PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 cargo clippy --workspace --all-targets -- -D warnings
```
