# Changelog

All notable changes to the **QQL** project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [0.4.0] - 2026-09-12

### 🚀 Prepared Statements, Parameters & Bulk Ingestion
- **AST Parameter Binding**: Bind parameters directly against pre-parsed statement trees (`stmt.bind(...)`, `client.execute(stmt, params=...)`) without textual re-parsing; support for nested dictionary expansions (`:loc.lat`) and optional vector preview truncation (`truncate_vectors=True`).
- **Statement-Scoped Batch Execution**: Pass `params=[dict0, dict1]` to bind and execute multiple statements in a single call, matching parameters 1:1 with statement count (`QQL-BIND-BATCH-LENGTH` on length mismatch).
- **Duplicate Parameter Collision Detection**: Flattened parameter namespaces reject colliding keys fail-closed with `QQL-BIND-DUPLICATE-PARAM` (e.g. `{"loc.lat": 1, "loc": {"lat": 2}}`).
- **Expanded Placeholder Positions**: Full placeholder support across `LIMIT`, `OFFSET`, `SCROLL AFTER :cursor`, `FACET LIMIT`, `QueryInput::Text`, `HYBRID TEXT`, `CROSS RERANK`, and formula `TARGET = :datetime`.
- **Parameter Binding Fail-Closed**: Binding against DDL or unsupported statement types fails closed with `QQL-BIND-UNSUPPORTED-STATEMENT`.
- **Vector & Point Placeholders in the Plan Layer**: `PlanVectorValue::from_value` accepts packed `F32Array`, dense/sparse/flat-multivector shapes; unbound `:name` / `?` vector and query placeholders survive template planning (`plan_template`) and bind at the IR layer instead of failing the plan gate.
- **Type-Safe Binding Invariants**:
  - Bound `LIMIT` parameters enforce `> 0` across `QUERY`, `SCROLL`, and `FACET` (`QQL-BIND-INVALID-INTEGER`).
  - Vector element bindings enforce finite float ranges, rejecting values beyond `f32::MAX` (`QQL-VALIDATION-VECTOR`).
  - Literal colons inside query strings (e.g. `QUERY TEXT ':heart:'`) are cleanly preserved and never misclassified as unbound placeholders.
  - Re-binding an already bound `Stmt` fails closed with `QQL-BIND-ALREADY-BOUND`.
  - Triple-quoted (`"""`) and raw (`r'...'`) string literals are protected from placeholder replacement.
- **Zero-Walk Typed-Array Parameters**: `Float32Array` / `Float64Array` (Node) and 1-D float buffers (NumPy, `array.array`, memoryviews in Python) bind as packed `f32` vectors with a single copy instead of a per-element walk; flat multivector params (`{data: [...], dim: N}`) bind as ColBERT multi-vectors; Python float lists of 32+ elements bind as packed `F32Array` with one copy while smaller or nested lists keep exact list semantics; raw `Buffer`/`ArrayBuffer` without a float view is rejected fail-closed with helpful guidance.
- **Whole-Point Upsert Parameters**: `UPSERT INTO c VALUES :p0, :p1` / `VALUES :rows` / `VALUES ?, ?` bind whole points from dicts (or lists of dicts, splicing N points). Accepts the standard `{id, vector, payload}` shape; nested placeholders compose; misshapen values fail closed (`QQL-BIND-TYPE-MISMATCH`, missing `id` → `QQL-VALIDATION-UPSERT-ID`). Fast path pre-fetches collection schema once during `prepare()` and skips re-parse and re-fetch per execution. Point-vector mapping clones on match, ensuring named dense and sparse vectors are never dropped.
- **Bulk Ingestion Helpers (`upsert_many` / `upsertMany`)**: High-throughput declarative bulk ingestion across every driver: `client.upsert_many("docs", rows, batch_size=100)` (Python), `client.upsertMany("docs", rows, { batchSize: 100 })` (Node), and `exec.upsert_many("docs", rows, 100, OnError::Stop).await` (Rust). Prepares the `:rows` template once and moves (never clones) each chunk through the point-splice path (~7% faster on 10k points). `batch_size < 1` fails closed (`QQL-VALIDATION-UPSERT-BATCH`); empty rows return an empty `ok` report; `OnError::Continue` collects per-chunk failures; typed arrays ride `upsert_many` natively on all platforms.
- **Offline Quickstart Paths**: `examples/quickstart.py`, `examples/quickstart.mjs`, and `examples/rust/quickstart` run an end-to-end vector pipeline with no running server: hybrid CTE as text → `inject_filter` + `SHARD` → bind → route compilation → `:rows` splice.

