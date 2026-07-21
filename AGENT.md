# QQL Agent & Developer Reference Guide

Welcome to the QQL Rust codebase. This guide details the architecture, design philosophy, contract testing standards, and key implementation guidelines for developers and AI coding agents.

---

## 1. Workspace Architecture

The workspace is organized into a modular, multi-crate Rust workspace under the `crates/` directory:

```
qql/ (workspace root)
├── crates/
│   ├── qql-core/         # Lexer, parser, typed AST, semantic validation, explain, filter injection
│   ├── qql-plan/         # Typed operation lowering: AST → Route { method, path, body }
│   ├── qql-runtime/      # Executor (package name `qql`), REST & gRPC adapters, embedding resolution
│   ├── qql-edge/         # Local executor: fastembed-rs + qdrant-edge (zero-network)
│   ├── qql-cli/          # CLI binary and interactive REPL
│   ├── pyqql/            # Python bindings (PyO3)
│   ├── nqql/             # Node.js bindings (N-API)
│   └── qql-wasm/         # WebAssembly bindings (wasm-bindgen)
```

### Three-Layer Architecture

```
qql-core (parse → typed AST → explain → inject_filter)
    ↓
qql-plan (AST → typed RequestBody enum → Route { method, path, body })
    ↓
qql-runtime (resolve_embeddings → execute_route() via REST reqwest or gRPC tonic)
```

No JSON-as-IR. No duplicate planning. No compatibility shims. No custom serde renames.

### Crate Division Boundaries

* **`qql-core`**: The parser, lexer, typed AST (`QueryExpr` enum, `FilterExpr`, `ComparisonOp`, etc.), AST transforms (`inject_filter`), and explain formatting. Performs NO network or file I/O. Has NO knowledge of Qdrant endpoints, REST JSON shapes, or transport protocols. Features: `default = []`, `serde`, `json`, `std`. Uses owned `String` types throughout — no lifetime parameters on input.

* **`qql-plan`**: Transport-neutral lowering layer. Converts AST `Stmt` into a `Route { method: Method, path: String, body: Option<RequestBody> }`. All field names match the OpenAPI wire format — no `serde(rename)`. Contains typed filter, query, mutation, DDL, and embedding types. Depends ONLY on `qql-core`. No networking, no tokio, no reqwest.

* **`qql-runtime`**: The executor and transport adapters. Package name is `qql`. The `Executor` holds a `Box<dyn QdrantOps>` (single unified trait) and optional `Embedder`. Handles automated text-to-vector embedding resolution (`resolve_embeddings`) before delegating DML operations to `qql_plan::routing::route()` → `client.execute_route()`. DDL operations call dedicated admin methods on `QdrantOps`. Features: `default = ["grpc", "rest"]`, `grpc`, `rest`.

* **`qql-edge`**: In-process vector search using qdrant-edge + optional fastembed-rs. Zero network. Implements `QdrantOps` with `execute_route()` dispatching to `EdgeShard` operations. Uses `qdrant-edge` 0.7.x.

* **`qql-cli`**: CLI binary. Uses the executor via REST/adapter construction. Dump uses parser + `route()` + `execute_route()`.

* **Foreign Bindings**: PyO3 (`pyqql`), N-API (`nqql`), Wasm-bindgen (`qql-wasm`). Expose parser, tokenization, filter injection, explain, `compile_query` (via `routing::route()`), and first-class `Client` classes. Keep public class names (`Client`, `HttpEmbedder`, `Stmt`), return shapes, and error mappings aligned.

### Permanently Removed Abstractions

The following old abstractions have been permanently removed — do NOT reintroduce them:

- `offline.rs` / `CompiledQuery` — replaced by `qql_plan::routing::route()`
- `filter_conv/` — replaced by `qql_plan::filter::lower_filter()`
- `pipeline/` module — replaced by `qql_plan::types`
- `QdrantCoreOps` / `QdrantAdminOps` dual-trait — merged into single `QdrantOps`
- `QueryMode`, `QueryType`, `SearchWith`, `SelectStmt` — replaced by `QueryExpr` enum (12 variants)
- `qdrant-client` crate dependency — replaced by raw `tonic` 0.14
- `SELECT` / `INSERT INTO` keywords — replaced by `QUERY POINTS` / `UPSERT INTO`
- String filter operators (`"="`, `">"`, etc.) — replaced by `ComparisonOp` enum
- `Token::pos` — replaced by `Token::span: Span { start, end }`
- `QqlError::runtime()` — replaced by `QqlError::execution(code, message, span)`
- `QqlError::syntax()` — replaced by `QqlError::parse(code, message, span)`

