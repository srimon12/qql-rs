# QQL WASM Playground

Interactive browser playground for **[QQL](../README.md)** — write queries, inspect the compiler pipeline, embed text **in the browser**, and execute against a live Qdrant cluster.

This app replaces the older static `demo/` page (plain HTML + dual-layer highlight hacks) with a Vite + React + shadcn stack and proper CodeMirror editing.

---

## Features & Capabilities

| Capability | Implementation |
|---|---|
| Offline parse & multi-statement plan | `qql-wasm` (`analyze`, AST, routes array, explain, tokenization) |
| Tenant Isolation Sandbox | AST-native `inject_filter` & `shard_key` WASM mutation & wire diffing |
| SDK Code Exporter | Multi-statement Python, Node.js, Rust SDK snippets & cURL export |
| Offline embeddings (default) | Transformers.js · `Xenova/all-MiniLM-L6-v2` · **384-d** |
| Optional remote embeddings | OpenAI-compatible HTTP (Ollama, LM Studio, OpenAI) |
| Live datasets & search | Live Qdrant REST (`berlin_airbnb` 2.5k listings & `sec10k`) |
| Compiler Audit Bar | Real-time security strip for tenant WHERE filters & physical SHARD keys |

**Default path:** no LM Studio / Ollama required for embeddings. Only Qdrant must be reachable for **Execute**.

Embedding family matches `examples/sec10k-qql` (all-MiniLM-L6-v2 · 384-d cosine dense vectors).

---

## Quick start

### Prerequisites

- Node.js 20+
- **pnpm** (do not use npm for this package)
- Qdrant with the `sec10k` collection if you want the showcase presets to hit real data  
  (see `examples/sec10k-qql/`)
- Built WASM package at `demo/pkg` (linked as `qql-wasm`)

### Run

```bash
cd wasm-demo
pnpm install
pnpm dev
```

Open **http://localhost:5173**

1. Confirm **Settings → Qdrant REST URL** (default `http://localhost:6333`).
2. Leave embedder on **In-browser MiniLM** (default).
3. Pick a preset (e.g. Hybrid RRF or Multi-Tenant Isolation).
4. Click **Execute** (or `⌘/Ctrl+Enter`).  
   First embed downloads MiniLM into the browser cache (WebGPU if available, else WASM).
5. Inspect **Metrics**, **Response**, **Plan**, **Wire JSON**.

### Scripts

| Command | Purpose |
|---|---|
| `pnpm dev` | Vite dev server |
| `pnpm build` | Production build (`dist/`) |
| `pnpm preview` | Serve production build |
| `pnpm typecheck` | `tsc --noEmit` |
| `pnpm lint` | ESLint |
| `pnpm format` | Prettier |

---

## Features

### Editor

- CodeMirror 6 with a QQL StreamLanguage (keywords from `qql-core`)
- Live lint underlines from `analyze()` error spans
- Debounced re-analysis (~80 ms)
- Line numbers, fold gutter, wrap, history

### Inspector tabs

| Tab | Content |
|---|---|
| **Plan** | REST method + path + plan explanation |
| **Metrics** | Embedder status, parse/embed/network/total timings |
| **Wire JSON** | Qdrant request body from the first statement |
| **AST** | Parsed statement tree |
| **Tokens** | Lexer table (kind, literal, span) |
| **Explain** | Human-readable plan text |
| **Response** | Live execute JSON from Qdrant |

### Embedders

| Provider | Behavior |
|---|---|
| **browser** (default) | `setEmbedder` → Transformers.js MiniLM, lazy-loaded |
| **http** | `setHttpEmbedder` → OpenAI-compatible `/v1/embeddings` |
| **none** | Raw vectors only (no text resolution) |

Settings persist in `localStorage` under `qql-playground-settings-v2`.

### Presets (SEC 10-K showcase)

All target collection **`sec10k`** with custom shards `rtx` / `honeywell` / `3m` / `ge` (when ingested via the sec10k example).

| Preset | What it demos |
|---|---|
| Hybrid RRF | Dense + sparse fusion + shard |
| **Multi-Tenant Isolation** | `WHERE tenant_id = …` **and** `SHARD '…'` defense in depth |
| CTE Prefetch + Fusion | Multi-stage prefetch DAG + score thresholds |
| Formula Boost | `FORMULA` score rewrite |
| Grouped Aggregation | `GROUP BY` … `SIZE` |
| MMR Diversified | Diversity pruning |
| SCROLL + COUNT | Pagination + counts |
| DBSF Fusion | Alternative fusion |
| Upsert + Delete | Point lifecycle |

Multi-tenancy background: `skills/qql-skill/references/qql-multitenancy.md`.

### Shortcuts

| Key | Action |
|---|---|
| `⌘/Ctrl+Enter` | Execute |
| `d` (outside inputs) | Toggle light/dark theme |

---

## Architecture

```
┌──────────────────────────────────────────────────────────────┐
│  wasm-demo (React + Vite)                                    │
│  ┌─────────────────┐   analyze()    ┌─────────────────────┐  │
│  │ CodeMirror QQL  │ ─────────────► │ qql-wasm            │  │
│  └─────────────────┘                │  Client.execute     │  │
│                                     └──────────┬──────────┘  │
│  Embed:                                        │             │
│   browser → Transformers.js (lazy)             │ REST        │
│   http    → OpenAI-compatible POST             ▼             │
│   none    → skip                          Qdrant cluster     │
└──────────────────────────────────────────────────────────────┘
```

| Package / path | Role |
|---|---|
| `qql-wasm` (`file:../demo/pkg`) | Parser, planner, browser Client |
| `@huggingface/transformers` | In-browser MiniLM (code-split) |
| `@uiw/react-codemirror` + `@codemirror/*` | Editor + JSON viewers |
| shadcn/ui + Tailwind 4 | UI (do not reinvent components) |

---

## Refresh the WASM package

After rebuilding `crates/qql-wasm` and copying into `demo/pkg`:

```bash
pnpm add file:../demo/pkg
```

---

## Bootstrap (planned)

A one-click collection bootstrap is **not** in this app yet. The intended path is: ship a `.qql` script users can upload/run (CREATE + indexes + sample UPSERT). Until then, provision `sec10k` with `examples/sec10k-qql/`.

---

## Troubleshooting

| Symptom | Fix |
|---|---|
| Execute fails, Qdrant errors | Settings → REST URL; CORS if remote; cluster up |
| “No vectors” / embed errors | Wait for MiniLM download; check Metrics tab; try WebGPU browser |
| Dim mismatch | Collection must be **384-d** for default MiniLM |
| HTTP embedder fails | Endpoint must accept `{ model, input: string[] }` |
| Stale WASM behavior | Re-link `demo/pkg`, hard-refresh browser |
| pnpm build-script policy | `pnpm-workspace.yaml` sets `allowBuilds` for optional native deps (ORT node unused in browser) |

---

## Related

| Path | Description |
|---|---|
| [`AGENT.md`](./AGENT.md) | Agent / contributor map of this package |
| [`../demo/`](../demo/) | Legacy static playground (WASM only, no React) |
| [`../examples/sec10k-qql/`](../examples/sec10k-qql/) | Ingest + agent demo for `sec10k` |
| [`../crates/qql-wasm/`](../crates/qql-wasm/) | WASM crate source |
| [`../skills/qql-skill/references/`](../skills/qql-skill/references/) | QQL + multitenancy references |
