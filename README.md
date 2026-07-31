<div align="center">

  <img src="https://raw.githubusercontent.com/srimon12/qql-rs/main/docs/assets/qql-banner.png" alt="QQL Banner" width="600" />

  # QQL — Declarative Vector Search Engine

  **QQL is to Qdrant what SQL is to Postgres.**  
  Expressive, declarative queries to search, filter, rerank, recommend, and transform vectors in one language.

  [![CI](https://github.com/srimon12/qql-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/srimon12/qql-rs/actions/workflows/ci.yml)
  [![Release](https://img.shields.io/github/v/release/srimon12/qql-rs?color=blue)](https://github.com/srimon12/qql-rs/releases)
  [![Crates.io](https://img.shields.io/crates/v/qql.svg)](https://crates.io/crates/qql)
  [![PyPI](https://img.shields.io/pypi/v/pyqql.svg)](https://pypi.org/project/pyqql/)
  [![npm](https://img.shields.io/npm/v/@veristamp/nqql.svg)](https://www.npmjs.com/package/@veristamp/nqql)
  [![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

  [Docs](docs/README.md) • [Syntax](docs/syntax.md) • [Spec](language/v1/spec/semantics.md) • [Skill](skills/qql-skill/README.md) • [Releases](https://github.com/srimon12/qql-rs/releases)

</div>

---

### ⚡ Quick Install

#### 💻 CLI One-Liner (Linux, macOS, Windows)

```bash
# Linux & macOS (Shell)
curl -fsSL https://raw.githubusercontent.com/srimon12/qql-rs/main/scripts/install.sh | sh

# Windows (PowerShell)
irm https://raw.githubusercontent.com/srimon12/qql-rs/main/scripts/install.ps1 | iex
```

#### 📦 Language SDKs

```bash
# Edge verions are heavier but comes with qdrant_edge and fastembed-rs in a single package
# for minimal footprint prefer vanilla sdk without the -edge.

# Python
pip install pyqql OR pyqql-edge

# Node.js
npm install @veristamp/nqql OR @veristamp/nqql-edge  

# WebAssembly
npm install qql-wasm

# Rust
cargo add qql qql-core
```

#### 🧩 VS Code / Cursor Extension

```bash
# Marketplace
code --install-extension srimon12.qql-lang

# Local build (from editors/vscode)
# npm run package && code --install-extension qql-lang-*.vsix --force
```

Syntax highlighting, live WASM diagnostics, hover plans, CodeLens, outline, and smart completions for `.qql` files. See [`editors/vscode`](editors/vscode).

#### 🤖 AI Agent Skill (Cursor, Claude Code, codex etc)

```bash
npx skills add srimon12/qql-rs --skill qql-skill
```

---

### 💡 Example

```sql
QUERY TEXT 'chest pain'
FROM medical
USING dense
WHERE department = 'cardio'
SHARD 'hospital-east'   -- optional custom-shard routing
LIMIT 5;
```

Isolation on untrusted QQL: `inject_filter(stmt, "tenant_id", "=", tenant)`.  
Routing: `SHARD '…'` in QQL (or `stmt.shard_key` after parse).  
Partition DDL: `CREATE SHARD KEY '…' ON COLLECTION …`.

---

### 📚 Documentation

- 📖 **Docs index**: [`docs/README.md`](docs/README.md)
- 📐 **Syntax**: [`docs/syntax.md`](docs/syntax.md)
- 🔒 **inject_filter / multitenancy**: [`docs/inject_filter.md`](docs/inject_filter.md) · [`skills/qql-skill/references/qql-multitenancy.md`](skills/qql-skill/references/qql-multitenancy.md)
- 🤖 **Agent skill**: [`skills/qql-skill/README.md`](skills/qql-skill/README.md)
- 🗺️ **Gaps**: [`skills/qql-skill/references/qql-gaps.md`](skills/qql-skill/references/qql-gaps.md)
- ⚙️ **Spec**: [`language/v1/spec/semantics.md`](language/v1/spec/semantics.md)
- 🛠️ **Releasing**: [`RELEASING.md`](RELEASING.md)

---

<div align="center">
  <sub>Built with 🦀 Rust for Qdrant and Edge Vector DBs. Licensed under MIT.</sub>
</div>
