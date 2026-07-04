# qql-wasm

WebAssembly (WASM) bindings for the QQL parser, compiled using `wasm-bindgen`.

---

## Features
* **Browser-Native**: Parse and manipulate QQL statement trees directly on the client side.
* **Edge-Native**: Deploy query validation and parsing to Cloudflare Workers, Vercel Edge, or other V8/WASM runtimes.
* **No Dependency Bloat**: A highly optimized, minimal WASM build with zero network overhead.

---

## Installation

Add the wasm module to your frontend or Node.js environment:
```bash
npm install qql-wasm
```

---

## Usage

```javascript
import init, { parse, inject_filter } from 'qql-wasm';

async fn run() {
    await init();
    
    // Parse QQL query
    const ast = parse("QUERY 'edge compute' FROM servers LIMIT 5");
    console.log(ast);
}
```
