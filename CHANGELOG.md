# Changelog

All notable changes to the **QQL** project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

### 🔄 Changed
- **QQL 1.5 IDF corpus** — `PARAMS (idf = …)` takes `'global'` or a QQL `WHERE` filter (`idf = WHERE tenant_id = 'acme'`). The AST stores `Option<FilterExpr>`. The Qdrant JSON `{corpus: {must: […]}}` form is removed (`QQL-VALIDATION-IDF`). Isolation remains `WHERE` / `inject_filter`; routing remains `SHARD`; IDF only scopes sparse term statistics.

## [0.2.1] - 2026-08-22

### 📦 Packaging
- **Workspace 0.2.1** — all crates, `pyqql` / `pyqql-edge` (PyPI), `@veristamp/nqql` / `@veristamp/nqql-edge` + platform packages (npm), and `qql-wasm` move to **0.2.1** together.
- **VS Code extension 0.2.4** — Marketplace packaging fix. The `0.2.3` upload was built from a `--target bundler` WASM bundle whose ESM entry imports the `.wasm` binary directly, which fails to load in the extension host (`ERR_UNKNOWN_FILE_EXTENSION`, Node ≤ 22) and breaks diagnostics, completions, and formatting. `0.2.4` ships the correct CommonJS bundle (`--target nodejs`, synchronous init); anyone who installed `0.2.3` should update. The extension version stays independent of the workspace version.

### 🚀 Added
- **Canonical QQL formatter (`qql fmt`)** — a new `qql-core::fmt` AST-based pretty-printer normalizes QQL source (clause order, keyword casing, string escaping, whitespace) and always re-parses to an identical AST. Exposed as `qql fmt [FILE] [--check] [--write]` in the CLI, `formatQuery()` in `qql-wasm`, and a **Format Document** provider in the VS Code extension. Round-trip + idempotence are guaranteed by property tests over the full conformance fixture corpus.
- **Qdrant 1.19.0 language surface** — end-to-end wiring for the six body/API features in the 1.19 release:
  - `SHOW QUOTAS` / `SET QUOTA (…) [WAIT bool]` → `GET|PUT /quotas` (REST only; gRPC and edge fail-loud)
  - `memory = 'cold'|'cached'|'pinned'` on HNSW / VECTOR / SPARSE / QUANTIZATION / indexes, plus `payload_memory` in `PARAMS` (payload rejects `pinned`)
  - `WHERE field MATCH PREFIX '…'` and `WHERE SLICE (total, index)`
  - `PARAMS (idf = 'global' | {corpus: …})` for per-query sparse IDF corpora
  - Keyword index `prefix = true` and dense `datatype = 'turbo4'` (TurboQuant 4-bit)
- **Typed placement / datatype enums** — `MemoryPlacement` and `VectorDatatype` in `qql-core` (parse once, serialize as OpenAPI lowercase strings).
- **Read affinity transport support** — `RestQdrant::with_route_affinity` / `GrpcQdrant::with_route_affinity` send `X-Qdrant-Route-Affinity` (HTTP header / gRPC metadata). This is transport metadata, not a request-body field, so it is not expressible via openapi/proto schemas.
- **Route affinity on host SDKs** — `pyqql.Client(route_affinity=…)` (readable via `client.route_affinity`), `nqql` `new Client({ routeAffinity })` (readable via `client.routeAffinity`), and the WASM `client.setRouteAffinity(key)` / `client.routeAffinity` getter. Applies `X-Qdrant-Route-Affinity` on REST and `x-qdrant-route-affinity` metadata on gRPC; edge remains single-node (no affinity). Includes `pyqql.parse_json` for parity with `nqql.parseJson` / `pyqql-edge.parse_json`.
- **qdrant-edge 0.8.0** — retrieve API, optional `score_threshold`, IDF on search params, fail-loud quotas.
- **Upstream Qdrant API sync** — new `scripts/sync_qdrant_api.py` cross-checks the vendored `openapi.json` and protos against upstream Qdrant at an immutable pinned commit (`scripts/qdrant-api-manifest.json`, currently `v1.19.0`). The umbrella `qdrant.proto` is verified as its derived internal-import-free variant; `quota_internal.proto` is now vendored for reference (messages only — upstream serves quota usage via the internal cluster service on the peer port, with no setter RPC, so REST remains the only quota transport). Enforced by a CI `Qdrant API sync` job.

