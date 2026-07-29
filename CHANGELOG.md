# Changelog

All notable changes to the **QQL** project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

### 🏗️ Architecture
- **Transport-Independent Plan IR**: `qql-plan` is now 100% transport-agnostic and free of REST/gRPC client types. `PlannedOperation` is the single source of truth IR lowered directly by `qql-runtime` (`RestQdrant`, `GrpcQdrant`) and `qql-edge` (`EdgeQdrant`) without intermediate serialization round-trips.
- **Universal Batch Key**: Standardized `statement_batch_key(stmt)` and `PlannedOperation::batch_key(&self)` exported from `qql-plan` root for cross-crate request batching.
- **Model-Aware Sparse Embedder & Single-Pass Joint Embeddings**:
  - Updated `Embedder::embed_sparse` to accept `(text: &str, model: &str)`, enabling model-aware sparse model routing (`USING SPARSE MODEL 'splade'`).
  - Added `embed_joint` / `embed_joint_batch` and `JointEmbeddingOutput` struct to `Embedder` trait for BGE-M3 single-pass joint dense+sparse+multi embedding.
- **Single production parser**: remove pest/`syntax.rs` from `qql-core`; `AstLowerer` is the only runtime frontend. `language/v1/grammar.pest` remains the language contract for docs/CI (`qql-grammar-gen`).
- **Single embedding owner**: remove dead `qql-plan` embedding job extractor; embeddings live only in `qql-embed`.
- **Plan is the IR**: `to_rest_route` is fallible; CROSS RERANK no longer invents a fake Qdrant path. SDK `compile_statement` returns `CompiledStatement { stmt_type, route: Option<Route> }`.
- Deprecate silent `routing::route()` empty-GET fallback; prefer `try_route` / `compile_statement`.
- Remove dead `client::CollectionSchema` duplicate.
- `inject_shard_key` and `inject_filter` fail closed on unsupported statement types (no silent no-op for DDL).

### 🔴 Breaking Changes
- **String point IDs rejected at edge**: String-form point IDs in edge queries now fail with `QQL-EDGE-INVALID-POINT-ID` instead of being silently dropped.
- **`USING name` without vector kind is fail-closed**: `USING name` without explicit `AS DENSE|SPARSE|MULTI` annotation now fails with `QQL-VECTOR-KIND` when the kind cannot be resolved from schema.
- **`route()` deprecated**: The silent `routing::route()` empty-GET fallback is deprecated. Use `try_route()` or `compile_statement()` instead.
- **`embed_sparse` rejects non-default models**: Sparse embedding now requires explicit model routing; passing a non-default model without specification is rejected.
- **Auto-embed without topology produces dense-only**: Auto-embedding without explicit topology information now generates dense vectors only (no orphan sparse vectors). Hybrid remains schema-driven.
- **Deleted internal modules**: `syntax.rs` (qql-core) and `embedding.rs` (qql-plan) have been removed as part of internal refactors.
- **Empty dense query vectors rejected at edge**: Dense query vectors with zero dimensions are now rejected at the edge layer.
- **Point reference query rejected at edge**: Using `QUERY ... FROM collection` with point references is now rejected at edge with `QQL-EDGE-UNSUPPORTED-POINT-REF`.
- **`RECOMMEND STRATEGY average_vector` rejected at edge**: The `average_vector` recommendation strategy is no longer supported at the edge and is rejected.
- **`CROSS RERANK` routes return `ClientSideOnly`**: Cross-encoder reranking routes are flagged as `ClientSideOnly` — they must go through an executor and cannot be dispatched directly.

### 🚀 Added
- **QQL 1.2 Specification**: Bumped language version to `1.2` in `language/v1/spec/versioning.md` covering all additive syntax additions.
- **`DELETE PAYLOAD`**: Added syntax, AST, planner IR, REST route (`POST /collections/{c}/points/payload/delete`), gRPC (`DeletePayloadPoints`), and Edge support for deleting specific payload keys from targeted points (`DELETE PAYLOAD key1, key2 FROM collection WHERE ...`).
- **`min_should` Filter Conjunction Threshold**: Added `min_should` field to `FilterCompound` in `qql-plan`.
- **HNSW `inline_storage`**: Added `inline_storage` config field to `HnswConfig` in `qql-plan` and `qql-core`.
- **Text Index `stemmer`**: Added `stemmer` validation to `CREATE INDEX ... WITH (stemmer = 'english')`.
- **Full Qdrant Feature Coverage**:
  - **`FilterCompound.shard_key`**: Wired top-level and nested `shard_key` propagation into `FilterCompound` across query, count, scroll, and prefetch plans.
  - **`QueryRequest.lookup_from`**: Extracted lookup collection and vector specifications into `QueryRequest.lookup_from` (`LookupRequest`).
  - **`CountRequest.exact`**: Added `pub exact: Option<bool>` to `CountStmt` in `qql-core` and wired `CountRequest.exact` for exact point count queries (`COUNT FROM coll WITH (exact = true)`).
- **`ACORN` Search Parameter**: Added `acorn: { enable, max_selectivity }` to `SearchParams` AST and `qql-plan`.
- **`USING HYBRID` Syntax**: Added `USING HYBRID` shorthand syntax expanding to dense + sparse prefetch queries.
- **`CROSS RERANK` Syntax**: Added cross-encoder pair scoring with `CROSS RERANK` grammar parsing.
- **Image Embeddings (CLIP)**: Added `QueryInput::Image` parsing and vision embedding support.
- Multivector / ColBERT path: `USING name AS MULTI`, schema `multivector_config` → `MultiDense`, `Embedder::embed_multi`, `RERANK` multivector targets, `HYBRID RERANK` materializes `colbert` MaxSim vector.
- Schema-first `USING` resolution before embedding; fail-closed `QQL-VECTOR-KIND` when kind is unknown offline.
- `SHARD '<key>'` on CLEAR PAYLOAD, DELETE VECTOR, UPDATE … VECTOR, and UPDATE … PAYLOAD (AST + plan + REST/gRPC).
- `PlannedOperation::compile_stmt_type` / `routing::compile_statement` for reliable SDK `compile()` metadata.
- `pyqql.Client.compile()` parity with nqql; compile payloads include `stmt_type`.
- Plan-layer `PlanQueryInput::Image` (no longer silently rewritten as Document).

### 🐛 Fixed
- gRPC dense `vector_params` now propagates OpenAPI `datatype` (uint8 / float16 / float32).
- Auto-embed without topology adds dense only (no orphan sparse vectors); hybrid remains schema-driven.
- CROSS RERANK no longer falls back to payload `text` when a different FIELD is requested.
- SDK `compile()` no longer mislabels DROP INDEX as `drop_collection` or SHOW SHARD KEYS as `show_collection`.
- gRPC mutation envelopes carry real server `time` from `PointsOperationResponse`.
- Binding structure parity: `PyStmt::to_dict` in `pyqql` updated to use `serde_json::to_value` before `pythonize` for 100% dictionary alignment with `to_json()`.
- CLI table mode renders CROSS_RERANK results.
- REST/edge reject bare CrossRerank routes; client-side path is required.

### 📚 Documentation
- Update `docs/syntax.md`, skills (`SKILL.md`, examples, gaps, Python/Node/Rust/WASM SDKs), crate READMEs (`qql-core`, `qql-embed`, `qql-runtime`, `qql-plan`, `qql-cli`), `AGENT.md`, and `language/v1` notes for vector roles + multivector.
- qql-plan README: 22 PlannedOperation variants including CrossRerank.
- Clarify parser uses AstLowerer; pest grammar is language contract only, not the production lowerer.

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
