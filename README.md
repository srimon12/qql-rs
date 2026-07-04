# QQL — Qdrant Query Language

**Native Rust parser for QQL. Use from Python, TypeScript, Go, or Rust — no gateway, no server required.**

```python
from pyqql import parse

stmt = parse("QUERY 'chest pain' FROM medical LIMIT 5 WHERE department = 'cardio'")
# → structured AST you can introspect, transform, inject filters into
```

---

## Architecture

```
┌───────────────────────────────────────────────┐
│                 Your Application              │
│                                               │
│  ┌──────────────┐       ┌──────────────────┐  │
│  │ QQL Parser   │       │ Qdrant Client    │  │
│  │ (native Rust)│       │ (official SDK)   │  │
│  │              │       │                  │  │
│  │ parse(qql)   │       │ query_points()   │  │
│  │ → AST        │       │ upsert()         │  │
│  │              │       │ create_collection│  │
│  └──────┬───────┘       └────────┬─────────┘  │
│         │                        │            │
│         │  AST → SDK call        │            │
│         └────────────────────────┘            │
└───────────────────────────────────────────────┘
```

**Key principle:** The parser gives you a typed AST. You decide what to do with it — feed it to `qdrant-client`, transform it, inject filters, validate it. No gateway, no YAML policies, no JWT interceptor chain.

---

## Crates

| Crate | Description | Dependencies |
|-------|-------------|-------------|
| `qql-core` | Lexer, parser, AST, errors. Pure parsing. | `phf` (compile-time keyword map) |
| `qql` | Runtime: pipeline, filter conversion, BM25 sparse, HTTP embedding, config. Optional. | `qql-core`, `tokio`, `reqwest` |
| `qql-cli` | CLI binary (`qql`). Exec, explain, REPL, dump, convert. | `qql-core`, `qql`, `clap` |
| `pyqql` | Python bindings via PyO3 (`pip install pyqql`) | `qql-core` |
| `nqql` | Node.js bindings via napi-rs (`npm install nqql`) | `qql-core` |
| `qql-wasm` | WASM bindings (`wasm-bindgen`), browser-side parsing | `qql-core` |

---

## Usage

### Python

```bash
pip install pyqql qdrant-client
```

```python
from pyqql import parse, tokenize

# Parse any QQL statement
stmt = parse("QUERY 'search' FROM docs LIMIT 10")
print(stmt)  # Debug representation of the AST

# Tokenize for syntax highlighting or validation
tokens = tokenize("SELECT * FROM docs WHERE id = '123'")
for t in tokens:
    print(f"{t['kind']:15s} {t['text']!r:30s} @{t['pos']}")

# Use the official qdrant-client to actually talk to Qdrant
from qdrant_client import QdrantClient
client = QdrantClient(host="localhost")

# AST injection — no gateway needed
stmt = parse("QUERY 'patients' FROM medical LIMIT 50")
# Your middleware injects tenant isolation directly:
# stmt → inject filter WHERE org_id = 'acme-corp'
# → qdrant_client.query_points(filter=...) 
```

### Node.js

```bash
npm install nqql @qdrant/qdrant-js
```

```js
const { parse } = require('nqql');
const ast = parse("QUERY 'heart attack' FROM medical LIMIT 5");
console.log(ast);
```

### Rust

```toml
[dependencies]
qql-core = { git = "..." }  # parser only
# or
qql = { git = "..." }       # full runtime + CLI
```

```rust
use qql_core::parser::Parser;
use qql_core::ast::Stmt;

let stmt = Parser::parse("QUERY 'search' FROM docs LIMIT 10").unwrap();
match stmt {
    Stmt::Query(q) => {
        println!("collection: {:?}", q.collection);
        println!("query_text: {:?}", q.query_text);
        println!("limit: {}", q.limit);
    }
    _ => {}
}
```

### CLI

```bash
# Explain a query (no Qdrant needed)
qql explain "QUERY 'hello' FROM docs LIMIT 5 USING HYBRID"

# Parse + validate
qql exec "CREATE COLLECTION docs (dense VECTOR(384, COSINE), sparse SPARSE)"

# Convert REST JSON to QQL
qql convert payload.json

# Dump a collection as .qql
qql dump medical backup.qql
```

