# WebAssembly SDK (`qql-wasm`) Reference & Examples

WASM bindings for browser and edge (Cloudflare Workers, Vercel Edge, Deno, Bun).

## Install

```bash
npm install qql-wasm
```

---

## 1. Client-Side Validation & Filter Injection

Validate and inject filters in the browser — no server round-trip needed.

```js
import init, { parse, isValid, inject_filter } from 'qql-wasm';

await init();

// Validate user input instantly
if (!isValid("QUERY 'machine learning' FROM papers LIMIT 20")) {
    throw new Error("Invalid QQL");
}

// Parse into AST, inject tenant filter
const stmt = parse("QUERY 'search' FROM docs LIMIT 10");
inject_filter(stmt, "tenant_id", "=", "acme");
```

---

## 2. Offline Route Compilation

Lower QQL to a typed REST route object without a Qdrant connection.

```js
import init, { compile, parse_all } from 'qql-wasm';

await init();

// Compile a single statement to a route
const route = compile("QUERY 'search' FROM docs USING dense LIMIT 10");
// → { method: "POST", path: "/collections/docs/points/query", payload: {...} }

// Compile a whole .qql script
for (const stmt of parse_all(`
  CREATE COLLECTION docs HYBRID (dense VECTOR(768, COSINE), sparse SPARSE)
    WITH PARAMS (replication_factor = 3);
  CREATE INDEX ON COLLECTION docs FOR tenant_id TYPE keyword WITH (is_tenant = true);
`)) {
    console.log(stmt);
}
```

---

## 3. Execute via Browser Fetch

Full client from browser, with optional embedder for text-to-vector resolution.

```js
import init, { Client } from 'qql-wasm';

await init();

const client = new Client("http://localhost:6333", null);

// Optional: configure embedder for automatic text → vector
client.setOpenAIEmbedder("sk-...", "text-embedding-3-small", 1536);

const result = await client.execute(
    "QUERY 'vector databases' FROM docs USING dense LIMIT 10"
);
```
