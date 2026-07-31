# nqql

Node.js N-API bindings for QQL (parser, plan, execute).

## Proposition

Same QQL surface as Python/Rust/CLI — live Qdrant over **REST or gRPC**,
optional HTTP embedders, AST `injectFilter`, and first-class `SHARD` routing.

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
| `Client({ url, apiKey?, useGrpc?, embedder? })` | Live execute |
| `HttpEmbedder({ endpoint, model, dimension, apiKey?, multi*?, image*?, rerank*? })` | Embeddings (dense, multi/ColBERT, image/CLIP, rerank) |
| `parse` / `parseJson` / `isValid` / `tokenize` | Frontend |
| `injectFilter` / `stmt.injectFilter` | Isolation |
| `stmt.shardKey` | Same as QQL `SHARD '…'` (no `injectShardKey`) |
| `compileQuery` / `explain` / `explainStmt` | Offline |
| `execute` / `executeStmt` | Free-function execute |

### Isolation vs routing

| Concern | API | Wire |
|---------|-----|------|
| Isolation | `injectFilter` | **Filter** |
| Routing | `SHARD '…'` or `stmt.shardKey` | `shard_key` / `ShardKeySelector` |
| Partition DDL | `CREATE SHARD KEY` | Admin API |

`injectFilter` ops: `= > >= < <=` only (no `!=`).

## Execution report

```json
{ "ok": true, "results": [{ "ok": true, "operation": "QUERY", "message": "…", "data": null }], "succeeded": 1, "failed": 0 }
```

`onError`: `"stop"` | `"continue"`.

## Docs

- [Syntax](../../docs/syntax.md) · [Filters](../../docs/filters.md) · [inject_filter](../../docs/inject_filter.md)
- [Multitenancy](../../skills/qql-skill/references/qql-multitenancy.md) · [Node skill](../../skills/qql-skill/references/node-sdk.md)
