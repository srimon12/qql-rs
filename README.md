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

  [Documentation](docs/syntax.md) • [Specification](language/v1/spec/semantics.md) • [Releases](https://github.com/srimon12/qql-rs/releases)

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
# Python
pip install pyqql pyqql-edge

# Node.js
npm install @veristamp/nqql @veristamp/nqql-edge

# WebAssembly
npm install @veristamp/qql-wasm

# Rust
cargo add qql qql-core
```

#### 🤖 AI Agent Skill (Cursor, Claude Code, codex etc)

```bash
npx skills add srimon12/qql-rs --skill qql-skill
```

---

### 💡 Example

```sql
QUERY 'chest pain' FROM medical 
  USING DENSE MODEL 'nomic' ON FIELD title INTO title_vec 
  LIMIT 5 
  WHERE department = 'cardio';
```

---

### 📚 Documentation

- 📖 **Syntax Reference**: [`docs/syntax.md`](docs/syntax.md)
- ⚙️ **Language Specification**: [`language/v1/spec/semantics.md`](language/v1/spec/semantics.md)
- 🔒 **Multi-Tenancy Guide**: [`skills/qql-skill/references/qql-multitenancy.md`](skills/qql-skill/references/qql-multitenancy.md)
- 🛠️ **Release Manual**: [`RELEASING.md`](RELEASING.md)

---

<div align="center">
  <sub>Built with 🦀 Rust for Qdrant and Edge Vector DBs. Licensed under MIT.</sub>
</div>
