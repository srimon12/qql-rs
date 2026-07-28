# nqql-edge

Native Node.js bindings for local QQL execution. `nqql-edge` combines the QQL
runtime with `qdrant-edge` for in-process vector storage and FastEmbed for
optional local ONNX inference.

## Features

- **In-Process Vector Storage**: Run Qdrant search engine locally inside Node.js process with zero server daemon requirement
- **Embedded ONNX Inference**: Automatically fetch and run FastEmbed ONNX models on-device
- **Native Route Lowering**: Lower QQL queries to typed route objects via `compileQuery`
- **Native Parsing**: Rust-speed QQL parsing in Node.js returning `Stmt` objects
- **Filter Injection**: Programmatically add tenant isolation filters
- **Validation**: Check if a query string is valid QQL
- **Smart Batching**: Auto-batches contiguous same-collection query/mutation statements

## Requirements & Supported Platforms

- **Node.js**: `>=18`
- **Supported Platforms**:
  - Linux x64 (`glibc`) — `@veristamp/nqql-edge-linux-x64-gnu`
  - macOS arm64 (`Apple Silicon`) — `@veristamp/nqql-edge-darwin-arm64`
  - Windows x64 (`msvc`) — `@veristamp/nqql-edge-win32-x64-msvc`
- *Note: Prebuilt native packages are not published for macOS Intel (Darwin x64) because ONNX Runtime lacks Darwin x64 N-API prebuilds.*

## Installation

```bash
npm install @veristamp/nqql-edge
```

Embedding models download on first use from HuggingFace and cache locally.

## Quick Start

```javascript
const {
  localExecutor, listEmbeddingModels, httpExecutor,
  parse, parseJson, isValid, injectFilter, tokenize,
  compileQuery, explain, explainStmt, execute, executeStmt, version
} = require('@veristamp/nqql-edge');

// 1. Create a local edge executor (embedded ONNX + local storage)
const client = localExecutor('./qql-data', {
  model: 'bge-small-en-v1.5',
  onDiskPayload: true,
});

await client.execute('CREATE COLLECTION docs HYBRID');
await client.execute('UPSERT INTO docs VALUES {id: 1, text: "hello from edge"}');

const report = await client.execute("QUERY 'hello' FROM docs USING dense LIMIT 5");
console.log(report);

await client.close();

// 2. Pure AST Parsing & Filter Injection
const stmts = parse("QUERY 'full text match' FROM articles LIMIT 10");
const rawJson = parseJson("QUERY 'full text match' FROM articles LIMIT 10");
const valid = isValid("QUERY 'test' FROM docs");

stmts[0].injectFilter("tenant_id", "=", "acme-corp");
const securedAst = injectFilter("QUERY 'search' FROM docs LIMIT 10", "org_id", "=", "acme-corp");
```

## Execution Results & Errors

### ExecutionReport Format

All execution methods return an `ExecutionReport` object:

```json
{
  "ok": true,
  "results": [
    {
      "ok": true,
      "operation": "QUERY",
      "message": "Found 5 hits",
      "data": [ ... ]
    }
  ],
  "succeeded": 1,
  "failed": 0
}
```

### Failure Policy (`onError`)

| Policy | Behavior |
|---|---|
| `"stop"` (default) | Halts batch execution on the first error and throws an exception. |
| `"continue"` | Continues executing remaining statements, collecting failures into `results` with `ok: false`. |

## Filter Injection Operators

`injectFilter` accepts comparison operators:

- **Accepted**: `=`, `==`, `eq`, `>`, `gt`, `>=`, `gte`, `<`, `lt`, `<=`, `lte`
- **Rejected**: `!=`, `neq`, `<>`, `in`, `is_null`, `contains` (throws error — wrap with `NOT` or write in QQL query)

## API Summary

| Export | Description |
|---|---|
| `localExecutor(dataDir, options)` | Create a fully local edge Client backed by fastembed-rs & qdrant-edge |
| `listEmbeddingModels()` | List dense ONNX models available for `localExecutor({ model })` |
| `httpExecutor(dataDir, url, key, model, dim)` | Create an edge Client with local vector storage and remote HTTP embedder |
| `Stmt` | Parsed statement object (`injectFilter`, `toObject`, `toJson` / `toJSON`, `shardKey`) |
| `parse(input)` | Parse into array of `Stmt` objects |
| `parseJson(input)` | Parse to raw JSON string (bypasses V8 object allocation) |
| `isValid(input)` | Validate QQL syntax |
| `tokenize(input)` | Tokenize QQL input string |
| `injectFilter(query, field, op, value)` | Inject tenant filter into statement AST |
| `compileQuery(input)` | Lower QQL statement into `{ stmt_type, method, path, payload }` route object |
| `explain(query)` | Inspect the execution plan without executing network calls |
| `execute(query, options?)` | One-shot execute with temporary edge client; `options.onError` is `"stop"` or `"continue"` |
| `executeStmt(stmt, options?)` | Free-function execute a pre-parsed Stmt |
| `version` | Package runtime version string |

## Documentation Links

- [QQL Syntax Guide](https://github.com/srimon12/qql-rs/blob/main/docs/syntax.md)
- [Filter Documentation](https://github.com/srimon12/qql-rs/blob/main/docs/filters.md)
- [Filter Injection Guide](https://github.com/srimon12/qql-rs/blob/main/docs/inject_filter.md)
- [Changelog](https://github.com/srimon12/qql-rs/blob/main/CHANGELOG.md)
