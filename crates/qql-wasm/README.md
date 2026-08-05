# qql-wasm

WebAssembly bindings for QQL: parse, plan, optional browser execute (`fetch`).

## Proposition

Run the same QQL frontend offline in the browser (or Workers/Deno/Bun with the
right wasm-bindgen target). Optional `Client` posts **REST** to Qdrant and can
attach HTTP/JS embedders. Plan IR is shared with native crates. Language
surface tracks **Qdrant ≥ 1.19** (parse/plan/compile always; live quotas and
1.19 body fields need a ≥ 1.19 server when executing).

## Install

```bash
npm install qql-wasm
# or: wasm-pack build --target web|bundler|nodejs
```

## Quick start

```javascript
import init, {
  Client, Stmt, parse, isValid, inject_filter, compile, explain, analyze, tokenize,
} from "qql-wasm";

await init();

// Offline: tokens + AST + route + explain
const info = analyze("QUERY TEXT 'machine learning' FROM docs USING dense LIMIT 10");
console.log(info.valid, info.route, info.explain);

// Live REST (client feature)
const client = new Client("http://localhost:6333", null);
client.setHttpEmbedder(
  "http://localhost:11434/v1/embeddings",
  "all-minilm:l6-v2",
  384,
  null,
);

const report = await client.execute(
  "QUERY TEXT 'machine learning' FROM docs USING dense LIMIT 5"
);

// Isolation + optional routing
const stmt = new Stmt("QUERY TEXT 'search' FROM docs USING dense LIMIT 10");
stmt.injectFilter("tenant_id", "=", "acme");
// Prefer: ... SHARD 'acme' ...
stmt.shardKey = "acme"; // same field as SHARD — no injectShardKey
await client.executeStmt(stmt);

stmt.free();
client.free();
```

## API

### Free functions

| Export | Role |
|--------|------|
| `parse` / `isValid` / `tokenize` | Frontend |
| `inject_filter` | Isolation |
| `analyze` | tokens + AST + route(s) + explain |
| `compile` / `explain` | Offline REST projection / plan text |

### `Stmt`

| Member | Role |
|--------|------|
| `injectFilter` | Isolation |
| `shardKey` | Get/set routing (= QQL `SHARD`) |
| `toJSON` / `toObject` / `compileRoute` | Serialize / project |

### `Client` (`client` feature)

| Method | Role |
|--------|------|
| `setHttpEmbedder` / `setEmbedder` | Dense embed hosts |
| `execute` / `executeStmt` | REST execute → `ExecutionReport` |
| `compile` / `explain` | Offline helpers |

## Features

| Feature | Default | Role |
|---------|---------|------|
| `client` | yes | `gloo-net` fetch client + embedders |

```bash
# Parser/compiler only
cargo build -p qql-wasm --target wasm32-unknown-unknown --no-default-features
```

## Limits

- JS host ABI (`wasm-bindgen`), not generic WASI
- Single-threaded async on `wasm32-unknown-unknown`
- Multivector embed needs a multi-capable host embedder (default HTTP is dense)
- Live `Client` is REST-only (no gRPC) — so `SHOW QUOTAS` / `SET QUOTA` work
  against Qdrant ≥ 1.19 when executed; offline `compile`/`analyze` project
  them to `GET|PUT /quotas`
- Route affinity is not exposed on the WASM client

```javascript
// Offline plan of 1.19 surfaces
const info = analyze("SHOW QUOTAS");
// Live REST (server ≥ 1.19)
await client.execute(
  "SET QUOTA (enabled = true, max_resident_memory_percent = 80) WAIT true"
);
await client.execute(
  "QUERY TEXT 'q' FROM docs USING dense WHERE title MATCH PREFIX 'Comp' LIMIT 5"
);
```

## Docs

- [Syntax](../../docs/syntax.md) · [WASM skill](../../skills/qql-skill/references/wasm-sdk.md)
