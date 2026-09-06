# Changelog

All notable changes to the **QQL** project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

## [0.3.2] - 2026-09-06

### 🚀 Added
- **Prepared statements** — bind parameters against a pre-parsed `Stmt` (`stmt.bind(...)`, `client.execute(stmt, params=...)`) without string re-parsing; statement-scoped parameter lists (`params=[dict0, dict1]`) batch-execute one bind per statement (length must match the statement count); nested dictionary parameter expansion (`:loc.lat`).
- **Typed execution results** — `ScoredPoint` dataclass and typed `ExecutionReport` accessors (`.hits()`, `.points()`, `.facet()`, `.count()`); `client.execute_hits()`; FACET results normalize to the hits array directly; numeric point IDs preserved as integers instead of string round-trips.
- **Typed Python exception hierarchy** — every pyqql / pyqql-edge error is a `QqlError` subclass carrying `.code` / `.kind` / `.span`: `QqlSyntaxError`, `QqlValidationError`, `QqlExecutionError`, `QqlTransportError`, `QqlBackendError` (each also subclasses the builtin category it used to raise, so existing `except` clauses keep working). Classes live in a byte-identical `_errors.py` shared by both packages, enforced by CI.
- **`ExecutionReport.groups()`** — grouped-query accessor on the Python and Node report classes (previously only the raw dict), with both response envelopes normalized.
- **Request correlation ids** — every REST request sends `x-request-id` (and gRPC metadata); transport/backend errors echo the id in the message AND expose it as a structured `.fields` map with a `.request_id` attribute (Python and Node), so server-side anomalies like intermittent empty BM25 windows can be traced in Qdrant's logs without message parsing.
- **Default `USING bm25` model resolution** — an unspecified `USING bm25` model resolves to the server-side `Qdrant/bm25` model.
- **Vector truncation on bind** — `bind(..., truncate_vectors=True)` renders compact `[0.1, 0.2, ... (N dims)]` previews of long vector literals; `Stmt` gained string/repr rendering.
- **Cross-SDK DX parity (QQL 1.7)** — `Stmt.bind()` / `Stmt.compileRoute()` / `Stmt.toString()` / `Stmt.toReadableString()` and the typed `ExecutionReport` / `ScoredPoint` surface land in `pyqql`, `pyqql-edge`, `nqql`, and `nqql-edge` (`.pyi` stubs and TS `.d.ts` updated); `qql-wasm` gains `Stmt.bind` / `compileRoute` / `toString` / `toReadableString` / `explain` and the optional-params module `bind` (reports remain plain objects there); grammar formally declares parameter placeholders (query inputs, point IDs, scalars, clauses) with `qql.generated.pest` and conformance snapshots regenerated.
- **Compile-time parameter binding everywhere** — `compile_query(query, params=...)` and `Client.compile(query, params=...)` accept parameter bindings on all four SDKs (`pyqql`, `pyqql-edge`, `nqql`, `nqql-edge`); `bind(query, params?)` accepts `Stmt` inputs on the Node SDKs (returns a bound `Stmt`, or the readable string with `truncateVectors`); `bind(params)` is optional everywhere (omitting it is a no-op, mirroring `pyqql`).
- **Audit-gap conformance fixtures** — exponent-overflow literals (`1e999`) rejected on the value path (`QQL-PARSE-FLOAT`) and the formula path (`QQL-PARSE-NUMBER`); `LIMIT` beyond `u64::MAX` rejected at parse time (`QQL-PARSE-POSITIVE-INTEGER`).

