# Changelog

All notable changes to the **QQL** project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

### 🏗️ Architecture
- **Transport-agnostic Plan IR**: `qql-plan` is now free of REST/gRPC client types. `PlannedOperation` is the single source of truth, lowered directly by every backend (`RestQdrant`, `GrpcQdrant`, `EdgeQdrant`). `to_rest_route()` is fallible and `compile_statement()` returns `CompiledStatement { stmt_type, route }` for reliable SDK metadata.
- **Single parser frontend**: Removed the pest runtime parser; `AstLowerer` is the sole production parser. `language/v1/grammar.pest` remains the canonical language contract, fed through `qql-grammar-gen` for docs and CI.
- **Single embedding owner**: Extracted all embedding logic into the new `qql-embed` crate (was spread across `qql-plan`). `Embedder` trait now covers dense, sparse, multi (ColBERT), image (CLIP), cross-encoder rerank, and joint (BGE-M3 single-pass) embeddings.
- **Universal batch key**: `statement_batch_key()` and `PlannedOperation::batch_key()` enable cross-crate smart batching — contiguous same-collection queries and mutations are grouped automatically.
- **Fail-closed injection**: `inject_shard_key` and `inject_filter` now reject unsupported statement types with clear errors instead of silently no-oping on DDL.

### 🔴 Breaking Changes
- **`USING name` is fail-closed**: `USING name` without `AS DENSE|SPARSE|MULTI` now requires schema resolution. When the vector kind is unknown (offline, no topology), it fails with `QQL-VECTOR-KIND` instead of silently defaulting to dense.
- **`route()` deprecated**: Use `try_route()` or `compile_statement()`. The old `route()` panics on client-side-only operations like `CROSS RERANK`.
- **Edge error hardening**: Previously-silent failures at the edge layer now produce explicit errors: non-UUID string point IDs (`QQL-EDGE-INVALID-POINT-ID`), empty dense query vectors, point-reference queries (`QQL-EDGE-UNSUPPORTED-POINT-REF`), `RECOMMEND STRATEGY average_vector`, and bare `CROSS RERANK` routes (`ClientSideOnly`).
- **`embed_sparse` model gate**: The default `embed_sparse` now rejects non-empty, non-`"default"` model names. Implement `embed_sparse(model)` on your embedder to support model-aware sparse routing (e.g. SPLADE, BGE-M3).
- **Auto-embed dense-only without topology**: UPSERT auto-embedding without explicit `USING` or topology now produces dense vectors only — no orphan sparse vectors are injected into dense-only collections.

### 🚀 Added

**QQL 1.2 Language** (additive — all v1.1 syntax remains valid):
- `DELETE PAYLOAD key1, key2 FROM collection WHERE filter` — targeted payload key deletion
- `USING HYBRID DENSE n SPARSE n FUSION RRF` — tail-form hybrid shorthand (expands to same AST as `QUERY HYBRID`)
- `CROSS RERANK TEXT 'q' MODEL 'x' ON FIELD f` — cross-encoder pair scoring (client-side)
- `QUERY IMAGE '/path.jpg' MODEL 'clip-vit'` — CLIP vision embedding input
- `USING name AS MULTI` / `AS MULTIVECTOR` — ColBERT late-interaction multivector target
- `RERANK TEXT 'q' MODEL 'r' USING colbert PREFETCH (c)` — late-interaction MaxSim rerank
- `PARAMS (acorn = true, max_selectivity = 0.5)` — ACORN search parameter
- `PARAMS (timeout = 5, consistency = 'majority')` — request-level timeout and read consistency
- `COUNT FROM coll WITH (exact = true)` — exact point counting
- `SHARD '<key>'` on all DML statements (COUNT, SCROLL, UPSERT, DELETE, CLEAR PAYLOAD, DELETE VECTOR, UPDATE … VECTOR/PAYLOAD) and `CREATE/DROP/SHOW SHARD KEY` DDL
- `SCROLL … WITH VECTOR [true|false|(names)]` — optional vector selector on scroll

**Vectors & Embeddings**:
- Schema-first `USING` resolution — the executor queries the collection topology to fill vector kinds before embedding, enabling `USING sparse` to work without explicit `AS SPARSE` annotation
- Multivector (ColBERT) pipeline: collection `MULTIVECTOR (comparator = max_sim)` config → `embed_multi` → `MultiDense` queries → `RERANK` late-interaction scoring
- Image (CLIP) pipeline: `QUERY IMAGE` / `UPSERT USING IMAGE` → `embed_image` → dense vector search
- Cross-encoder reranking: `CROSS RERANK` → `rerank_pairs()` → client-side pair scoring against prefetch candidates
- `embed_joint` / `JointEmbeddingOutput` for BGE-M3 single-pass dense+sparse+multi embedding
- Model-aware sparse embedding: `embed_sparse(text, model)` enables SPLADE/BGE-M3 sparse routing

**SDK & CLI**:
- `pyqql.Client.compile()` parity with nqql; all SDK `compile()` return stable `stmt_type` labels
- `inject_shard_key()` available on all SDKs (Python, Node, Rust, WASM) plus `Stmt.shard_key` getter/setter
- Edge SDKs: `nqql-edge` and `pyqql-edge` now support multi-model `FastEmbedder` (dense/sparse/multi/image/reranker ONNX slots), `localExecutor`/`httpExecutor` constructors, and `EdgeUnsupported` error catalog
- WASM: `Stmt` class, `analyze()` (parse+explain+route in one call), `compileBytes()`/`explainBytes()`, smart batching
- CLI: `qql doctor` (connection health + embedder snapshot), `qql config edge`, `--edge` flag for local execution, psql-style table output

**Query & Config**:
- `min_should` filter conjunction threshold, `FilterCompound.shard_key`, `QueryRequest.lookup_from`
- `HNSW.inline_storage` config, `stemmer` validation on text index creation
- `GROUP BY` with `OFFSET` (via `group_offset`), `MMR` with sparse vector targets
- Full REST/gRPC parity for all query variants, formula expressions, and geo/nested/match filters

### 🐛 Fixed
- gRPC dense `vector_params` now propagates OpenAPI `datatype` (uint8 / float16 / float32).
- CROSS RERANK no longer falls back to payload `text` when a different FIELD is requested.
- SDK `compile()` no longer mislabels DROP INDEX as `drop_collection` or SHOW SHARD KEYS as `show_collection`.
- gRPC mutation envelopes carry real server `time` from `PointsOperationResponse`.
- `PyStmt::to_dict` in pyqql uses `serde_json::to_value` before `pythonize` for full dictionary alignment with `to_json()`.
- CLI table mode renders CROSS_RERANK results; REST/edge reject bare CrossRerank routes.

### 📚 Documentation
- Rewritten skill references: expanded `qql-examples.md` (25 examples), new `qql-multitenancy.md`, new `qql-install.md`, updated all SDK references (Python, Node, Rust, WASM) with `inject_shard_key`, batch execution, and Stmt manipulation.
- All crate READMEs updated with accurate API tables and feature documentation.
- `language/v1` bumped to 1.2 with 3 new valid fixtures, 3 new AST snapshots, and updated semantics spec.

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
