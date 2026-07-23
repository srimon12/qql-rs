# nqql

Node.js native bindings for the QQL parser, plan compiler, and execution engine, compiled using N-API (`napi-rs`).

## Features

- **Live Qdrant Execution**: Connect to live Qdrant instances over REST (default) or gRPC
- **First-Class Embedding Inference**: Integrate custom HTTP embedder models (Ollama, OpenAI, vLLM, TEI)
- **Zero-Copy Route Lowering**: Lower QQL queries to typed route objects via `compileQuery`
- **Native parsing**: Rust-speed QQL parsing in Node.js returning `Stmt` objects
- **Filter injection**: Programmatically add tenant isolation filters
- **Validation**: Check if a query string is valid QQL
- **Smart batching**: Auto-batches contiguous same-collection query/mutation statements

## Installation

```bash
npm install nqql
```

## Quick Start

```javascript
const {
  Client, HttpEmbedder, Stmt,
  parse, parseAll, parseBatch,
  parseFastJson, parseBatchFastJson,
  isValid, injectFilter, tokenize,
  compileQuery, explain, explainStmt,
  execute, executeStmt
} = require('nqql');

// 1. Connect to live Qdrant with optional embedding provider
const embedder = new HttpEmbedder({
    endpoint: "http://localhost:11434/v1/embeddings",
    model: "all-minilm:l6-v2",
    dimension: 384,
    apiKey: ""                          // or api_key (snake-case also accepted)
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
const stmt = parse("QUERY 'full text match' FROM articles LIMIT 10");
const valid = isValid("QUERY 'test' FROM docs");
const secured = injectFilter("QUERY 'search' FROM docs LIMIT 10", "org_id", "=", "acme-corp");

// 3. Lower to route without executing
const route = compileQuery("QUERY 'search' FROM docs LIMIT 10");
console.log("Compiled route:", route);  // { stmt_type, payload }

// 4. Fast JSON parse (skips N-API object allocation)
const stmtJson = parseFastJson("QUERY 'hello' FROM docs LIMIT 10");

// 5. Free-function convenience execute
const result2 = await execute("SHOW COLLECTIONS", { url: "http://localhost:6333" });
```

## API Summary

| Export | Description |
|---|---|
| **Classes** | |
| `Client(options)` | Class for executing QQL against a live Qdrant database |
| `HttpEmbedder(options)` | First-class HTTP embedding provider configuration |
| `Stmt` | Parsed statement object (`injectFilter`, `toObject`, `toJSON`, `shardKey` property) |
| **Parsing** | |
| `parse(input)` | Parse single statement to AST `Stmt` object |
| `parseAll(input)` | Parse semicolon-delimited script to array of `Stmt` objects |
| `parseBatch(queries)` | Batch-parse array of query strings |
| `parseFastJson(query)` | Parse single statement, returns JSON string (avoids N-API object alloc) |
| `parseBatchFastJson(queries)` | Batch-parse, returns JSON string |
| `isValid(input)` | Validate QQL syntax |
| `tokenize(input)` | Tokenize QQL input string |
| **Filter / Route** | |
| `injectFilter(query, field, op, value)` | Inject tenant filter into statement AST |
| `compileQuery(input)` | Lower QQL statement into `{ stmt_type, method, path, payload }` route object |
| **Explain** | |
| `explain(query)` | Inspect the execution plan without executing network calls |
| `explainStmt(stmt)` | Explain a pre-parsed Stmt object |
| **Execute** | |
| `execute(query, options?)` | Free-function async execute (returns JSON string) |
| `executeStmt(stmt, options?)` | Free-function execute a pre-parsed Stmt |
| `Client.explain(query)` | Explain query via Client |
| `Client.explainStmt(stmt)` | Explain Stmt via Client |
| `Client.compile(query)` | Compile query to route via Client |
| `Client.execute(query)` | Execute string, Stmt, or array (auto-batched) — returns JSON string |

### Client options

| Option | Default | Description |
|---|---|---|
| `url` | `http://localhost:6333` | Qdrant REST URL |
| `apiKey` / `api_key` | — | Qdrant API key |
| `useGrpc` / `use_grpc` | `false` | Use gRPC transport |
| `embedder` | — | `HttpEmbedder` instance or inline dict |

## Stmt class

```javascript
const stmt = new Stmt("QUERY 'search' FROM docs LIMIT 10");
stmt.injectFilter("tenant_id", "=", "acme-corp");
stmt.shardKey = "shard-01";             // setter (QUERY/COUNT/SCROLL/UPSERT/DELETE only)
const obj = stmt.toObject();            // JS object
const json = stmt.toJSON();             // JSON string
```
