# QQL WASM Playground

Browser playground for **qql-wasm**: write QQL, inspect the plan / wire JSON / AST / tokens, and execute against a live Qdrant instance with an optional OpenAI-compatible embedder.

## Stack

- Vite + React + TypeScript
- pnpm
- shadcn/ui (Base UI)
- CodeMirror 6 for the editor & JSON views
- `qql-wasm` (linked from `../demo/pkg`)

## Setup

```bash
# from repo root, ensure WASM pkg exists
# (demo/pkg is produced by the qql-wasm build)

cd wasm-demo
pnpm install
pnpm dev
```

Open http://localhost:5173

## Features

| Feature | Description |
|---|---|
| Live analysis | `analyze()` on each keystroke (debounced) |
| Syntax highlight | CodeMirror StreamLanguage for QQL keywords |
| Error spans | CodeMirror lint diagnostics from parse errors |
| Visual plan | REST method + path + explanation |
| Wire JSON | Qdrant request body |
| AST / Tokens | Full tree + lexer table |
| Execute | WASM `Client` → embedder → Qdrant REST |
| Presets | Hybrid, CTE, formula, group, MMR, scroll, DBSF, mutation |
| Settings | Qdrant URL/key + embedder config (localStorage) |

## Refresh WASM package

After rebuilding `qql-wasm`:

```bash
# copy/link into demo/pkg, then reinstall local dep
pnpm add file:../demo/pkg
```

## Scripts

```bash
pnpm dev       # playground
pnpm build     # production build
pnpm preview   # preview build
pnpm typecheck
```