### 🔧 Changed
- **Non-finite formula constants report `QQL-PARSE-NUMBER`** (previously the generic `QQL-PARSE-SYNTAX`), matching `parse_numeric_literal`'s stable code for score thresholds and decay targets.
- **Workspace on Rust Edition 2024** — all crates and examples; edition-2024 idioms eligible (let-chains, resolver 3 / MSRV-aware resolution).
- **Centralized SDK shared core** — `pyqql` / `pyqql-edge` share `pyqql-common` (Stmt, parser surface, error mapping, input dispatch) and `nqql` / `nqql-edge` share `nqql-common` (Stmt ops, parser surface, execution dispatch); `qql-wasm` routes through the same `qql-core::params_json` batch contract, so the helper layer that drifted three times in the DX audit can no longer diverge. The JS wrapper layer and Python report classes live in byte-identical `dx-common.js` / `_dx_report.py` copies enforced by a CI diff check.
- **Statement-scoped batch params are strictly validated everywhere** — a params list whose entries are all objects/arrays must match the statement count exactly (`QQL-BIND-BATCH-LENGTH`); length mismatches no longer silently fall back to whole-list positional binding. Scalar lists are shared positional values on every SDK (previously `pyqql` scoped scalar lists per statement when the length matched).
- **`Stmt.toString()` is the canonical, re-parseable form** on `nqql`, `nqql-edge`, and `qql-wasm` (mirrors Python `str(stmt)`); the truncated preview moves to `Stmt.toReadableString()` (mirrors Python `repr(stmt)`).
- **`is_valid` is a full parse + plan gate on the edge SDKs** — `pyqql-edge` and `nqql-edge` join `pyqql`, `nqql`, and `qql-wasm` in validating plan-level semantics via `qql_plan::parse_and_plan`.
- **`qql-wasm` `ExecuteOptions` drops `truncateVectors`** — the option was never read on `execute` / `executeStmt`; truncation stays on the module-level `bind()` where it applies.

