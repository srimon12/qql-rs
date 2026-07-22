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

const result = await client.executeStmt(stmt);
```

---

## 2. Schema-as-Code

Parse and execute a `.qql` schema file — same file works from Node, Python, Rust, WASM.

```js
const { parseAll, Client } = require('nqql');

const schema = `
CREATE COLLECTION docs HYBRID (dense VECTOR(768, COSINE), sparse SPARSE)
  WITH HNSW (m = 16)
  WITH PARAMS (replication_factor = 3, shard_number = 4);

CREATE INDEX ON COLLECTION docs FOR title TYPE text;
CREATE INDEX ON COLLECTION docs FOR tenant_id TYPE keyword WITH (is_tenant = true);
`;

const client = new Client({ url: "http://localhost:6333" });
for (const stmt of parseAll(schema)) {
    await client.executeStmt(stmt);
}
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