---

## Supported QQL Syntax

### Statements

| Statement | Example |
|-----------|---------|
| QUERY | `QUERY 'text' FROM collection LIMIT 10` |
| QUERY (hybrid) | `QUERY 'text' FROM collection LIMIT 10 USING HYBRID` |
| QUERY (sparse) | `QUERY 'text' FROM collection LIMIT 10 USING SPARSE` |
| QUERY (recommend) | `QUERY RECOMMEND WITH (positive = ('id1', 'id2')) FROM collection LIMIT 10` |
| QUERY (context) | `QUERY CONTEXT PAIRS (('pos', 'neg')) FROM collection LIMIT 10` |
| QUERY (discover) | `QUERY DISCOVER TARGET 'id' CONTEXT PAIRS (('pos', 'neg')) FROM collection LIMIT 10` |
| QUERY (order by) | `QUERY ORDER BY field ASC FROM collection LIMIT 10` |
| QUERY (sample) | `QUERY SAMPLE FROM collection LIMIT 5 WHERE status = 'active'` |
| QUERY (relevance feedback) | `QUERY RELEVANCE FEEDBACK TARGET 'q' FEEDBACK ((1, 0.9), (2, 0.1)) FROM collection LIMIT 10` |
| WITH + PREFETCH + FUSION | `WITH cte AS (...) QUERY ... PREFETCH (cte) FUSION RRF` |
| INSERT | `INSERT INTO collection VALUES {'id': 1, 'text': 'hello'} USING HYBRID` |
| INSERT with EMBED | `INSERT INTO collection VALUES {...} EMBED field INTO vector_name` |
| CREATE COLLECTION | `CREATE COLLECTION name HYBRID WITH QUANTIZATION (type = 'scalar')` |
| CREATE COLLECTION (explicit) | `CREATE COLLECTION name (dense VECTOR(384, COSINE), sparse SPARSE)` |
| CREATE INDEX | `CREATE INDEX ON COLLECTION name FOR field TYPE keyword` |
| ALTER COLLECTION | `ALTER COLLECTION name WITH HNSW (m = 32)` |
| DROP COLLECTION | `DROP COLLECTION name` |
| SHOW COLLECTIONS | `SHOW COLLECTIONS` |
| SHOW COLLECTION | `SHOW COLLECTION name` |
| SELECT | `SELECT * FROM collection WHERE id = 'uuid'` |
| SCROLL | `SCROLL FROM collection WHERE filter LIMIT 100` |
| DELETE | `DELETE FROM collection WHERE field = 'value'` |
| UPDATE (payload) | `UPDATE collection SET PAYLOAD = {'key': 'val'} WHERE id = 1` |
| UPDATE (vector) | `UPDATE collection SET VECTOR = [0.1, 0.2] WHERE id = 1` |

### WHERE Clauses

All standard filter operators:

| Operator | Example |
|----------|---------|
| `=` | `field = 'value'`, `field = 42` |
| `!=` | `field != 'value'` |
| `>` / `>=` / `<` / `<=` | `age > 18`, `price >= 100` |
| `IN` | `status IN ('active', 'pending')` |
| `NOT IN` | `status NOT IN ('deleted')` |
| `BETWEEN` | `age BETWEEN 18 AND 65` |
| `IS NULL` / `IS NOT NULL` | `deleted_at IS NULL` |
| `IS EMPTY` / `IS NOT EMPTY` | `tags IS NOT EMPTY` |
| `MATCH` | `content MATCH 'text'` |
| `MATCH ANY` | `content MATCH ANY 'text'` |
| `MATCH PHRASE` | `content MATCH PHRASE 'exact phrase'` |
| `AND` / `OR` / `NOT` | `a = 1 AND (b = 2 OR c = 3)` |
| `NESTED(path, filter)` | `NESTED('address', city = 'NYC')` |

### BOOST (Score Shaping)

```sql
QUERY 'search' FROM docs LIMIT 10
  BOOST ($score * 2.0 + CASE WHEN priority = 'high' THEN 10 ELSE 0 END)
```

