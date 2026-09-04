<div align="center">

  <img src="https://raw.githubusercontent.com/srimon12/qql-rs/main/docs/assets/qql-banner.png" alt="QQL Banner" width="600" />

  # QQL — Qdrant Query Language for VS Code

  A full IDE experience for [QQL](https://github.com/srimon12/qql-rs) — syntax highlighting, live linting, hover plans, outline, CodeLens, REST compile, curl export, and smart completions.

  **QQL is to Qdrant what SQL is to Postgres.**

</div>

---

## Features

### Language support (QQL 1.5 / Qdrant 1.19)

Highlights and completions cover the full QQL 1.5 surface, including:

| Feature | Example |
|---------|---------|
| In-database faceting | `FACET room_type FROM stays WHERE price < 150 LIMIT 5 EXACT true;` |
| Implicit vector search | `QUERY [0.1, 0.2, ...] FROM docs LIMIT 10;` |
| Default payload | Point payloads returned by default (`WITH PAYLOAD true`) |
| Quotas | `SHOW QUOTAS;` / `SET QUOTA (enabled = true, max_resident_memory_percent = 80) WAIT true;` |
| Memory placement | `WITH VECTOR (memory = 'cached')`, `WITH HNSW (memory = 'cold')`, `payload_memory = 'cold'` |
| MATCH PREFIX | `WHERE title MATCH PREFIX 'Comp'` |
| SLICE sampling | `WHERE SLICE (4, 1)` |
| Per-query IDF | `PARAMS (idf = 'global')` or `PARAMS (idf = WHERE tenant_id = 'acme')` |
| TurboQuant datatype | `datatype = 'turbo4'` (aliases `t4`, `f32`, `f16`, `u8`) |
| Keyword prefix index | `CREATE INDEX … TYPE keyword WITH (prefix = true)` |

### Syntax Highlighting

The generated TextMate grammar highlights QQL keywords, constants, strings, numbers, comments, parameter placeholders (`:name`, `?`), comparison operators, formula variables (`$score`), and dotted paths (`field.nested`, `items[].name`).

Also injects into Markdown fenced blocks:

````markdown
```qql
QUERY TEXT 'hello' FROM docs USING dense LIMIT 10;
```
````

### Live Diagnostics

Every `.qql` file is parsed in real time by the same WASM build of `qql-core`. Parse errors appear as red squiggles with the exact error code, message, and span from the Rust pipeline.

- Updates within ~300ms of typing (configurable)
- Byte-accurate spans (UTF-8 → UTF-16 conversion)
- Zero network — the WASM binary is bundled

### Hover Intelligence

- **Keyword docs** for statements, modes, clauses, filters, formula helpers
- **Live plan** for the enclosing statement (intent, collection, CTEs, limit)
- **REST route** summary when the statement compiles (`POST /collections/…/points/query`)

### Outline & Folding

- **Outline / breadcrumbs** list every top-level statement with kind + collection
- **CTE children** nest under `WITH` queries
- **Folding** for multi-line statements, parenthesized regions, and comment blocks
- Region markers: `-- #region` / `-- #endregion`

### CodeLens

Above each statement:

| Lens | Action |
|------|--------|
| **Explain** | Open the execution plan |
| **REST** | Open the compiled Qdrant REST route (JSON) |
| **curl** | Copy a ready-to-run curl command |

Disable with `qql.codeLens.enabled`.

### Commands

| Command | Default keybinding | Description |
|---------|-------------------|-------------|
| **QQL: Explain Document / Selection** | `Ctrl+K Ctrl+E` | Plan for doc or selection |
| **QQL: Compile to REST Route** | `Ctrl+K Ctrl+R` | Compiled route JSON |
| **QQL: Copy as curl** | `Ctrl+K Ctrl+C` | Clipboard curl (uses `qql.baseUrl`) |
| **QQL: Show AST** | — | Parsed AST as JSON |
| **QQL: Re-analyze Document** | click status bar | Force re-parse |

Also available from the editor title bar and right-click **QQL** submenu.

### Status Bar

Shows `✓ QQL N` when valid, or `✗ QQL` with the error code on failure. Click to re-analyze.

### Go to Definition

Jump from a CTE reference in `PREFETCH (…)` back to its `name AS (` definition.

### Smart Completions

- **Contextual follow-ups** — after `QUERY` suggest modes; after `FUSION` suggest `RRF`/`DBSF`; after `TYPE` suggest index types; …
- **Collection names** harvested from the current file
- **CTE names** suggested inside `PREFETCH`
- **Snippets** for hybrid, CTE fusion, rerank, recommend, DDL, shards, quotas, MATCH PREFIX, SLICE, memory placement, …
- Full keyword list still available for filter-as-you-type

Snippet prefixes (Insert Snippet): `qnearest`, `qhybrid`, `qcte`, `qcreate`, `qcreatemem`, `qupsert`, `qcross`, `qcount`, `qrecommend`, `qquota`, `qsetquota`, `qprefix`, `qslice`.

### Language Ergonomics

- Comment toggle (`Ctrl+/` → `-- `)
- Bracket colorization + auto-close for `{}` `[]` `()`
- Smart indent on open parens/braces
- Continue `-- ` comments on Enter

---

## Settings

| Setting | Default | Description |
|---------|---------|-------------|
| `qql.diagnostics.debounceMs` | `300` | Debounce before re-analyze |
| `qql.codeLens.enabled` | `true` | Statement CodeLens |
| `qql.baseUrl` | `http://localhost:6333` | Base URL for curl export |

---

## How It Works

```
.qql file
    │
    ▼ (debounced)
 AnalysisService  ── qql-wasm analyze()
    │                     │
    │              ┌──────┴──────┐
    │              │ Rust WASM   │
    │              │ lexer/parse │
    │              │ plan/route  │
    │              └──────┬──────┘
    │                     │
    ├── Diagnostics (errors)
    ├── Status bar (valid / N stmts)
    ├── CodeLens (Explain · REST · curl)
    ├── Outline symbols + CTE children
    ├── Hover (keyword docs + plan)
    └── Completions (context + collections)
```

Commands (`explain`, `compile`, `curl`, `AST`) call the same WASM surface (`explain`, `compile`, `parse`, `analyze`).

- **No network** for editing features
- **No language server process** — everything runs in the extension host

---

## Requirements

- VS Code 1.85+
- No extra runtime deps — WASM is bundled

---

## Installation

### Marketplace

```bash
code --install-extension srimon12.qql-lang
```

### From `.vsix` (local / GitHub Release)

VSIX binaries are **not** committed to the repo. Build one locally or download from [GitHub Releases](https://github.com/srimon12/qql-rs/releases).

```bash
code --install-extension qql-lang-*.vsix --force
```

Extension packaging version is in `package.json` (**0.2.4**). It ships the QQL **0.2.1** WASM parser from this monorepo (crate version need not match the VSIX version). Note: the checked-in WASM binary may still reflect a pre-1.4 parse surface until rebuilt with `wasm-pack`; TextMate / keyword artifacts stay in sync with QQL **1.4** via `qql-grammar-gen generate` without a WASM rebuild.

### Build from source

```bash
git clone https://github.com/srimon12/qql-rs
cd qql-rs/editors/vscode

# Build the WASM parser (Node target)
wasm-pack build ../../crates/qql-wasm --release --target nodejs --out-dir wasm

npm install
npm run check
npm run compile
npm run package          # npx @vscode/vsce package → qql-lang-<version>.vsix
code --install-extension qql-lang-*.vsix --force
```

---

## Related Projects

| Project | Description |
|---------|-------------|
| [`qql-rs`](https://github.com/srimon12/qql-rs) | Rust reference — parser, planner, runtime, CLI, edge, bindings |
| [`qql-go`](https://github.com/srimon12/qql-go) | Go — gateway, RPC, policy engine, MCP server |

---

## License

MIT — see [LICENSE](LICENSE) for details.
