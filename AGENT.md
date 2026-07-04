# QQL Agent & Developer Reference Guide

Welcome to the QQL Rust codebase. This guide details the architecture, design philosophy, and key implementation guidelines for developers and AI coding agents.

---

## 1. Workspace Architecture

The workspace is organized into a modular, multi-crate Rust workspace under the `crates/` directory:

```
qql/ (workspace root)
├── crates/
│   ├── qql-core/         # Standalone Lexer, Parser, AST structures, & AST Transforms (no_std compatible)
│   ├── qql-runtime/      # Core execution pipeline, Qdrant client integrations, embedding resolution, & DML/DDL executor
│   ├── qql-cli/          # Command Line Interface (CLI) and interactive query REPL
│   ├── pyqql/            # Python bindings (PyO3)
│   ├── nqql/             # Node.js bindings (Neon)
│   └── qql-wasm/         # WebAssembly bindings (wasm-bindgen)
```

### Crate Division Boundaries
* **`qql-core`**: High-performance syntax parsing. No network or file I/O dependencies. Must remain fully `no_std` compatible (using `alloc` for boxing/vectors). Exposed structures are compiled into other environments (e.g. WASM, NodeJS).
* **`qql-runtime`**: Heavy operations. Integrates with the official `qdrant-client` crate, handles embedding models, sparse vector representations, and executes pipeline graphs.
* **Foreign Bindings**: Expose parsed AST payloads or runtime executor endpoints to Python, JS, or browser runtime target layers.

---

## 2. Minimalist Code Design Philosophy

We enforce a strict **"Minimal Vibe"** across the codebase:
1. **Size Constraints**: Files should remain modular and concise (targeting less than 300-400 lines per file). 
   * *Example*: The previously monolithic `executor.rs` was refactored into:
     * `executor/mod.rs` (entrypoint and statement dispatch)
     * `executor/ddl.rs` (collection and index management operations)
     * `executor/dml.rs` (query, insert, select, scroll, delete, and payload updates)
     * `executor/helpers.rs` (type conversion and serialization utilities)
2. **Error Propagation**: Avoid redundant pre-emptive roundtrips. For example, rather than verifying collection existence via `collection_exists` before executing `DELETE` or `UPDATE`, we dispatch the request directly and bubble up Qdrant's native downstream errors. This minimizes database load and network latency.

---

## 3. AST Query Transformation & Filter Injection

For production RAG systems, query security and tenant isolation must be enforced before queries reach Qdrant.

### The CTE/Prefetch Leak Risk
QQL supports complex multi-vector searches with Common Table Expressions (CTEs) and prefetch DAGs (e.g. `WITH prefetch_1 AS (...) QUERY ...`).
* Merging security filters only into the top-level compiled query is **insecure**. 
* The prefetch subqueries will run without security boundaries, allowing cross-tenant leaks.

### AST-level Solution
The AST injection module inside `qql-core::ast::transform` recursively modifies the AST:
```rust
// Recursively injects filters into QueryStmt and all nested CTE prefetches
pub fn inject_query_filter<'a>(q: &mut QueryStmt<'a>, field: &'a str, op: &'a str, value: &Value<'a>) {
    q.query_filter = merge_filters(q.query_filter.take(), build_filter(field, op, value.clone()));
    for cte in &mut q.ctes {
        inject_query_filter(&mut cte.stmt, field, op, value);
    }
}
```
Always use this recursive approach to mutate AST structures rather than post-compilation payload modifications.

---

## 4. Developer workflow

### Testing
To run the full suite of unit tests, use:
```bash
PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 cargo test --all-targets
```
*Note: `PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1` is required due to the presence of PyO3-linked modules in the workspace.*

### Formatting & Clippy
Always verify formatting and run code lints before committing changes:
```bash
cargo fmt --check
PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 cargo clippy --all-targets
```
If formatting fails, auto-apply corrections using:
```bash
cargo fmt
```