### 🔄 Changed
- **Language version 1.4** — additive contract for quotas, memory placement, `MATCH PREFIX`, `SLICE`, per-query `idf`, keyword `prefix`, and dense `turbo4`. Spec, conformance counts (38 / 261 / 53 / 38), website, crate READMEs, skills, and VS Code **0.2.2** (snippets + description) updated together.
- **Bundled editor WASM rebuilt** — `editors/vscode/wasm/` now matches the current `qql-wasm` crate: the QQL 1.4 parse surface (`QUOTA`/`PREFIX`/`SLICE`/`turbo4`/`payload_memory`) and the route-affinity API are live in extension diagnostics, completions, and formatting. The bundle is now built with wasm-pack's **`nodejs`** target (CommonJS entry, synchronous init) so it loads under the extension host's plain `require()` on all supported Node versions — a `bundler`-target rebuild is no longer loadable there. The bundle directory tracks an explicit file whitelist; CI gains an `editor-check` job that fails when the committed bundle's export surface lags a fresh wasm-pack build.
- **Release tooling covers the extension** — `RELEASING.md` documents the language **1.4** spec version, the editor WASM refresh step, and VSIX packaging; `scripts/check_release.py` validates `editors/vscode/package.json` and requires the bundled WASM to expose current exports.
- **Qdrant 1.19.0 protocol pin** — `openapi.json` and public gRPC protos under `crates/qql-runtime/proto/` updated to 1.19.0. Only public services are compiled (internal raft/telemetry/quota protos are not vendored). Legacy `/points/search`, `/recommend`, and `/discover` REST endpoints were removed upstream; the runtime already used unified `/points/query`.
- **`SET QUOTA` is a full replace** — `PUT /quotas` replaces the whole config; omitted keys (including `key = null`) are unset in the replacement body, not a merge of the previous limits.
- **Fallible IDF corpus lowering** — malformed `idf.corpus` objects return `QQL-PLAN-IDF` instead of panicking in the planner.

### 🐛 Fixed
- **SDK declaration drift** — `pyqql_edge.Client.compile()` added to the `.pyi` stubs; the duplicated `compileBytes` / `explainBytes` / `formatQuery` declarations removed from the qql-wasm custom TS section (wasm-bindgen already emits them); `nqql-edge`'s non-constructible `Client` documented in `index.d.ts`.
- **Wrapper option validation** — `nqql.executeStmt` and the nqql-edge standalone `execute`/`executeStmt` paths now reject invalid `onError` values instead of silently ignoring them.
- **`pyqql_edge.Stmt.to_dict` shape** — now round-trips through serde before pythonizing, matching `to_json()` and `pyqql.Stmt.to_dict`.
- **Error-code reference** — renamed the phantom `QQL-VALIDATION-FEEDBACK-STRATEGY` to the emitted `QQL-PARSE-FEEDBACK-STRATEGY`, added the missing lexical/parse codes (`QQL-LEX-NUMBER`, `QQL-PARSE-COUNT-CONFIG`, `QQL-PARSE-QUOTA`, `QQL-PARSE-RERANK`, `QQL-PARSE-SHARD-KEY-CONFIG`), regenerated the complete code set over all five emitting crates, and corrected the HNSW `memory` row (`pinned` is accepted).
- **Formatter documentation** — `qql fmt`, WASM `formatQuery()`, and the VS Code Format Document provider are now documented on the website (CLI, WASM SDK, editors pages), in the agent skill, and the Python/Node SDK pages list `parse_json`.

### 📚 Documentation
- Full docs pass for 1.19/1.4: crate READMEs, `docs/syntax.md` / `docs/filters.md`, website language/reference/edge/SDK pages, skills (`qql-examples` §26–§30, gaps, multitenancy, install), and examples catalog.
- Stale conformance counts (35 valid / 249 statements) refreshed to 38 / 261 in `docs/STORY.md` and `docs/parser_generation_design.md`.

### ⚠️ Deprecations (upstream dual-write)
- QQL still accepts `on_disk` / `on_disk_payload` / `always_ram` and dual-writes them with the new `memory` placement through Qdrant 1.19; prefer `memory` / `payload_memory` for new scripts. Upstream plans removal around 1.21.

## [0.2.0]

### 📦 Packaging (VS Code only)
- **VS Code extension 0.2.1** — Marketplace packaging bump (immutable `0.2.0` slot). Ships the same QQL **0.2.0** WASM parser; crate / SDK versions stay at **0.2.0**. Fixes analysis feedback loops (CodeLens / host thrash), UTF-8 statement slicing for explain, shared analysis cache, Output channel for plans/routes, and Biome + typecheck scripts. VSIX binaries are no longer committed; build via `npm run package` (`@vscode/vsce`).

