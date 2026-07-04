# QQL — The Query Language for Vector Search

**QQL is to Qdrant what SQL is to Postgres.** Write queries that search,
filter, rerank, recommend, and transform — in one language, across every
language.

```python
parse("QUERY 'chest pain' FROM medical LIMIT 5 WHERE department = 'cardio'")
# → inspectable, injectable, transformable AST
```

Available as **native libraries** for Python, Node.js, Go, Rust, and WASM.
No gateway. No YAML policies. No server sidecars.

---

## Quickstart

### Python
```bash
pip install pyqql
```
```python
from pyqql import parse, tokenize, is_valid, inject_filter

# Parse any QQL statement into an AST
ast = parse("QUERY 'machine learning' FROM papers LIMIT 20 WHERE year >= 2024")

# Check if a query is valid without parsing it fully
if is_valid("CREATE COLLECTION docs HYBRID"):
    print("valid QQL")

# Inject security filters — no string concatenation
safe = inject_filter('''QUERY 'papers' FROM docs LIMIT 50''',
    "org_id", "IN", '{"list": [{"str": "acme"}, {"str": "globex"}]}')

# Tokenize for syntax highlighting or analysis
for t in tokenize("SELECT * FROM docs WHERE id = 1"):
    print(t['kind'], t['text'])
```

### Node.js
```bash
npm install nqql
```
```js
import { parse, injectFilter, isValid } from 'nqql';

const ast = parse("QUERY 'search' FROM docs LIMIT 10");
const safe = injectFilter("QUERY 'x' FROM docs LIMIT 5", "tenant_id", "=", '{"str": "acme"}');
```

### Go
```bash
import "github.com/srimon12/qql-rs/crates/gqql"
```
```go
ast, _ := gqql.Parse("QUERY 'search' FROM docs LIMIT 10")
safe, _ := gqql.InjectFilter("QUERY 'x' FROM docs LIMIT 5", "tenant_id", "=", `{"str": "acme"}`)
isValid := gqql.IsValid("SELECT * FROM docs WHERE id = 1")
```

### Rust
```toml
qql-core = "0.1"    # parser only
qql = "0.1"         # full runtime + executor
```
```rust
use qql_core::parser::Parser;
use qql_core::ast;

let stmt = Parser::parse("QUERY 'search' FROM docs LIMIT 10").unwrap();
if let ast::Stmt::Query(q) = &stmt {
    println!("querying {} with {:?}", q.collection.unwrap(), q.query_text);
}
```

### WASM (Browser)
```js
import init, { parse, tokenize, is_valid } from 'qql-wasm';

const ast = parse("QUERY 'hello' FROM docs LIMIT 5");
const tokens = tokenize("CREATE COLLECTION docs");
```

---

## API Surface

Every language binding exposes the same set of functions:

| Function | Returns | Description |
|----------|---------|-------------|
| `parse(input)` | debug string | Parse a single QQL statement |
| `parse_all(input)` | `Vec<string>` | Parse a semicolon-delimited script |
| `parse_batch(queries)` | `Vec<string>` | Batch-parse multiple queries (minimizes FFI overhead) |
| `tokenize(input)` | `Vec<Token>` | Tokenize for highlighting, validation, or analysis |
| `is_valid(input)` | `bool` | Lightweight syntax validation |
| `inject_filter(query, field, op, value_json)` | debug string | Inject a WHERE clause programmatically |

---

## Examples

See the [`examples/`](examples/) directory for 4 progressive levels
across all 5 languages:

| Level | What it shows |
|-------|---------------|
| **01 Basic** | `parse`, `tokenize`, `is_valid` |
| **02 Medium** | `inject_filter` with string, numeric, boolean values |
| **03 Expert** | Multi-tenant query gateway pattern |
| **04 Batch** | Script parsing and batch FFI for throughput |

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

| SDK | Language | parse | tokenize | is_valid | inject_filter | parse_all | parse_batch | Runtime |
|-----|----------|-------|----------|----------|---------------|-----------|-------------|---------|
| **pyqql** | Python | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | — |
| **nqql** | Node.js | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | — |
| **gqql** | Go | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | — |
| **qql-wasm** | WASM | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | — |
| **qql-core** | Rust | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | — |
| **qql** | Rust | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |

