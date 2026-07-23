# qql-core

Transport-free QQL frontend: lexer, strict parser, typed AST, validation,
AST transforms (`inject_filter`), and intent-only explain output.
Performs no I/O and does not generate Qdrant JSON.

## Parser

The parser produces one of these [`Statement`] variants:

| Variant | SQL example |
|---------|-------------|
| `Query` | `QUERY 'search' FROM docs USING dense LIMIT 10` |
| `Upsert` | `UPSERT INTO docs VALUES {id: 1, text: 'hello'}` |
| `Scroll` | `SCROLL FROM docs WHERE status = 'active' LIMIT 50` |
| `Delete` | `DELETE FROM docs WHERE id = 1` |
| `ClearPayload` | `CLEAR PAYLOAD FROM docs WHERE id = 1` |
| `DeleteVector` | `DELETE VECTOR colbert FROM docs WHERE id = 42` |
| `UpdateVector` | `UPDATE docs SET VECTOR = [0.1, 0.2, 0.3] WHERE id = 1` |
| `UpdatePayload` | `UPDATE docs SET PAYLOAD = {status: 'active'} WHERE id = 1` |
| `Count` | `COUNT FROM docs WHERE status = 'active'` |
| `CreateCollection` | `CREATE COLLECTION docs (dense VECTOR(384, COSINE))` |
| `AlterCollection` | `ALTER COLLECTION docs WITH PARAMS (replication_factor = 2)` |
| `DropCollection` | `DROP COLLECTION docs` |
| `CreateIndex` | `CREATE INDEX ON COLLECTION docs FOR title TYPE text` |
| `DropIndex` | `DROP INDEX ON COLLECTION docs FOR title` |
| `CreateShardKey` | `CREATE SHARD KEY 'acme' ON COLLECTION docs` |
| `DropShardKey` | `DROP SHARD KEY 'acme' ON COLLECTION docs` |
| `ShowCollections` | `SHOW COLLECTIONS` |
| `ShowCollection` | `SHOW COLLECTION docs` |
| `ShowShardKeys` | `SHOW SHARD KEYS ON COLLECTION docs` |

## Query contract

`QUERY` is the sole retrieval entry point. Direct point retrieval and
similarity-by-point are distinct:

```sql
QUERY POINTS (42, 'point-a') FROM docs WITH PAYLOAD true;
QUERY NEAREST POINT 42 FROM docs USING dense LIMIT 10;
```

The typed `QueryExpr` enum covers nearest text/vector/point, recommend, context,
discover, order-by, random sample, RRF/DBSF fusion, formula scoring, relevance
feedback, MMR, hybrid shorthand, and explicit rerank. Fusion and rerank own their
required prefetch topology in the AST.

### Clause order

```
QUERY <expression>
FROM <collection>
[USING <vector>]
[PREFETCH (...)]
[WHERE <filter>]
[SHARD '<key>']
[PARAMS (...)]
[SCORE THRESHOLD <number>]
[GROUP BY <field> [SIZE <n>] [LOOKUP FROM <collection>]]
[WITH PAYLOAD <selector>]
[WITH VECTOR <selector>]
[LIMIT <positive integer>]
[OFFSET <non-negative integer>]
```

Each clause occurs at most once and only in this order.

## Search params (PARAMS)

```ebnf
search-param = "hnsw_ef", "=", integer
             | "exact", "=", boolean
             | "acorn", "=", boolean
             | "indexed_only", "=", boolean
             | "quantization", "=", object
             | "rrf_k", "=", integer
             | "rrf_weights", "=", array
```

`acorn` (Adaptive Cardinality Estimator for ONgRN) controls approximate search
selectivity estimation. When `acorn = true`, Qdrant uses ACORN to estimate
filter cardinality and adapt the search strategy.

`quantization` accepts an object matching the Qdrant QuantizationSearchParams
schema: `{ "ignore": bool, "rescore": bool, "oversampling": float }`.

## Errors

Every error has an explicit `ErrorKind` (`Lex`, `Parse`, or `Validation`),
a stable machine-readable code, human message, and optional byte
`Span { start, end }`.

## Features

- `serde`: AST/token/error serialization
- `json`: enables `serde` + fallible host JSON conversion for `Value`
- `std`: implements `std::error::Error` for `QqlError`

## API

```rust
use qql_core::ast::{QueryExpr, Stmt};
use qql_core::parser::Parser;

let statement = Parser::parse("QUERY TEXT 'hello' FROM docs LIMIT 5;")?;
if let Stmt::Query(query) = statement {
    assert!(matches!(query.expression, QueryExpr::Nearest { .. }));
}

let script = Parser::parse_all(
    "SHOW COLLECTIONS; QUERY POINTS (1, 2) FROM docs;",
)?;
# Ok::<(), qql_core::error::QqlError>(())
```

Multiple statements require `;`. A single trailing semicolon is optional;
leading and repeated empty statements are rejected.

## Verification

```bash
cargo test -p qql-core -- --test-threads=4
```

Tests cover: positive parsing for all statement types, negative parsing
(rejected syntax), lexer roundtrip, filter lowering, transform roundtrip
(inject_filter across CTEs/prefetches), DDL config block parsing, and
formula expression parsing.