### 🐛 Fixed
- **Same-collection QUERY batches return real hits** — the `/points/query/batch` per-item response carries the points at its top level (`QueryResponse { points }`, OpenAPI), but hit extraction only understood the single-query envelope, so every statement in a same-collection batch silently reported 0 hits.
- **Unbound placeholders fail closed on every path** — `execute(str)` with no params used to ship the raw `:placeholder` and get a 422 back; the executor probes before any network I/O and `plan()` probes again at the compile gate, both raising `QQL-BIND-MISSING-PARAM`.
- **Formula parameters bind on the prepared path** — `TARGET = :now` stored the bare identifier (indistinguishable from a DEFAULTS key), so prepared formulas shipped unresolved variables ("Expected number value for judgment_date"); the parser now preserves the `:` prefix (matching the `?idx` form), so binding an ISO string produces the same inline `datetime(...)` the string path has, and DEFAULTS keys stay untouched.
- **gRPC clients no longer panic on construction** — `Client(..., use_grpc=True)` panicked with `there is no reactor running` because tonic's lazy channel captures the tokio reactor at construction while the binding hosts built it on their foreign (Python / JS) thread; pyqql and nqql now construct the channel inside their driving runtime. Pinned by a contract test.
- **`QUERY POINTS (id, …)` silently returned empty** — the REST get-points response carries `result` as a bare point array, but hit extraction only understood the query API's `{points: [...]}` envelope; the points now surface with correct ids (gRPC was unaffected).
- **`on_error="continue"` no longer loses successful statements** — when a batched RPC fails (or returns the wrong cardinality), the group is retried statement-by-statement so per-statement success/failure stays accurate and aligned; per-item `status: "error"` entries inside a 200 batch response are also reported as failures instead of successes.
- **`close()` is a real gate** — a closed executor fails every execution entry point with `QQL-CLIENT-CLOSED` (previously a no-op; `Client.is_closed` getter added on both Python SDKs).
- **Re-binding an already-bound `Stmt` raises** — `bound.bind(params)` and `execute(bound_stmt, params=…)` used to silently ignore the new params; both raise `QQL-BIND-ALREADY-BOUND`.
- **`compile_query` accepts `QUERY VECTOR :x` params** — the explicit spelling failed to parse while `bind()` / `execute` accepted it; `VECTOR :name` / `VECTOR ?` now parse to the same parameter node as the implicit `QUERY :x USING` form, so all three paths behave identically.
- **Matrix params bind on the `Stmt` path** — a list of number lists binds as a ColBERT multi-vector (`MultiDense`), matching the string path, so multi-vector queries can be prepared statements.
- **`None` parameters fail closed with a clear code** — binding `None` used to render the text `null` and die downstream with a misleading "query input requires …" parse error; both textual and AST binding now raise `QQL-BIND-NULL-PARAM`.
- **numpy arrays (and any `tolist()` array-like) bind directly** — query inputs accept them like qdrant-client does; unsupported values now report an accurate bind error (was the generic "unsupported filter value type" `SyntaxError`).
- **Empty scripts fail closed** — `execute("")` / `execute([])` used to return a silently-empty `ok: true` report while `";;"` errored; both now raise `QQL-VALIDATION-EMPTY-SCRIPT`.
- **`LIMIT 0` rejects at parse time with the honest reason** — live verification against Qdrant 1.19.1 showed the query API answers 422 "internal.limit: value 0 invalid, must be 1 or larger", so the one-shot acceptance was reverted: `LIMIT 0` fails at the parse gate (`QQL-PARSE-POSITIVE-INTEGER`) instead of shipping a runtime 422.
- **gRPC request ID correlation compilation** — request ID correlation generation and header constants centralized in `crate::client` so `qql` compiles with `--no-default-features --features grpc` without depending on the gated `rest` module.
- **`nqql-edge` one-shot `execute()` / `executeStmt()` silently dropped `options.params`** — the standalone options normalizer stripped the field, so `:name` / `?` placeholders flowed to the server unbound; params now pass through with type validation.
- **Invalid parameter types fail closed on `Stmt` paths** — `stmt.bind(42)` and scalar entries in scoped batch params raise `QQL-BIND-INVALID-PARAMS` (`nqql`, `nqql-edge`, `qql-wasm`) instead of silently returning an unbound statement; matches the `pyqql` contract.
- **One-shot Node clients leaked connections** — module-level `execute()` / `executeStmt()` in `nqql` now close their temporary client (and honor `options.params`), matching `nqql-edge` and the Python SDKs.
- **Node `ScoredPoint` / `ExecutionReport` parity with `pyqql`** — `payload` defaults to `null`, `text` comes from the top-level hit key only, non-dict hit entries are filtered, negative statement indices use Python list semantics, and absent report keys default to `false` / `[]` / `0`.
- **Regenerated stale `native.d.ts`** — the committed NAPI-RS declaration files for `nqql` / `nqql-edge` predated the prepared-statement surface; regenerated via `napi build`.
- **VS Code rebuild docs used a relative `--out-dir`** — `wasm-pack` resolves `--out-dir` against the crate directory, so the documented command wrote to `crates/qql-wasm/wasm` instead of the editor bundle; the correct absolute-relative path is documented and the orphaned build output removed.
- **CI editor check compares class member surface** — the bundled editor WASM gate now diffs `Stmt` / `Client` members (not just free functions) against a fresh wasm-pack build, catching drift like a missing `Stmt.bind`.
- **Release gate covers the shared binding crates** — `scripts/check_release.py` validates the `pyqql-common` / `nqql-common` manifests (metadata, `publish = false`) and their root pins, fails closed on unknown workspace crate directories, and the shared crates are declared once in root `[workspace.dependencies]` (inherited via `workspace = true`), so a version bump rewrites one manifest instead of four.

### 📚 Documentation
- Python SDK guide and skill reference document the typed exception hierarchy, the `close()` / re-binding contracts, the implicit `QUERY :x USING` preference, and the new error codes (`QQL-BIND-NULL-PARAM`, `QQL-BIND-ALREADY-BOUND`, `QQL-CLIENT-CLOSED`, `QQL-VALIDATION-EMPTY-SCRIPT`); the error-code reference gains a request-correlation section.

