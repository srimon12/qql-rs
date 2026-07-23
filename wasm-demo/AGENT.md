# AGENT.md — QQL WASM Playground (`wasm-demo`)

Guidance for AI coding agents and humans extending this package.  
This is the **canonical browser playground** for QQL. Prefer editing here over the legacy `demo/` HTML app.

---

## Mission

Ship a developer-grade playground that:

1. Runs **qql-wasm** fully in the browser (parse, plan, explain, execute).
2. Embeds text **offline-first** with Transformers.js (all-MiniLM-L6-v2 · 384-d).
3. Talks to a **real Qdrant** cluster (SEC 10-K `sec10k` showcase).
4. Surfaces **metrics** (parse / embed / network / total) for every execute.
5. Uses **market libraries** (CodeMirror, shadcn, Transformers.js) — no home-grown editors or highlight layers.

**Not in scope here:** nqql (Node native), pyqql, qql-edge, LiteRT conversion pipelines, or a full in-browser vector DB.

---

## Stack constraints

| Rule | Detail |
|---|---|
| Package manager | **pnpm only** — never npm/yarn for install scripts |
| UI | shadcn/ui (Base UI) + Tailwind 4 — reuse `src/components/ui/*` |
| CSS theme | **Do not rewrite `src/index.css`** for ad-hoc branding; use tokens |
| Engine | **qql-wasm** only for QQL — not nqql |
| Embed default | browser MiniLM; HTTP optional |
| Types | TypeScript strict; `pnpm typecheck` / `tsc -b` before claiming done |

Workspace pnpm policy lives in `pnpm-workspace.yaml` (`allowBuilds` for optional native scripts; browser path does not need onnxruntime-node builds).

---

## Directory map

```
wasm-demo/
├── AGENT.md                 ← this file
├── README.md                ← user-facing docs
├── package.json
├── pnpm-workspace.yaml      ← allowBuilds for pnpm 11
├── vite.config.ts           ← aliases, wasm, transformers exclude, COOP/COEP
├── components.json          ← shadcn config
├── src/
│   ├── main.tsx             ← ThemeProvider defaultTheme="dark"
│   ├── App.tsx              ← shell: toolbar, presets, resizable panels
│   ├── index.css            ← shadcn/tailwind tokens — touch carefully
│   ├── hooks/
│   │   └── use-qql.ts       ← wasm init, analyze, execute, metrics, settings
│   ├── lib/
│   │   ├── browser-embedder.ts  ← lazy Transformers.js MiniLM
│   │   ├── presets.ts           ← SEC 10-K QQL presets
│   │   ├── qql-types.ts         ← AnalysisResult, settings, ExecMetrics
│   │   ├── qql-language.ts      ← CodeMirror StreamLanguage for QQL
│   │   ├── editor-theme.ts      ← CM themes from CSS variables
│   │   └── utils.ts             ← cn()
│   ├── components/
│   │   ├── playground/      ← feature UI (editor, inspector, metrics, settings)
│   │   ├── ui/              ← shadcn primitives
│   │   └── theme-provider.tsx
│   └── types/
│       └── qql-wasm.d.ts    ← ambient types for linked package
└── dist/                    ← production build output
```

---

## Data flow (must preserve)

```
User types QQL
    → debounced analyze(source)          # qql-wasm, sync JSON string
    → AnalysisResult { valid, tokens, ast, route, explain, error }
    → CodeMirror lint from error.start/end
    → Inspector tabs update

User hits Execute
    → Client(url, apiKey)
    → embedProvider === "browser"
          ? setEmbedder(async texts => MiniLM vectors)
          : setHttpEmbedder(...) | none
    → client.execute(source)
    → probe timings around embed callback + wall total
    → Response + Metrics tabs
```

### Critical APIs (qql-wasm)

```ts
import init, { analyze, Client } from "qql-wasm"
await init()

const json = analyze(qql)           // stringified AnalysisResult
const client = new Client(url, apiKey | null)
client.setEmbedder(async (texts: string[]) => number[][])
client.setHttpEmbedder(endpoint, model, dim, apiKey | null)
const resJson = await client.execute(qql)  // string
```

Constructor is **positional** `(url?, api_key?)` — not an options object (unlike nqql).

### Embedding contract

- Browser model id: `Xenova/all-MiniLM-L6-v2` (`BROWSER_EMBED_MODEL`)
- Dimension: **384** (`BROWSER_EMBED_DIM`) — must match `sec10k` dense config
- Pooling: `mean` + `normalize: true`
- Load is **lazy** on first embed (dynamic `import("@huggingface/transformers")`)
- Prefer WebGPU, fall back to WASM; surface device in Metrics + toolbar badge

Do **not** load the full MiniLM pipeline on every page load.

---

## Settings & persistence