### 🔁 Bidirectional REST ↔ QQL (`qql convert`) & Live Capture (`qql record`)
- **Contract-Driven Converter (`qql-convert` crate)**: Decodes Qdrant OpenAPI request JSON into typed `qql-core` AST and renders through `qql_core::fmt` — the same emitter behind `qql fmt` — ensuring every emitted statement re-parses by construction. Covers all 25 statement routes of the statement → endpoint matrix (queries, groups, point retrieval, scroll, count, facet, upserts, deletes, payload/vector mutations, and collection/index/shard/quota DDL), lowering geo predicates to native `GEO_BBOX` / `GEO_RADIUS` / `GEO_POLYGON` AST. Fields QQL cannot represent fail closed with a typed `ConvertError` (`InvalidJson`, `UnsupportedEndpoint`, `MissingCollection`, `UndecodableBody`, `InvalidField`) with path context instead of emitting placeholder text.
- **Flexible Converter Inputs**: A single entry point `convert(input, collection)` handles wrapped `{method, path, query?, body?}` requests, bare request bodies, or JSONL captures. Unknown request keys fail closed; `wait`, `timeout`, and `consistency` query parameters are recovered into `WAIT` and `PARAMS`. Bare bodies require an explicit collection (no `FROM unknown`). Verified by `tests/contract.rs` against `openapi.json` and `tests/route_parity.rs` via route re-planning.
- **Zero-Code-Change Traffic Capture (`qql record`, opt-in `record` feature)**: A transparent proxy that forwards Qdrant REST traffic unchanged (status, headers, auth, query strings) while appending collection and quota requests as wrapped JSONL, plus converted QQL with `--qql-out`. Bodyless `SHOW` / `DROP` routes and query parameters are captured cleanly; conversion failures append `-- ERROR` comment annotations and never interrupt request forwarding.
- **Roundtrip Fidelity & Hardening**:
  - `WAIT false` survives roundtrip conversion: the planner emits `?wait=false` explicitly on mutations and create-index instead of omitting it, so `plan → route → convert → replan` never flips durability to `true`.
  - Formatter fidelity: collection modes emit alongside explicit vector definitions, vector placeholders render bare `?`, quoted formula variables stay quoted, `$score` stays bare, and `DATETIME(...)` / `DATETIME_KEY(...)` output in canonical uppercase.
  - Converter strictness: bodyless routes reject non-empty bodies fail-closed; partial `mmr` objects fail instead of inventing defaults.
  - Recorder transparency: repeated HTTP headers survive both directions, capture line numbers increment only on successful writes, startup line counting streams instead of loading full capture into memory, same-file output detection handles `./`-qualified and symlinked paths, and `--target` is URL-validated at startup.

### 📦 First-Class `BATCH { … }` Blocks & Syntax Expansion
- **First-Class `BATCH { … }` Blocks**: Execute multiple queries or mutations in a single network roundtrip (one statement, one wire RPC). Members plan through existing lowering and must share one collection and one batch family (`BatchFamily::Query` or `Mutation`); header `WAIT` and `PARAMS` propagate to REST, gRPC, and WASM; ambient statement grouping remains unchanged. `qql convert` decodes both batch endpoints into unified blocks with full route-parity test coverage.
- **Batch `UPDATE … SET VECTOR`**: `UPDATE … SET VECTOR` maps directly to REST `PUT /points/vectors` and gRPC `UpdatePointVectors` (`repeated PointVectors`). Supports both single-point updates (`SET VECTOR [name] = … WHERE id = …`, including RHS name maps `{dense: …, sparse: …}`) and multi-point batches (`SET VECTOR VALUES {id, vector}, …`), filling `UpdateVectorRequest.points` and enabling `qql convert` to emit single compact statements.
- **Filter Syntax Expansions**: Added `MIN SHOULD n (...)` threshold conditions, `MATCH TOKENS`, `MATCH EXCEPT`, full unsigned 64-bit integer (`u64`) literals, and non-ISO string range bounds.
- **Scroll & Ordering Expansions**: Added `SCROLL … ORDER BY …`, scroll payload selectors, `ORDER BY … START FROM …`, full `LOOKUP` selectors, and `lookup_from` shard routing.
- **Mutation Expansions**: Added upsert `UPDATE FILTER` and `UPDATE MODE`, payload `KEY`, and payload `OVERWRITE` (routed as batch endpoints on REST; gRPC maps protobuf variants).
- **Inference Inputs & DDL Configuration**: Added input `OPTIONS`, `OBJECT {...}` inference inputs, per-point inference vectors, text-index stopwords and languages, collection `WAL`, `STRICT_MODE`, `METADATA`, read-fan-out, placement blocks, and quantization configuration options.
- **Language Conformance Specification 1.7**: Conformance corpus expanded to 41 valid test files (300 statements), 73 invalid cases, 41 AST snapshots, and 41 canonical format goldens.