---

## CLI

The Rust runtime includes a CLI for execution, debugging, and data migration:

```bash
cargo install qql-cli

# Execute a query
qql exec "QUERY 'search' FROM docs LIMIT 10"

# Explain the query plan
qql explain "SELECT * FROM docs WHERE id = '123'"

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
QUERY 'semantic search' FROM docs LIMIT 10
QUERY 'hybrid search'   FROM docs LIMIT 10 USING HYBRID
QUERY 'keyword search'  FROM docs LIMIT 10 USING SPARSE
```

### Recsys modes
```sql
QUERY RECOMMEND WITH (positive = ('id1'), negative = ('id2'))
QUERY CONTEXT PAIRS (('pos', 'neg')) FROM docs LIMIT 10
QUERY DISCOVER TARGET 'id' CONTEXT PAIRS (('pos', 'neg'))
QUERY RELEVANCE FEEDBACK TARGET 'q' FEEDBACK ((1, 0.9), (2, 0.1))
```

### Multi-stage retrieval (CTE + Prefetch + Fusion)
```sql
WITH dense AS (QUERY 'search' USING dense LIMIT 100),
     sparse AS (QUERY 'search' USING sparse LIMIT 100)
QUERY 'search' FROM docs LIMIT 10
  PREFETCH (dense WHERE priority = 'high', sparse)
  FUSION RRF WITH (rrf_k = 60)
```

### Score shaping (BOOST)
```sql
QUERY 'search' FROM docs LIMIT 10
  BOOST ($score + 0.3 * popularity)
  DEFAULTS (popularity = 1.0)
```

### Filters
```sql
WHERE tenant_id = 'acme'
  AND status IN ('active', 'pending')
  AND score >= 0.5
  AND created_at BETWEEN '2024-01-01' AND '2025-01-01'
  AND tags IS NOT EMPTY
```

---

## Architecture

```
┌─────────────────────────────────────────────────────┐
│                   Your Application                  │
│                                                     │
│  ┌─────────────────┐       ┌────────────────────┐   │
│  │  QQL Parser SDK  │       │  Qdrant SDK        │   │
│  │  (Python/JS/Go/  │       │  (official client) │   │
│  │   Rust/WASM)     │       │                    │   │
│  │                   │       │  query_points()    │   │
│  │  parse()          │       │  upsert()          │   │
│  │  tokenize()       │  ─►   │  create_collection │   │
│  │  inject_filter()  │       │  query_batch()     │   │
│  │  is_valid()       │       └────────┬───────────┘   │
│  └───────────────────┘              │               │
│                                     ▼               │
│                              ┌──────────────┐      │
│                              │   Qdrant      │      │
│                              │   (vector DB) │      │
│                              └──────────────┘      │
└─────────────────────────────────────────────────────┘
```

The parser gives you a typed AST. You decide what to do with it —
feed it to the Qdrant SDK, inject security filters, validate it,
or log it for audit. No gateway, no YAML, no interceptors.

---

## Benchmarks

Parser throughput across all SDKs (ns/op, lower is better):

| Query | Rust | Go | Python | Node.js |
|-------|------|----|--------|---------|
| Simple | **389 ns** | 529 ns | 5,832 ns | 6,917 ns |
| Hybrid | **514 ns** | 636 ns | 6,149 ns | 6,881 ns |
| Full | **1,234 ns** | 1,565 ns | 12,285 ns | 12,815 ns |
| CTE Prefetch | **2,662 ns** | 3,278 ns | 53,456 ns | 53,872 ns |

Native SDKs (Rust, Go) have no FFI tax. Bindings (Python, Node.js)
trade ~5–10 µs per call for the convenience of using QQL from your
preferred language — negligible next to embedding inference (50–200 ms).

Full benchmark report at [`bench/README.md`](bench/README.md).

---

## Contributing

```bash
# Build everything
make build

# Run all tests
make test

# Run all benchmarks
make bench

# Run all examples
make examples
```

See the [`Makefile`](Makefile) for individual targets.
