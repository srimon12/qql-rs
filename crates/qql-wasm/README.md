# qql-wasm

WebAssembly bindings for the QQL parser, plan compiler, and browser execution engine, compiled with `wasm-bindgen`.

## Features

- **Browser-native**: Parse and compile QQL directly in browser applications
- **Edge-native**: Run on Cloudflare Workers, Vercel Edge, Deno, or Bun
- **Zero-copy routing**: Lower QQL statements directly to typed Qdrant REST routes via `qql-plan`
- **Offline analysis**: Unity API `analyze()` returns tokens, AST, route, explain string, and errors in one call
- **Small footprint**: `--no-default-features` builds exclude the HTTP client, producing a parser/compiler-only binary

## Installation

```bash
npm install qql-wasm
```

## Quick Start

```javascript
import init, { Client, Stmt, parse, parse_all, isValid, inject_filter, compile, explain, analyze } from 'qql-wasm';

async function run() {
    await init();

    // 1. Offline analysis (tokens + AST + route + explain in one call)
    const info = analyze("QUERY 'machine learning' FROM docs LIMIT 10");
    console.log(info.valid, info.statements_count, info.route, info.explain);

    // 2. Client configuration & Browser Execution
    //    `client` feature required — default build includes it.
    const client = new Client("http://localhost:6333", null);

    // Optional: OpenAI-compatible HTTP embedder
    // endpoint required — no default URL
    client.setHttpEmbedder(
        "http://localhost:11434/v1/embeddings",
        "nomic-embed-text",
        768,
        null, // api_key optional
    );
    // Or browser-side: client.setEmbedder(async (texts) => vectors);
    console.log("Has embedder:", client.hasEmbedder());

    // Execute QQL statement via browser fetch
    const response = await client.execute("QUERY 'machine learning' FROM docs LIMIT 5");
    console.log(response);

    // Execute pre-parsed Stmt
    const stmt = new Stmt("QUERY 'search' FROM docs LIMIT 10");
    const stmtResponse = await client.executeStmt(stmt);

    // 3. Compile / Explain (offline)
    const routeJson = compile("QUERY 'search' FROM docs LIMIT 10");
    console.log("Compiled route:", routeJson);

    const plan = explain("QUERY 'search' FROM docs LIMIT 10");
    console.log("Execution plan:", plan);

    const clientPlan = client.explain("QUERY 'search' FROM docs LIMIT 10");
    const clientRoute = client.compile("QUERY 'search' FROM docs LIMIT 10");

    // 4. Stmt class (programmatic manipulation)
    const myStmt = new Stmt("QUERY 'docs' FROM items LIMIT 10");
    myStmt.injectFilter("tenant_id", "=", "acme");
    myStmt.shardKey = "shard-01";
    console.log(myStmt.toJSON(), myStmt.toObject());

    // 5. Pure AST Parsing & Validation
    const ast = parse("QUERY 'search' FROM docs LIMIT 10");
    const valid = isValid("QUERY 'search' FROM docs");
    const safeAst = inject_filter("QUERY 'docs' FROM items LIMIT 10", "tenant_id", "=", "acme");
    const batchAsts = parse_all("QUERY 'a' FROM c LIMIT 1; QUERY 'b' FROM c LIMIT 1");
    const tokenList = tokenize("QUERY 'search' FROM docs");
}

run();
```

## API Summary

### Free functions (always available)

| Export | Description |
|---|---|
| `parse(input)` | Parse single statement to AST object |
| `parse_all(input)` | Parse semicolon-delimited script to array of AST objects |
| `parse_batch(queries)` | Batch-parse array of query strings |
| `isValid(input)` | Validate QQL syntax |
| `inject_filter(query, field, op, value)` | Inject tenant filter into statement AST |
| `tokenize(input)` | Tokenize QQL string — each token has `kind`, `text`, `pos`, `end`, `len` |
| `analyze(input)` | Unity API — returns `{ valid, statements_count, tokens, ast, route, explain, error }` |
| `compile(input)` | Lower QQL statement to typed Qdrant REST route JSON string |
| `explain(input)` | Format query execution plan |

### `Stmt` class

| Method | Description |
|---|---|
| `new Stmt(input)` | Parse a QQL string into a Stmt object |
| `stmt.injectFilter(field, op, value)` | Inject WHERE filter (mutates in place) |
| `stmt.shardKey` | Get/set shard key (QUERY/COUNT/SCROLL/UPSERT/DELETE only) |
| `stmt.toJSON()` | Serialise AST to JSON string |
| `stmt.toObject()` | Serialise AST to JS object |

### `Client` class (`client` feature)

| Method | Description |
|---|---|
| `new Client(url?, api_key?)` | Browser client (default url: `http://localhost:6333`) |
| `client.setEmbedder(fn)` | JS batch embedder: `async (texts: string[]) => number[][]` |
| `client.setHttpEmbedder(endpoint, model, dim, apiKey?)` | OpenAI-compatible HTTP embedder (**endpoint required**) |
| `client.setRemoteEmbedder(endpoint, model, dim, apiKey?)` | Alias for `setHttpEmbedder` |
| `client.hasEmbedder()` | Check whether any embedder is configured |
| `client.execute(query)` | Execute string or string[] (smart-batches same-collection stmts) |
| `client.executeStmt(stmt)` | Execute a pre-parsed Stmt object |
| `client.compile(query)` | Lower QQL to route JSON (same as free `compile`) |
| `client.explain(query)` | Format execution plan (same as free `explain`) |

## Feature Flags

| Feature | Default | Description |
|---------|---------|------------|
| `client` | yes | Browser HTTP client (`gloo-net`) + `setEmbedder`/`setHttpEmbedder`. Disable for parser/compiler-only. |

```bash
# Parser/compiler only (~200KB smaller)
cargo build -p qql-wasm --target wasm32-unknown-unknown --no-default-features
```

## Limitations

- **`async_trait(?Send)`**: The WASM `Client` impl uses `#[async_trait(?Send)]` because `wasm32-unknown-unknown` targets lack the `Send` marker. Building the WASM crate natively for a host target (e.g., `wasm32-wasip1`) will fail with a `Send` mismatch on the `Embedder` trait. Always target `wasm32-unknown-unknown` when building with `wasm-pack`.
- **`.wasm` size**: Release builds with `wasm-opt -O` are typically a few hundred KB. Measure with `ls -lh pkg/*.wasm` after `wasm-pack build --release --target web`.