### 🔀 Cluster Migration & Sharding (`qql migrate` & `qql dump`)
- **Collection Mover (`qql migrate`)**: Live schema and point migration between clusters without snapshot dependencies: `CREATE COLLECTION` with reshard/quantize overrides → `CREATE INDEX` before points → suppressed `indexing_threshold` during bulk load → checkpointed scroll → `:rows` upsert → optimizer restore (guaranteed on success, error, or Ctrl+C) → exact `COUNT` verification → optional `--cutover` alias swap. Flags include: `--to`, `--target-url` / `--target-api-key`, `--workers`, `--batch-size`, `--shard-number` / `--replication-factor` / `--sharding-method`, `--quantize scalar|binary|product|turbo`, `--where`, `--checkpoint` / `--resume` / `--restart`, `--dry-run`, `--recreate`, `--no-fast-bulk`, `--no-verify`, `--no-wait`, and `--json`.
- **Automatic Shard-Key Discovery**: `--shard-key-field <field>` discovers custom shard keys via `FACET <field> … LIMIT 10000 EXACT true` (with payload-only scroll fallback on truncation or unindexed fields), creates them in the schema phase with `is_tenant = true` indexes, and routes each point with a typed `SHARD` key (`SHARD 'acme'` for keywords, `SHARD 101` for numbers). `--shard-key` pins a single literal; `--on-missing-shard-key error|skip|default=<key>` handles gaps; standalone Qdrant clusters fail during schema setup rather than hanging ingest.
- **Sharded Dump Roundtrip (`qql dump`)**: `qql dump` on a `custom` sharded collection emits `CREATE SHARD KEY` statements plus per-shard `SHARD`-routed `UPSERT` batches (auto-sharded output remains unchanged), allowing `qql execute` to replay the dump onto a fresh collection with exact count matching. Shard key listing handles both REST (`{"key": …}`) and gRPC (bare scalar) response formats.
- **Migration Resiliency & Resume**: Resume logic normalizes endpoint URLs and hashes checkpoint names rather than gating on cutover flags, ensuring restarts consistently resume from valid checkpoints. Discovery merges keys across pages, cache keys drop redundant wait suffixes, and restore failures chain with ingest errors instead of masking them. Supports batch-delay throttling, per-family quantization overrides, and structured JSON output.
- **Migration Endpoint Clarity**: `--source-edge` explicitly designates the in-process edge backend as the migration source alongside global `--edge`. Pure edge-to-edge migration fails closed with actionable guidance (`--target-url` or `qql edge bootstrap`). Edge-to-remote publishing is fully supported.
- **End-to-End Shard Verification**: Verified by `crates/qql-cli/src/migrate/berlin_shard_migration.py` against a 3-node Qdrant cluster covering collection creation, ingestion, dry-run migration, checkpoint resume, and dump roundtrip.

