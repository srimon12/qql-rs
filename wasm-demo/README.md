# QQL WASM Playground

Browser playground for **qql-wasm**: write QQL, inspect plan / metrics / wire / AST / tokens, and execute against a live Qdrant cluster (SEC 10-K `sec10k` showcase).

## Proposition

| Layer | Default | Optional |
|---|---|---|
| Parse / plan | `qql-wasm` offline | — |
| Embeddings | **In-browser** `Xenova/all-MiniLM-L6-v2` (384-d) via Transformers.js | HTTP OpenAI-compatible (Ollama / LM Studio) |
| Search / storage | Your local Qdrant (`sec10k` shards) | — |

Same embedding family as `examples/sec10k-qql` (all-MiniLM-L6-v2 · 384-d). No LM Studio required for Execute.

## Stack

- Vite + React + TypeScript + pnpm
- shadcn/ui (Base UI)
- CodeMirror 6
- `@huggingface/transformers` (lazy-loaded)
- `qql-wasm` (`file:../demo/pkg`)

## Setup

```bash
cd wasm-demo
pnpm install
pnpm dev
```

Open http://localhost:5173 — point **Settings → Qdrant** at `http://localhost:6333` (or your cluster).

First browser-embed run downloads MiniLM into the browser cache (WebGPU when available, else WASM).

## Metrics tab

| Metric | Meaning |
|---|---|
| Parse / plan | `analyze()` wall time |
| Embed | time inside `setEmbedder` (MiniLM or HTTP) |
| Network / Qdrant | approx `total − embed` |
| Total execute | full `client.execute` wall time |
| Model load | one-time pipeline load |

## Presets

SEC 10-K shaped queries: Hybrid RRF, CTE fusion, formula boost, GROUP BY, MMR, SCROLL/COUNT, DBSF, upsert+delete — collection `sec10k`, shards `rtx` / `honeywell` / `3m` / `ge`.

## Scripts

```bash
pnpm dev
pnpm build
pnpm preview
pnpm typecheck
```

## Refresh WASM package

```bash
pnpm add file:../demo/pkg
```
