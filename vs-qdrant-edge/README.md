# QQL Edge vs Qdrant Edge — Head-to-Head (in-process, same engine)

Reproducible comparison of **`pyqql-edge`** (QQL language + planner + FastEmbed
on top of the `qdrant-edge` Rust crate) against **`qdrant-edge-py`** (Qdrant's
own Python bindings for the same crate) on the same corpus, same engine
version, same machine — zero network.

The QQL side runs the **fully typed pipeline** (typed requests *and* responses):
`BackendResponse`/`ExecData` is canonical, gRPC and edge construct it straight
from protobuf/`qdrant_edge` (zero JSON envelopes), REST parses its JSON once at
its own boundary, Python consumes native classes with no dict/dataclass hop.
See [§ Fully typed](#fully-typed-pipeline-request-and-response).

Short answers: leverage **shifts** (concurrency, writes, text, DX — not
single-thread raw reads); and **keep the `qdrant-edge` crate** — vendoring
means forking ~216k lines of storage engine, see [§ Keep or vendor](#keep-or-vendor).

---

## What is — and is not — head-to-head here

| group | query input | embedding in the timed call? | claim it supports |
|---|---|---|---|
| **`engine`** (12 scenarios) | **precomputed vectors** from `../vs-qdrant/data/queries.json`, same bytes both sides | **no** — neither side embeds | engine + application layer on identical inputs |
| **`text`** (2 scenarios) | raw text | **yes — each side brings its own embedder** (QQL in-path FastEmbed/Bm25, official fastembed Python / `Bm25`) | end-to-end text→results stack, *not* engine-only |

`query_hybrid` uses precomputed dense + precomputed BM25 sparse in both
prefetches — no in-path BM25. `results/edge.json` records a
`query_input_sha256_16` digest per scenario, and result parity (IDs exact,
scores ≤ 2e-4) proves the same inputs reached both engines.

---

## TL;DR (Qdrant Edge 0.8.0, localhost, 2026-09-10 — typed pipeline)

| | official `qdrant-edge-py` 0.8.0 | `pyqql-edge` 0.4.0 | qql / official |
|---|---:|---:|---:|
| **Engine reads, 1 thread (12 scenarios, precomputed vectors)** | — | **0 / 12** | best 0.94x (`dense_filtered`, `count_legal`); 0.89–0.90x ×5 |
| **Engine reads, 8 Python threads (same shards)** | 997 q/s — flat | **4,196 q/s** | **4.21x faster — GIL released vs held** (3.0–4.2x across runs) |
| **Text layer** | — | — | `text_sparse` **1.25x (qql wins)**; `text_dense` 0.73x |
| **`FACET district`** | 0.020 ms | 0.026 ms | 0.76x — **works now** (was `QQL-EDGE-FACET`) |
| **`optimize()`** | 449 ms | 429 ms | parity — **works now** (was not exposed) |
| Ingest total (prep + submit), berlin 8k | 1.39 s | 1.21 s | **1.15x faster** |
| Ingest total, legal 2k + ColBERT | 1.07 s | 0.71 s | **1.49x faster** |
| Cold import | 12.1 ms | 18.5 ms | 0.65x |
| Application LOC (17 scenarios) | 277 | **164** | **0.59x** |

Single-thread reads sit at 0.50–0.94x. Sub-millisecond scenarios carry ±~0.1x
run-to-run variance (dense measured 0.81–1.01x across runs); the concurrency,
write, facet/optimize and text-path results are stable directionally.

---

## Fully typed pipeline (request and response)

The architecture is now symmetric: `PlannedOperation` is the canonical request
IR and `BackendResponse` is the canonical response IR, with transport shapes as
projections — not the internal currency.

```
Backend → BackendResponse { data: ExecData, telemetry }     ← canonical response
              ├── REST : HTTP JSON → ExecData once, at the boundary   (wire stays JSON)
              ├── gRPC : protobuf → ExecData directly   (zero JSON builders on any op)
              └── edge : qdrant_edge → ExecData directly (zero JSON builders)
              ↓
        one normalization → ExecResponse (typed) → native bindings
```

```rust
pub enum ExecData {
    Hits(Vec<SearchHit>),
    Groups(Vec<GroupedSearchResult>),          // QUERY … GROUP BY
    Count(u64),
    Facet(Vec<FacetHit>),
    Mutation { affected: Option<u64> },        // writes; upsert count from the request
    Raw(serde_json::Value),                    // schemaless metadata ONLY
}
```

What was deleted (no compatibility layer retained):

- `QdrantOps::execute_planned_typed` and the JSON `execute_planned -> Value`
  — one typed method remains; `BackendResponse::from_envelope` is gone from
  shared code (REST parses via crate-private `crate::envelope::parse_backend_response`).
- Batch methods return `Vec<BackendResponse>`; no per-item JSON.
- gRPC: 10 JSON response builders deleted (`scored_point_to_json`,
  `retrieved_point_to_json`, `groups_result_to_json`, mutation/DDL envelopes…),
  every op — reads, groups, writes, DDL — builds typed data straight from
  protobuf with typed telemetry (no `usage_to_json` in the hot path).
- edge: `execute_planned_envelope` and every JSON converter deleted; all ops
  typed straight from `qdrant_edge`; engine calls are synchronous (the
  `spawn_blocking` hop is gone; callers already detach the GIL).
- Python: `_dx_report.py` **deleted from both packages**; `__init__` imports
  the native `ExecutionReport`/`ScoredPoint` directly; `wrap_execution_report`
  collapsed to `PyExecutionReport::wrap`; `.pyi` rewritten to the native surface.
- Scores serialize as shortest f32 (`0.95`, not `0.949999988079071`), matching
  Qdrant's JSON text and the native getter — JSON view and typed view agree.

**`Raw` remnants (schemaless by design):** `SHOW COLLECTIONS`, `SHOW COLLECTION`
(edge + gRPC; free-form collection config), `SHOW SHARD KEYS` (gRPC; union-typed
list), REST quotas. Typing them means re-modelling Qdrant's entire
collection-config schema in the plan IR — the same reason DDL sub-configs stay
schemaless maps.

New capabilities from the same refactor: **`FACET` works on edge** (was a hard
error) and **`optimize()` is exposed** (`Client.optimize(collection)`, edge
only; 429 ms vs official 449 ms on the 8k shard, both merge segments).

Measured effect of the whole refactor (before/after, qql/official):

| scenario | before | after (range over runs) |
|---|---:|---:|
| query_dense | 0.76x | **0.81x** (0.81–1.01) |
| query_dense_filtered | 0.92x | **0.94x** (0.91–0.99) |
| query_sparse | 0.44x | **0.71x** (0.65–0.71) |
| query_hybrid | 0.76x | **0.83x** (0.83–0.87) |
| count_berlin | 0.79x | **0.89x** (0.84–1.01) |
| count_legal | 0.71x | **0.94x** (0.71–0.94) |
| facet_district | `QQL-EDGE-FACET` error | **0.76x** |
| scroll_pages | 0.37x | **0.89x** (0.89–0.94) |
| retrieve_points | 0.20x | **0.50x** (0.31–0.50) |
| prepared_rerun | 0.80x | **0.89x** (0.82–0.91) |
| batch_reads | 0.82x | **0.89x** (0.81–0.89) |
| query_colbert | 0.90x | 0.90x |
| text_sparse | 0.54x | **1.25x** (0.98–1.25) |
| text_dense | 0.78x | 0.73x (0.73–0.87) |
| optimize | not exposed | parity (429 vs 449 ms) |
| cold import | 31.9 ms | **18.5 ms** |

### Remaining backlog

1. Bind 1-D float **Python lists** as `F32Array` — done for flat lists ≥32
   elements (the binder is not slot-aware; nested/short/int lists keep exact
   `Value::List` semantics, documented). Query-vector slots are f32 by design.
2. gRPC/edge writes and DDL — done, all typed.
3. Sync edge path — done, no `spawn_blocking` on data operations.
4. `optimize()` — done.
5. Schemaless metadata variants (`Collections`, `Collection`, `ShardKeys`) —
   the only remaining `Raw` producers; deferred deliberately.

---

## Keep or vendor

**Recommendation: keep consuming `qdrant-edge` from crates.io. Do not vendor
the engine.**

| metric | value |
|---|---:|
| Rust files in `qdrant-edge` 0.8.0 | 1,059 |
| Lines of Rust (excluding tests) | **215,760** |
| Inlined modules | `segment/` (HNSW, quantization, payload indexes), `shard/` (optimizers, WAL, snapshots), `sparse/`, `bm25/`, `wal/`, `common/`, `blobstore/`, `posting_list/` |
| API cadence | 0.4 → 0.8 in ~6 months |

Our layer above is ~4.8k LOC with `QdrantOps` as the seam; extend that seam
(with typed capability methods), never fork the executor per backend.

---

## PyO3 design: why we lose 1 thread and win N

| | `qdrant-edge-py` 0.8 | `pyqql-edge` 0.4 |
|---|---|---|
| Call model | synchronous `#[pymethods]`; **no runtime** | async executor + `block_on` per op (edge data calls synchronous) |
| GIL | **held for the whole engine call** (no `allow_threads`/`detach`) | `py.detach(...)` around execution |
| Results | `Vec<ScoredPoint>` reinterpreted via `#[repr(transparent)]` + `bytemuck`, lazy payload | native `#[pyclass]` hits from `ExecData::Hits`, lazy payload |
| Query input | typed `#[pyclass]` objects → `.into()` newtype move | params → typed IR → plan (numpy/`F32Array` fast path) |
| Error model | `PyException(backend string)` | typed `QqlError` (code/kind/span) |

The PyO3 rule is explicit: *release the GIL during CPU work*. Official holds
it, so its throughput is flat across threads; we detach, so we scale — the
structural win for any Python server doing concurrent vector search.

---

## Results

### Engine head-to-head — precomputed query vectors, same bytes both sides

| scenario | official ops/s | qql ops/s | qql/official | p50 official | p50 qql | parity |
|---|---:|---:|---:|---:|---:|---|
| query_dense | 1,079 | 876 | 0.81x | 0.927 ms | 1.142 ms | ids exact, Δscore 0 |
| query_dense_filtered | 115 | 108 | 0.94x | 8.692 ms | 9.241 ms | ids exact, Δscore 0 |
| query_sparse | 9,686 | 6,867 | 0.71x | 0.103 ms | 0.146 ms | ids exact, Δscore 2e-6 |
| query_hybrid (RRF) | 910 | 753 | 0.83x | 1.099 ms | 1.329 ms | ids exact, Δscore 0 |
| count_berlin | 2,904 | 2,571 | 0.89x | 0.344 ms | 0.389 ms | exact |
| count_legal | 12,484 | 11,788 | 0.94x | 0.080 ms | 0.085 ms | exact |
| facet_district | 50,098 | 38,316 | 0.76x | 0.020 ms | 0.026 ms | exact |
| scroll_pages (3×256) | 419 | 373 | 0.89x | 2.387 ms | 2.682 ms | exact 768 ids |
| retrieve_points (10 ids) | 43,613 | 21,684 | 0.50x | 0.023 ms | 0.046 ms | exact ids + payloads |
| prepared_rerun (4 vectors) | 258 | 230 | 0.89x | 3.877 ms | 4.353 ms | ids exact, Δscore 0 |
| batch_reads (4 statements) | 267 | 237 | 0.89x | 3.744 ms | 4.221 ms | ids exact, Δscore 0 |
| query_colbert | 68 | 61 | 0.90x | 14.796 ms | 16.389 ms | ids exact, Δscore 0 |

### Threaded throughput — same shard, 40 queries per thread

| threads | official q/s | qql q/s | qql/official |
|---:|---:|---:|---:|
| 1 | 822 | 706 | 0.86x |
| 2 | 838 | 1,229 | 1.47x |
| 4 | 1,006 | 2,633 | 2.62x |
| 8 | 997 | 4,196 | **4.21x** |

### Text → vector layer (both sides embed in-path; not engine-only)

| scenario | official ops/s | qql ops/s | qql/official | p50 official | p50 qql | parity |
|---|---:|---:|---:|---:|---:|---|
| text_dense | 146 | 106 | 0.73x | 6.873 ms | 9.413 ms | ids exact, Δscore 1.1e-4 |
| text_sparse | 6,465 | 8,065 | **1.25x** | 0.155 ms | 0.124 ms | ids exact, Δscore 2e-6 |

`text_sparse` oscillates around parity-to-win (0.98–1.25x across runs): same
engine, same BM25 semantics (parity proves identical sparse vectors), QQL's
native-hit path removed the former 0.54x deficit.

### Ingest — submission throughput

| collection | official prep | official submit | qql prep | qql submit | end-to-end |
|---|---:|---:|---:|---:|---:|
| berlin 8,000 (dense + BM25) | 0.34 s | 1.05 s | 0.005 s | 1.20 s | **qql 1.15x faster** |
| legal 2,000 (dense + BM25 + ColBERT) | 0.66 s | 0.41 s | 0.002 s | 0.71 s | **qql 1.49x faster** |

### Capabilities / startup / size

| | official | qql |
|---|---:|---:|
| `optimize()` on the 8k shard | 449 ms (`changed=True`) | **429 ms** (`changed=True`) |
| cold import (module) | 12.1 ms | 18.5 ms |
| executor / embedder construction, warm cache | 0.38 s (fastembed Python, when used) | 0.25 s (executor + FastEmbed-rs) |
| scenario application LOC | 277 | 164 (0.59x) |

---

## Findings

1. **Same-engine parity is total**: identical IDs on all 14 benchmarked
   scenarios; scores ≤ 2e-4 (`text_dense` differs only by ONNX float paths).
2. **Engine identity is provable**: our shard through the official binding and
   their shard through theirs differ by ~0.02 ms; all remaining deltas are
   application-layer.
3. **The typed refactor moved every engine read from 0.20–0.92x to
   0.50–0.94x**, with five scenarios at 0.89–0.94x. Sub-ms scenarios carry
   ±~0.1x run variance.
4. **`FACET` and `optimize()` now work on edge** — both were unavailable before
   (hard error / not exposed), now at 0.76x and parity respectively.
5. **GIL release is the structural advantage** — 3.0–4.2x at 8 threads;
   official is flat at ~1,000 q/s regardless of thread count.
6. **Typed bindings matter**: flat float lists ≥32 elements now bind as
   `F32Array` with one copy; numpy remains the explicit fast path; native
   `ScoredPoint`/`ExecutionReport` removed the dict + dataclass hop.
7. **Stale builds lie**: the checked-in `.so` predated the Sep-9 runtime fix
   (32.8 ms → 1.4 ms dense); always rebuild the release extension first.

---

## Reproduce

```bash
cd vs-qdrant-edge
../vs-qdrant/.venv/bin/pip install qdrant-edge-py==0.8.0

# release extensions from the current tree
cd ..
maturin build --release -m crates/pyqql-edge/Cargo.toml -o /tmp/pyqql-edge-wheels
maturin build --release -m crates/pyqql/Cargo.toml      -o /tmp/pyqql-wheels
unzip -o /tmp/pyqql-edge-wheels/pyqql_edge-*.whl 'pyqql_edge/pyqql_edge.abi3.so' -d /tmp/ext-edge
unzip -o /tmp/pyqql-wheels/pyqql-*.whl 'pyqql/pyqql.abi3.so' -d /tmp/ext-pyqql
cp /tmp/ext-edge/pyqql_edge/pyqql_edge.abi3.so crates/pyqql-edge/pyqql_edge/
cp /tmp/ext-pyqql/pyqql/pyqql.abi3.so crates/pyqql/pyqql/
cd vs-qdrant-edge

../vs-qdrant/.venv/bin/python bench.py --reps 5 --iters 50
make quick          # --no-ingest --reps 1 --iters 5 --skip-colbert
```

## Caveats

1. Single machine, in-process, no network; ratios are the portable claim.
2. Unoptimized shards for the read scenarios (reads run before the `optimize`
   pass); at 100k+ points HNSW build policy dominates and both sides can now
   request it.
3. `text` scenarios include embedding on both sides — not engine-only.
4. Best-idiom asymmetry is intentional: official = Python lists + request
   graphs; QQL = SQL + numpy buffers + prepared/batched statements.
5. Embedding models are warm-cached; first-use download excluded.
6. Only within-folder ratios are meaningful (`vs-qdrant` is remote REST/gRPC).