### 🧱 Unified Typed Pipeline & Execution Reports
- **Closed Typed Response Model**: Every backend response normalizes into a closed `ExecData` enum (`Hits`, `Groups`, `Count`, `Facet`, `Mutation`, `Collections`, `Collection`, `ShardKeys`, `Quotas`) — eliminating raw JSON passthrough. gRPC and in-process edge build typed data directly from protobuf and `qdrant_edge` types; REST responses are parsed once at the boundary against strict OpenAPI schemas, failing closed with `QQL-BACKEND-ENVELOPE` on missing or mistyped fields.
- **Native Python & Node Result Classes**: `pyqql`, `pyqql-edge`, and `nqql` return native PyO3 / N-API `ExecutionReport` and `ScoredPoint` classes (retiring the legacy pythonized-dict `_dx_report.py` and dataclass wrappers). Supports dict-style `[]` / `.get()` access and report-level properties: `ok`, `results`, `succeeded`, `failed`, and `telemetry`. `ExecutionReport.from_results([...])` enables constructing offline reports from typed specifications.
- **Typed `ScoredPoint` Model**: Exposes `.id`, `.score`, `.payload`, `.vector` (dense, sparse, multi-dense, or named), `.collection`, and `.text` (derived from `payload["text"]`). Preserves numeric point IDs as integers, keeps non-standard string IDs intact, defaults `score` defensively to `0.0`, and formats scores using shortest round-trip float representations (e.g. `0.95` instead of `0.949999988079071`).
- **Typed Report Accessors**: Added `.hits()`, `.points()`, `.ids()`, `.facet()`, `.count()`, and `.groups()` accessors, plus metadata accessors `.collections()`, `.collection()`, `.shard_keys()`, and `.quotas()`. `.groups()` returns native `ScoredPoint` hits. Facet bucket aggregations (`{ value, count }`) no longer instantiate pseudo-`ScoredPoint`s, returning empty arrays from `.hits()`.
- **Typed Exception Hierarchy**: `pyqql` and `pyqql-edge` raise typed `QqlError` subclasses with structured `.code`, `.kind`, `.span`, and `.fields` attributes: `QqlSyntaxError`, `QqlValidationError`, `QqlExecutionError`, `QqlTransportError`, and `QqlBackendError`.
- **Python DB-API Subset**: Pure-Python `pyqql.connect()` provides a DB-API 2.0 compliant `Connection` and `Cursor` interface (`execute`, `executemany`, `fetchone`, `fetchmany`, `fetchall`, row iteration, 7-tuple `description`, and `rowcount`). `executemany` on `VALUES :rows` delegates to bulk `upsert_many`. Supports multi-statement isolation via `cursor.nextset()`, native parameter binding, and unified error catching (`except QqlError`). Mirrored in `pyqql-edge`.
- **Memory-Bounded Scroll Cursors**: Node `scrollCursor` / `scrollStream` (async iterator and WHATWG stream with max 1 buffered page), Python `Client.scroll_cursor(...)` / `Client.scroll_cursor_async(...)`, and WASM `dx.js` `scrollCursor`. Supports parameterized `where` clauses, custom `shardKey` partition routing, automatic identifier escaping, and unadvancing cursor detection.
- **Execution Profiling & Telemetry**: `Executor::explain_analyze`, Python `Client.explain_analyze`, and Node/WASM `Client.explainAnalyze` combine static query plans with measured client-side phase timings and honest server telemetry (`server_time_s`, hardware/inference `usage`). Aggregated across batch operations in WASM `dx.js`.
- **Typed Formulas in IR**: Formula expression trees are plan-owned (`PlanFormula`) and convert directly to protobuf and edge engine expressions without JSON intermediaries. `CASE` conditions are preserved on gRPC, datetime `TARGET` decay inference applies consistently, edge `$score` maps to the reserved score variable, and unsupported functions fail closed with `QQL-EDGE-UNSUPPORTED-FORMULA-FUNCTION`.
- **Typed DDL Requests**: Collection, index, quantization, and optimizer configurations are typed plan structs. gRPC converts directly to protobuf, including collection `wal_config`, `strict_mode_config`, and `metadata`; REST serializes exact OpenAPI bodies. Invalid index options fail at plan time (`QQL-PLAN-INDEX-TYPE`, `QQL-PLAN-INDEX-OPTION`), and out-of-range DDL integers fail closed on gRPC (`QQL-GRPC-DDL-RANGE`).
- **WASM Strict Response Envelopes & SDK Parity**: `qql-wasm` parses exact OpenAPI response schemas, failing closed with `QQL-BACKEND-ENVELOPE` on malformed responses. Modularized into 9 focused modules; added `parseJson` (zero-copy AST JSON), `Client.upsertMany`, typed array parameter conversion, `dx.js` `executeHits`, and shared batch orchestration (`build_query_batch`, `build_update_batch`, `verify_batch_cardinality`, and retry on `on_error = "continue"`).
- **Shared Runtime Core & Anti-Drift**: Centralized foreign SDK logic into `pyqql-common` and `nqql-common`. Cross-language adapters (`dx-common.js`, `test_dx.py`, `_errors.py`) are maintained as byte-identical copies enforced by CI diff gates.
- **Native FACET over gRPC**: `FACET` statements execute through Qdrant's `Points.Facet` RPC (`GrpcQdrant::facet`) instead of failing with `QQL-GRPC-FACET`.
- **Collection Schema Caching**: `Executor` caches collection schemas and vector topologies with `RwLock` and invalidates on DDL, eliminating redundant `GET /collections/{name}` roundtrips during query kind resolution and upsert routing.
- **32-Byte AST Layout**: Boxed parameter spans (`Option<Box<Span>>`), restoring `size_of::<Value>()` to 32 bytes and improving AST traversal performance by 3–11%.
- **Tunable Local BM25 (`k1` / `b` / `avg_len`)**: Configurable document encoder for sparse `TEXT` / `USING SPARSE` via `Bm25Params` on every driver (Rust `HttpEmbedderOptions` / `LocalExecutorOptions` / `QqlConfig`, Python `HttpEmbedder` / `local_executor`, Node `{ bm25K1, bm25B, bm25AvgLen }`, CLI `qql config edge --bm25-*`, and WASM `client.setBm25Params`). Write-path only; query weights remain unit; invalid values fail closed (`QQL-VALIDATION-CONFIG`).
- **Executor Concurrency & Latch Safety**: `Executor::close()` uses double-checked locking and latches closed state atomically after backend cleanup. Request IDs use monotonic 64-bit nanosecond timestamps.