### 🧪 Tests
- Pinned `u64::MAX` LIMIT/OFFSET passthrough (plain / grouped / hybrid `LIMIT*10` boundary) and beyond-u64 rejection for `QUERY` and `SCROLL`.
- Pinned bare `NaN` as a string filter value (QQL has no NaN literal; numeric non-finite forms are rejected) and `1 - -2` formula lowering (lexer folds the sign into the literal — no double negation; `--` after whitespace is a line comment).
- `--` lexer suite (CRLF, 3-dash, in-string safety), non-finite float contexts (vector / sparse / mmr / oversampling), RERANK + CTE prefetch (plan + gRPC), and the gRPC scroll-limit guard (`QQL-GRPC-SCROLL-LIMIT`).
- Cross-SDK DX suites extended: `test_dx.js` (network-free bind/compileRoute/toString/error-code/report coverage) is wired into the `npm test` script of `nqql` and `nqql-edge`; params passthrough for one-shot edge execution is pinned in `test_options.js`; `compile_query(params=...)` coverage in the `pyqql` / `pyqql-edge` suites.

## [0.3.1] - 2026-09-04

### 📦 Packaging
- **Workspace 0.3.1** — synchronized across all crates, PyPI (`pyqql` / `pyqql-edge`), npm (`@veristamp/nqql` / `@veristamp/nqql-edge` + platform packages), and the bundled editor WASM; `Cargo.lock` refreshed. `scripts/check_release.py` now drives this: `set <version>` / `bump major|minor|patch` rewrite every version site in one step, refresh the lockfile, and re-validate; check mode additionally validates root `[workspace.dependencies]` pins and `VERSION` ↔ workspace consistency.

### 🚀 Added
- **Formula functions `MAX` / `MIN` / `ACOSH`** (QQL 1.6, Qdrant upstream API sync) — `MAX`/`MIN` fold n ≥ 1 operands and `ACOSH(x)` is unary; wired grammar → parser → typed AST → canonical formatter → plan lowering → gRPC protobuf. The edge backend fails closed (`QQL-EDGE-UNSUPPORTED-FORMULA-FUNCTION`).
- **Vector-dimension cap** — `CREATE COLLECTION` rejects dimensions above 65536 at parse time with `QQL-PARSE-VECTOR-SIZE` (mirrors the Qdrant `VectorParams.size` maximum).
- **Canonical-format conformance goldens** — `language/v1/fixtures/formatted/*.txt` join the conformance contract (40 files), verified natively and against the bundled editor WASM via corpus-driven extension tests.
- **`qql` single-dependency re-exports** — `QqlError` / `ErrorKind` / `Span`, `Stmt` / `Parser` / `inject_filter` / `ComparisonOp` / `Value`, and the plan contract types (`PlannedOperation`, batch + DDL request types) are re-exported from `qql` (Rust API guidelines C-REEXPORT), with a compile-time test enforcing that a full `QdrantOps` impl and the parse → inject policy flow build from `qql::` paths alone.
- **Native `FACET` statement** — first-class grammar, AST (`Stmt::Facet(Box<FacetStmt>)`), and planning (`PlannedOperation::Facet`) mapping to Qdrant's `POST /collections/{collection}/facet` aggregation endpoint. Supports `WHERE` filtering, `LIMIT`, `EXACT true` distributed counting, and `SHARD` partition routing. Validated across `qql-runtime`, `qql-edge`, `qql-wasm`, and `language/v1` conformance fixtures.
- **Implicit vector array literals** — queries accept float array literals directly as `QUERY [0.1, 0.2, ...] FROM ...` without requiring the explicit `VECTOR` keyword prefix.
- **Formula decay ISO datetime string parsing & variable auto-inference** — decay functions (`EXP_DECAY`, `GAUSS_DECAY`, `LIN_DECAY`) now accept standard ISO 8601 string literals `TARGET = "2024-01-01T00:00:00Z"` lowering to `FormulaExpr::Datetime`, and bare payload field names automatically infer `{"datetime_key": name}`.
- **VS Code extension `FACET` support** — added `qfacet` snippet, hover card documentation in `KEYWORD_DOCS`, and autocompletions in `qql-lang`.
- **SDK parity: `Stmt::new()` constructor in `nqql-edge`** — Node edge binding now exports a constructible `Stmt(query)` handle mirroring `nqql` and `qql-wasm`.
- **SDK parity: `Stmt.compile_route()`** — exposed AST route compilation (`compile_route` / `compileRoute`) directly on `Stmt` across `pyqql`, `pyqql-edge`, `nqql`, and `nqql-edge` to avoid re-parsing queries.
- **SDK parity: `Client.close()`** — added explicit connection release `close()` on remote clients in `pyqql` (with `__enter__`/`__exit__` context manager support) and `nqql`.
- **SDK parity: full token spans** — `tokenize()` outputs across `pyqql`, `pyqql-edge`, `nqql`, and `nqql-edge` now provide `{ kind, text, pos, end, len }` matching `qql-wasm` and the lexer `Span` contract.
- **SDK parity: version export** — `pyqql` exports `__version__` matching the cargo package version.
- **CLI gRPC scheme detection** — `qql-cli` recognizes `grpc://` protocol URIs in addition to `:6334` port matching.

