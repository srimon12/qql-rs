# Changelog

All notable changes to the **QQL** project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [0.1.3] - 2026-07-28

### 🔴 Critical
- **`0b3f443`** — `fix(rust)`: Unify AST serialization (`ShowCollections` → `{"ShowCollections": {}}`, `CountStmt.collection` → `QueryCollection::Explicit`) and fix HYBRID UPSERT named-vector mapping.
- **`ab73e7b`** — `fix(sdk)`: Fix scoped binary loading, add `toJSON` alias, support `apiKey`/`api_key` aliases, export `version`/`__version__`, update TypeScript declarations, expand all READMEs with ExecutionReport schema, error docs, and operator matrix.
- **`167816c`** — `fix`: Restore correct platform names in npm `optionalDependencies`.

### 🚀 Added
- **`ab73e7b`** — Add 290 comprehensive tests across nqql (104), pyqql (104), nqql-edge (42), pyqql-edge (40).
- **`dbb839b`** — `test(nqql)`: Skip live Qdrant tests in CI.
- **`eb6e0c3`** — `test(pyqql)`: Skip live Qdrant tests in CI.
- **`c252bcd`** — `chore(release)`: Bump version to 0.1.3; add centralized root `VERSION` file; update `check_release.py`.

### 🔒 Changed & Scoped
- **`e9873d8`** — `ci`: Switch npm publishing to OIDC Trusted Publishers.

### 📚 Documentation
- **`6ecaaca`** — `docs`: Fix `parseFastJson` → `parseJson`, update `inject_filter.md` examples, replace invalid placeholders in skill references.
- **`86af6c0`** — `docs(changelog)`: Add 0.1.3 release notes.

### 🛠️ Maintenance
- **`c95cf31`** — `chore`: Apply `cargo fmt` and regenerate conformance snapshots.
- **`27282f4`** — `chore`: Update `Cargo.lock` for version bump.

---

## [0.1.2] - 2026-07-28

### 🚀 Added
- **`371d6d2`** — `feat(language)`: Add support for QQL 1.1 `ON FIELD` and `INTO` spec modifiers, multi-spec embedding options, and new string delimiters across language spec and conformance suite.
- **`f36922f`** — `feat(embed)`: Enhance embedding specifications to support multi-target fields and explicit field resolution.
- **`b07d2c3`** — `feat(lexer)`: Add support for raw strings (`r'...'`), triple-quoted multiline strings (`'''...'''`), and backtick strings (`` `...` ``) in lexer and grammar.

### 🔒 Changed & Scoped
- **`a07f1fc`** — `fix(scope)`: Update package names to use `@veristamp` organization scope (`@veristamp/nqql`, `@veristamp/nqql-edge`) and unscoped `qql-wasm` across documentation, packages, and CI workflows.
- **`140c401`** — `chore(release)`: Bump version to `0.1.2` for all 13 workspace crates, Python packages, and Node package manifests.

### 🐛 Fixed & Hardened
- **`1fe4055`** — `fix(review)`: Address CodeRabbit PR review feedback for `v0.1.2` release (fast $O(N)$ string scanning, duplicate target vector validation, empty target checks, and `with_url` error propagation).
- **`6920bb6`** — `fix(release)`: Improve meta package publishing logic, rate-limit sleep delays, and error handling in release workflow.
- **`ed3fe20`** — `style`: Format codebase with `cargo fmt`.
- **`3dbac0a`** — `style`: Codebase linting, `cargo fmt`, and clippy fixes.

### 🛠️ Refactored
- **`def53ab`** — `refactor(error)`: Refactor error handling across `qql-edge` and `qql-runtime` modules with structured metadata fields.
- **`b04e3ca`** — `refactor(tests)`: Update `explain()` assertions in `pyqql` and `pyqql-edge` to verify structured `{ ok, query, plan }` response dictionaries.

---

## [0.1.1] - 2026-07-26

- Initial public release of QQL Rust engine, Python SDK (`pyqql`), Node.js N-API bindings (`nqql`), WebAssembly package (`qql-wasm`), and CLI (`qql-cli`).