### 🚀 Added
- **QQL documentation website** — a new docs site at `qql.veristamp.in` (Astro + Starlight) replaces the legacy playground. 40+ pages across Start, Language, Guides, Edge, SDKs, Tools, Reference, and Contributing, including a dedicated **Edge runtime** section (9 pages), a **Filter injection** security guide, an **Error codes** reference, and **editors** documentation for the VS Code extension. All language, CLI, SDK, and reference pages were rewritten and deepened.
- **Interactive playground** — integrated into the site and backed by a freshly built `qql-wasm` bundle. Every documented example is extracted and parsed at build time, so the docs cannot drift from the parser.
- **Grammar as the single source of truth** — `qql-grammar-gen` now derives five artifacts from `language/v1/grammar.pest`: the generated pest grammar, the VS Code TextMate grammar, the VS Code and playground keyword tables, and the Rust `KEYWORDS` map (`keywords.generated.rs`) used by the `qql-core` lexer.
- **Language & conformance** — new conformance fixtures for `CROSS RERANK` and formula division defaults, grammar support for `USING MULTI/MULTIVECTOR/IMAGE` embedding specs, and a documented contract map in `language/v1/README.md`. The conformance suite now stands at 35 valid files (249 statements) / 53 invalid cases / 35 AST snapshots.
- **Python typing** — `.pyi` stubs and `py.typed` markers for `pyqql` and `pyqql-edge`.
- **Executable grammar contract** — `qql-conformance` now compiles `language/v1/grammar.pest` in test-only code and checks the canonical grammar against the fixture corpus without adding Pest to the `qql-core` runtime parser.
- **VS Code 0.2.0** — live diagnostics, hover plans, CTE go-to-definition, document symbols, folding, CodeLens, commands, status bar state, contextual completions, QQL snippets, and Markdown fenced-block support.
- **Updated examples** — WASM examples now use the checked-in generated package, and the edge example documents the required opt-in CLI feature.

### 🏗️ Architecture
- **Lexer/grammar lockstep** — the `TokenKind` enum stays hand-written, but its keyword map is generated and guarded by bi-directional drift tests (grammar → keywords, and parser keyword vocabulary → grammar), and `qql-grammar-gen check` rejects grammar rules unreachable from the entry productions. A dead `sharding_method_val` rule and its unused `TokenKind` variants were removed.
- **Parser-generation roadmap** — `docs/parser_generation_design.md` evaluates pest/LALRPOP/custom generators against the `no_std`, zero-dependency core and lays out a phased migration.
- **Shared editor analysis** — the extension now performs one debounced WASM analysis per document and shares the result across diagnostics, hover, symbols, CodeLens, and completion providers.

### 🔒 Changed & Scoped
- **Edge fail-loud hardening** — `PARAMS (timeout)` and `PARAMS (consistency)` are now rejected on the edge backend with `QQL-EDGE-UNSUPPORTED-TIMEOUT` / `QQL-EDGE-UNSUPPORTED-CONSISTENCY` instead of being silently ignored.
- **Fail-closed `set_shard_key`** in the Python, Node, and WASM bindings — assigning a shard key to a statement type that does not support routing now raises an error.
- **Installer & release tooling** — install scripts validate released platform targets (with a build-from-source hint otherwise) and `RELEASING.md` was aligned to `0.1.5` (edge verification notes the `--features edge` requirement).
- **Versioning** — `language/v1/spec/versioning.md` documents the post-`0.1.5` contract corrections and hardening.
- **Language version 1.3** — the specification now documents multivector and image embedding directives, strict COUNT and shard-key configuration, and the current 35 valid / 249 statement / 53 invalid / 35 snapshot conformance corpus.
- **Fallible planning surface** — removed the panicking `qql_plan::routing::route()` helper and the shard-key-dropping `top_level_filter_with_shard()` helper; use `try_route()` or `compile_statement()`.

### 🐛 Fixed
- **Parser/grammar alignment** — fixed triple-quoted string preservation, four-quote SQL strings, malformed numeric literals, non-finite floats, COUNT clause ordering, index-type validation, rerank inputs, feedback strategy validation, raw-string prefix handling, and identifier segment validation.
- **Planner and execution safety** — replaced unsupported PREFETCH panics, integer overflow, and silent prefetch degradation with structured errors; preserved CTE-backed RERANK prefetches.
- **Backend correctness** — fixed `DELETE PAYLOAD` batching, edge reads that created missing collections, gRPC/edge point-lookup envelopes, gRPC RRF parameters, and lossy gRPC filter conversion.
- **SDK and CLI correctness** — aligned Python and Node declarations with native APIs, fixed Node edge HTTP embedding and model forwarding, corrected CLI REPL ANSI output, and rebuilt the bundled editor WASM as a release artifact.