### 🔗 Local Edge In-Process Engine Parity (`qql-edge`)
- **Blocking Optimizer Control (`qql edge optimize <collection>`)**: CLI command executing the qdrant-edge optimizer loop (merge, index, vacuum, config-mismatch). Reports counts before and after with `--json` support. Re-reads counts post-run and warns (`indexing_lag: true`, `status: "warn"`) when vector indexing lags behind point count.
- **Remote Shard Snapshot Bootstrap (`qql edge bootstrap`)**: Streams remote shard snapshots (`GET /collections/{c}/shards/{id}/snapshot`), unpacks via `unpack_snapshot`, verifies shard loading, and atomically swaps into the edge data directory. Multi-shard sources require `--shard-id` (`QQL-SNAPSHOT-SHARD`); downloads enforce a 120s per-read stall timeout and verify `Content-Length` (`QQL-SNAPSHOT-IO`).
- **Per-Vector Configuration Passthrough**: `CREATE COLLECTION` lowers per-vector `on_disk`, `datatype`, quantization, per-vector HNSW, and sparse index options onto `EdgeVectorParams` and `EdgeSparseVectorParams`. Memory tiers (`pinned` → RAM, `cached`/`cold` → mmap) map to engine storage switches. Unsupported sparse quantization (`turbo4`) fails closed.
- **Configurable WAL Segment Capacity**: Seed WAL segment sizes via `qql config edge --wal-segment-mb N`, `QQL_EDGE_WAL_SEGMENT_MB`, or `LocalExecutorOptions::wal_segment_capacity`, exposed in Python (`wal_segment_mb=N`) and Node (`walSegmentMb: N`) SDKs. Persisted in `edge_config.json` on initialization to protect embedded environments.
- **Grouping Query Driver**: `QUERY … GROUP BY` executes via qdrant-edge's native grouping driver with typed group keys, hydrated hits, and client-side `OFFSET` group trimming. `LOOKUP FROM` fails closed with `QQL-EDGE-UNSUPPORTED-GROUP-LOOKUP`.
- **Capability & DDL Parity**: `ALTER COLLECTION … WITH HNSW` and `WITH OPTIMIZERS` persist through engine configuration setters; create-time `WITH PARAMS (on_disk_payload = …)` is honored; partial `WITH HNSW` configurations merge over defaults; `CREATE INDEX … TYPE text WITH (stemmer = 'english', stopwords = [...])` lowers onto `TextIndexParams`.
- **Precise Engine Error Mapping**: Mapped `qdrant-edge` internal errors per `OperationError` variant to stable codes (`QQL-EDGE-STORAGE`, `QQL-EDGE-CORRUPT`, `QQL-EDGE-BAD-INPUT`, `QQL-EDGE-DIMENSION`, `QQL-EDGE-CANCELLED`), eliminating generic `QQL-EDGE-LIB`.
- **Fail-Closed DDL Enforcement**: Added explicit fail-closed checks in `EdgeUnsupported` rejecting unsupported DDL properties: collection WAL configuration (`QQL-EDGE-UNSUPPORTED-WAL`), strict mode configuration (`QQL-EDGE-UNSUPPORTED-STRICT-MODE`), and collection metadata (`QQL-EDGE-UNSUPPORTED-METADATA`).

