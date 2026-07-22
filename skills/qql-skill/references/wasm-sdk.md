# WebAssembly SDK (`qql-wasm`) Reference & Examples

WASM bindings for browser and edge (Cloudflare Workers, Vercel Edge, Deno, Bun).

## Install

```bash
npm install qql-wasm
```

---

## 1. Stmt Class — Parse Once, Reuse

The `Stmt` class wraps a parsed AST. Manipulate it before execution.

```js
import init, { Stmt, inject_filter } from 'qql-wasm';

await init();

// Parse into a Stmt object
const stmt = new Stmt("QUERY 'search' FROM docs USING dense LIMIT 10");

// Read / write the shard key
stmt.shardKey = "acme";
console.log(stmt.shardKey);  // → "acme"

// Inject a tenant filter (mutates in place)
stmt.injectFilter("tenant_id", "=", "acme");

// Serialise to JSON
const json = stmt.toJSON();
const obj = stmt.toObject();

// Free parsing functions also available
import { parse, parse_all } from 'qql-wasm';
const ast = parse("QUERY 'hello' FROM docs LIMIT 5");
const stmts = parse_all("Q1; Q2; Q3;");
```

---

## 2. Client-Side Validation & Filter Injection

Validate and inject filters in the browser — no server round-trip needed.

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

---

## 3. Execute via Browser Fetch

Full client from browser, with optional embedder for text-to-vector resolution.
`execute()` accepts a single string, a semicolon-delimited multi-statement string,
or an array of strings. Contiguous same-collection QUERYs use `/points/query/batch`;
contiguous mutations (UPSERT/DELETE/UPDATE/CLEAR/DELETE VECTOR) use `/points/batch`.
`executeStmt()` accepts a pre-parsed `Stmt` object.

```js
import init, { Client, Stmt } from 'qql-wasm';

await init();

const client = new Client("http://localhost:6333", null);
client.setOpenAIEmbedder("sk-...", "text-embedding-3-small", 1536);

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

// Batch — array of strings
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

## 4. Offline Route Compilation

Lower QQL to a typed REST route object without a Qdrant connection.

```js
import init, { compile, parse_all } from 'qql-wasm';

await init();

const route = compile("QUERY 'search' FROM docs USING dense LIMIT 10");
// → JSON string with stmt_type and payload

for (const stmt of parse_all(`
  CREATE COLLECTION docs HYBRID (dense VECTOR(768, COSINE), sparse SPARSE)
    WITH PARAMS (replication_factor = 3);
  CREATE SHARD KEY 'acme' ON COLLECTION docs WITH (shards_number = 2);
`)) {
    console.log(stmt);
}
```
