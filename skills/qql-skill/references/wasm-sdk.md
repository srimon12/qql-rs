# WebAssembly SDK (`qql-wasm`) Reference & Examples

WASM bindings for browser and edge (Cloudflare Workers, Vercel Edge, Deno, Bun).

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
    const embeddings = await myModel.embed(texts);
    return embeddings;  // number[][]
});
```

Check whether an embedder is configured: `client.hasEmbedder()`

---

## 3. Execute

`execute()` accepts a single string, a semicolon-delimited multi-statement string, or an array of strings. Contiguous same-collection QUERYs use `/points/query/batch`; contiguous mutations (UPSERT/DELETE/UPDATE/CLEAR/DELETE VECTOR) use `/points/batch`.

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

// Read / write the shard key
stmt.shardKey = "acme";
console.log(stmt.shardKey);  // -> "acme"

// Inject a tenant filter (mutates in place)
stmt.injectFilter("tenant_id", "=", "acme");

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
import init, { compile, parse_all } from 'qql-wasm';
await init();

const route = compile("QUERY 'search' FROM docs USING dense LIMIT 10");
// -> JSON string with stmt_type and payload

for (const stmt of parse_all(`
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
import init, { parse, parse_all, parse_batch, isValid, inject_filter,
              tokenize, compile, explain } from 'qql-wasm';
await init();

parse("QUERY 'x' FROM docs LIMIT 5");                  // Parse to JS object
parse_all("Q1; Q2;");                                    // Parse multi-statement
parse_batch(["Q1", "Q2"]);                               // Parse batch
isValid("QUERY 'x' FROM docs LIMIT 5");                  // Validate
inject_filter("QUERY 'x'", "tenant_id", "=", "acme");   // Inject filter (string -> object)
tokenize("QUERY 'x'");                                   // Lex to tokens array
compile("QUERY 'x' FROM docs LIMIT 5");                  // Compile to route JSON string
explain("QUERY 'x' FROM docs LIMIT 5");                  // Explain plan string
```
