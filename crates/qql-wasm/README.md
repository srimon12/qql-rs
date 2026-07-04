# qql-wasm

WebAssembly bindings for the QQL parser, compiled with `wasm-bindgen`.

## Features

- **Browser-native**: Parse and manipulate QQL in the browser
- **Edge-native**: Run on Cloudflare Workers, Vercel Edge, Deno, or any WASM runtime
- **Zero I/O**: Pure parsing — no network calls, no system dependencies
- **Minimal size**: Optimized WASM build with no bloat

## Installation

```bash
npm install qql-wasm
```

## Usage

```javascript
import init, { parse, parseAll, isValid, injectFilter, tokenize } from 'qql-wasm';

async function run() {
    await init();

    // Parse single statement
    const ast = parse("QUERY 'edge compute' FROM servers LIMIT 5");
    console.log(ast);

    // Parse multiple statements
    const stmts = parseAll("INSERT INTO docs ...; QUERY 'text' FROM docs ...");

    // Validate
    const valid = isValid("SELECT * FROM docs WHERE id = 1");

    // Inject filter
    const secured = injectFilter(
        "QUERY 'search' FROM docs LIMIT 10",
        "org_id", "=", "\"acme-corp\""
    );

    // Tokenize
    const tokens = tokenize("QUERY 'hello' FROM docs LIMIT 5");
}

run();
```

## API

| Function | Returns | Description |
|---|---|---|
| `parse(input)` | `string` | Parse single statement → debug AST |
| `parseAll(input)` | `string` | Parse multiple semicolon-separated statements |
| `isValid(input)` | `boolean` | Check if query string is valid QQL |
| `injectFilter(query, field, op, value)` | `string` | Inject filter into query AST |
| `tokenize(input)` | `string` | Tokenize query string (JSON) |