### Current QueryExpr Variants (12 total)

```
Points, Nearest, Recommend, Context, Discover, OrderBy,
SampleRandom, Fusion, Formula, RelevanceFeedback, Hybrid, Rerank
```

### Error Model

```rust
pub enum ErrorKind { Lex, Parse, Validation, Execution, Transport, Backend }
pub struct QqlError { kind: ErrorKind, code: &'static str, message: String, span: Option<Span> }
pub struct Span { start: usize, end: usize }
```

Error kind is explicit — never inferred from position. No `runtime` constructor.

### QdrantOps Trait

```rust
pub trait QdrantOps: Send + Sync {
    async fn list_collections(&self) -> Result<Vec<String>, QqlError>;
    async fn collection_exists(&self, name: &str) -> Result<bool, QqlError>;
    async fn get_collection_info(&self, name: &str) -> Result<CollectionInfo, QqlError>;
    async fn create_collection(&self, req: CreateCollectionReq) -> Result<(), QqlError>;
    async fn update_collection(&self, req: serde_json::Value) -> Result<(), QqlError>;
    async fn delete_collection(&self, name: &str) -> Result<(), QqlError>;
    async fn create_field_index(&self, req: CreateFieldIndexReq) -> Result<(), QqlError>;
    async fn execute_route(&self, route: Route) -> Result<serde_json::Value, QqlError>;
}
```

A single trait with 8 methods. All DML flows through `execute_route()`. DDL uses dedicated admin methods. Three implementations: `RestQdrant`, `GrpcQdrant`, `EdgeQdrant`.

### Statement → Endpoint Matrix (14 routes)

| QQL Statement | Endpoint | Method |
|---|---|---|
| `QUERY ...` (search) | `/points/query` | POST |
| `QUERY ... GROUP BY` | `/points/query/groups` | POST |
| `QUERY POINTS (ids)` | `/points` | POST |
| `SCROLL ...` | `/points/scroll` | POST |
| `UPSERT ...` | `/points` | PUT |
| `DELETE ...` | `/points/delete` | POST |
| `UPDATE ... VECTOR` | `/points/vectors` | PUT |
| `UPDATE ... PAYLOAD` | `/points/payload` | POST |
| `CREATE COLLECTION` | `/collections/{c}` | PUT |
| `ALTER COLLECTION` | `/collections/{c}` | PATCH |
| `DROP COLLECTION` | `/collections/{c}` | DELETE |
| `CREATE INDEX` | `/collections/{c}/index` | PUT |
| `SHOW COLLECTIONS` | `/collections` | GET |
| `SHOW COLLECTION` | `/collections/{c}` | GET |

### gRPC Stack

- `qdrant-client` dropped entirely — replaced with `tonic` 0.14 + `tonic-prost` + `tonic-prost-build`
- Proto files in `proto/`, compiled at build time via `tonic-prost-build`
- `GrpcQdrant` wraps `tonic::Channel` with `connect_lazy`
- `grpc_route.rs` converts qql-plan typed structs → generated protobuf types directly (no JSON intermediary)
- `grpc.rs` is a thin ~290-line wrapper; heavy conversion lives in `grpc_route.rs` (~1,570 lines)
- Tonic features: `channel`, `codegen`, `tls-ring`, `tls-webpki-roots` (no server, no axum, no router)

### Serialization Policy

- `qql-core`: Serde optional (`default = []`, features `serde` and `json` separately). Parser-only consumers pay for nothing.
- `qql-plan`: Always depends on serde/serde_json — builds JSON wire bodies matching OpenAPI format exactly.
- `qql-runtime`: Uses serde/serde_json in REST adapter. gRPC adapter uses typed protobuf conversion.
- Bindings: All enable `qql-core/serde` + `qql-core/json` for AST serialization and `Value::from_json()`.

---

## 2. OpenAPI Schema Contract Testing

