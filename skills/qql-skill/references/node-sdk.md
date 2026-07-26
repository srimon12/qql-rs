# Node.js SDK (`nqql`) Reference & Examples

Native Node.js bindings via N-API (napi-rs).

## Install

```bash
npm install nqql
```

---

## 1. Client Constructor

The `Client` constructor accepts a single options object:

```js
const { Client } = require('nqql');

// Minimal
const client = new Client({ url: "http://localhost:6333" });

// With API key
const client = new Client({ url: "http://localhost:6333", apiKey: "sk-..." });

// With gRPC
const client = new Client({ url: "http://localhost:6334", useGrpc: true });

// With embedder (for text-to-vector resolution in UPSERT/QUERY)
const client = new Client({
    url: "http://localhost:6333",
    embedder: {
        endpoint: "http://localhost:11434/v1/embeddings",
        apiKey: "",
        model: "all-minilm:l6-v2",
        dimension: 384,
    },
});
```

---

## 2. Multi-Tenant Filter Injection

Parse user query, inject tenant isolation, execute.

```js
const { parse, injectFilter, Client } = require('nqql');

const client = new Client({ url: "http://localhost:6333" });

// User query from UI / API
const [stmt] = parse("QUERY 'supply chain risks' FROM sec10k SHARD 'honeywell' LIMIT 10");

// Platform injects tenant filter -- single call, recursive into CTEs and prefetches
stmt.injectFilter("tenant_id", "=", "honeywell");

// Set the shard key on the statement
stmt.shardKey = "honeywell";

const result = await client.execute(stmt);
```

Note: `injectFilter` does not support `!=`. Use equality and wrap with `NOT`, or rewrite the query.

---

## 3. Unified Execute

`execute()` accepts a string, a Stmt, a multi-statement string, or an array. It returns an `ExecutionReport` object and auto-batches adjacent compatible statements.
Pass `{ onError: "continue" }` to collect per-statement failures; the default is `"stop"`.

```js
const { parse, Client } = require('nqql');

const client = new Client({ url: "http://localhost:6333" });

// Single string
const result = await client.execute("QUERY 'search' FROM docs USING dense LIMIT 10");

// Pre-parsed Stmt -- programmatic manipulation before execution
const [stmt] = parse("QUERY 'search' FROM docs USING dense LIMIT 10");
stmt.shardKey = "acme";
const stmtResult = await client.execute(stmt);

// Multi-statement (semicolons) -- one call for DDL scripts
const schema = await client.execute(`
  CREATE COLLECTION docs HYBRID (dense VECTOR(768, COSINE), sparse SPARSE)
    WITH HNSW (m = 16);

  CREATE INDEX ON COLLECTION docs FOR title TYPE text;
  CREATE SHARD KEY 'acme' ON COLLECTION docs WITH (shards_number = 2);
`);

// Batch -- array of strings
const results = await client.execute([
    "QUERY 'a' FROM docs USING dense LIMIT 10",
    "QUERY 'b' FROM docs USING dense LIMIT 10",
]);
// -> 2 queries, 1 network call, one ExecutionReport object
```

---

## 4. Complex Retrieval

Multi-stage hybrid retrieval with CTE, Fusion, and Rerank.

```js
const { Client } = require('nqql');

const client = new Client({ url: "http://localhost:6333" });

const result = await client.execute(`
  WITH
    dense  AS (QUERY TEXT 'vector databases' USING dense  LIMIT 100),
    sparse AS (QUERY TEXT 'vector databases' USING sparse LIMIT 100),
    fused  AS (
      QUERY FUSION RRF FROM docs
        PREFETCH (dense WHERE priority = 'high', sparse)
        LIMIT 50
    )
  QUERY RERANK TEXT 'vector databases' MODEL 'bge-reranker'
    FROM docs
    USING colbert
    PREFETCH (fused)
    LIMIT 10
`);
```

---

## 5. Stmt Class

```js
const { parse } = require('nqql');

const [stmt] = parse("QUERY 'search' FROM docs USING dense LIMIT 10");

// Read / write shard key
stmt.shardKey = "acme";
console.log(stmt.shardKey);  // -> "acme"

// Inject filter
stmt.injectFilter("tenant_id", "=", "acme");

// Serialise
console.log(stmt.toJSON());
console.log(stmt.toObject());
```

---

## 6. Free Functions

```js
const { parse, parseJson, isValid, injectFilter, tokenize, compileQuery } = require('nqql');

parse("QUERY 'x' FROM docs LIMIT 5");                    // Always Stmt[]
parse("Q1; Q2;");                                        // Script -> Stmt[]
parseJson("QUERY 'x' FROM docs LIMIT 5");                // Raw JSON string (2× faster, no V8 objects)
isValid("QUERY 'x' FROM docs LIMIT 5");                  // Validate
injectFilter("QUERY 'x' FROM docs", "tenant_id", "=", "acme");
tokenize("QUERY 'x'");
compileQuery("QUERY 'x' FROM docs LIMIT 5");
```

`parseJson()` returns the raw JSON string directly from Rust, bypassing V8 object
allocation entirely. It is **1.85–2.15× faster** than `parse()`. Prefer it for
HTTP/IPC forwarding or any path that serialises to JSON anyway.

---

## 7. Free-Standing Execute

A top-level `execute()` function creates a temporary client per call:

```js
const { execute } = require('nqql');

// Single query
const result = await execute("QUERY 'search' FROM docs USING dense LIMIT 10");

// With options
const result = await execute("QUERY 'search' FROM docs USING dense LIMIT 10", {
    url: "http://localhost:6333",
    apiKey: "sk-...",
});
```
