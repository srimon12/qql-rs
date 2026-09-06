# Workspace Rust Guidelines & Agent Reference (qql-rs)

This workspace is a high-performance, modular Rust engine containing multiple crates: core execution, parsing, query planning, embeddings, CLI, WASM, Python FFI (`pyqql`), and Node.js FFI (`nqql`).

This guide details the core rules, architecture, design philosophy, contract testing standards, and developer workflows for contributors and AI coding agents.

---

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

---

## 1. Workspace Architecture

The workspace is organized into a modular, multi-crate Rust workspace under the `crates/` directory:

```
qql/ (workspace root)
├── crates/
│   ├── qql-core/         # Lexer, parser, typed AST, explain, filter injection
│   ├── qql-plan/         # Fallible planner: AST → PlannedOperation; REST projection
│   ├── qql-embed/        # Shared Embedder trait, wire-compatible BM25, resolve_embeddings (batch dense)
│   ├── qql-runtime/      # Executor (package name `qql`), REST & gRPC adapters, HttpEmbedder
│   ├── qql-edge/         # Local in-process executor: fastembed-rs + qdrant-edge
│   ├── qql-cli/          # CLI binary and interactive REPL
│   ├── pyqql/            # Python bindings (PyO3)
│   ├── nqql/             # Node.js bindings (N-API)
│   └── qql-wasm/         # WebAssembly bindings (wasm-bindgen)
```

### Execution Pipeline

```
source / host AST
    │
    ▼
qql-core: parse + semantic AST validation
    │
    ▼
qql-runtime: prepare_statement
  - named-vector validation + kind/multi from collection schema
  - embedding resolution (qql-embed): Dense | Sparse | MultiDense
  - upsert collection prep
    │
    ▼
qql-plan: plan() → Result<PlannedOperation, PlanError>
    │
    ├── batch classification (BatchFamily::Query | Mutation | Single)
    │     └── contiguous same-collection ops → execute_query_batch / execute_update_batch
    │
    ├── individual dispatch → to_rest_route() → Route → client.execute_route()
    │                                                  └── REST: serialized JSON
    │                                                  └── gRPC: execute_grpc_route() typed protobuf conversion
    │
    └── response normalization (ExecResponse)
```

Canonical plan is `PlannedOperation` (transport-neutral). `Route { method, path, query, body }` is the **REST projection** of a plan, not the source of truth. Semantic types (`PlanQueryInput`, `PlanPointId`, `PlanVectorValue`) remain typed until a transport boundary. gRPC converts typed plan structs directly to protobuf via `to_query_points`, `to_vector_input`, `plan_vector_to_proto`, etc. — no JSON intermediary for query vectors or point IDs. Formula lowering still emits `serde_json::Value` (lower_formula_expr → to_formula_expression round-trips through JSON).

### Crate Division Boundaries

* **`qql-core`**: The parser, lexer, typed AST (`QueryExpr` enum, `FilterExpr`, `ComparisonOp`, etc.), AST transforms (`inject_filter`), and explain formatting. Performs NO network or file I/O. Has NO knowledge of Qdrant endpoints, REST JSON shapes, or transport protocols. Features: `default = []`, `serde`, `json`, `std`. Uses owned `String` types throughout — no lifetime parameters on input.

* **`qql-plan`**: Transport-neutral lowering layer. Contains the fallible planner `plan()` returning `PlannedOperation`, typed filter/query/mutation/DDL types (`PlanPointId`, `PlanVectorValue`, `PlanQueryInput`), and `to_rest_route()` for the **optional** REST projection. Also provides `BatchKey` + `statement_batch_key()` + `PlannedOperation::batch_key()` for executor sharing (Rust + WASM). `Route` and `RequestBody` are REST-specific. Depends ONLY on `qql-core`. No networking, no tokio, no reqwest.

