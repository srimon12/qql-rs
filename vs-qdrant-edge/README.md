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

## TL;DR (Qdrant Edge 0.8.0, localhost, 2026-09-11 — release rebuild + rerun)

| | official `qdrant-edge-py` 0.8.0 | `pyqql-edge` 0.4.0 | qql / official |
|---|---:|---:|---:|
| **Engine reads, 1 thread (12 scenarios, precomputed vectors)** | — | **0 / 12** | best 0.96x (`count_berlin`, `dense_filtered`); worst 0.49x (`retrieve_points`) |
| **Engine reads, 8 Python threads (same shards)** | 1,109 q/s — flat | **4,445 q/s** | **4.01x faster — GIL released vs held** (3.0–4.2x across runs) |
| **Text layer** | — | — | `text_sparse` 1.04x (0.95–1.25x across runs); `text_dense` 0.79x |
| **`FACET district`** | 0.019 ms | 0.025 ms | 0.77x |
| **`optimize()`** | 519.9 ms | 446.3 ms | **1.16x this run** (one-shot merges are noisy, 0.79–1.16x across runs) |
| Ingest total (prep + submit), berlin 8k | 1.45 s | 1.29 s | **1.12x faster** |
| Ingest total, legal 2k + ColBERT | 1.02 s | 0.70 s | **1.46x faster** |
| Cold import | 11.6 ms | 19.7 ms | 0.59x |
| Application LOC (17 scenarios) | 277 | **164** | **0.59x** |