### 🔄 Changed
- **QQL language version 1.6** — additive minor per `language/v1/spec/versioning.md`; the conformance corpus grows to 40 valid files (276 statements), 59 invalid cases, 40 AST snapshots, and 40 canonical formats.
- **`is_valid` is a full parse + plan gate** — `is_valid` / `analyze` in `pyqql`, `nqql`, and `qql-wasm` validate plan-level semantics through the shared `qql_plan::parse_and_plan` instead of syntax alone; binding READMEs document the tightened contract.
- **Default `WITH PAYLOAD true`** — `lower_output_selector` now defaults to returning all payload attributes (`Some(PayloadSelectorReq::All(true))`) when `WITH PAYLOAD` is omitted, matching intuitive SQL retrieval semantics. Explicit `WITH PAYLOAD false` continues to omit payload fields.
- **Decomposed runtime gRPC monoliths** — split `crates/qql-runtime/src/grpc.rs` (1,000 lines) and `crates/qql-runtime/src/grpc_route.rs` (4,000 lines) into focused domain submodules (`query`, `execute_write`, `execute_read`, `execute_ddl`, `ddl`, `filter`, `formula`, `responses`, `schema`, `points`, `ops`).
- **Centralized workspace dependencies** — configured `[workspace.dependencies]` in root `Cargo.toml` (`serde`, `serde_json`, `tokio`, `phf`, `uuid`, `async-trait`, and internal crates) and converted member manifests to `.workspace = true`.
- **Standard error trait** — implemented `core::error::Error` unconditionally on `QqlError` in `qql-core`, enabling standard error interoperability in `#![no_std]` contexts.
- **Documentation & Skills modernization** — updated all website docs, agent skills, and specification documents to showcase modern QQL idioms (simplified queries without boilerplate `WITH PAYLOAD true`, implicit vectors, and `FACET`).

### ⚡ Performance
- **Zero-allocation query and plan explain formatting** — converted 110+ `output.push_str(&format!(...))` heap allocations across `qql-core::fmt` and `qql-core::explain` to in-place `write!` and `writeln!` via `core::fmt::Write`.
- **Single-pass table alignment** — rewrote `qql-cli::table` column alignment to a single short-circuiting pass over rows.
- **SIMD string case matching** — replaced manual byte-by-byte iterator loops in `qql-core` parser with standard `eq_ignore_ascii_case`.

