# qql-wasm

WebAssembly bindings for the QQL parser and offline compiler, compiled with `wasm-bindgen`.

## Features

- **Browser-native**: Parse and compile QQL directly in the browser
- **Edge-native**: Run on Cloudflare Workers, Vercel Edge, Deno, or Bun
- **Zero I/O**: Pure client-side compilation to Qdrant REST JSON objects
- **Minimal size**: Highly optimized WASM binary (~150KB)

## Installation

```bash
npm install qql-wasm
```

## Quick Start

```javascript
import init, { Client, HttpEmbedder, parse, isValid, compile } from 'qql-wasm';

async function run() {
    await init();

    // 1. Client & HttpEmbedder configuration
    const embedder = new HttpEmbedder("http://localhost:11434/v1/embeddings", "nomic-embed-text", 768);
    const client = new Client("http://localhost:6333", null, embedder);

    // Compile QQL to Qdrant REST payload object
    const restPayload = client.compile("QUERY 'edge compute' FROM servers LIMIT 5");
    console.log(restPayload);

    // 2. Pure AST Parsing & Filter Injection
    const ast = parse("QUERY 'edge compute' FROM servers LIMIT 5");
    const valid = isValid("SELECT * FROM docs WHERE id = 1");
}

run();
```

## API Summary

| Export | Description |
|---|---|
| `Client(url, api_key, embedder)` | Client for compiling QQL queries for Qdrant |
| `HttpEmbedder(endpoint, model, dimension, api_key)` | First-class HTTP embedding provider configuration |
| `compile(input)` | Compile QQL statement directly to Qdrant REST JSON object |
| `explain(input)` | Explain query execution plan |
| `parse(input)` | Parse single statement to AST object |
| `isValid(input)` | Validate QQL syntax |