Supports: arithmetic (`+`, `-`, `*`, `/`), functions (`ABS`, `SQRT`, `LOG`, `LN`, `EXP`, `POW`),
geo (`GEO_DISTANCE`), decay (`GAUSS_DECAY`, `EXP_DECAY`, `LIN_DECAY`),
conditionals (`CASE WHEN ... THEN ... ELSE ... END`), match (`MATCH(field, values)`).

---

## Filters: All 15 Variants (Tested)

| Variant | AST Type | FilterConv Support |
|---------|----------|-------------------|
| Compare (`=`, `!=`, `>`, `>=`, `<`, `<=`) | `FilterExpr::Compare` | ✅ |
| Range | `FilterExpr::Between` | ✅ |
| In | `FilterExpr::In` | ✅ |
| NotIn | `FilterExpr::NotIn` | ✅ |
| IsNull | `FilterExpr::IsNull` | ✅ |
| IsNotNull | `FilterExpr::IsNotNull` | ✅ |
| IsEmpty | `FilterExpr::IsEmpty` | ✅ |
| IsNotEmpty | `FilterExpr::IsNotEmpty` | ✅ |
| MatchText | `FilterExpr::MatchText` | ✅ |
| MatchAny | `FilterExpr::MatchAny` | ✅ |
| MatchPhrase | `FilterExpr::MatchPhrase` | ✅ |
| And | `FilterExpr::And` | ✅ |
| Or | `FilterExpr::Or` | ✅ |
| Not | `FilterExpr::Not` | ✅ |
| Nested | `FilterExpr::Nested` | ✅ |

All 15 variants are parsed, stored in the AST, and convertible to Qdrant native filter types.
37 test cases cover every variant and combination.

---

## CLI

```bash
# Build
cargo build -p qql-cli --release
./target/release/qql --help

qql exec    "QUERY 'search' FROM docs LIMIT 10"
qql explain "SELECT * FROM docs WHERE id = '123'"
qql convert /tmp/payload.json
qql dump    medical backup.qql
```

---

## Python Integration (Demo)

```bash
# Build the wheel once
cd crates/pyqql
maturin build --release --out /tmp/pyqql-dist

# Use from any Python project
pip install /tmp/pyqql-dist/pyqql-0.1.0-*.whl
pip install qdrant-client
```

See `/tmp/qql_demo.py` for a complete working demo:

```bash
uv run --with /tmp/pyqql-dist/pyqql-0.1.0-*.whl python /tmp/qql_demo.py
```

Example output:
```
=== Raw Tokens ===
  QUERY            'QUERY'                         @0
  STRING           'chest pain treatment'          @6
  FROM             'FROM'                          @29
  IDENTIFIER       'medical'                       @34
  LIMIT            'LIMIT'                         @42
  INTEGER          '5'                             @48
  WHERE            'WHERE'                         @50
  IDENTIFIER       'department'                    @56
  EQUALS           '='                             @67
  STRING           'cardio'                        @69

=== CTE / PREFETCH / FUSION ===
  INPUT:
WITH
  dense AS (QUERY 'search' USING 'dense' LIMIT 100),
  sparse AS (QUERY 'search' USING 'sparse' LIMIT 100)
QUERY 'search' FROM docs LIMIT 10
  PREFETCH (dense, sparse) FUSION RRF
  WHERE tenant_id = 'acme-corp'

  OUTPUT: Query(QueryStmt { ctes: [...], prefetch_refs: [...], ... })
```

---

## Status

| Component | Tests | Status |
|-----------|-------|--------|
| Lexer (128 keywords, zero-copy) | 33 | ✅ |
| Parser (14 statement types, all clauses) | 159 | ✅ |
| AST (Stmt, FilterExpr, FormulaExpr) | 37 | ✅ |
| Filter conversion (15 variants) | 37 | ✅ |
| BM25 sparse vector | 11 | ✅ |
| Pipeline + nodes | 24 | ✅ |
| CLI (exec, explain, dump, convert, script) | — | ✅ |
| Python bindings (PyO3) | — | ✅ |
| Node.js bindings (napi-rs) | — | ✅ scaffolded |
| WASM bindings | — | ✅ scaffolded |
| Total | 314 | All passing |
