# qql-convert

Qdrant REST JSON to QQL statement converter.

## Proposition

Migrate REST code to QQL without hand-translation. The converter is
contract-driven and AST-based:

```text
OpenAPI request JSON -> qql_core::ast::Stmt -> qql_core::fmt::format_stmt
```

Every emitted string comes from the canonical QQL formatter (the same emitter
behind `qql fmt`), so it re-parses and is byte-stable under
`format_stmt(parse(emit))`. Decoding fails closed: shapes the QQL AST cannot
express return a typed `ConvertError` instead of a placeholder or a silently
dropped field.

## Install

```bash
cargo add qql-convert
```

CLI entry point (no library code needed for one-off migration):

```bash
cargo build --release -p qql-cli
qql convert --collection docs capture.jsonl
```

## Quick start

```rust
let stmts = qql_convert::convert(r#"{"ids": [1]}"#, Some("docs")).unwrap();
assert_eq!(stmts, ["QUERY POINTS (1) FROM docs"]);
```

Bare bodies without a collection return `ConvertError::MissingCollection`.

```bash
# Wrapped request (method + path + body) — collection derived from the path
echo '{"method":"POST","path":"/collections/docs/points/query","body":{"limit":5}}' \
  | qql convert

# Bare body — caller supplies the collection
echo '{"ids": [1]}' | qql convert --collection docs

# JSONL capture from `qql record` — one request per line
qql convert --collection docs capture.jsonl
```

Pair with the recorder for zero-code-change capture (see the
[`qql-cli` recorder](https://github.com/srimon12/qql-rs/blob/main/crates/qql-cli/README.md#recorder-qql-record-opt-in)):

```bash
qql record --listen 127.0.0.1:6334 --target http://127.0.0.1:6333 \
  --out capture.jsonl --qql-out capture.qql
# ... run the app ...
qql convert --collection docs capture.jsonl   # replay/migrate later
```

## Input shapes

1. **Wrapped request** — `{"method", "path", "query"?, "body"?}`; the
   collection is derived from the path. `query` recovers `wait` / `timeout` /
   `consistency` (also accepted as a `?…` suffix on `path`).
2. **Bare body** — raw Qdrant REST JSON without path context; the caller must
   supply the collection. Ambiguous shapes fail with `UndecodableBody`.
3. **JSONL capture** — one wrapped request or bare body per line (`qql record
   --out`). Selected when the buffer is not a single JSON value.
4. **HTTP snippet** — a `METHOD /path` line plus a JSON body, exactly as docs
   and blogs print it. Full URLs and an `HTTP/x` suffix are accepted, and
   several snippets may follow each other (blank lines optional):
   ```http
   POST /collections/docs/points/query
   {"query": {"nearest": [0.1, 0.2]}, "using": "dense", "limit": 5}
   ```
5. **`curl` command** — a pasted invocation, including multi-line
   `\`-continuations, `-X` / `--request`, `-d` / `--data*` / `--json`,
   `--header`, and several commands separated by newlines or `;`:
   ```bash
   curl -X POST http://localhost:6333/collections/docs/points/count \
     -H 'Content-Type: application/json' \
     -d '{"exact": true}' | qql convert
   ```
   A missing `-X` infers the method (`--data` implies POST, else GET).

Markdown fences around any of the above are stripped as paste noise.

Paste shapes reuse the same lowering as wrapped requests, so they cannot
drift. Anything shell-dynamic fails closed instead of guessing: `$VAR` /
`$(…)` / backticks, `--data @file` (paste the JSON inline), `|` / `&` /
subshells, and `--data-urlencode` / `--config` / `-T` file flags. Collection
segments that look like placeholders (`<your-collection>`,
`{collection_name}`, `$COLLECTION`) are rejected with `InvalidField` on every
path — including `--collection` — rather than emitting QQL that cannot
re-parse.

## Errors

`convert` / `convert_stmts` return `ConvertError` — never a placeholder string:

| Variant | Meaning |
|---------|---------|
| `InvalidJson` | Input is not valid JSON (holds the parse message) |
| `UnsupportedEndpoint` | Wrapped request targets an endpoint with no QQL mapping (holds `"METHOD path"`) |
| `MissingCollection` | Bare body without a collection name (`--collection`) |
| `UndecodableBody` | Body absent or structurally undecodable for the endpoint (holds a reason) |
| `InvalidField` | Recognized body with an invalid/unrepresentable field (holds `path` + reason) |
| `InvalidLine` | A JSONL line failed (holds the 1-based line number + source error) |

## Contract sources

* `crates/qql-runtime/openapi.json` — request schemas per path (the input
  contract; asserted by `tests/contract.rs`).
* `crates/qql-plan` — the inverse lowering. `tests/route_parity.rs` plans a
  corpus (every `QueryExpr` variant, mutations, DDL, and `bench/queries.json`),
  converts its REST body *and* query string back to QQL, re-plans, and asserts
  the two routes are identical.

## Docs

- [qql-cli recorder + CLI](https://github.com/srimon12/qql-rs/blob/main/crates/qql-cli/README.md#recorder-qql-record-opt-in)
- [Convert + migration guide](https://github.com/srimon12/qql-rs/blob/main/skills/qql-skill/references/convert-migration.md)
- [Syntax](https://github.com/srimon12/qql-rs/blob/main/docs/syntax.md)
- [docs.rs](https://docs.rs/qql-convert)

## Test

```bash
cargo test -p qql-convert
```
