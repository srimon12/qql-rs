# qql-wasm

WebAssembly bindings for the QQL parser, plan compiler, and browser execution engine, compiled with `wasm-bindgen`.

## Features

- **Browser-native**: Parse and compile QQL directly in browser applications
- **Edge-native**: Run on Cloudflare Workers, Vercel Edge, Deno, or Bun
- **Zero-copy routing**: Lower QQL statements directly to typed Qdrant REST routes via `qql-plan`
- **Minimal footprint**: Highly optimized WASM binary (~150KB)

## Installation

```bash
npm install qql-wasm
```

## Quick Start

```javascript
import init, { Client, parse, parse_all, isValid, inject_filter, compile, explain } from 'qql-wasm';

async function run() {
    await init();

    // 1. Client configuration & Browser Execution
    const client = new Client("http://localhost:6333", null);
    
    // Optional: Configure OpenAI/Ollama HTTP embedder or custom JS embedder (e.g. Transformers.js)
    client.setOpenAIEmbedder("api-key", "text-embedding-3-small", 1536);

    // Execute QQL statement via browser fetch
    const response = await client.execute("QUERY 'machine learning' FROM docs LIMIT 5");
    console.log(response);

    // 2. Offline Plan Compilation & Explanation
    const routeJson = compile("QUERY 'search' FROM docs LIMIT 10");
    console.log("Compiled route:", routeJson);

    const plan = explain("QUERY 'search' FROM docs LIMIT 10");
    console.log("Execution plan:", plan);

    // 3. Pure AST Parsing & Safe Filter Injection
    const ast = parse("QUERY 'search' FROM docs LIMIT 10");
    const valid = isValid("QUERY 'search' FROM docs");
    const safeAst = inject_filter("QUERY 'docs' FROM items LIMIT 10", "tenant_id", "=", "acme");
}

run();
```

## API Summary

| Export | Description |
|---|---|
| `Client(url, api_key)` | Browser client for compiling and executing QQL queries via fetch |
| `client.setEmbedder(fn)` | Set custom JS embedder function (`async (texts) => vectors`) |
| `client.setOpenAIEmbedder(...)` | Configure OpenAI / Ollama compatible HTTP embedder |
| `client.execute(query)` | Execute QQL query directly against Qdrant from the browser |
| `compile(input)` | Lower QQL statement to typed Qdrant REST route object |
| `explain(input)` | Format query execution plan |
| `parse(input)` | Parse single statement to AST object |
| `parse_all(input)` | Parse semicolon-delimited script to array of AST objects |
| `parse_batch(queries)` | Batch-parse array of query strings |
| `isValid(input)` | Validate QQL syntax |
| `inject_filter(query, field, op, value)` | Programmatically inject tenant filter into statement AST |
