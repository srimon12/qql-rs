# nqql

Node.js native bindings for the QQL parser and execution engine, compiled using N-API (`napi-rs`).

## Features

- **Live Qdrant Execution**: Connect to live Qdrant instances over REST (default) or gRPC
- **First-Class Embedding Inference**: Integrate custom HTTP embedder models (Ollama, OpenAI, vLLM, TEI)
- **Fast JSON Parsing**: High-throughput JSON string and object deserialization
- **Native parsing**: Rust-speed QQL parsing in Node.js
- **Filter injection**: Add tenant isolation filters to parsed queries
- **Validation**: Check if a query string is valid QQL

## Installation

```bash
npm install nqql
```

## Quick Start

```javascript
const { Client, HttpEmbedder, parse, isValid, injectFilter } = require('nqql');

// 1. Connect to live Qdrant with optional custom embedding provider
const embedder = new HttpEmbedder({
    endpoint: "http://localhost:11434/v1/embeddings",
    model: "nomic-embed-text",
    dimension: 768,
    apiKey: "optional-key"
});

const client = new Client({
    url: "http://localhost:6333",
    apiKey: "optional-qdrant-secret",
    useGrpc: false,
    embedder: embedder
});

// Execute QQL query
const result = client.execute("QUERY 'cardiology' FROM medical_records LIMIT 5");
console.log(result);

// Explain query execution plan
const plan = client.explain("QUERY 'test' FROM docs LIMIT 5");
console.log(plan);

// 2. Pure AST Parsing & Filter Injection
const ast = parse("QUERY 'full text match' FROM articles LIMIT 10");
const valid = isValid("SELECT * FROM docs WHERE id = 1");
const secured = injectFilter("QUERY 'search' FROM docs LIMIT 10", "org_id", "=", '"acme-corp"');
```

## API Summary

| Export | Description |
|---|---|
| `Client(options)` | Class for executing QQL against a live Qdrant database |
| `HttpEmbedder(options)` | First-class HTTP embedding provider configuration |
| `execute(query, options)` | One-off helper function to execute a QQL statement |
| `explain(query)` | Inspect the execution plan without executing network calls |
| `parse(input)` | Parse single statement to AST object |
| `parseFastJson(input)` | Fast JSON-path string parse for V8 performance |
| `isValid(input)` | Validate QQL syntax |
| `injectFilter(query, field, op, valueJson)` | Inject tenant filter into statement AST |
