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
    "org_id",
    "=",
    "acme",
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
| `parse_batch(queries)` | `Vec<Stmt>` | Batch-parse multiple queries (minimizes FFI overhead) |
| `tokenize(input)` | `Vec<Token>` | Tokenize for highlighting, validation, or analysis |
| `is_valid(input)` / `isValid` | `bool` | Lightweight syntax validation |
| `inject_filter(query, field, op, value)` | AST | Programmatically inject a WHERE clause into statement AST |
| `compile(query)` / `compile_query` | Route object | Lower QQL statement into typed `{ method, path, payload }` route |

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
| **Batch** | Sequential loop | `parse_all()` / `parse_batch()` — single FFI call |

---

## Language Status

| Crate / SDK | Language | parse | tokenize | is_valid | inject_filter | parse_all | parse_batch | Runtime Executor |
|---|---|---|---|---|---|---|---|---|
| **pyqql** | Python (PyO3) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **qql-core** | Rust parser | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | — |
| **qql-plan** | Rust planner | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | — |
| **qql** | Rust runtime | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **nqql** | Node.js (N-API) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **qql-wasm** | WebAssembly | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |

---

## CLI

The Rust runtime includes a CLI for execution, debugging, and data migration:

```bash
cargo install qql-cli

# Execute a query against Qdrant
qql exec "QUERY 'search' FROM docs USING dense LIMIT 10"

# Explain the query plan
qql explain "QUERY POINTS (1) FROM docs"

# Convert REST JSON payloads to QQL
qql convert payload.json

# Dump a collection as a .qql script
qql dump medical backup.qql
```

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
QUERY CONTEXT (POSITIVE 1 NEGATIVE 2) FROM docs USING dense LIMIT 10;
QUERY DISCOVER TARGET 'target_text' CONTEXT (POSITIVE 1 NEGATIVE 2) FROM docs USING dense LIMIT 10;
QUERY RELEVANCE FEEDBACK TARGET 'query' FEEDBACK ((1, 0.9), (2, 0.1)) STRATEGY NAIVE (a=1.0, b=0.75, c=0.25) FROM docs USING dense LIMIT 10;
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

## Architecture

```
qql-core (parse → typed AST → explain → inject_filter)
    ↓
qql-plan (AST → typed RequestBody → Route { method, path, query, body })
    ↓
qql-runtime (resolve_embeddings → execute_route via REST reqwest or gRPC tonic)
```

The parser gives you a typed AST. Lowering produces transport-neutral routes (`Route`). The runtime executes routes over REST or high-performance gRPC.

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
