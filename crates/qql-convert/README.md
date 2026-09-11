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

1. **Wrapped request** — `{"method": ..., "path": ..., "body": ...}`; the
   collection is derived from the path. The `(method, path)` pair is resolved
   against the "Statement → Endpoint Matrix" in the workspace `AGENTS.md`
   (26 routes); everything else is `UnsupportedEndpoint`.
2. **Bare body** — raw Qdrant REST JSON without path context; the caller
   supplies the collection via `json_to_qql_with_collection`
   (`json_to_qql` falls back to `"unknown"`). Bare detection is a documented,
   ordered heuristic; ambiguous shapes fail with `UndecodableBody`.

```rust
let stmts = qql_convert::json_to_qql_with_collection(r#"{"ids": [1]}"#, "docs").unwrap();
assert_eq!(stmts, ["QUERY POINTS (1) FROM docs"]);
```

## Contract sources

* `crates/qql-runtime/openapi.json` — request schemas per path (the input
  contract; asserted by `tests/contract.rs`).
* `crates/qql-plan` — the inverse lowering. `tests/route_parity.rs` plans a
  corpus (every `QueryExpr` variant, mutations, DDL, and `bench/queries.json`),
  converts its REST body back to QQL, re-plans, and asserts the two routes are
  identical.