All generated route payloads are validated directly against Qdrant's official [`openapi.json`](file:///data/codebases/qql-rs/openapi.json) specification in `crates/qql-runtime/src/contract_test.rs`:

1. **`Query` Schema Validation**: All 11 query expression variants are validated against `# /components/schemas/Query`.
2. **`Filter` Schema Validation**: All 17 filter expression variants (`Compare`, `Between`, `In`, `MatchText`, `MatchPhrase`, `MatchAny`, `IsNull`, `IsEmpty`, `HasVector`, `ValuesCount`, `Nested`, `GeoBoundingBox`, `GeoRadius`, `PointId`, and compound logic) are validated against `# /components/schemas/Filter`.
3. **`PointRequest` & `ScrollRequest` Validation**: Validated against `# /components/schemas/PointRequest` and `# /components/schemas/ScrollRequest`.

---

## 3. Minimalist Code Design Philosophy

1. **Size Constraints**: Target <400 lines per file where possible. Split large files into modules.
2. **Error Propagation**: Dispatch directly; bubble up downstream errors. No pre-emptive checks.
3. **No JSON-as-IR**: `RequestBody` is typed. JSON only at the REST boundary.
4. **No duplicate planners**: `qql_plan::routing::route()` is the single source of truth for statement → HTTP mapping.
5. **No glue code**: Each layer has one responsibility. No wrappers around wrappers.

---

## 4. AST Query Transformation & Filter Injection

```rust
pub fn inject_filter(
    statement: &mut Stmt,
    field: &str,
    operator: ComparisonOp,   // typed enum (Eq, Gt, Gte, Lt, Lte)
    value: Value,             // owned, no lifetime
) -> Result<(), QqlError>
```

Recursively injects into QueryStmt, all CTEs, and Scroll. Callers must convert their string operators before calling.

---

## 5. Grammar and Runtime Invariants

* Parsing is strict: malformed clauses return `QqlError::Parse`, never silently keep defaults.
* `Span { start, end }` uses byte offsets. `Token::pos` is `pub(crate)`; public API uses `span`.
* Script splitting requires semicolons between statements. `parse_all()` rejects adjacent unseparated statements.
* `SELECT` is rejected as an unrecognized statement. Use `QUERY POINTS` for point retrieval.
* Duplicate object keys, config keys, CTE names, and query clauses are rejected.
* `QqlError` always carries an explicit `ErrorKind` and `Span`.

---

## 6. Host Language SDK Reference Manuals

Dedicated reference guides for each host SDK live under `skills/qql-skill/references/`:

- **[`qql-examples.md`](file:///data/codebases/qql-rs/skills/qql-skill/references/qql-examples.md)**: Pure QQL query examples (` ```sql ` code blocks strictly).
- **[`python-sdk.md`](file:///data/codebases/qql-rs/skills/qql-skill/references/python-sdk.md)**: Python `pyqql` PyO3 client and AST functions.
- **[`node-sdk.md`](file:///data/codebases/qql-rs/skills/qql-skill/references/node-sdk.md)**: Node.js `nqql` N-API client and `parseFastJson` usage.
- **[`wasm-sdk.md`](file:///data/codebases/qql-rs/skills/qql-skill/references/wasm-sdk.md)**: WebAssembly `qql-wasm` browser & edge client.
- **[`rust-sdk.md`](file:///data/codebases/qql-rs/skills/qql-skill/references/rust-sdk.md)**: Native Rust `qql` runtime & `qql-core` SDK reference.

---

## 7. Developer Workflow

### Testing
```bash
PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 cargo test --workspace --all-targets
```

### Formatting & Clippy
```bash
cargo fmt --check
PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 cargo check --workspace --all-targets
PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 cargo clippy --workspace --all-targets -- -D warnings
```

### Token Definition Hygiene
When adding a new keyword token to `token.rs`:
1. Add the variant to `pub enum TokenKind`.
2. Add a `Variant => "STRING"` entry to `gen_as_str!`.
3. Add a `"STRING" => TokenKind::Variant` entry to `gen_keywords!`.

### Workspace Hygiene
* Keep workspace version in root `Cargo.toml` as single source of truth.
* Minimize dependency surface. Check unused deps with `cargo +nightly udeps`.
* Inspect `git status` before making changes; don't overwrite unrelated work.
