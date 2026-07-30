<div align="center">

  <img src="https://raw.githubusercontent.com/srimon12/qql-rs/main/docs/assets/qql-banner.png" alt="QQL Banner" width="600" />

  # QQL 1.2 — Qdrant Query Language for VS Code

  Syntax highlighting, live linting, and autocompletion for [QQL](https://github.com/srimon12/qql-rs) — a SQL-like query language for the [Qdrant](https://qdrant.tech) vector database.

  **QQL is to Qdrant what SQL is to Postgres.**

</div>

---

## Features

### Syntax Highlighting

All 130+ QQL 1.2 keywords tokenized into distinct scopes — statements, query expressions (IMAGE, CROSS, MULTI), filters, DDL, formula expressions, and boolean/null constants. Strings, numbers, comments, operators, and dotted paths (`field.nested`, `items[].name`) are all highlighted correctly.

| Token type | Scope |
|-----------|-------|
| Statements (`QUERY`, `UPSERT`, `CREATE`, …) | `keyword.control` |
| Clauses (`FROM`, `WHERE`, `LIMIT`, …) | `keyword.other` |
| Query modes (`HYBRID`, `MMR`, `FUSION`, …) | `keyword.query` |
| DDL (`COLLECTION`, `HNSW`, `QUANTIZATION`, …) | `keyword.ddl` |
| Filters (`MATCH`, `NESTED`, `GEO_BBOX`, …) | `keyword.filter` |
| Formula (`CASE WHEN`, `ABS`, `GEO_DISTANCE`, …) | `keyword.formula` |
| Booleans (`true`, `false`) | `constant.language.boolean` |
| Null | `constant.language.null` |
| Strings | `string.quoted.single` / `.double` |
| Numbers (int, float, sci notation) | `constant.numeric` |
| Comments (`-- …`) | `comment.line` |

### Live Diagnostics

Every `.qql` file is parsed in real-time by the same WASM parser that powers `qql-core`. Parse errors appear as red squiggly underlines with the exact error code, message, and position from the Rust compiler pipeline.

```
QUERY missing FROM docs;   ← QQL-PARSE-UNEXPECTED: Expected FROM
```

- Errors update within 300ms of typing
- Error spans are precise (byte-level from the Rust parser, converted to VS Code positions)
- Zero external dependencies — the 1.35 MB WASM binary is bundled directly

### Autocompletion

**130+ keyword completions** — every QQL keyword is available at your cursor.

**28 snippet templates** — common query patterns insert with placeholders:

| Snippet | What it generates |
|---------|------------------|
| `QUERY NEAREST` | Basic semantic search with `TEXT`/`USING`/`LIMIT` |
| `QUERY HYBRID` | Hybrid dense+sparse RRF search |
| `QUERY HYBRID DBSF` | Hybrid with DBSF fusion |
| `CTE + FUSION` | Multi-stage retrieval with `WITH`/`PREFETCH`/`FUSION RRF` |
| `CTE + RERANK` | Two-stage retrieval with cross-encoder reranking |
| `QUERY RECOMMEND` | Recommendation from positive/negative examples |
| `QUERY MMR` | Maximal Marginal Relevance diversity search |
| `QUERY FORMULA` | Score shaping with arithmetic and payload fields |
| `QUERY POINTS` | Direct point ID retrieval |
| `QUERY ORDER BY` | Ordered payload scan |
| `QUERY SAMPLE` | Random sampling |
| `UPSERT INTO` | Point upsert with auto-embedding |
| `CREATE COLLECTION` | Collection DDL |
| `CREATE COLLECTION HYBRID` | Hybrid dense+sparse collection DDL |
| `CREATE INDEX` | Payload index creation |
| `SCROLL` | Cursor-based pagination |
| `COUNT` | Point counting with filter |
| `DELETE` | Point deletion by filter |

### Language Configuration

- **Comment toggle:** `Ctrl+/` inserts `-- `
- **Bracket matching:** `{}`, `[]`, `()` highlight pairs and auto-close
- **String auto-close:** `'` and `"` auto-close with the matching quote

---

## Requirements

- VS Code 1.85 or newer
- No additional dependencies — the extension bundles everything

---

## Supported File Types

- `.qql` files — full highlighting, linting, and completions

---

## Extension Size

| Component | Size |
|-----------|------|
| WASM parser binary | 1.35 MB |
| JS loader + providers | ~14 KB |
| TextMate grammar | 4.2 KB |
| **Total VSIX** | **846 KB** |

The WASM binary is compiled from `qql-core` (the Rust reference implementation) and contains the complete QQL v1 parser — lexer, recursive-descent parser, Pratt formula parser, and AST builder — all running in-process.

---

## How It Works

```
.qql file → VS Code onDidChangeTextDocument
                │
                ▼ (300ms debounce)
          qql-wasm analyze()
                │
                ▼
        ┌──────────────┐
        │  Rust parser  │  (1.35 MB WASM binary)
        │  • Lexer      │
        │  • Parser     │
        │  • AST build  │
        └──────┬───────┘
               │
               ▼
    { valid, error: { code, message, start, end }, tokens, … }
               │
               ▼
      VS Code DiagnosticCollection (red squiggly)
```

- **Parsing** happens synchronously in the extension host (Node.js)
- **No network calls** — the parser is fully self-contained
- **No file writes** — everything is in-memory

---

## Installation

### From VS Code Marketplace (recommended)

1. Open VS Code
2. Go to Extensions (`Ctrl+Shift+X`)
3. Search for **QQL**
4. Click Install

Or from the command line:

```bash
code --install-extension srimon12.qql-lang
```

### From `.vsix` (offline / local build)

```bash
# Download the .vsix from GitHub Releases
code --install-extension qql-lang-0.1.5.vsix
```

### Build from source

```bash
git clone https://github.com/srimon12/qql-rs
cd qql-rs/editors/vscode

# Build the WASM parser
wasm-pack build ../../crates/qql-wasm --release --target nodejs --out-dir wasm

# Install dependencies
npm install

# Compile TypeScript
npm run vscode:prepublish

# Package
npx vsce package

# Install locally
code --install-extension qql-lang-0.1.5.vsix
```

---

## Related Projects

| Project | Description |
|---------|-------------|
| [`qql-rs`](https://github.com/srimon12/qql-rs) | Rust reference implementation — parser, planner, runtime, CLI, edge, bindings |
| [`qql-go`](https://github.com/srimon12/qql-go) | Go implementation — gateway, RPC, policy engine, MCP server |
---

## License

MIT — see [LICENSE](LICENSE) for details.
