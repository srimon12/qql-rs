# nqql

Node.js native bindings for the QQL parser, plan compiler, and execution engine, compiled using N-API (`napi-rs`).

## Features

- **Live Qdrant Execution**: Connect to live Qdrant instances over REST (default) or gRPC
- **First-Class Embedding Inference**: Integrate custom HTTP embedder models (Ollama, OpenAI, vLLM, TEI)
- **Native Route Lowering**: Lower QQL queries to typed route objects via `compileQuery`
- **Native parsing**: Rust-speed QQL parsing in Node.js returning `Stmt` objects
- **Filter injection**: Programmatically add tenant isolation filters
- **Validation**: Check if a query string is valid QQL
- **Smart batching**: Auto-batches contiguous same-collection query/mutation statements

## Requirements & Supported Platforms

- **Node.js**: `>=18`
- **Supported Platforms**:
  - Linux x64 (`glibc`) — `@veristamp/nqql-linux-x64-gnu`
  - macOS x64 (`x86_64`) & arm64 (`Apple Silicon`) — `@veristamp/nqql-darwin-x64`, `@veristamp/nqql-darwin-arm64`
  - Windows x64 (`msvc`) — `@veristamp/nqql-win32-x64-msvc`
- *Note: Linux musl (Alpine) is not currently supported.*

## Installation

```bash
npm install @veristamp/nqql
```

## Quick Start

```javascript
const {
  Client, HttpEmbedder, Stmt,
  parse, parseJson,
  isValid, injectFilter, tokenize,
  compileQuery, explain, explainStmt,
  execute, executeStmt, version
} = require('@veristamp/nqql');

// 1. Connect to live Qdrant with optional embedding provider
const embedder = new HttpEmbedder({
    endpoint: "http://localhost:11434/v1/embeddings",
    model: "all-minilm:l6-v2",
    dimension: 384,
    apiKey: ""                          // or api_key (snake_case also accepted)
});

const client = new Client({
    url: "http://localhost:6333",
    apiKey: "optional-qdrant-secret",   // or api_key
    useGrpc: false,                     // or use_grpc
    embedder: embedder
});

// Execute QQL query (auto-embeds text to vector)
const result = await client.execute("QUERY 'cardiology' FROM medical_records USING dense LIMIT 5");
console.log(result);

// Explain query execution plan
const plan = client.explain("QUERY 'cardiology' FROM medical_records USING dense LIMIT 5");
console.log(plan);

// 2. Pure AST Parsing & Filter Injection
// parse() always returns an array of Stmt objects
const stmts = parse("QUERY 'full text match' FROM articles LIMIT 10");
// parseJson() returns raw JSON string (bypasses V8 object allocation)
const rawJson = parseJson("QUERY 'full text match' FROM articles LIMIT 10");
const valid = isValid("QUERY 'test' FROM docs");

// Inject tenant filter into query string (returns AST) or Stmt object
const securedAst = injectFilter("QUERY 'search' FROM docs LIMIT 10", "org_id", "=", "acme-corp");
stmts[0].injectFilter("tenant_id", "=", "acme-corp");

// 3. Lower to route without executing
const route = compileQuery("QUERY 'search' FROM docs LIMIT 10");
console.log("Compiled route:", route);  // { stmt_type, method, path, payload }

// 4. Free-function convenience execute
const result2 = await execute("SHOW COLLECTIONS", { url: "http://localhost:6333" });
```

## Execution Results & Errors

### ExecutionReport Format

All execution methods (`client.execute`, `execute`, `executeStmt`) return an `ExecutionReport` object:

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

### Structured Errors

Native errors thrown by `nqql` include structured diagnostic fields:

```javascript
try {
  parse("INVALID SYNTAX");
} catch (err) {
  console.log(err.code); // "QQL-PARSE-STATEMENT"
  console.log(err.kind); // "Parse"
  console.log(err.span); // { start: 0, end: 7 }
}
```

## Filter Injection Operators

`injectFilter` accepts comparison operators:

- **Accepted**: `=`, `==`, `eq`, `>`, `gt`, `>=`, `gte`, `<`, `lt`, `<=`, `lte`
- **Rejected**: `!=`, `neq`, `<>`, `in`, `is_null`, `contains` (throws error — wrap with `NOT` or write in QQL query)

## API Summary

| Export | Description |
|---|---|
| `Client(options)` | Class for executing QQL against a live Qdrant database |
| `HttpEmbedder(options)` | First-class HTTP embedding provider configuration |
| `Stmt` | Parsed statement object (`injectFilter`, `toObject`, `toJson` / `toJSON`, `shardKey`) |
| `parse(input)` | Parse into array of `Stmt` objects |
| `parseJson(input)` | Parse to raw JSON string (bypasses V8 object allocation) |
| `isValid(input)` | Validate QQL syntax |
| `tokenize(input)` | Tokenize QQL input string |
| `injectFilter(query, field, op, value)` | Inject tenant filter into statement AST |
| `compileQuery(input)` | Lower QQL statement into `{ stmt_type, method, path, payload }` route object |
| `explain(query)` | Inspect the execution plan without executing network calls |
| `explainStmt(stmt)` | Explain a pre-parsed Stmt object |
| `execute(query, options?)` | Execute string or Stmt list; `options.onError` is `"stop"` or `"continue"` |
| `executeStmt(stmt, options?)` | Free-function execute a pre-parsed Stmt |
| `version` | Package runtime version string |

## Documentation Links

- [QQL Syntax Guide](https://github.com/srimon12/qql-rs/blob/main/docs/syntax.md)
- [Filter Documentation](https://github.com/srimon12/qql-rs/blob/main/docs/filters.md)
- [Filter Injection Guide](https://github.com/srimon12/qql-rs/blob/main/docs/inject_filter.md)
- [Changelog](https://github.com/srimon12/qql-rs/blob/main/CHANGELOG.md)