### 💻 CLI, Developer Experience & Editor Tooling
- **CLI Parameter Support**: Added `--param key=value` (`-p`) and `--params-file <path>` to `qql exec` and `qql explain`. Binds directly on the AST via `execute_with_params`, allowing batch ingestion files to execute directly from the terminal.
- **Interactive REPL Parameter Management**: Added `\param [key=value | clear]` (`\p`) command in the REPL for inspecting, setting, and clearing session-scoped parameters.
- **Terminal Table Formatters**: Added dedicated table layouts for `QUERY POINTS` hits, `FACET` aggregation buckets, `SHOW SHARD KEYS`, and `SHOW QUOTAS`. Truncates wide text/vector cells at 80 columns (`QQL_MAX_COL_WIDTH`) with zero-allocation padding.
- **Staged Diagnostics (`qql check`)**: Multi-stage triage command executing syntax format check, offline explain, embedding dimension probe, `USING` topology validation, and backend doctor, with `--json` output for CI pipelines.
- **Doctor Dimension Probing (`qql doctor`)**: Probes `EMBED_URL` for true output dimensions and compares against `EMBED_DIM` and collection schemas, reporting `QQL-EMBEDDING-DIM` or `QQL-BACKEND-DIMENSION-MISMATCH` with actionable remediation. Added support for `qql --version`.
- **VS Code Extension DX (v0.4.0)**: Upgraded to TypeScript 7.0.2 and `@types/vscode: ^1.137.0` (with `engines.vscode: ^1.137.0`). Ships 44 completion snippets (covering queries, mutations, batches, convert, migrate, facet, scroll), hover card documentation, parameter hints, syntax highlighting, and bundled QQL 1.7 WASM engine.
- **Agent Skill & Modular Reference Manuals**: Overhauled `skills/qql-skill/` into 9 focused reference manuals under `references/` (`qql-query.md`, `qql-read.md`, `qql-mutations.md`, `qql-filters.md`, `qql-params.md`, `qql-embeddings.md`, `qql-ddl.md`, `cli.md`, `convert-migration.md`), accompanied by 40+ runnable examples in `examples/` verified by automated CI parse tests.

### ⚠️ Breaking & Behavioral Changes
- **Typed Shard Keys End-to-End**: `SHARD` routing keys are strongly typed as `ShardKey`/`PlanShardKey` (`Keyword` vs `Number`) from parser to wire. `SHARD 101`, `CREATE`/`DROP SHARD KEY 101`, and integer `shard_keys` entries retain numeric representations (which hash differently than strings in Qdrant). SDK getters/setters accept and return `str | int` (Python), `string | number | bigint` (Node), and string/`BigInt` (WASM). AST JSON serializes as `{"Keyword": …}` or `{"Number": …}`. The legacy non-contract `?shard_key=` query parameter is no longer emitted.
- **Grammar-Aligned Formatter Output**: Bare vector literals format compactly (`QUERY [0.1, 0.2]`, `VECTOR` prefix optional), and `RECOMMEND` example inputs render explicit `VECTOR` / `TEXT` / `POINT` keywords. Canonical formatter goldens are enforced by a Pest conformance gate.
- **Script Canonicality & Newlines**: `qql fmt --check`, `qql fmt --write`, and `qql check` share a unified canonicality helper ensuring consistent trailing newline handling. `qql convert` terminates each statement to produce canonical scripts.
- **Empty Script Rejection**: `execute("")` and `execute([])` fail closed with `QQL-VALIDATION-EMPTY-SCRIPT` across all SDKs instead of returning an empty `{ ok: true }` report.
- **`Stmt.toString()` Output Format**: Node and WASM `Stmt.toString()` returns canonical, re-parseable QQL syntax with positional markers normalized to bare `?`. Truncated previews move to `Stmt.toReadableString()`.
- **Rust Edition 2024**: Entire workspace upgraded to Rust Edition 2024 with modern MSRV and let-chain resolution.
- **Strict Limit Constraints**: Literal `LIMIT 0` and bound `LIMIT 0` are rejected across `QUERY`, `SCROLL`, and `FACET` (`QQL-PARSE-POSITIVE-INTEGER` / `QQL-BIND-INVALID-INTEGER`). `FACET ... WITH (limit = 0)` is strictly rejected. `OFFSET 0` remains valid.
- **Strict Grammar Validation**: Trailing commas, empty `PARAMS ()`, `ALTER COLLECTION` without `WITH`, numeric field names, non-scalar `BETWEEN`/`IN` bounds, float HNSW integers (`m = 2.0`), operator object keys, and duplicate/unknown `FACET WITH` keys are rejected at parse time.
- **Fail-Closed Filter Injection**: `inject_filter` on `UPSERT` with non-`Eq` operators or the `id` field fails closed with `QQL-VALIDATION-FILTER-INJECT` instead of silently no-oping; `ComparisonOp::parse_inject_op` is ASCII-case-insensitive.
- **Imperative Executor Helpers Removed**: `Executor::scroll_ids`, `upsert_records`, and `upsert_columns` are removed from the Rust executor and SDK clients in favor of declarative QQL (`SCROLL FROM … AFTER :offset LIMIT …`) and `upsert_many` / `upsertMany`.
- **Point ID String Preservation**: Non-standard string IDs are preserved as strings rather than being coerced to empty strings.
- **Closed `ExecData` & Strict OpenAPI Envelopes**: `ExecData` is a closed enum (no raw JSON passthrough). In Rust, `ExecData::Raw`, `as_raw()`, and legacy `*_json` accessors are removed; `SearchHit` no longer carries a denormalized `text` field (SDK `.text` derives it from payload). Malformed backend responses fail closed with `QQL-BACKEND-ENVELOPE`.
- **Python Report Construction**: `ExecutionReport({...})` dict hydration is replaced by `ExecutionReport.from_results([...])`; `report.groups()` returns `ScoredPoint` hits; `_dx_report.py` is removed from `pyqql` and `pyqql-edge`.
- **Error Code Cleanup**:
  - Removed obsolete codes: `QQL-RESPONSE-SERIALIZE`, `QQL-EDGE-FILTER-SERIALIZE`, `QQL-EDGE-FILTER-DESERIALIZE`, `QQL-GRPC-INDEX`, `QQL-EDGE-UNSUPPORTED-FIELD-TYPE`, and `QQL-EDGE-UNSUPPORTED-ACORN`.
  - Added new codes: `QQL-BACKEND-BATCH`, `QQL-BACKEND-OPTIMIZE`, `QQL-BACKEND-ENVELOPE`, `QQL-PLAN-INDEX-TYPE`, `QQL-PLAN-INDEX-OPTION`, `QQL-GRPC-DDL-RANGE`, `QQL-EDGE-UNSUPPORTED-FORMULA-FUNCTION`, `QQL-EDGE-UNSUPPORTED-WAL`, `QQL-EDGE-UNSUPPORTED-STRICT-MODE`, and `QQL-EDGE-UNSUPPORTED-METADATA`.

