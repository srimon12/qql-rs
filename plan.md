# Later plan — debt round 3+ (status after round 2, waves 1-2, bm25-compat & audit)

Status of round 2 (`fix/debt-round2`): scores→f64, PlanRangeBound, span threading, planner core, 7 splits, CLI exit code.
Status of subsequent waves (`fix/cleanup-waves-1-2`, `feat/bm25-full-compat`, `feat/grpc-zero-copy`):
- PR #159: RestProjectionError classified, body_ref removed (Route.body is public), DDL exact spans threaded.
- PR #160: local BM25 pipeline bit-for-bit parity with server Qdrant, avg_len estimator, murmur3 collision merging.
- PR #184: plan IR holds AST `Value`s; dynamic values render once at the transport boundary (no plan-time JSON tree, no gRPC subtree pre-clone).
- Current audit fixes: WASM `send_json` classified into typed `QqlError` (no raw string errors), `HttpEmbedder` BM25 pipeline precompilation, and error codes documentation synchronization.

---

## 1. Full owned-lowering rewrite (the memcpys) — [OPEN]
- **Why left**: lowerers clone vectors out of `&Stmt`; true fix rewrites every
  `lower_*` to consume `Stmt`. Multi-day, touches plan/query/mutation/ddl.
- **Pre-work done**: prepared borrow-probe, batch retry-by-ref, upsert
  single-pass, template-reuse proof test.
- **Plan**: per-module, behind no flag: `query.rs` legs first (hottest),
  then mutation/upsert, then DDL. Criterion bench on 10k×128-dim upsert
  before/after each module. Keep `plan(&Stmt)` as the compat wrapper until
  the last module lands, then flip `prepare_statement` to `plan_owned`.
- **Done when**: `plan_owned` exists, no `Dense(d.clone())`-class clones in
  lowering, benches show ≤1 copy end-to-end.

## 2. `params_json` flatten double-clone — [OPEN]
- Consume-the-map redesign: flatten directly into the binder's lookup map
  instead of clone-then-lookup. Files: `qql-core/src/params/*`.
- Done when: one heap copy per bound value, `execute_with_named_params`
  comment updated, bind benches (if added) flat.

## 3. `normalize_planned` decoupling (unblocks `into_*_batch`) — [RESOLVED]
- Normalization moved `dispatch.rs` → `executor/normalize.rs` (free fns;
  `Executor::normalize_planned` kept as a thin wrapper). Batch hot paths use
  `normalize_query_item` / `normalize_update_item`, which read only the owned
  wire batch — never the operation vector.
- Added moving builders `into_query_batch` / `into_update_batch` plus
  `planned_to_update_operation_owned` and cold-path inverse
  `update_operation_into_planned` (only the collection `String` clones, behind
  `cold_path`). Ambient flush paths (`qql-runtime`, `qql-wasm`) move with zero
  clones; borrowed `build_*` stay for borrowing callers (forced `BATCH`,
  REST projection) with their clones intact by design.
- Done criteria met: no `request.clone()` on any owned batch path, no
  `operations.clone()` anywhere (retries move or borrow).

## 4. Deferred span coverage — [PARTIALLY DONE / POLICY DEFENDED]
- **Completed in 58e6471**: DDL exact spans threaded through all collection/field/index errors.
- **Deferred by policy**: Count/Facet/Scroll/mutation collections, group `LookupSpec`
  collections, literal DDL option values, replica states, `validate.rs`/`prefetch.rs`/`params.rs`.
- **Rule stays**: no span beats a wrong span.

## 5. `Route::body_json` public cloner — [RESOLVED / OBSOLETE]
- Resolved in commit `672f2c7`: `Route.body: Option<serde_json::Value>` is public;
  `body_ref()` was removed to avoid duplicate accessors. Callers borrow `&op.route.body`.

## 6. Range follow-ups — [MONITORED]
- Fallible `PlanRangeBound` lowering is in place and verified against OpenAPI contracts.
  Watch for real queries rejected by the bool/null/array/object RHS restriction.
  If legit use appears, add explicit cast syntax rather than reopening `Value` bounds.

## 7. Codes in waiting — [RESOLVED]
- `QQL-CONFIG` is grandfathered (keep).
- `RestProjectionError` was resolved and classified in `be63bdc`.
- WASM `send_json` was resolved in current audit: HTTP 4xx/5xx and serialization
  errors are classified through `classify_backend_error_code` into typed `QqlError` objects.

## 8. Splits not yet taken (size hygiene >400 lines) — [OPEN]
- DONE (`fix/edge-backend-split`): `qql-edge/src/backend/filter_converter.rs`
  (822 → `filter_converter/{core,matching,geo,tests}`, max 398) and
  `config_builder.rs` (775 → `config_builder/{vectors,hnsw,quantization,shared,tests}`,
  max 312). Pure moves + one drive-by (`HasId` drops `.cloned()`: `&PlanPointId`
  already implements `IntoPlanPointId`); stable paths re-exported from `mod.rs`.
- Still open:
  - `ddl.rs` (1764 lines: collection/index/shard/quota)
  - `qql-plan/src/plan.rs` (1536 lines)
  - `qql-plan/src/query.rs` (1068 lines)
  - `qql-plan/src/routing.rs` (1008 lines)
  - `qql-plan/src/filter.rs` (959 lines)
  - `qql-core/src/parser/config_parsers.rs` (910 lines)
  - `qql-edge/src/backend/mod.rs` (852 lines: `EdgeQdrant` impl)
  - `qql-runtime/src/rest_response.rs` (813 lines)
  - `qql-runtime/src/grpc_route/query.rs` (697 lines: decompose the big `to_query_variant` match)
  - `qql-edge/src/backend/ops.rs` (643 lines)
  - `qql-edge/src/embedder.rs` (~1900 lines: fastembed inference models)
- One file per PR, pure moves, re-export stable paths.

## 9. Contract to defend in review — [INVARIANTS]
- Scalar scores: shortest round-trip, rounded once at ingestion.
- Vectors: `Vec<f32>` passthrough (documented asymmetry on `SearchHit`).
- `qql run`: nonzero exit on any statement failure.
- Opaque `AFTER` strings: passthrough (pinned); revisit only with proof of Qdrant offset semantics.
- Tenant re-stamp last-writer-wins: logged in `inject_filter` rustdoc; revisit only if a caller needs conflict signals.

## 10. Hygiene backlog (no action unless touched) — [OPEN]
- Committed `.vsix` binaries in `editors/vscode` (release artifacts; leave).
- `nqql` native score path never audited for the f32-widen trap (safe today via REST JSON; recheck if it ever goes native).
- `pyqql_edge` stub parity, WASM docs nits — check on next SDK touch.
