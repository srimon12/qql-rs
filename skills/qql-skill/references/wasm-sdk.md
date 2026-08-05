# WebAssembly SDK (`qql-wasm`) Reference & Examples

WASM bindings for browser and edge (Cloudflare Workers, Vercel Edge, Deno, Bun).

Language surface includes **Qdrant 1.19 / QQL 1.4** features expressible in QQL
(`SHOW QUOTAS`, memory/`turbo4`, `MATCH PREFIX`, `SLICE`, `PARAMS (idf = …)`).
Parse/compile/execute those strings against a backend that supports them
(quotas: **REST only**).

**Not exposed in qql-wasm yet:** client-side **route affinity**
(`X-Qdrant-Route-Affinity`). That is Rust transport-only
(`RestQdrant` / `GrpcQdrant` `.with_route_affinity`) — do not invent a WASM
`Client` constructor parameter for it.

## Install

```bash
npm install qql-wasm
```

## Wasm Initialization

All functions require calling `init()` first. The WASM binary must be served alongside your application.

```js
import init from 'qql-wasm';
await init();
```

---

## 1. Client Constructor

The `Client` constructor takes separate `url` and `api_key` arguments (not an options object):

```js
import init, { Client } from 'qql-wasm';
await init();

// Minimal -- defaults to http://localhost:6333
const client = new Client();

// With URL
const client = new Client("http://localhost:6333");

// With URL and API key
const client = new Client("https://qdrant.example.com:6333", "sk-...");
```

---

## 2. Embedder Configuration

The WASM client supports two embedder modes. Embedders must be configured before executing any query that needs text-to-vector resolution.

### HTTP Embedder (OpenAI-compatible)

Works with any provider that accepts `{"model", "input": [...]}` and returns `{"data":[{"embedding":[...],"index":0},...]}`:

```js
// OpenAI
client.setHttpEmbedder(
    "https://api.openai.com/v1/embeddings",
    "text-embedding-3-small",
    1536,
    "sk-..."  // Optional API key for the embedding endpoint
);

// Ollama local
client.setHttpEmbedder(
    "http://localhost:11434/v1/embeddings",
    "all-minilm:l6-v2",
    384
);
```

Endpoint is required -- no default URL. Always sends the full text batch in one request.

### JS Function Embedder

For Transformers.js, custom providers, or in-browser models:

```js
client.setEmbedder(async (texts) => {
    // Called with the full batch -- batch inside the callback
    // Return number[][] of single dense vectors (one row per text).
    const embeddings = await myModel.embed(texts);
    return embeddings;  // number[][]
});
```

Check whether an embedder is configured: `client.hasEmbedder()`

**Prepare order:** on `execute`, WASM fetches collection topology (dense /
sparse / multivector names), fills `USING` kinds, then embeds. So
`USING sparse` and multivector `USING colbert` work without `AS` when Qdrant
is reachable. ColBERT multi-vector TEXT embedding requires a host that
implements multi-vector embed (default HTTP embedder is single dense only);
pass precomputed `VECTOR [[...], ...]` or use `AS MULTI` with a multi-capable
embedder when available.

---

## 3. Execute

`execute()` accepts a string, a semicolon-delimited script, or an array of
strings. Pass `{ onError: "continue" }` to collect per-statement failures; the
default is `"stop"`. Adjacent compatible operations use Qdrant batch endpoints.
Every execution path returns an `ExecutionReport` object with `ok`, `results`,
`succeeded`, and `failed` fields.

```js
// Single query
const result = await client.execute(
    "QUERY 'vector databases' FROM docs USING dense LIMIT 10"
);

// Multi-statement (semicolons auto-detected)
const schemaResult = await client.execute(`
    CREATE COLLECTION docs HYBRID (dense VECTOR(768, COSINE), sparse SPARSE)
      WITH HNSW (m = 16);

    CREATE INDEX ON COLLECTION docs FOR title TYPE text;
`);

// Batch -- array of strings
const results = await client.execute([
    "QUERY 'a' FROM docs USING dense LIMIT 10",
    "QUERY 'b' FROM docs USING dense LIMIT 10",
]);

// Execute a pre-parsed Stmt (skips the parse step)
const stmt = new Stmt("QUERY 'search' FROM docs USING dense LIMIT 10");
stmt.shardKey = "acme";
const stmtResult = await client.executeStmt(stmt);
```

---

## 4. Stmt Class -- Parse Once, Reuse

The `Stmt` class wraps a parsed AST. Manipulate it before execution.

```js
import init, { Stmt } from 'qql-wasm';
await init();

// Parse into a Stmt object
const stmt = new Stmt("QUERY 'search' FROM docs USING dense LIMIT 10");

// Isolation (always on untrusted QQL)
stmt.injectFilter("tenant_id", "=", "acme");

// Routing: prefer SHARD 'acme' in the QQL string; or set after parse:
stmt.shardKey = "acme";  // same field as SHARD — no injectShardKey API
console.log(stmt.shardKey);  // -> "acme"

// Serialise to JSON
const json = stmt.toJSON();
const obj = stmt.toObject();
```

---

## 5. Client-Side Validation & Filter Injection

Validate and inject filters in the browser -- no server round-trip needed.

```js
import init, { parse, isValid, inject_filter } from 'qql-wasm';
await init();

// Validate user input instantly
if (!isValid("QUERY 'machine learning' FROM papers LIMIT 20")) {
    throw new Error("Invalid QQL");
}

// Inject tenant filter into a raw query string
const safe = inject_filter("QUERY 'search' FROM docs LIMIT 10", "tenant_id", "=", "acme");
```

Note: `inject_filter` does not support `!=`. Use equality and wrap with `NOT`, or rewrite the query.

---

## 6. Offline Route Compilation

Lower QQL to a typed REST route object without a Qdrant connection.

```js
import init, { compile, parse } from 'qql-wasm';
await init();

const route = compile("QUERY 'search' FROM docs USING dense LIMIT 10");
// -> { stmt_type, method, path, payload }

for (const stmt of parse(`
  CREATE COLLECTION docs HYBRID (dense VECTOR(768, COSINE), sparse SPARSE)
    WITH PARAMS (replication_factor = 3);
  CREATE SHARD KEY 'acme' ON COLLECTION docs WITH (shards_number = 2);
`)) {
    console.log(stmt);
}
```

---

## 7. Analysis

The `analyze()` function returns a comprehensive result with tokens, AST, route, and explanation in one call:

```js
import init, { analyze } from 'qql-wasm';
await init();

const result = analyze("QUERY 'search' FROM docs USING dense LIMIT 10");
// { valid: true, tokens: [...], ast: ..., route: ..., explain: "...", error: null }
```

---

## 8. Free Functions

```js
import init, { parse, isValid, inject_filter,
              tokenize, compile, explain } from 'qql-wasm';
await init();

parse("QUERY 'x' FROM docs LIMIT 5");                  // Always returns an array
parse("QUERY 'x' FROM docs; COUNT FROM docs");           // Parse multi-statement
isValid("QUERY 'x' FROM docs LIMIT 5");                  // Validate
inject_filter("QUERY 'x'", "tenant_id", "=", "acme");   // Inject filter (string -> object)
tokenize("QUERY 'x'");                                   // Lex to tokens array
compile("QUERY 'x' FROM docs LIMIT 5");                  // Compile to a route object
explain("QUERY 'x' FROM docs LIMIT 5");                  // Explain plan string
```
