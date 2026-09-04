# Workspace Rust Guidelines (qql-rs)

This workspace is a high-performance, modular Rust engine containing multiple crates: core execution, parsing, query planning, embeddings, CLI, WASM, Python FFI (`pyqql`), and Node.js FFI (`nqql`).

## Core Rules for Agents Operating Here

1. **Verify Before Declaring Done**:
   - `cargo check --workspace --all-targets`
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - `cargo test -p qql-core -p qql-plan -p qql-embed`
   - `cargo fmt --check`

2. **Memory Safety & Unsafe Code**:
   - Every `unsafe` block must include a descriptive `// SAFETY:` invariant check.
   - For FFI boundaries (`pyqql` using PyO3, `nqql` using NAPI-RS, `qql-wasm`), consult `unsafe-checker`, `rust-pyo3`, and `rust-napi`.

3. **Simplicity & Surgical Edits**:
   - No speculative abstractions. Touch only files directly related to the user's task.
   - Preserve existing architecture and naming conventions.

4. **QQL Language Idioms**:
   - **Payloads included by default**: `QUERY` returns point payloads by default (`WITH PAYLOAD true`). Do not add redundant `WITH PAYLOAD true` clauses. Use `WITH PAYLOAD false` only when stripping payloads for minimal bandwidth.
   - **Compact vector literals**: Use `QUERY [0.1, 0.2, ...] FROM ...` directly. The `VECTOR` keyword prefix is optional for array literals.
   - **Formula decay datetime targets**: Write `TARGET = "2024-01-01T00:00:00Z"` with standard ISO 8601 strings; bare field identifiers automatically infer `datetime_key`.
   - **In-database faceting**: Use `FACET <field> FROM <collection> [WHERE ...] [LIMIT ...] [EXACT true]` for categorical value aggregation instead of pulling points into memory.
   - **Zero SDK dependency**: `pyqql` and `nqql` are standalone runtimes. Never import `qdrant_client` or third-party wrappers inside QQL query code.
