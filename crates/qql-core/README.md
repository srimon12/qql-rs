# qql-core

Transport-free QQL frontend: lexer, parser, typed AST, validation,
`inject_filter`, and explain. **No I/O, no Qdrant JSON.**

Canonical grammar: [`language/v1/grammar.pest`](../../language/v1/grammar.pest)
→ `qql-grammar-gen` → checked-in `grammar/` (do not edit by hand).

## Proposition

Own the **language surface** for every SDK. Hosts and agents parse here, inject
policy here, then hand `Stmt` to `qql-plan` / runtime.

## Statement surface

| Kind | Examples |
|------|----------|
| Query | `QUERY`, CTEs, hybrid, formula, rerank, recommend, … |
| DML | `UPSERT`, `DELETE`, `SCROLL`, `COUNT`, payload/vector updates |
| DDL | `CREATE/ALTER/DROP COLLECTION`, indexes, **`CREATE/DROP/SHOW SHARD KEY`** |
| Meta | `SHOW COLLECTIONS` / `SHOW COLLECTION` |

### Clause order (QUERY)

```
QUERY <expr> FROM <coll>
  [USING HYBRID … | USING <vec> [AS DENSE|SPARSE|MULTI]]
  [PREFETCH (…)] [WHERE …] [SHARD '…'] [PARAMS (…)]
  [SCORE THRESHOLD n] [GROUP BY …] [WITH PAYLOAD|VECTOR …]
  [LIMIT n] [OFFSET n]
```

### `SHARD KEY` vs `SHARD`

| Form | Role |
|------|------|
| `CREATE SHARD KEY 'acme' ON COLLECTION c` | DDL — define custom partition |
| `… SHARD 'acme'` | DML — route this request |

Routing field after parse: `stmt.set_shard_key(Some("acme".into()))`  
(same AST field; **no** `inject_shard_key`).

### PARAMS (selected)

Body search params: `hnsw_ef`, `exact`, `acorn`, `max_selectivity`, `quantization`, …  
**Request-level** (REST query string / gRPC fields): `timeout`, `consistency`.

## API

```rust
use qql_core::ast::{inject_filter, ComparisonOp, Stmt, Value};
use qql_core::parser::Parser;

let mut stmt = Parser::parse(
    "QUERY TEXT 'hello' FROM docs USING dense LIMIT 5"
)?;

// Isolation (recurses CTEs / prefetches)
inject_filter(
    &mut stmt,
    "tenant_id",
    ComparisonOp::Eq,
    Value::Str("org_99".into()),
)?;

// Optional routing (custom sharding)
stmt.set_shard_key(Some("org_99".into()));
// Prefer authoring: ... SHARD 'org_99' LIMIT 5
```

| Feature | Role |
|---------|------|
| `serde` | AST serialize |
| `json` | `Value` from/to JSON |
| `std` | `std::error::Error` |

## Docs

- [Syntax](../../docs/syntax.md) · [inject_filter](../../docs/inject_filter.md) · [Multitenancy](../../skills/qql-skill/references/qql-multitenancy.md)

## Test

```bash
cargo test -p qql-core
```