Single-thread reads sit at 0.49–0.96x. Sub-millisecond scenarios carry ±~0.1x
run-to-run variance (`count_legal` 0.71–0.94x across runs); the concurrency,
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
    Collections(Vec<String>),                  // SHOW COLLECTIONS
    Collection(CollectionInfo),                // SHOW COLLECTION
    ShardKeys(Vec<PlanShardKey>),              // SHOW SHARD KEYS
    Quotas(QuotaConfig),                       // SHOW / SET QUOTA
}
```

Deleted with no compatibility layer retained: the JSON `execute_planned ->
Value` and `execute_planned_typed` (one typed method remains),
`BackendResponse::from_envelope`, the shared `envelope.rs` parser, all 10
gRPC + all edge JSON response builders, the edge `spawn_blocking` hop
(engine calls are synchronous; callers detach the GIL), the Python
`_dx_report.py` hop (native `ExecutionReport`/`ScoredPoint` directly),
proto→JSON metadata builders, and the formula/DDL `Value` round-trips
(`PlanFormula` and every DDL config are plan-owned types straight to
protobuf). REST parses each response against its OpenAPI shape in the
crate-private `rest_response` module and fails closed with
`QQL-BACKEND-ENVELOPE`. Scores serialize as shortest f32 (`0.95`, not
`0.949999988079071`) — JSON view and typed view agree.

**Remaining JSON is at real boundaries only:** REST wire bodies, point payload
values (schemaless by Qdrant's model), the `--json` report export, and the
JS/WASM object layer. No request config or response envelope is JSON-as-IR
anymore.

New capabilities from the same refactor: **`FACET` works on edge** (was a hard
error) and **`optimize()` is exposed** (`Client.optimize(collection)`, edge
only; 446 ms vs official 520 ms this run — one-shot segment merges are noisy,
0.79–1.16x across runs).

Measured effect of the whole refactor (before/after, qql/official):

| scenario | before | after (range over runs) |
|---|---:|---:|
| query_dense | 0.76x | **0.81x** (0.81–1.01) |
| query_dense_filtered | 0.92x | **0.96x** (0.91–0.99) |
| query_sparse | 0.44x | **0.72x** (0.65–0.72) |
| query_hybrid | 0.76x | **0.81x** (0.81–0.87) |
| count_berlin | 0.79x | **0.96x** (0.84–1.01) |
| count_legal | 0.71x | **0.76x** (0.71–0.94) |
| facet_district | `QQL-EDGE-FACET` error | **0.77x** (0.76–0.82) |
| scroll_pages | 0.37x | **0.88x** (0.88–0.94) |
| retrieve_points | 0.20x | **0.49x** (0.31–0.50) |
| prepared_rerun | 0.80x | **0.91x** (0.82–0.91) |
| batch_reads | 0.82x | **0.92x** (0.81–0.92) |
| query_colbert | 0.90x | 0.91x (0.89–0.91) |
| text_sparse | 0.54x | **1.04x** (0.95–1.25) |
| text_dense | 0.78x | 0.79x (0.73–0.87) |
| optimize | not exposed | 0.79–1.16x (446–542 ms vs 430–520 official) |
| cold import | 31.9 ms | **19.7 ms** |

### Remaining backlog

Single open item: single-thread engine reads trail at 0.49–0.96x — the
parse→plan→normalize application layer over the same engine (identity proven:
our shard through the official binding differs by ~0.02 ms). The structural
win is concurrency (GIL detached, 4.01x at 8 threads), not shaving the
single-call path. Closed: flat-list `F32Array` binding, typed gRPC/edge
writes and DDL, sync edge path (no `spawn_blocking`), `optimize()`,
schemaless metadata variants (`ExecData::Raw` deleted).

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
| query_dense | 1,179.4 | 957.5 | 0.81x | 0.848 ms | 1.044 ms | ids exact, Δscore 0 |
| query_dense_filtered | 118.2 | 113.6 | 0.96x | 8.459 ms | 8.801 ms | ids exact, Δscore 0 |
| query_sparse | 10,628.3 | 7,644.1 | 0.72x | 0.094 ms | 0.131 ms | ids exact, Δscore 2e-6 |
| query_hybrid (RRF) | 1,004.4 | 811.0 | 0.81x | 0.996 ms | 1.233 ms | ids exact, Δscore 0 |
| count_berlin | 2,687.3 | 2,583.2 | 0.96x | 0.372 ms | 0.387 ms | exact |
| count_legal | 13,883.1 | 10,600.2 | 0.76x | 0.072 ms | 0.094 ms | exact |
| facet_district | 51,599.6 | 39,482.0 | 0.77x | 0.019 ms | 0.025 ms | exact |
| scroll_pages (3×256) | 424.2 | 374.7 | 0.88x | 2.357 ms | 2.669 ms | exact 768 ids |
| retrieve_points (10 ids) | 43,871.2 | 21,425.7 | 0.49x | 0.023 ms | 0.047 ms | exact ids + payloads |
| prepared_rerun (4 vectors) | 290.6 | 263.9 | 0.91x | 3.442 ms | 3.790 ms | ids exact, Δscore 0 |
| batch_reads (4 statements) | 281.2 | 259.7 | 0.92x | 3.556 ms | 3.851 ms | ids exact, Δscore 0 |
| query_colbert | 70.7 | 64.3 | 0.91x | 14.139 ms | 15.543 ms | ids exact, Δscore 0 |

### Threaded throughput — same shard, 40 queries per thread

| threads | official q/s | qql q/s | qql/official |
|---:|---:|---:|---:|
| 1 | 1,080 | 953 | 0.88x |
| 2 | 1,113 | 1,676 | 1.51x |
| 4 | 1,038 | 3,188 | 3.07x |
| 8 | 1,109 | 4,445 | **4.01x** |

### Text → vector layer (both sides embed in-path; not engine-only)

| scenario | official ops/s | qql ops/s | qql/official | p50 official | p50 qql | parity |
|---|---:|---:|---:|---:|---:|---|
| text_dense | 163.7 | 129.7 | 0.79x | 6.108 ms | 7.710 ms | ids exact, Δscore 1.1e-4 |
| text_sparse | 7,997.5 | 8,312.0 | 1.04x | 0.125 ms | 0.120 ms | ids exact, Δscore 2e-6 |

`text_sparse` oscillates around parity (0.95–1.25x across runs, 1.04x this
run): same engine, same BM25 semantics (parity proves identical sparse
vectors), QQL's native-hit path removed the former 0.54x deficit.

### Ingest — submission throughput

| collection | official prep | official submit | qql prep | qql submit | end-to-end |
|---|---:|---:|---:|---:|---:|
| berlin 8,000 (dense + BM25) | 0.36 s | 1.09 s | 0.005 s | 1.28 s | **qql 1.12x faster** |
| legal 2,000 (dense + BM25 + ColBERT) | 0.62 s | 0.41 s | 0.002 s | 0.70 s | **qql 1.46x faster** |

### Capabilities / startup / size

| | official | qql |
|---|---:|---:|
| `optimize()` on the 8k shard | 519.9 ms (`changed=True`) | 446.3 ms (`changed=True`) — 1.16x this run |
| cold import (module) | 11.6 ms | 19.7 ms |
| executor / embedder construction, warm cache | 0.54 s (fastembed Python, when used) | 0.32 s (executor + FastEmbed-rs) |
| scenario application LOC | 277 | 164 (0.59x) |

---

## Findings

1. **Same-engine parity is total**: identical IDs on all 14 benchmarked
   scenarios; scores ≤ 2e-4 (`text_dense` differs only by ONNX float paths).
2. **Engine identity is provable**: our shard through the official binding and
   their shard through theirs differ by ~0.02 ms; all remaining deltas are
   application-layer.
3. **The typed refactor moved every engine read from 0.20–0.92x to
   0.49–0.96x**, with five scenarios at 0.91–0.96x. Sub-ms scenarios carry
   ±~0.1x run variance.
4. **`FACET` and `optimize()` now work on edge** — both were unavailable before
   (hard error / not exposed); facet now measures 0.77x and optimize ranges
   0.79–1.16x across runs (segment merges are one-shot and noisy).
5. **GIL release is the structural advantage** — 3.0–4.2x at 8 threads;
   official is flat at ~1,000 q/s regardless of thread count.
6. **Typed bindings matter**: flat float lists ≥32 elements now bind as
   `F32Array` with one copy; numpy remains the explicit fast path; native
   `ScoredPoint`/`ExecutionReport` removed the dict + dataclass hop.
7. **Stale builds mislead**: the checked-in `.so` predated the Sep-9 runtime fix
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
7. Both contenders are release builds for this run (rebuilt `pyqql-edge` abi3
   extension + vendored `qdrant-edge-py` 0.8.0); the benchmark runs one leg at
   a time with no other load.