### 🐛 Engine Reliability & Bug Fixes
- **Named Backend Error Codes**: REST and gRPC failures map to stable codes instead of generic `QQL-BACKEND` / `QQL-GRPC`: auth (`QQL-BACKEND-AUTH`), missing collection (`QQL-BACKEND-COLLECTION-NOT-FOUND`), dimension mismatch (`QQL-BACKEND-DIMENSION-MISMATCH`), index not ready (`QQL-BACKEND-INDEX-NOT-READY`), and strict mode/quota (`QQL-BACKEND-STRICT-MODE`).
- **gRPC Durability Propagation (`WAIT`)**: UPSERT, DELETE, UPDATE, CLEAR PAYLOAD, and CREATE INDEX statements propagate `WAIT true` / `WAIT false` to gRPC requests instead of unconditionally waiting.
- **Edge Unbound Placeholder Rejection**: Unbound vector or query `Param` / `PositionalParam` placeholders reaching the edge executor fail with named parameter errors instead of executing unbound.
- **Canonical Placeholder Formatting**: `format_stmt` normalizes positional parameter markers to bare `?`, resolving `QQL-PARSE-TRAILING` errors when re-parsing queries with `LIMIT ?`, `OFFSET ?`, or formula targets.
- **SCROLL & FACET Formatter Invariants**: Fixed parameter formatting in `ScrollStmt` and `FacetStmt` where `limit_param` previously rendered as `None` or was silently dropped.
- **gRPC Request Correlation**: Outgoing gRPC requests inject `x-request-id` into metadata; errors append `(request id: ...)` and populate `.fields["request_id"]`.
- **One-Shot Client Lifecycle**: Node one-shot `execute()` and `executeStmt()` guarantee executor cleanup on both error and success paths via `run_then_close`.
- **Batch Query Hit Preservation**: Fixed hit extraction for `/points/query/batch` same-collection batches where points were silently dropped from the response envelope.
- **`on_error = "continue"` Script Resilience**: Unbound parameter failures are recorded as discrete step failures (`operation: "BIND"`) rather than aborting batch execution and discarding prior results.
- **gRPC Lazy Channel Initialization**: Resolved `there is no reactor running` panics on foreign threads by initializing Tonic channels inside the driving runtime.
- **BM25 Default Model Resolution**: Consolidated unspecified `USING bm25` targets to resolve canonically to `Qdrant/bm25` in `qql-plan`.
- **Float Equality Filters**: `WHERE rating = 4.5` lowers to `range(gte, lte)` instead of `match`, which Qdrant rejects at runtime with 400 `MatchInterface`. Integer and string equality continue using `match`.
- **Unscored Retrieval Accessors**: `.hits()` / `.points()` no longer drop points without a `score`, ensuring `SCROLL` and `QUERY POINTS` return rows instead of `[]` across Python, Node, and WASM.
- **Batched `DELETE PAYLOAD`**: Same-collection `DELETE PAYLOAD` batches on REST, gRPC, and edge like other mutation operations; REST projection failures surface as `QQL-PLAN-SERIALIZE` instead of panicking.
- **Hybrid Sparse Model Leg**: Hybrid queries embed sparse legs with the request's `MODEL` instead of silently falling back to `"default"`; WASM client rejects non-default dense models and empty embedding vectors on query paths.
- **gRPC Converter Validation**: Unbound-parameter panics in gRPC query converters return `QQL-BIND-*` errors; all non-test `panic!` calls have been removed from the runtime crate.
- **CLI Explain Parity**: `qql explain` binds `--param` / `--params-file` on the AST identically to `exec` (whole-point `:rows` templates explain as executed) and renders `WAIT` for `UPSERT`.
- **Formula Parity on gRPC & Edge**: `CASE` conditions preserve expression trees on gRPC, datetime `TARGET` inference applies there, edge `$score` resolves to the reserved score variable, and unsupported edge formula functions fail closed.
- **Per-Vector `ALTER COLLECTION` Diffs**: `ALTER COLLECTION c WITH VECTOR <name> (...)` and `WITH SPARSE <name> (...)` lower onto typed PATCH `vectors` / `sparse_vectors` maps (REST) and `VectorsConfigDiff` / `SparseVectorConfig` (gRPC). Unnamed vectors target defaults, unknown names fail fast with `QQL-UNKNOWN-VECTOR`, and `datatype` diffs fail closed (`QQL-PARSE-VECTOR-DIFF` / `QQL-PLAN-VECTOR-DIFF`).
- **DDL Configuration Correctness**: REST preserves quantization `memory`, product compression defaults to `x4`, stemmer/stopwords serialize in OpenAPI schema shapes, collection-scoped `datatype` defaults apply, per-vector `on_disk` takes precedence over collection defaults, gRPC maps binary `query_encoding`, and edge honors sparse index `full_scan_threshold` and `on_disk`.
- **gRPC DDL Mapping for WAL, Strict Mode & Metadata**: `execute_create_collection` and `execute_update_collection` map collection `wal_config`, `strict_mode_config`, and `metadata` directly to protobuf requests.
- **gRPC `SHOW COLLECTION` Status Parity**: Protobuf `CollectionStatus` enum maps to canonical lowercase names (`green`/`yellow`/`grey`/`red`) instead of numeric strings (`"1"`).