### 📚 Documentation
- Edge timeout/consistency behavior, error codes, backend compatibility, and API surfaces updated to match the hardened runtime.
- Documentation validation now rejects unwrapped QQL/SQL fences, parse-and-plan checks examples and fixtures, and verifies same-site documentation anchors.

### 🔗 Major release work
- **Core language and execution hardening — [PR #25](https://github.com/srimon12/qql-rs/pull/25)** — grammar/runtime conformance, parser and AST fixes, planner safety, backend correctness, binding alignment, and structured error handling.
- **VS Code packaging cleanup — [PR #26](https://github.com/srimon12/qql-rs/pull/26)** — removed the obsolete packaging path, corrected completion output, and added editor source checks.
- **Website validation and documentation — [PR #27](https://github.com/srimon12/qql-rs/pull/27)** — executable documentation checks, raw-fence validation, anchor checking, and updated reference content.
- **VS Code language intelligence — [PR #28](https://github.com/srimon12/qql-rs/pull/28)** — full editor-host analysis, providers, commands, snippets, release WASM, and the `0.2.0` VSIX.

---

## [0.1.5] - 2026-07-30

### 🔴 Breaking Changes
- **Removed `inject_shard_key` / `injectShardKey`** from `qql-core` and all SDKs (`pyqql`, `pyqql-edge`, `nqql`, `nqql-edge`, `qql-wasm`).
  Routing is now strictly expressed via QQL `SHARD 'key'` syntax or host property setters (`stmt.shard_key` / `set_shard_key`). `inject_filter` is reserved exclusively for logical security isolation.

### 🏗️ Architecture & Wire Protocol
- **Clean Transport Routing**: Removed `shard_key` from `FilterCompound`. REST and gRPC lower routing parameters into request-level `shard_key` params or `ShardKeySelector` payloads, avoiding filter payload overhead.
- **Turbo Quantization IR**: Added `QuantizationConfig::Turbo` to `qql-plan` with bit-width parameters (`bits = 1|2|4|8`), OpenAPI payload serialization, and gRPC converter lowering.

### 🚀 Added & Improved
- **Refreshed Showcase Examples**: Updated SEC 10-K, Berlin Airbnb, Medical showcase, and language binding demos (`examples/`) to use `SHARD` syntax, Turbo quantization, and `fastembed-rs` ONNX inference.
- **SDK & Crate Documentation**: Harmonized API tables across all 13 crate READMEs and updated agent skills (`skills/qql-skill/`).

---

## [0.1.4] - 2026-07-29

### 🏗️ Architecture
- **Transport-agnostic Plan IR**: `qql-plan` is now free of REST/gRPC client types. `PlannedOperation` is the single source of truth, lowered directly by every backend (`RestQdrant`, `GrpcQdrant`, `EdgeQdrant`). `to_rest_route()` is fallible and `compile_statement()` returns `CompiledStatement { stmt_type, route }` for reliable SDK metadata.
- **Single parser frontend**: Removed the pest runtime parser; `AstLowerer` is the sole production parser. `language/v1/grammar.pest` remains the canonical language contract, fed through `qql-grammar-gen` for docs and CI.
- **Single embedding owner**: All embedding logic now concentrated in `qql-embed` (removed the duplicate from `qql-plan`). `Embedder` trait covers dense, sparse, multi (ColBERT), image (CLIP), cross-encoder rerank, and joint (BGE-M3 single-pass) embeddings.
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
- Filter improvements: `min_should` conjunction threshold, filter-level shard key propagation, lookup collection support
- `HNSW.inline_storage` config, `stemmer` support on text index creation
- `GROUP BY` with `OFFSET` (via `group_offset`), `MMR` with sparse vector targets
- Full REST/gRPC parity: all query variants, formula expressions, geo and nested/match filters

### 🐛 Fixed
- gRPC dense `vector_params` now propagates OpenAPI `datatype` (uint8 / float16 / float32).
- CROSS RERANK no longer falls back to payload `text` when a different FIELD is requested.
- SDK `compile()` no longer mislabels DROP INDEX as `drop_collection` or SHOW SHARD KEYS as `show_collection`.
- gRPC mutation envelopes carry real server `time` from `PointsOperationResponse`.
- `PyStmt::to_dict` in pyqql uses `serde_json::to_value` before `pythonize` for full dictionary alignment with `to_json()`.
- CLI table mode renders CROSS_RERANK results; REST/edge reject bare CrossRerank routes.

### 📚 Documentation
- Skill references: `SKILL.md` updated with 1.2 features, `qql-examples.md` expanded with multivector/reranker examples, `qql-gaps.md` updated (closed gaps dropped), `qql-multitenancy.md` expanded with `inject_shard_key` patterns. SDK references (Python, Node, Rust, WASM) updated with `inject_shard_key` and batch execution.
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