| Key | Value |
|---|---|
| Storage key | `qql-playground-settings-v2` |
| Default `embedProvider` | `"browser"` |
| Default Qdrant | `http://localhost:6333` |
| Legacy providers | `"openai"` / `"remote"` migrate → `"http"` in `loadSettings()` |

When changing settings shape, bump the storage key or extend `migrateProvider`.

---

## Presets

Source: `src/lib/presets.ts`.

- All showcase queries use collection **`sec10k`** and real tenant shards when data exists.
- **Multi-Tenant Isolation** (`multitenant`) must keep dual isolation:
  - `WHERE tenant_id = '<tenant>'`
  - `SHARD '<tenant>'`
- Reference: `skills/qql-skill/references/qql-multitenancy.md`
- Adding a preset: extend `PresetId`, append to `PRESETS` with comments explaining the QQL pattern.

Do not invent `SELECT` / old SQL keywords — QQL uses `QUERY` / `UPSERT` / etc. (see repo `AGENT.md`).

---

## Metrics

Defined in `ExecMetrics` (`qql-types.ts`), collected in `use-qql.ts`:

| Field | Meaning |
|---|---|
| `parseMs` | Last `analyze()` duration |
| `embedMs` | Time inside embedder callback (null if no embed) |
| `networkMs` | ≈ `totalMs - embedMs` when embed ran |
| `totalMs` | Full `client.execute` wall clock |
| `embedBackend` | `webgpu` / `wasm` / `http` / `none` |

Embed timing uses a mutable `probeRef` closed over by `setEmbedder` — keep that pattern if rebinding the client.

---

## UI conventions

- Layout: header toolbar + horizontal `ResizablePanelGroup` (editor | inspector) + footer.
- Components: add via `pnpm dlx shadcn@latest add <name>` when possible.
- Icons: `lucide-react`.
- Theme: `ThemeProvider`; `d` toggles outside editable fields.
- Base UI Select/Tabs/Dialog use `value` / `onValueChange` / `open` / `onOpenChange` (not Radix-only assumptions).

---

## Vite / WASM notes

`vite.config.ts`:

- Alias `@` → `./src`
- `optimizeDeps.exclude`: `qql-wasm`, `@huggingface/transformers`
- `assetsInclude: ["**/*.wasm"]`
- `server.fs.allow` parent monorepo for linked `demo/pkg`
- COOP/COEP headers for WASM threads where needed (`credentialless` + `same-origin`)

Linked package:

```json
"qql-wasm": "file:../demo/pkg"
```

After rebuilding wasm:

```bash
# from repo: produce demo/pkg then
pnpm add file:../demo/pkg
```

Ambient types: `src/types/qql-wasm.d.ts` (package `index.d.ts` may be incomplete).

---

## What not to do

1. **Do not** depend on `nqql` or Node native addons in this app.
2. **Do not** reintroduce textarea + highlight-layer dual editors.
3. **Do not** hardcode a second MiniLM dim without a new collection story.
4. **Do not** use npm for dependency installs.
5. **Do not** treat LiteRT as required — optional future backend behind the same `setEmbedder` interface is fine; Transformers.js is the shipped path.
6. **Do not** silently drop multitenant dual isolation from the multitenant preset.
7. **Do not** put large model weights in the repo; HF cache/download at runtime is expected.

---

## Planned / deferred work

| Item | Notes |
|---|---|
| Bootstrap via `.qql` upload | User runs CREATE/UPSERT script from file; not implemented yet |
| Sample payload bundle | Tiny offline demo without full SEC ingest |
| LiteRT embed path | Same 384-d contract if a verified `.tflite` lands |
| Code-split further | Main chunk still large; CM + UI already split from transformers |

---

## Verification checklist

Before finishing a change:

```bash
cd wasm-demo
pnpm typecheck    # or ./node_modules/.bin/tsc --noEmit
pnpm build        # must bundle qql_wasm_bg.wasm; transformers chunk lazy
pnpm dev          # smoke: analyze live, metrics tab, execute against local Qdrant
```

Manual smoke:

1. Load page → parse badge Valid on default Hybrid preset.
2. Metrics → embedder idle until first execute.
3. Execute → MiniLM download progress → Response JSON + embed/total ms.
4. Switch Multi-Tenant Isolation → both `tenant_id` and `SHARD` in wire/plan.
5. Settings → HTTP embedder still configurable; Save rebinds client.

---

## Related monorepo docs

| Doc | Use |
|---|---|
| `/AGENT.md` (repo root) | Workspace architecture, planner, QdrantOps |
| `crates/qql-wasm/README.md` | WASM API surface |
| `skills/qql-skill/references/wasm-sdk.md` | Host WASM usage patterns |
| `skills/qql-skill/references/qql-multitenancy.md` | Tenant isolation patterns |
| `examples/sec10k-qql/` | How `sec10k` is provisioned |
| `demo/` | Legacy playground — do not extend for new features |
