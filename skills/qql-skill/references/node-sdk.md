# Node.js SDK (`nqql`) Reference & Examples

Native Node.js bindings via N-API (napi-rs).

Language surface includes **Qdrant 1.19 / QQL 1.5** features expressible in QQL
(`SHOW QUOTAS`, memory/`turbo4`, `MATCH PREFIX`, `SLICE`, `PARAMS (idf = …)`).
Pass those strings to `client.execute` when the backend supports them (quotas:
**REST only**). Parameter placeholders (`:name` / `?`) and prepared-statement
binding arrive with **QQL 1.7**.

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
const { bind, compileQuery } = require('@veristamp/nqql');

const bound = bind(`
  QUERY TEXT :q FROM sec10k USING sparse
  WHERE tenant_id = :tenant
  SHARD :tenant
  PARAMS (idf = WHERE tenant_id = :tenant)
  LIMIT 10
`, { q: "supply chain", tenant: "honeywell" });

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

Prepared-statement helpers (QQL 1.7): `stmt.bind(params?)` returns a **new**
bound `Stmt` (named object or positional array), `stmt.compileRoute(params?)`
lowers the statement — optionally binding first — to its
`{ stmt_type, method, path, payload }` route, `stmt.toString()` renders
canonical re-parseable QQL (Python `str(stmt)` parity), and
`stmt.toReadableString()` renders the truncated preview (Python `repr(stmt)`
parity — long vectors collapse to `[0.1, 0.2, ... (384 dims)]`).

```js
const [stmt] = parse("QUERY TEXT :q FROM docs WHERE category = :cat LIMIT :lim");

const bound = stmt.bind({ q: "vector databases", cat: "tech", lim: 10 });
console.log(bound.toString());        // QUERY TEXT 'vector databases' FROM docs ...
console.log(stmt.toReadableString()); // placeholder preview (readable)

const route = stmt.compileRoute({ q: "vector databases", cat: "tech", lim: 10 });
console.log(route.method, route.path);
```

---

## 6. Parameter Binding & Prepared Queries

```js
const { Client, bind } = require('@veristamp/nqql');
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

// Prepared statement: parse once, execute repeatedly with different params
const [stmt] = parse("QUERY TEXT :query FROM docs WHERE category = :cat LIMIT :limit");
const res3 = await client.execute(stmt, { params: { query: "neural nets", cat: "ai", limit: 5 } });

// Standalone binding — `bind` accepts a string or a Stmt.
// Stmt + { truncateVectors: true } returns the readable string instead of a Stmt.
const bound = bind("QUERY TEXT :q FROM docs LIMIT :lim", { q: "test", lim: 10 });
console.log(bound); // QUERY TEXT 'test' FROM docs LIMIT 10

const readable = bind(stmt, { query: "test", cat: "tech", limit: 5 }, { truncateVectors: true });

// Nested dictionary parameters expand to dotted keys ({"loc": {"lat": 1.0}} binds :loc.lat)
const geo = await client.execute(
  "QUERY 'coffee' FROM venues WHERE location GEO_RADIUS { center: {lat: :loc.lat, lon: :loc.lon}, radius: :rad } LIMIT 5",
  { params: { loc: { lat: 52.52, lon: 13.40 }, rad: 1000 } }
);

// Statement-scoped batch parameters: array length must EXACTLY match the
// statement count; each entry is an object (named) or an array (positional)
const batch = await client.execute(
  [
    "QUERY TEXT :q FROM docs LIMIT 5",
    "QUERY TEXT :q FROM articles LIMIT 10",
  ],
  { params: [{ q: "quantum" }, { q: "relativity" }] }
);
```

Mixed styles fail closed with `QQL-BIND-MIXED-STYLE`; missing values raise
`QQL-BIND-MISSING-PARAM`, extra positional values `QQL-BIND-UNUSED-PARAMS`,
and wrong types `QQL-BIND-TYPE-MISMATCH` (see the
[error codes](/docs/reference/error-codes/)).

---

## 6b. Typed Result Accessors & `executeHits`

`execute()` returns an `ExecutionReport` object with `ok`, `results`,
`succeeded`, and `failed`, plus typed accessors.

```js
const { Client, executeHits } = require('@veristamp/nqql');

const client = new Client({ url: "http://localhost:6333" });

// 1. Typed hits: report.hits(stmt) / report.points(stmt) -> ScoredPoint[]
const report = await client.execute("QUERY TEXT 'neural search' FROM docs LIMIT 5");
for (const hit of report.hits()) {
  console.log(hit.id);        // number (e.g. 42) or UUID string
  console.log(hit.score);     // number
  console.log(hit.payload);   // object (null when absent)
  console.log(hit.text);      // top-level text shortcut (null when absent)
  console.log(hit.get("title", "n/a")); // payload access with optional default
}

// Negative statement index counts from the end (Python list semantics)
const batch = await client.execute("COUNT FROM docs; QUERY TEXT 'q' FROM docs LIMIT 5");
console.log(batch.hits(-1)); // last statement's hits

// 2. Shortcut: executeHits() (module or Client) -> ScoredPoint[] directly
const hits = await client.executeHits("QUERY TEXT 'neural search' FROM docs LIMIT 5");

// 3. Facet -> normalized [{ value, count }]
const facetReport = await client.execute("FACET category FROM docs LIMIT 10");
for (const item of facetReport.facet()) {
  console.log(item.value, item.count);
}

// 4. Count -> integer
const countReport = await client.execute("COUNT FROM docs WHERE category = 'tech'");
console.log(countReport.count());

// 5. Point retrieval -> points()
const pointReport = await client.execute("QUERY POINTS (1, 2, 3) FROM docs");
console.log(pointReport.points());
```

---

## 7. Free Functions & Explain

```js
const { parse, parseJson, isValid, injectFilter, tokenize, compileQuery, explain, bind } = require('@veristamp/nqql');

parse("QUERY 'x' FROM docs LIMIT 5");                    // Always Stmt[]
parse("QUERY 'x' FROM docs LIMIT 5; COUNT FROM docs");   // Script -> Stmt[]
parseJson("QUERY 'x' FROM docs LIMIT 5");                // Raw JSON string (2× faster, no V8 objects)
isValid("QUERY 'x' FROM docs LIMIT 5");                  // Parse + plan gate
injectFilter("QUERY 'x' FROM docs", "tenant_id", "=", "acme");
tokenize("QUERY 'x'");
compileQuery("QUERY 'x' FROM docs LIMIT 5");                   // Route object
compileQuery("QUERY TEXT :q FROM docs LIMIT :lim", { q: "x", lim: 5 }); // With parameter binding

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
