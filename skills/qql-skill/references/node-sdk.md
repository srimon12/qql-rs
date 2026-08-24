# Node.js SDK (`nqql`) Reference & Examples

Native Node.js bindings via N-API (napi-rs).

Language surface includes **Qdrant 1.19 / QQL 1.5** features expressible in QQL
(`SHOW QUOTAS`, memory/`turbo4`, `MATCH PREFIX`, `SLICE`, `PARAMS (idf = …)`).
Pass those strings to `client.execute` when the backend supports them (quotas:
**REST only**).

**Route affinity** (`X-Qdrant-Route-Affinity`) is exposed on `Client` via the
`routeAffinity` constructor option (readable with `client.routeAffinity`). See
§1 below.

## Install

```bash
npm install @veristamp/nqql
```

---

## 1. Client Constructor

The `Client` constructor accepts a single options object:

```js
const { Client } = require('@veristamp/nqql');

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

// With Qdrant 1.19 read affinity (sticky replica reads)
const client = new Client({
    url: "http://localhost:6333",
    routeAffinity: "session-acme-42",
});
console.log(client.routeAffinity); // "session-acme-42"
```

---

## 2. Multi-tenant isolation + optional routing

**Isolation** = `injectFilter` (always). **Routing** = `SHARD '…'` in QQL or `stmt.shardKey`.  
No `injectShardKey` free function.

```js
const { parse, Client } = require('@veristamp/nqql');
const client = new Client({ url: "http://localhost:6333" });

// Preferred: SHARD in QQL
const [stmt] = parse(`
  QUERY TEXT 'supply chain risks' FROM sec10k USING dense
  SHARD 'honeywell' LIMIT 10
`);
stmt.injectFilter("tenant_id", "=", "honeywell");
const result = await client.execute(stmt);

// Host-resolved after parse:
// stmt.shardKey = "honeywell";
```

`injectFilter` does not support `!=` — use equality or rewrite the query.

Sparse IDF is QQL, not an inject and not a JSON corpus. `compileQuery` lowers
`idf = WHERE tenant_id = '…'` to Qdrant `params.idf.corpus` — do not build that
object in JS.

```js
const { parse, bind, compileQuery, injectFilter } = require('@veristamp/nqql');

const bound = bind(`
  QUERY TEXT :q FROM sec10k USING sparse
  WHERE tenant_id = :tenant
  SHARD :tenant
  PARAMS (idf = WHERE tenant_id = :tenant)
  LIMIT 10
`, { q: "supply chain", tenant: "honeywell" });

const [stmt] = parse(bound);
stmt.injectFilter("tenant_id", "=", "honeywell");
const route = compileQuery(bound);
// route.payload.params.idf.corpus.must[0].key === "tenant_id"
```

---

## 3. Unified Execute

`execute()` accepts a string, a Stmt, a multi-statement string, or an array. It returns an `ExecutionReport` object and auto-batches adjacent compatible statements.
Pass `{ onError: "continue" }` to collect per-statement failures; the default is `"stop"`.

```js
const { parse, Client } = require('@veristamp/nqql');

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

`USING dense` / `USING sparse` / `USING colbert` without `AS` are resolved from
the collection schema before embedding. Use `AS DENSE`, `AS SPARSE`, or
`AS MULTI` when you need an explicit role (or offline embed without schema).

```js
const { Client } = require('@veristamp/nqql');

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
  QUERY RERANK TEXT 'vector databases' MODEL 'answerai-colbert-small-v1'
    FROM docs
    USING colbert
    PREFETCH (fused)
    LIMIT 10
`);

// Multivector nearest (schema has colbert WITH MULTIVECTOR):
// await client.execute("QUERY TEXT 'q' FROM docs USING colbert LIMIT 10");
// Offline: "... USING colbert AS MULTI LIMIT 10"
```

---

## 5. Stmt Class

```js
const { parse } = require('@veristamp/nqql');

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

## 6. Parameter Binding & Prepared Queries

```js
const { Client, bind, bindNamed, bindPositional } = require('@veristamp/nqql');
const client = new Client({ url: "http://localhost:6333" });

// Execute with named parameters (:name)
const res1 = await client.execute(
  "QUERY TEXT :query FROM docs WHERE category = :cat AND rating >= :min_rating LIMIT :limit",
  { params: { query: "machine learning", cat: "tech", min_rating: 4.5, limit: 10 } }
);

// Execute with positional parameters (?)
const res2 = await client.execute(
  "QUERY TEXT ? FROM docs WHERE category = ? LIMIT ?",
  { params: ["machine learning", "tech", 10] }
);

// Standalone string binding
const bound = bind("QUERY TEXT :q FROM docs LIMIT :lim", { q: "test", lim: 10 });
console.log(bound); // QUERY TEXT 'test' FROM docs LIMIT 10
```

---

## 7. Free Functions & Explain

```js
const { parse, parseJson, isValid, injectFilter, tokenize, compileQuery, explain, bind } = require('@veristamp/nqql');

parse("QUERY 'x' FROM docs LIMIT 5");                    // Always Stmt[]
parse("QUERY 'x' FROM docs LIMIT 5; COUNT FROM docs");   // Script -> Stmt[]
parseJson("QUERY 'x' FROM docs LIMIT 5");                // Raw JSON string (2× faster, no V8 objects)
isValid("QUERY 'x' FROM docs LIMIT 5");                  // Validate
injectFilter("QUERY 'x' FROM docs", "tenant_id", "=", "acme");
tokenize("QUERY 'x'");
compileQuery("QUERY 'x' FROM docs LIMIT 5");

// Hierarchical ASCII tree plan
const planTree = explain("QUERY TEXT 'hello' FROM docs USING dense LIMIT 10");
console.log(planTree);
// Query Plan
// └── Target: docs
//     ├── Query: text('hello') via dense
//     └── Limit: 10
```

---

## 8. Free-Standing Execute

A top-level `execute()` function creates a temporary client per call:

```js
const { execute } = require('@veristamp/nqql');

// Single query
const result = await execute("QUERY 'search' FROM docs USING dense LIMIT 10");

// With options & parameters
const result = await execute("QUERY TEXT :q FROM docs USING dense LIMIT :lim", {
    url: "http://localhost:6333",
    apiKey: "sk-...",
    params: { q: "search", lim: 10 },
});
```