### 🛠️ Workspace, Tooling & Verification
- **Decomposed Core Modules (<400 Lines)**: Decomposed large monolithic files into cohesive submodules across `qql-core`: `fmt/`, `params/`, `ast/statement/`, and `parser/query/`.
- **Decomposed Executor, Plan & CLI Modules**: Decomposed `qql-runtime` executor into `ddl/`, `batch/`, `dispatch/`, `prepared/`, and `response/`; decomposed `qql-plan` into domain types and routing modules; decomposed CLI `table/` and `dump/`.
- **Unified Vector Conversions**: Replaced duplicate vector extraction loops with canonical `vector_from_value`, unifying dense, sparse dictionary, and multi-dense vector validation across parsing and binding.
- **Strict ISO-8601 Datetime Parsing**: Centralized `looks_like_iso_datetime` in `formula.rs`, strictly rejecting trailing non-datetime characters across all parsing and binding paths.
- **String & Identifier Helpers**: Single-sourced `is_simple_ident` and `escape_string` in `ast/mod.rs`, eliminating duplicated format and escape logic.
- **Canonical Statement Kinds**: Added `Stmt::stmt_kind` and eliminated duplicate classifications in `transform.rs`.
- **Security Audit CI**: Added scheduled workflows for `cargo audit`, CodeQL, and npm/pnpm dependency scanning.
- **Private & Shared Crate CI Verification**: Automated clippy, tests, and anti-drift checks covering `pyqql-common`, `nqql-common`, and cross-SDK shared test suites.
- **Release Automation**: `scripts/check_release.py` supports atomic version synchronization across Cargo, PyPI, npm, and editor WASM.
- **Error-Code Sync Gate**: `scripts/check-error-codes.sh` verifies emitted-vs-documented error codes in both directions (plus section coverage and sort order) as a required CI gate.
- **Internal Parity & Verification Harnesses**: Automated verification suites validating exact result parity, durability barriers, and vector normalization across dense, sparse, and ColBERT multi-vector workloads.
- **Cross-Platform Release Matrix**: Added `aarch64-unknown-linux-gnu` release builds for the CLI, `pyqql` / `pyqql-edge` wheels, and `nqql` / `nqql-edge` addons, with smoke testing for every artifact.
- **Dependabot & Lockfile Refresh**: Dependabot configured targeting `dev` with weekly grouped updates and a 3-day cooldown. Lockfiles refreshed across workspace, website (Astro 7.3.2, Starlight 0.41.11), and VS Code.
- **Hardened Installers**: `install.sh` and `install.ps1` fail loudly if releases cannot be resolved, output PATH export instructions, and support Linux ARM.

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
