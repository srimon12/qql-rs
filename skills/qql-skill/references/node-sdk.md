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
const stmt = parse("QUERY 'supply chain risks' FROM sec10k SHARD 'honeywell' LIMIT 10");

// Platform injects tenant filter -- single call, recursive into CTEs and prefetches
injectFilter(stmt, "tenant_id", "=", "honeywell");

// Set the shard key on the statement
stmt.shardKey = "honeywell";

const result = await client.execute(stmt);
```

Note: `injectFilter` does not support `!=`. Use equality and wrap with `NOT`, or rewrite the query.

---

## 3. Unified Execute

`execute()` accepts a string, a Stmt, a multi-statement string (semicolons), or an array. All paths auto-batch same-collection QUERY statements. Returns a JSON string.

```js
const { parse, Client } = require('nqql');

const client = new Client({ url: "http://localhost:6333" });

// Single string
const result = await client.execute("QUERY 'search' FROM docs USING dense LIMIT 10");

// Pre-parsed Stmt -- programmatic manipulation before execution
const stmt = parse("QUERY 'search' FROM docs USING dense LIMIT 10");
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
// -> 2 queries, 1 network call. JSON.parse(results) for array
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
const { Stmt } = require('nqql');

const stmt = new Stmt("QUERY 'search' FROM docs USING dense LIMIT 10");

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
const { parse, parse_all, parse_batch, parse_json, parse_batch_json,
        is_valid, inject_filter, tokenize, compile_query } = require('nqql');

parse("QUERY 'x' FROM docs LIMIT 5");             // Parse single statement
parse_all("Q1; Q2;");                              // Parse multi-statement
parse_batch(["Q1", "Q2"]);                         // Parse batch
parse_json("Q1");                                  // Parse to JSON string
parse_batch_json(["Q1", "Q2"]);                    // Parse batch to JSON
is_valid("QUERY 'x' FROM docs LIMIT 5");           // Validate
inject_filter("QUERY 'x' FROM docs", "tenant_id", "=", "acme");  // Inject filter on string
tokenize("QUERY 'x'");                             // Lex to tokens
compile_query("QUERY 'x' FROM docs LIMIT 5");      // Compile to route
```

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
