# Node.js SDK (`nqql`) Reference & Examples

Native Node.js bindings via N-API (napi-rs).

## Install

```bash
npm install nqql
```

---

## 1. Multi-Tenant Filter Injection

Parse user query, inject tenant isolation, execute.

```js
const { parse, injectFilter, Client } = require('nqql');

const client = new Client({ url: "http://localhost:6333" });

// User query from UI / API
const stmt = parse("QUERY 'supply chain risks' FROM sec10k SHARD 'honeywell' LIMIT 10");

// Platform injects tenant filter — single call, recursive into CTEs and prefetches
injectFilter(stmt, "tenant_id", "=", "honeywell");

// Set the shard key programmatically
stmt.shardKey = "honeywell";

const result = await client.executeStmt(stmt);
```

---

## 2. Unified Execute

`execute()` accepts a string, a Stmt, a multi-statement string (semicolons),
or an array.  All paths auto-batch same-collection QUERY statements.
Returns a JSON string.

```js
const { parse, Client } = require('nqql');

const client = new Client({ url: "http://localhost:6333" });

// Single string
const result = await client.execute("QUERY 'search' FROM docs USING dense LIMIT 10");

// Pre-parsed Stmt — programmatic manipulation before execution
const stmt = parse("QUERY 'search' FROM docs USING dense LIMIT 10");
stmt.shardKey = "acme";
const stmtResult = await client.execute(stmt);

// Multi-statement (semicolons) — one call for DDL scripts
const schema = await client.execute(`
  CREATE COLLECTION docs HYBRID (dense VECTOR(768, COSINE), sparse SPARSE)
    WITH HNSW (m = 16);

  CREATE INDEX ON COLLECTION docs FOR title TYPE text;
  CREATE SHARD KEY 'acme' ON COLLECTION docs WITH (shards_number = 2);
`);

// Batch — array of strings
const results = await client.execute([
    "QUERY 'a' FROM docs USING dense LIMIT 10",
    "QUERY 'b' FROM docs USING dense LIMIT 10",
]);
// → 2 queries, 1 network call.  JSON.parse(results) for array
```

---

## 3. Complex Retrieval

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
