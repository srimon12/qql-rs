# nqql-edge

Node N-API bindings for **local** QQL: qdrant-edge + FastEmbed, no remote Qdrant.

## Proposition

Same language as `@veristamp/nqql`, executed in-process (qdrant-edge **0.8**).
Cluster features (`GROUP BY`, `SHARD`, ACORN, **`SHOW QUOTAS` / `SET QUOTA`**, …)
return explicit `QQL-EDGE-UNSUPPORTED-*` errors. Sparse `PARAMS (idf = …)` is
supported offline.

## Install

```bash
npm install @veristamp/nqql-edge
```

Node ≥ 18. Platforms: Linux x64, macOS arm64, Windows x64 (not macOS Intel).

## Quick start

```javascript
const {
  localExecutor, listEmbeddingModels, parse, injectFilter, version,
} = require("@veristamp/nqql-edge");

const client = localExecutor("./qql-data", {
  model: "bge-small-en-v1.5",
  onDiskPayload: true,
});

await client.execute("CREATE COLLECTION docs HYBRID");
await client.execute('UPSERT INTO docs VALUES {id: 1, text: "hello from edge"}');
const report = await client.execute(
  "QUERY TEXT 'hello' FROM docs USING dense LIMIT 5"
);

const [stmt] = parse("QUERY TEXT 'hello' FROM docs USING dense LIMIT 10");
stmt.injectFilter("tenant_id", "=", "acme");
// No custom SHARD on edge — use remote Qdrant for SHARD / CREATE SHARD KEY

await client.close();
console.log(version, listEmbeddingModels().length);
```

## API

| Export | Role |
|--------|------|
| `localExecutor(dataDir, options)` | FastEmbed + edge |
| `httpExecutor(dataDir, url, key, model, dim)` | Edge + HTTP embedder |
| `listEmbeddingModels()` | Dense ONNX catalog |
| `parse` / `parseJson` / `isValid` / `tokenize` | Frontend |
| `injectFilter` | Isolation |
| `stmt.shardKey` | AST property; edge **rejects** SHARD at execute |
| `bind(query, params)` | Substitute `:name` (object) or `?` (array) |
| `compileQuery` / `explain` / `execute` | Plan / run (`options.params` same as `bind`) |

Quotas, custom sharding, and `GROUP BY` require remote Qdrant. Offline sparse
IDF works: `PARAMS (idf = 'global')` or `PARAMS (idf = WHERE tenant_id = 'acme')`.

## Docs

- [qql-edge](../qql-edge/README.md) · [Gaps](../../skills/qql-skill/references/qql-gaps.md) · [Syntax](../../docs/syntax.md)
