# qql-core

`qql-core` is the transport-free Qdrant Query Language frontend. It contains the lexer, strict parser, typed AST, validation, AST transforms, and intent-only explain output. It performs no I/O and does not generate Qdrant JSON.

## Query Contract

`QUERY` is the only retrieval entry point. Direct point retrieval and similarity by point are distinct:

```sql
QUERY POINTS (42, 'point-a') FROM docs WITH PAYLOAD true;
QUERY NEAREST POINT 42 FROM docs USING dense LIMIT 10;
```

The typed `QueryExpr` enum covers nearest text/vector/point, recommend, context, discover, payload order, random sample, RRF/DBSF fusion, formula scoring, relevance feedback, MMR, hybrid shorthand, and explicit rerank. Fusion and rerank own their required prefetch topology in the AST rather than using flags.

Queries use one clause order:

```text
QUERY <expression>
FROM <collection>
[USING <vector>]
[PREFETCH (...)]
[WHERE <filter>]
[PARAMS (...)]
[SCORE THRESHOLD <number>]
[GROUP BY <field> [SIZE <positive integer>] [LOOKUP FROM <collection>]]
[WITH PAYLOAD <selector>]
[WITH VECTOR <selector>]
[LIMIT <positive integer>]
[OFFSET <non-negative integer>]
```

See [`../../docs/syntax.md`](../../docs/syntax.md) for the canonical grammar.

## Other Statements

The AST also covers collection and payload-index DDL, typed UPSERT points, scrolling, vector/payload updates, and deletes. Point IDs are `PointId`; vectors are dense, sparse, or multidense; update/delete targets are one `PointSelector` enum.

## Errors

Every error has an explicit `ErrorKind` (`Lex`, `Parse`, or `Validation`), stable code, message, and optional byte `Span { start, end }`.

## Features

- `serde`: AST/token/error serialization only.
- `json`: enables `serde` and fallible host JSON conversion for `Value`.
- `std`: implements `std::error::Error` for `QqlError`.

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

Multiple statements require semicolons. A single trailing semicolon is optional; leading and repeated empty statements are rejected.
