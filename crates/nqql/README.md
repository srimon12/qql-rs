# nqql

Node.js N-API bindings for QQL (parser, plan, execute).

## Proposition

Same QQL surface as Python/Rust/CLI — live Qdrant over **REST or gRPC**,
optional HTTP embedders, AST `injectFilter`, and first-class `SHARD` routing.
Language surface tracks **Qdrant ≥ 1.19** (quotas, `memory` placement,
`MATCH PREFIX` / `SLICE`, sparse `idf`, `turbo4`).

## Install

```bash
npm install @veristamp/nqql
```

Node **≥ 18**. Platforms: Linux x64 glibc, macOS x64/arm64, Windows x64.

## Quick start

```javascript
const {
  Client, HttpEmbedder, parse, isValid, injectFilter,
  compileQuery, explain, execute, version,
} = require("@veristamp/nqql");

const client = new Client({
  url: "http://localhost:6333",
  embedder: new HttpEmbedder({
    endpoint: "http://localhost:11434/v1/embeddings",
    model: "all-minilm:l6-v2",
    dimension: 384,
  }),
});

const report = await client.execute(
  "QUERY TEXT 'cardiology' FROM medical_records USING dense LIMIT 5"
);

// Isolation
const [stmt] = parse("QUERY TEXT 'risks' FROM sec10k USING dense LIMIT 10");
stmt.injectFilter("tenant_id", "=", "honeywell");

// Routing: prefer SHARD in QQL — or property after parse
// QUERY ... SHARD 'honeywell' LIMIT 10
stmt.shardKey = "honeywell";
await client.execute(stmt);

console.log(version, isValid("SHOW COLLECTIONS"), compileQuery("SHOW COLLECTIONS"));
```

## API summary

| Export | Role |
|--------|------|
| `Client({ url, apiKey?, useGrpc?, routeAffinity?, embedder? })` | Live execute |
| `HttpEmbedder({ endpoint, model, dimension, apiKey?, multi*?, image*?, rerank*? })` | Embeddings (dense, multi/ColBERT, image/CLIP, rerank) |
| `parse` / `parseJson` / `isValid` / `tokenize` | Frontend |
| `injectFilter` / `stmt.injectFilter` | Isolation |
| `stmt.shardKey` | Same as QQL `SHARD '…'` (no `injectShardKey`) |
| `compileQuery` / `explain` / `explainStmt` | Offline |
| `bind(query, params)` | Substitute `:name` (object) or `?` (array) |
| `execute` / `executeStmt` | Free-function execute (`options.params` same as `bind`) |

### Isolation vs routing

| Concern | API | Wire |
|---------|-----|------|
| Isolation | `injectFilter` | **Filter** |
| Routing | `SHARD '…'` or `stmt.shardKey` | `shard_key` / `ShardKeySelector` |
| Sparse IDF | QQL `PARAMS (idf = 'global' \| WHERE …)` | `params.idf` (planner, not an inject) |
| Partition DDL | `CREATE SHARD KEY` | Admin API |

`injectFilter` ops: `= > >= < <=` only (no `!=`).

### Qdrant 1.19 notes

```javascript
// Quotas: REST only (default :6333). useGrpc: true → QQL-GRPC-QUOTA
await client.execute("SHOW QUOTAS");
await client.execute(
  "SET QUOTA (enabled = true, max_resident_memory_percent = 80, " +
    "max_disk_usage_percent = 90, release_margin_percent = 5) WAIT true"
);

// Keyword prefix filter (index needs prefix = true)
await client.execute(
  "QUERY TEXT 'q' FROM docs USING dense WHERE title MATCH PREFIX 'Comp' LIMIT 5"
);

// Sparse IDF corpus — QQL WHERE filter, not a JSON object / inject API
await client.execute(
  "QUERY TEXT 'q' FROM docs USING sparse PARAMS (idf = 'global') LIMIT 5"
);
await client.execute(
  "QUERY TEXT 'q' FROM docs USING sparse " +
    "WHERE tenant_id = 'acme' SHARD 'acme' " +
    "PARAMS (idf = WHERE tenant_id = 'acme') LIMIT 5"
);
```

`SET QUOTA` fully replaces the cluster config.

### Route affinity (Qdrant 1.19+)

Pin reads to a stable replica with `routeAffinity` at construction — sent as
the `X-Qdrant-Route-Affinity` header (REST) / `x-qdrant-route-affinity` metadata
(gRPC). Empty string is treated as unset. Readable via `client.routeAffinity`.

```javascript
const client = new Client({
  url: "http://localhost:6333",
  routeAffinity: "session-acme-42",
});
console.log(client.routeAffinity); // "session-acme-42"
// One-shot convenience: execute(qql, { url, routeAffinity })
```

## Execution report

```json
{ "ok": true, "results": [{ "ok": true, "operation": "QUERY", "message": "…", "data": null }], "succeeded": 1, "failed": 0 }
```

`onError`: `"stop"` | `"continue"`.

## Docs

- [Syntax](../../docs/syntax.md) · [Filters](../../docs/filters.md) · [inject_filter](../../docs/inject_filter.md)
- [Multitenancy](../../skills/qql-skill/references/qql-multitenancy.md) · [Node skill](../../skills/qql-skill/references/node-sdk.md)
