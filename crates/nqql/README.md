# nqql

Node.js native bindings for the QQL parser, plan compiler, and execution engine, compiled using N-API (`napi-rs`).

## Features

- **Live Qdrant Execution**: Connect to live Qdrant instances over REST (default) or gRPC
- **First-Class Embedding Inference**: Integrate custom HTTP embedder models (Ollama, OpenAI, vLLM, TEI)
- **Zero-Copy Route Lowering**: Lower QQL queries to Qdrant OpenAPI REST routes via `compileQuery`
- **Native parsing**: Rust-speed QQL parsing in Node.js returning `Stmt` objects
- **Filter injection**: Programmatically add tenant isolation filters
- **Validation**: Check if a query string is valid QQL

## Installation

```bash
npm install nqql
```

## Quick Start

```javascript
const { Client, HttpEmbedder, parse, isValid, injectFilter, compileQuery, explain } = require('nqql');

// 1. Connect to live Qdrant with optional custom embedding provider (e.g. Ollama)
const embedder = new HttpEmbedder({
    endpoint: "http://localhost:11434/v1/embeddings",
    model: "all-minilm:l6-v2",
    dimension: 384,
    apiKey: ""
});

const client = new Client({
    url: "http://localhost:6333",
    apiKey: "optional-qdrant-secret",
    useGrpc: false,
    embedder: embedder
});

// Execute QQL query
const result = client.execute("QUERY 'cardiology' FROM medical_records USING dense LIMIT 5");
console.log(result);

// Lower query statement to typed route object
const route = compileQuery("QUERY 'search' FROM docs LIMIT 10");
console.log("Compiled route:", route);

// Explain query execution plan
const plan = client.explain("QUERY 'test' FROM docs LIMIT 5");
console.log(plan);

// 2. Pure AST Parsing & Filter Injection
const stmt = parse("QUERY 'full text match' FROM articles LIMIT 10");
const valid = isValid("QUERY 'test' FROM docs");
const secured = injectFilter("QUERY 'search' FROM docs LIMIT 10", "org_id", "=", "acme-corp");
```

## API Summary

| Export | Description |
|---|---|
| `Client(options)` | Class for executing QQL against a live Qdrant database |
| `HttpEmbedder(options)` | First-class HTTP embedding provider configuration |
| `parse(input)` | Parse single statement to AST `Stmt` object |
| `parseAll(input)` | Parse semicolon-delimited script to array of `Stmt` objects |
| `parseBatch(queries)` | Batch-parse array of query strings |
| `isValid(input)` | Validate QQL syntax |
| `injectFilter(query, field, op, value)` | Inject tenant filter into statement AST |
| `tokenize(input)` | Tokenize QQL input string |
| `compileQuery(input)` | Lower QQL statement into typed `{ method, path, payload }` route object |
| `explain(query)` | Inspect the execution plan without executing network calls |