* **`qql-embed`**: Shared embedding layer. `Embedder` trait (`embed_dense` / `embed_sparse_query` / `embed_sparse_document` / `embed_multi`), local wire-compatible BM25 (murmur3-32 token IDs, word tokenizer, English stopwords + snowball stemming — byte-identical to Qdrant's `qdrant/bm25`; queries embed unit weights, documents tf saturation), `resolve_query_vector_kinds` (schema topology → dense/sparse/multi flags), and `resolve_embeddings` (TEXT → Dense | Sparse | MultiDense). Unknown `USING` kinds fail closed (`QQL-VECTOR-KIND`). No Qdrant I/O. Used by runtime (`HttpEmbedder`), edge (`FastEmbedder`), and wasm (fetch/JS adapters).

* **`qql-runtime`**: The executor and transport adapters. Package name is `qql`. The `Executor` holds a `Box<dyn QdrantOps>` (single unified trait with 11 methods) and optional `Embedder`. Calls `prepare_statement` (**schema vector resolution first**, then embeddings, then upsert schema prep) → `plan()` → batch classification / dispatch. DDL flows through `plan()` → REST projection → `execute_route()` or `execute_grpc_route()`. Features: `default = ["grpc", "rest"]`, `grpc`, `rest`. Re-exports embed API via `qql::embedder` / `qql::sparse`.

* **`qql-edge`**: In-process vector search using qdrant-edge + optional fastembed-rs. Zero network. Implements `QdrantOps` with batch methods fanning out to individual routes (no native edge batch RPC). Uses `qdrant-edge` 0.7.x.

* **`qql-cli`**: CLI binary. Uses the executor via REST/adapter construction.

* **Foreign Bindings**: PyO3 (`pyqql`), N-API (`nqql`), Wasm-bindgen (`qql-wasm`). Expose parser, tokenization, filter injection, explain, `compile_query` (via `qql_plan::routing::compile_statement`), and `Client` classes. Keep public class names (`Client`, `HttpEmbedder`, `Stmt`), return shapes, and error mappings aligned.

### Permanently Removed Abstractions

The following old abstractions have been permanently removed — do NOT reintroduce them:

- `offline.rs` / `CompiledQuery` — replaced by `qql_plan::plan::plan()` + `PlannedOperation`
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
- `executor/ddl.rs` — DDL now flows through `qql_plan::plan` → REST projection / gRPC route
- `CompiledQuery` / `offline.rs` — eliminated; the deprecated panicking `routing::route()` wrapper is removed — `routing::try_route()` (fallible) and `compile_statement` supersede it around `plan()` + `to_rest_route()`
- `parser/syntax.rs` (pest grammar runtime) — removed from production runtime; pest survives only as a test-only harness in `qql-conformance` (dev-dependency that compiles `language/v1/grammar.pest` and gates the fixture corpus), never in `qql-core`. `qql-grammar-gen` instead derives keyword tables, TextMate/TS artifacts, and the generated pest copy from `grammar.pest`
- `qql-plan/src/embedding.rs` (embedding job extraction) — removed; embeddings are solely owned by `qql-embed`
- `CollectionSchema` (client.rs) — removed duplicate; backend schema is the only source

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

### QdrantOps Trait (11 methods)

```rust
pub trait QdrantOps: Send + Sync {
    // DDL / metadata
    async fn list_collections(&self) -> Result<Vec<String>, QqlError>;
    async fn collection_exists(&self, name: &str) -> Result<bool, QqlError>;
    async fn get_collection_info(&self, name: &str) -> Result<CollectionInfo, QqlError>;
    async fn create_collection(&self, collection_name: &str, req: &qql_plan::CreateCollectionRequest) -> Result<(), QqlError>;
    async fn update_collection(&self, collection_name: &str, req: &qql_plan::UpdateCollectionRequest) -> Result<(), QqlError>;
    async fn delete_collection(&self, name: &str) -> Result<(), QqlError>;
    async fn create_field_index(&self, collection_name: &str, req: &qql_plan::CreateIndexRequest) -> Result<(), QqlError>;
    async fn delete_field_index(&self, collection_name: &str, field_name: &str) -> Result<(), QqlError>;

    // Execution via PlannedOperation IR
    async fn execute_planned(&self, op: &qql_plan::PlannedOperation) -> Result<serde_json::Value, QqlError>;

    // Batch methods
    async fn execute_query_batch(&self, collection: &str, batch: &QueryBatchRequest) -> Result<Vec<serde_json::Value>, QqlError>;
    async fn execute_update_batch(&self, collection: &str, batch: &UpdateBatchRequest) -> Result<Vec<serde_json::Value>, QqlError>;
}
```

Three implementations: `RestQdrant`, `GrpcQdrant`, `EdgeQdrant`. The gRPC adapter bypasses `execute_route` for DML — it uses `execute_grpc_route()` which converts typed `RequestBody` variants directly to protobuf. For REST, `execute_route` serializes `RequestBody` as JSON.

### Statement → Endpoint Matrix (21 routes)

| QQL Statement | Endpoint | Method |
|---|---|---|
| `QUERY ...` (search) | `/points/query` | POST |
| `QUERY ... GROUP BY` | `/points/query/groups` | POST |
| `QUERY POINTS (ids)` | `/points` | POST |
| `FACET ...` | `/points/facet` | POST |
| `SCROLL ...` | `/points/scroll` | POST |
| `COUNT ...` | `/points/count` | POST |
| `UPSERT ...` | `/points` | PUT |
| `DELETE ...` | `/points/delete` | POST |
| `CLEAR PAYLOAD ...` | `/points/payload/clear` | POST |
| `DELETE VECTOR ...` | `/points/vectors/delete` | POST |
| `UPDATE ... VECTOR` | `/points/vectors` | PUT |
| `UPDATE ... PAYLOAD` | `/points/payload` | POST |
| `CREATE COLLECTION` | `/collections/{c}` | PUT |
| `ALTER COLLECTION` | `/collections/{c}` | PATCH |
| `DROP COLLECTION` | `/collections/{c}` | DELETE |
| `CREATE INDEX` | `/collections/{c}/index` | PUT |
| `DROP INDEX` | `/collections/{c}/index/{field}` | DELETE |
| `SHOW COLLECTIONS` | `/collections` | GET |
| `SHOW COLLECTION` | `/collections/{c}` | GET |
| `SHOW QUOTAS` | `/quotas` | GET |
| `SET QUOTA` | `/quotas` | PUT |

### gRPC Stack

- `qdrant-client` dropped entirely — replaced with `tonic` 0.14 + `tonic-prost` + `tonic-prost-build`
- Proto files in `proto/`, compiled at build time via `tonic-prost-build`
- `GrpcQdrant` wraps `tonic::Channel` with `connect_lazy`
- `grpc_route.rs` converts typed qql-plan structs → generated protobuf types directly for query vectors, point IDs, and vector values. DDL sub-configs still read from `serde_json::Value` fields (hnsw_config, optimizers_config, quantization_config). Formula expressions still round-trip through JSON via `lower_formula_expr` → `to_formula_expression`.
- `grpc.rs` is the thin Tonic client wrapper; heavy conversion lives in `grpc_route.rs`
- Tonic features: `channel`, `codegen`, `tls-ring`, `tls-webpki-roots` (no server, no axum, no router)
- API key support via `ApiKeyInterceptor` (RUN-009 fixed)
- DDL routes (CreateCollection, UpdateCollection, CreateIndex, DropIndex, DeleteCollection, shard operations) all handled in `execute_grpc_route`

### Serialization Policy

- `qql-core`: Serde optional (`default = []`, features `serde` and `json` separately). Parser-only consumers pay for nothing.
- `qql-plan`: Always depends on serde/serde_json — builds JSON wire bodies matching OpenAPI format exactly. Typed semantic primitives (`PlanPointId`, `PlanVectorValue`, `PlanQueryInput`) implement `Serialize` directly.
- `qql-runtime`: Uses serde/serde_json in REST adapter. gRPC adapter uses typed protobuf conversion.
- Bindings: All enable `qql-core/serde` + `qql-core/json` for AST serialization and `Value::from_json()`.

---

## 2. OpenAPI Schema Contract Testing

All generated route payloads are validated directly against Qdrant's official
`crates/qql-runtime/openapi.json` specification in
`crates/qql-runtime/src/contract_test.rs`:

1. **`Query` Schema Validation**: All 12 query expression variants are validated against `# /components/schemas/Query`.
2. **`Filter` Schema Validation**: All 17 filter expression variants are validated against `# /components/schemas/Filter`.
3. **`PointRequest` & `ScrollRequest` Validation**: Validated against `# /components/schemas/PointRequest` and `# /components/schemas/ScrollRequest`.

---

## 3. Minimalist Code Design Philosophy

1. **Size Constraints**: Target <400 lines per file where possible. Split large files into modules.
2. **Error Propagation**: Dispatch directly; bubble up downstream errors. No pre-emptive checks.
3. **No JSON-as-IR**: `RequestBody` is typed. JSON only at the REST boundary, except for DDL sub-configs and formula expressions which still use JSON within gRPC conversion.
4. **No duplicate planners**: `qql_plan::plan::plan()` is the single fallible planner. `routing::try_route()` is the fallible REST projection; the deprecated `route()` wrapper is removed. DDL goes through the same planner.
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

Recursively injects into QueryStmt (including all CTEs and prefetches), Scroll, Count, Delete,
UpdatePayload, and Upsert (when `operator == Eq` and `field != "id"`, injects into point
payloads). Callers must convert their string operators before calling.

**Fail-closed**: Returns a validation error (`QQL-VALIDATION-FILTER-INJECT`) for unsupported
statement types (DDL, SHOW, UpdateVector, non-Eq Upsert). Unlike earlier versions that
silently no-oped, this prevents accidental policy bypass.

---

## 5. Grammar and Runtime Invariants

* Parsing is strict: malformed clauses return `QqlError::Parse`, never silently keep defaults.
* `Span { start, end }` uses byte offsets. `Token::pos` is `pub(crate)`; public API uses `span`.
* Script splitting requires semicolons between statements. `parse_all()` rejects adjacent unseparated statements.
* `SELECT` is rejected as an unrecognized statement. Use `QUERY POINTS` for point retrieval.
* Duplicate object keys, config keys, CTE names, and query clauses are rejected.
* `QqlError` always carries an explicit `ErrorKind` and `Span`.
* `SHARD '<key>'` routing is supported on QUERY, COUNT, UPSERT, SCROLL, and DELETE for custom-sharded collections.
* Collection creation supports `shard_number`, `sharding_method`, and `shard_keys` via `WITH PARAMS`.
* Payload indexes support `is_tenant = true` for Qdrant-native tenant optimization.

---

## 6. Host Language SDK Reference Manuals

Dedicated reference guides for each host SDK live under `skills/qql-skill/references/`:

- **[`qql-examples.md`](file:///data/codebases/qql-rs/skills/qql-skill/references/qql-examples.md)**: Pure QQL query examples (` ```sql ` code blocks strictly).
- **[`python-sdk.md`](file:///data/codebases/qql-rs/skills/qql-skill/references/python-sdk.md)**: Python `pyqql` PyO3 client and AST functions.
- **[`node-sdk.md`](file:///data/codebases/qql-rs/skills/qql-skill/references/node-sdk.md)**: Node.js `nqql` N-API client and `parseJson` usage.
- **[`wasm-sdk.md`](file:///data/codebases/qql-rs/skills/qql-skill/references/wasm-sdk.md)**: WebAssembly `qql-wasm` browser & edge client.
- **[`rust-sdk.md`](file:///data/codebases/qql-rs/skills/qql-skill/references/rust-sdk.md)**: Native Rust `qql` runtime & `qql-core` SDK reference.
- **[`qql-multitenancy.md`](file:///data/codebases/qql-rs/skills/qql-skill/references/qql-multitenancy.md)**: Complete multi-tenant guide: shard routing, filter injection, `is_tenant` indexing.

---

## 7. Developer Workflow

### Testing
```bash
cargo test --workspace --all-targets
```

### Formatting & Clippy
```bash
cargo fmt --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
```

### Known Workspace Blockers
- `qql-wasm` builds only for the `wasm32-unknown-unknown` target (wasm-bindgen); CI builds it with wasm-pack on every run (`editor-check` job), including the `#[async_trait(?Send)]` WASM Embedder impl, which is cfg-gated against the host `+ Send` bound in `qql-embed`.
- `qql-edge`: Requires fastembed-rs with specific native dependencies.

### Token Definition Hygiene
When adding a new keyword token to `token.rs`:
1. Add the variant to `pub enum TokenKind`.
2. Add a `Variant => "STRING"` entry to `gen_as_str!`.
3. Add a `"STRING" => TokenKind::Variant` entry to `gen_keywords!`.

### Workspace Hygiene
* Keep workspace version in root `Cargo.toml` as single source of truth.
* Minimize dependency surface. Check unused deps with `cargo +nightly udeps`.
* Inspect `git status` before making changes; don't overwrite unrelated work.
