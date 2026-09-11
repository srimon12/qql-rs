# qql-convert

Qdrant REST JSON to QQL statement converter.

The converter is contract-driven and AST-based:

```text
OpenAPI request JSON -> qql_core::ast::Stmt -> qql_core::fmt::format_stmt
```

Every emitted string comes from the canonical QQL formatter (the same emitter
behind `qql fmt`), so it re-parses and is byte-stable under
`format_stmt(parse(emit))`. Decoding fails closed: shapes the QQL AST cannot
express return a typed `ConvertError` instead of a placeholder or a silently
dropped field.

## Input shapes

1. **Wrapped request** — `{"method", "path", "query"?, "body"?}`; the
   collection is derived from the path. `query` recovers `wait` / `timeout` /
   `consistency` (also accepted as a `?…` suffix on `path`).
2. **Bare body** — raw Qdrant REST JSON without path context; the caller must
   supply the collection. Ambiguous shapes fail with `UndecodableBody`.
3. **JSONL capture** — one wrapped request or bare body per line (`qql record
   --out`). Selected when the buffer is not a single JSON value.

```rust
let stmts = qql_convert::convert(r#"{"ids": [1]}"#, Some("docs")).unwrap();
assert_eq!(stmts, ["QUERY POINTS (1) FROM docs"]);
```

Bare bodies without a collection return `ConvertError::MissingCollection`.

## Contract sources

* `crates/qql-runtime/openapi.json` — request schemas per path (the input
  contract; asserted by `tests/contract.rs`).
* `crates/qql-plan` — the inverse lowering. `tests/route_parity.rs` plans a
  corpus (every `QueryExpr` variant, mutations, DDL, and `bench/queries.json`),
  converts its REST body *and* query string back to QQL, re-plans, and asserts
  the two routes are identical.
