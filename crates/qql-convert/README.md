# qql-convert

Qdrant REST JSON to QQL statement converter.

Accepts either a wrapped request (`{"method": ..., "path": ..., "body": ...}`,
where the collection is derived from the path) or a bare REST body (where the
caller supplies the collection via `json_to_qql_with_collection`).

```rust
let stmts = qql_convert::json_to_qql_with_collection(r#"{"ids": [1]}"#, "docs").unwrap();
assert_eq!(stmts, ["QUERY POINTS (1) FROM docs"]);
```