### 🐛 Fixed
- **Lexer error-stream termination** — the `Lexer` iterator re-yielded non-advancing lex errors forever (inputs like `1.` or `1e+` hung `flatten()` consumers); the token stream now halts after the first error, and `qql-wasm` `analyze` handles errors explicitly instead of flattening.
- **Formula boolean `MatchCondition` lowering** — single-value match conditions (e.g. `MATCH(is_superhost, true)`) now lower to `{"match": {"value": val}}` instead of `{"match": {"any": [...]}}`, avoiding HTTP 400 schema validation rejection from Qdrant.
- **REST error UTF-8 boundary panic** — protected 4096-byte response truncation in `qql-runtime` using `floor_char_boundary(4096)`.
- **Range filter bound inversion in CLI converter** — `convert_condition` in `qql-cli` now only emits `BETWEEN` for `(gte, lte)` pairs; strict inequalities emit explicit `> <` comparisons.
- **Incomplete string escaping in CLI converter** — replaced naive quote replacement with `escape_qql_string`.
- **Monotonic mutex growth in edge backend** — evicted collection keys from `self.opening` once cached or upon collection deletion in `qql-edge`.
- **Python error type consistency** — aligned `pyqql-edge` `parse_json` to raise `SyntaxError` on malformed queries.

### 🔒 Security
- **Website transitive deps (pnpm)** — `nanoid 3.3.16 → 3.3.18` (GHSA-2v37-7h3g-55p8, custom-generator infinite loop on size 0), `adm-zip 0.5.18 → 0.6.0` (GHSA-xcpc-8h2w-3j85, crafted-ZIP 4GB `Buffer.alloc`), `js-yaml 4.3.0 → 4.3.1` (GHSA-5p4m-2wfm-xmqj, quadratic `!!omap` CPU), `sharp 0.35.3/0.34.5 → 0.35.4` (libvips ≥8.18.6 for CVE-2026-33327/33328/35590/35591). Enforced via `website/pnpm-workspace.yaml` `overrides`; `pnpm audit` is clean.
- **VS Code extension transitive deps (npm)** — `js-yaml 4.3.0 → 4.3.1`, `brace-expansion 5.0.8 → 5.0.9` (GHSA-rgw5-rvv9-x895), `qs 6.15.3 → 6.16.0` (GHSA-x5fp-wj9c-mxmx / GHSA-4mjr-xmp4-gh2g) via `package.json` `overrides`; `npm audit` is clean.
- **Rust `rand` (RUSTSEC-2026-0097, Low/INFO)** — `rand 0.9.5` / `0.10.2` already at latest compatible; `0.7.3` / `0.8.8` remain via `qdrant-edge 0.8.0` (latest upstream, no patched `0.7`/`0.8` line). Not exploitable here: no `rand::rng()`/`thread_rng()` use in our code and no `log` feature enabled on the old lines.
- **Invalid-pointer CodeQL (Rust)** — removed the repo's only two `unsafe` blocks: `qql-wasm` no longer uses `Uint8Array::view` (uses `Uint8Array::from` copy), `qql-embed` BM25 lowercasing uses checked `from_utf8().expect()` instead of `from_utf8_unchecked`.
- **Playground XSS + open-redirect (CodeQL)** — `website/src/scripts/playground.ts` now validates `?ref=` with strict same-origin `isSafeDocsRef()` (`/docs` + reject `[\\<>"'`\s]` + `URL` origin/pathname check), sets the backlink via `setAttribute("href")`, and builds share URLs only from validated refs. Replaced `innerHTML` badge updates with `createElement`/`createTextNode`.
- **Medical demo clear-text logging/storage (CodeQL)** — `build-medical-corpus.py` / `run-benchmark.py` / `run_demo.py` log only file names, counts, opaque IDs and hit flags (no question/answer/context text); all generated/cache writes go through owner-only `0600` helper; eval manifest drops unused `answer` fields. `generated/` is now git-ignored and untracked (`git rm --cached`); files stay local only.

### 📚 Documentation
- **Full public-API Rust docs** — ~1,500 doc comments across the six published crates (`qql`, `qql-core`, `qql-plan`, `qql-embed`, `qql-edge`, `qql-cli`): AST variants, plan IR, the `QdrantOps` extension contract, executor/config/transports, tokens, and errors; docs.rs landing pages for `qql` and `qql-core`. A `missing_docs` workspace lint scoped to exactly CI's `cargo doc -D warnings` gate makes the standard self-enforcing. Generated code (`qdrant.rs`, `qdrant_grpc.rs`, `keywords.generated.rs`) keeps its local allowances.
- **Website and reference alignment to QQL 1.6** — formula reference (`MAX` / `MIN` / `ACOSH` + a clamping example validated by the docs pipeline), error-code tables (`QQL-PARSE-VECTOR-SIZE`, `QQL-PLAN-RECOMMEND-AVERAGE`, `QQL-EDGE-UNSUPPORTED-FORMULA-FUNCTION`), grammar/editors/language pages bumped to QQL 1.6.

## [0.3.0] - 2026-08-30

### 📦 Packaging
- **Workspace 0.3.0** — all crates, `pyqql` / `pyqql-edge` (PyPI), `@veristamp/nqql` / `@veristamp/nqql-edge` + platform packages (npm), and `qql-wasm` move to **0.3.0** together. This release merges the post-0.2.2 work with the previously unreleased 0.2.2 changes into a single version.
- **VS Code extension 0.3.0** — the extension version aligns with the workspace release again after several independent packaging slots (0.2.1 / 0.2.4 / 0.2.5). Ships the rebuilt `nodejs`-target WASM bundle (parameter binding + QQL 1.5 parse surface) and the `qidftenant` snippet.

### 🚀 Added
- **Wire-compatible client-side BM25** — `qql-embed` sparse embeddings are now byte-for-byte compatible with Qdrant's server-side `qdrant/bm25` model: murmur3-32 token IDs (same `murmur3_32` crate the server uses), word tokenizer (split on non-alphanumeric), Unicode lowercasing, English stopword removal (exact server list), and English snowball stemming (`qdrant-rust-stemmers`). Queries embed with unit term weights; documents with BM25 tf saturation (k1=1.2, b=0.75, avg_len=256). A golden test pins real server output from the Qdrant docs. Vectors can be mixed with server-side `qdrant/bm25` inference on the same collection (requires sparse vector `modifier = 'idf'`).
- **Batched sparse ingestion** — `Embedder::embed_sparse_document_batch` with default loop implementation; the edge embedder batches fastembed sparse inference at ingestion.
- **Parameter binding (`:name` / `?`)** — `qql-core::params` substitutes named and positional placeholders in QQL source before parse. `$` stays an identifier character (`$score`, `$1`), so placeholders are only `:name` and `?`. Colons inside compact dicts (`{a:b}`) are not placeholders. Strings and `--` comments are never rewritten. Exposed as `bind` / `bind_named` / `bind_positional` on Python, Node, WASM, and edge SDKs, `Executor::execute_with_params` / `execute_with_positional_params`, and `Client.execute(..., params=…)`.
- **Standalone value parse** — `Parser::parse_value` for binding literals.
- **CLI REPL module** — interactive session extracted to `qql-cli` `repl.rs` (multiline statements, `\f` format, `\d` doctor, `\e` script).
- **QQL 1.5 tenant IDF examples** — `query-idf-tenant.qql` plus keyword `prefix = true` on `create-index`. VS Code snippet `qidftenant`.
- **Website guides** — embedded in-process Qdrant + FastEmbed (Python / Node), and a QQL vs raw Qdrant JSON comparison. Open Graph image route for docs/landing.

### 🔄 Changed
- **BREAKING: sparse token IDs changed** — the previous FNV-1a + length-prefix hashing is replaced by murmur3-32. **Existing sparse collections must be re-embedded**; old and new vectors cannot be mixed within one collection. New vectors are interchangeable with server-side `qdrant/bm25`.
- **BREAKING: `Embedder` trait split** — `embed_sparse` / `embed_sparse_batch` are replaced by `embed_sparse_query` (unit weights, search text) and `embed_sparse_document` / `embed_sparse_document_batch` (BM25 tf saturation, ingestion text). Previously both sides used the query-style log-tf function and `build_document` was dead code. Custom embedders overriding `embed_sparse` must move to the role methods; embedders using the defaults need no changes.
- **Edge BM25 fidelity** — `qql-edge` `FastEmbedder` delegates its local BM25 fallback to `qdrant_edge::bm25_embed::EdgeBm25` (segment's exact tokenizer pipeline), replacing the hand-rolled hasher.
- **Sparse tokenizer follows the server** — underscores and hyphens are word boundaries (e.g. `test_fn` → `test` + `fn`), stopwords are dropped, and inflections stem (`running` → `run`), matching Qdrant's `qdrant/bm25` defaults. The pipeline is English-only, like the server defaults; non-English corpora should use server-side inference with explicit language options.
- **Host bind DX** — Python (`pyqql` / `pyqql-edge`), Node (`nqql` / `nqql-edge`), and WASM expose one `bind(query, params)`: dict/object for `:name`, list/array for `?`. `Client.execute` takes the same `params`. Removed `bind_named` / `bind_positional` / `bindNamed` / `bindPositional` from host SDKs. WASM `bind` takes a JS object or array, not a JSON string. Rust keeps typed `bind_named` / `bind_positional` and `execute_with_params` / `execute_with_positional_params`.
- **QQL 1.5 IDF corpus** — `PARAMS (idf = …)` takes `'global'` or a QQL `WHERE` filter (`idf = WHERE tenant_id = 'acme'`). The AST stores `Option<FilterExpr>`. The Qdrant JSON `{corpus: {must: […]}}` form is removed (`QQL-VALIDATION-IDF`). Isolation remains `WHERE` / `inject_filter`; routing remains `SHARD`; IDF only scopes sparse term statistics.
- **Language version 1.5** — `PARAMS (idf = …)` is `'global'` / bare `global`, or `WHERE <filter>` (`idf = WHERE tenant_id = 'acme'`). AST `IdfParams.corpus` is `Option<FilterExpr>`. The planner lowers that filter with `top_level_filter` (the old `value_to_json` JSON-corpus path is gone). Isolation stays `WHERE` / `inject_filter`; routing stays `SHARD`; IDF only scopes sparse term statistics. The Qdrant JSON `{corpus: {must: […]}}` form is **removed** (`QQL-VALIDATION-IDF` at parse). Unused `CORPUS` token dropped. Conformance: 39 valid files (265 statements), 56 invalid cases, 39 AST snapshots.
- **SDK crate splits** — `pyqql` embedder, `pyqql-edge` models, and `nqql-edge` tests moved out of the giant `lib.rs` files. JSON → AST values go through `Value::from_json` in one place. Node bindings reject invalid `params` shapes (object for named, array for positional).
- **Bundled editor WASM** — rebuilt (`nodejs` target) so diagnostics, format, and `bind` match current `qql-wasm` (parameter placeholders and `idf = WHERE …`).
- **Website chrome and landing** — Veristamp tokens (warm paper, terracotta, Newsreader + IBM Plex Sans + JetBrains Mono). Landing cut to hero, QQL vs JSON/Python compare, why, install, FAQ. Shared `chrome.css` for docs header/footer and playground dialogs.
- **Playground** — full-viewport editor/inspector shell; Connection and Example as clickable chips; policy dialog is an example list plus a two-row inject form (`Field`/`Op`, `Value`/`Type`). Policy is `inject_filter` + optional `SHARD` only.

### 🐛 Fixed
- **IDF JSON corpora** — `{corpus: {must: […]}}` no longer parses; write `idf = WHERE …`.
- **Docs keyword / 1.5 references** — skill and SDK pages, `docs/STORY.md` counts, and website language/examples/error-code pages match 1.5.
- **Playground policy layout** — native `<select>` no longer overlaps the field/value inputs (`box-sizing` + two-row grid).

### 📚 Documentation
- `docs/parameters.md` and executable `{% qqlExample %}` blocks for bound queries.
- Skills and SDK references: bind usage, and IDF as QQL `WHERE` (no host inject, no JSON corpus).
- Website language, tools/examples, backend-compatibility, and error-code pages updated for `idf = WHERE <filter>`.

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
