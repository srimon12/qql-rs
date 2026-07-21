# WebAssembly SDK (`qql-wasm`) Reference & Examples

WebAssembly bindings for QQL compiled with `wasm-bindgen` for browser and edge environments (Cloudflare Workers, Vercel Edge, Deno, Bun).

## Installation

```bash
npm install qql-wasm
```

## Quick Start in Browser / Edge

```javascript
import init, { Client, parse, parse_all, isValid, inject_filter, compile, explain } from 'qql-wasm';

async function run() {
    await init();

    // 1. Initialize Client
    const client = new Client("http://localhost:6333", null);
    
    // Optional: Configure OpenAI/Ollama embedder or Transformers.js embedder function
    client.setOpenAIEmbedder("api-key", "text-embedding-3-small", 1536);
    // Or custom browser transformer embedder:
    // client.setEmbedder(async (texts) => myLocalTransformer(texts));

    // 2. Execute Query via browser fetch
    const response = await client.execute("QUERY 'machine learning' FROM docs USING dense LIMIT 5");
    console.log(response);

    // 3. Offline Route Compilation
    const routeJson = compile("QUERY 'search' FROM docs USING dense LIMIT 10");
    console.log("Compiled route:", routeJson);

    // 4. AST Parsing & Safe Filter Injection
    const ast = parse("QUERY 'search' FROM docs LIMIT 10");
    const valid = isValid("QUERY 'search' FROM docs");
    const safeAst = inject_filter("QUERY 'docs' FROM items LIMIT 10", "tenant_id", "=", "acme");
}

run();
```
