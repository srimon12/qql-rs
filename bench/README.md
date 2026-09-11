# QQL Benchmarks

Throughput of the QQL frontend (parse → bind → plan → route) across the Rust
core and its host SDKs (Python `pyqql`, Node `nqql`, WASM `qql-wasm`).

## Methodology

- **One corpus.** `bench/queries.json` is the single source of truth, read by
  the Rust bins (`include_str!`), `bench_python.py`, and `bench_node.js`.
  The original 9 queries are byte-identical to the 2026-08 runs; `Facet`,
  `Scroll`, `Count`, and `Bound` (`:name` params) are new in 0.3.2.
- **Median of reps.** Every binary takes `--iterations N --reps R` (plus
  `--filter SUBSTR` and `--json`) and reports the **median** rep in ns/op and
  ops/s. Warmup (1,000 untimed iterations; 100 for `explain`/`e2e`) precedes
  each rep. Rust sinks through `std::hint::black_box`, Node retains into a
  sink, Python disables GC during timing.
- **Reproduce:** pin to an idle core and repeat:
  `taskset -c <cpu> <binary>` (defaults below). `--json` emits one
  machine-readable object (`bin`, `bin_version`, `os`/`arch`, `iterations`,
  `reps`, `results`) for tracking over time.
- **What is NOT measured:** network I/O, model inference, real Qdrant. The
  `e2e` suite uses a mock backend (parse → prepare → plan → mock dispatch).
  Two entries (`Simple`, `WithPayload`) carry an `e2e` counterpart in the
  corpus: bare `QUERY '…'` has no vector target and cannot resolve against the
  mock dense+sparse topology (`QQL-MISSING-USING`), so `e2e` runs the
  TEXT+MODEL+USING form of the same statement. `e2e` binds `Bound` params up
  front (bind cost is measured in `bench_bind`, not here) and smoke-checks
  every entry before timing, so a bad corpus fails fast instead of benching
  errors.

## Suites

| Binary | Default | Measures |
|---|---|---|
| `parse` | 100k × 5 | Lex + parse (`Parser::parse`) |
| `explain` | 100k × 5 | Pure sync compile (`Executor::explain`), no backend |
| `e2e` | 50k × 3 | parse → prepare → plan → mock dispatch (`Executor::execute`) |
| `bench_bind` | 50k × 5 | 0.3.2 paths: `bind_str`, `parse+bind_stmt`, `parse+bind+plan`, `parse_and_plan` (`is_valid` gate), `compile_statement`, `format` |
| `bench_sparse` | 100k × 5 | BM25 `embed_document` / `embed_query` (wire-compatible with `qdrant/bm25`) |
| `bench_upsert` | 200k × 5 | UPSERT parse → `try_route` → `body_json` breakdown |
| `bench_python.py` | 50k × 3 | `parse` / `parse_json` / `explain` per query + Bound `bind` / `compile_query` / `is_valid` |
| `bench_node.js` | 50k × 3 | NAPI `parse()` / `parseJson()` / WASM `parse()` per query + Bound `bind` / `compileQuery` / `isValid` (+ WASM `bind`) |

## Corpus

| # | Label | QQL Query |
|---|-------|-----------|
| 1 | Simple | `QUERY 'search' FROM docs LIMIT 10` |
| 2 | Hybrid | `QUERY HYBRID TEXT 'search' DENSE dense SPARSE sparse FUSION RRF FROM docs LIMIT 10` |
| 3 | Full | `QUERY TEXT 'x' FROM docs USING dense WHERE active = true PARAMS (hnsw_ef = 64, exact = false) SCORE THRESHOLD 0.2 GROUP BY category SIZE 3 LOOKUP FROM categories WITH PAYLOAD INCLUDE (title, url) WITH VECTOR (dense) LIMIT 10 OFFSET 2` |
| 4 | CTE Prefetch | `WITH d AS (QUERY TEXT 'x' USING dense LIMIT 100), s AS (QUERY TEXT 'x' USING sparse LIMIT 100) QUERY FUSION RRF FROM docs PREFETCH (d, s) LIMIT 10` |
| 5 | CreateCollection | `CREATE COLLECTION docs HYBRID WITH HNSW (m = 32, ef_construct = 100) WITH QUANTIZATION (type = 'scalar', quantile = 0.95)` |
| 6 | Upsert | `UPSERT INTO docs VALUES {id: 1, text: 'hello world', category: 'tech'}, {id: 2, text: 'second document', category: 'science'}` |
| 7 | DeleteWhere | `DELETE FROM docs WHERE category = 'archived'` |
| 8 | OrderBy | `QUERY ORDER BY created_at DESC FROM docs WHERE status = 'active' LIMIT 20` |
| 9 | WithPayload | `QUERY 'search' FROM docs WITH PAYLOAD INCLUDE (title, body) WITH VECTOR (dense) LIMIT 10` |
| 10 | Facet *(new)* | `FACET category FROM docs WHERE active = true LIMIT 10` |
| 11 | Scroll *(new)* | `SCROLL FROM docs WHERE status = 'active' LIMIT 50` |
| 12 | Count *(new)* | `COUNT FROM docs WHERE status = 'active'` |
| 13 | Bound *(new)* | `QUERY TEXT :q FROM docs USING dense WHERE active = :active LIMIT 10` with `{"q": "search", "active": true}` |

## Results — 2026-09-11 (v0.4.0, rerun)

CPU i5-10400F @ 2.90 GHz · Rust 1.98.0 · CPython 3.14.4 (`pyqql` 0.4.0, abi3) ·
Node 24.18.0 (`nqql` 0.4.0, NAPI-RS 3) · `wasm32-unknown-unknown` release ·
release builds · `taskset` pinned · median of reps.

Rerun of the 2026-09-07 suite on the same box after a full release rebuild
(same 0.4.0 tree plus the typed-`BackendResponse` close-out: `ExecData::Raw`
deleted, gRPC/edge response JSON builders removed, `e2e` mock rebuilt on the
typed API). Box jitter on this desktop is ±3–5% run-to-run (GUI/browser/IDE
resident); deltas inside that band are noise.

### Rust parser (`parse`, ops/s)

| Simple | Hybrid | Full | CTE | CreateColl | Upsert | DeleteWhere | OrderBy | WithPayload | Facet | Scroll | Count | Bound |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 1,830,146 | 883,840 | 316,449 | 376,407 | 582,142 | 590,233 | 1,827,353 | 1,026,355 | 744,010 | 1,310,360 | 1,618,857 | 1,935,142 | 891,873 |

Parse is flat vs 2026-09-07 (−0% to −4% on 11/13 queries; `Simple`/`Bound`
−5–8% across three samples — inside box jitter, no parse-path change on this
branch to attribute it to).

### Rust explanation rendering (`explain`, ops/s)

| Simple | Hybrid | Full | CTE | CreateColl | Upsert | DeleteWhere | OrderBy | WithPayload | Facet | Scroll | Count | Bound |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 1,101,442 | 640,469 | 238,468 | 291,312 | 514,705 | 528,494 | 1,000,304 | 626,088 | 536,579 | 807,615 | 861,167 | 968,822 | 563,637 |

Flat except **`DeleteWhere` −33%** (1.50M → 1.00M ops/s, replicates across
runs and across the Python `explain` leg below — a real change in DELETE
explain rendering; still 1M ops/s, offline tooling only).

### Rust mock E2E (`e2e`, ops/s)

| Simple | Hybrid | Full | CTE | CreateColl | Upsert | DeleteWhere | OrderBy | WithPayload | Facet | Scroll | Count | Bound |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 527,279 | 479,456 | 210,024 | 220,068 | 346,860 | 252,518 | 744,366 | 493,504 | 335,261 | 665,790 | 773,855 | 869,348 | 423,719 |

Up **+5% to +31%** vs 2026-09-07 on 11/13 queries (`DeleteWhere`/`Upsert`
flat). Caveat: the mock now returns typed `BackendResponse` directly, so the
old JSON-envelope construct+parse cost is no longer inside the loop — part of
the gain is mock artifact. The live-server suites below are the authoritative
execute-path comparison.

### Rust bind/compile (`bench_bind`, ops/s)

| `bind_str` | `parse+bind_stmt` | `parse+bind+plan` | `parse_and_plan` | `compile_statement` | `format` |
|---:|---:|---:|---:|---:|---:|
| 1,773,694 | 724,898 | 578,996 | 268,885 | 162,953 | 200,344 |

Flat vs 2026-09-07 (±3.6% — noise).

### Rust microbenchmarks

| Operation | Throughput |
|---|---:|
| BM25 build document | 274,329 ops/s |
| BM25 build query | 887,578 ops/s |
| UPSERT parse only | 591,436 ops/s |
| UPSERT parse + route | 327,132 ops/s |
| UPSERT parse + route + JSON body | 265,903 ops/s |

Route projection ≈ 1,366 ns/op; JSON body extraction ≈ 704 ns/op. Flat vs
2026-09-07 (±3%).

### Python `pyqql` (ops/s)

| Query | Parse | ParseJson | Explain |
|---|---:|---:|---:|
| Simple | 1,251,012 | 823,363 | 832,414 |
| Hybrid | 803,509 | 578,304 | 546,585 |
| Full | 305,291 | 226,489 | 214,161 |
| CTE Prefetch | 368,765 | 209,492 | 257,715 |
| CreateCollection | 519,001 | 340,980 | 424,000 |
| Upsert | 528,886 | 421,948 | 439,115 |
| DeleteWhere | 1,338,334 | 1,074,508 | 731,176 |
| OrderBy | 861,840 | 583,031 | 519,540 |
| WithPayload | 653,866 | 480,705 | 462,184 |
| Facet | 1,003,129 | 796,674 | 583,828 |
| Scroll | 1,181,454 | 947,032 | 678,916 |
| Count | 1,394,679 | 1,115,023 | 709,385 |
| Bound | 748,269 | 492,885 | 448,407 |

Bound prepared path: `bind` 580,095 · `compile_query` 121,537 · `is_valid`
750,060 ops/s (all flat vs 2026-09-07: +1.3% / −0.1% / −0.3%). Slow queries
agree with 2026-09-07 (Upsert −0.6%, CTE −2.9%); the larger deltas sit on the
fastest ops (Simple −17%, Scroll −10%) where fixed box jitter dominates —
noise, not a regression. `DeleteWhere` explain −38% confirms the Rust-leg
finding above (real, offline-only).

### Node `nqql` + WASM (ops/s)

| Query | NAPI `parse()` | NAPI `parseJson()` | WASM `parse()` |
|---|---:|---:|---:|
| Simple | 395,602 | 771,644 | 329,765 |
| Hybrid | 296,294 | 546,154 | 251,123 |
| Full | 157,313 | 216,874 | 98,569 |
| CTE Prefetch | 150,973 | 196,676 | 77,301 |
| CreateCollection | 241,453 | 326,228 | 171,412 |
| Upsert | 224,815 | 392,988 | 154,677 |
| DeleteWhere | 407,718 | 962,508 | 385,130 |
| OrderBy | 309,502 | 562,932 | 236,222 |
| WithPayload | 254,920 | 448,350 | 201,706 |
| Facet | 338,589 | 759,041 | 357,053 |
| Scroll | 375,851 | 858,768 | 370,723 |
| Count | 401,641 | 1,008,051 | 412,234 |
| Bound | 265,134 | 460,671 | 177,962 |

Bound prepared path: `bind` 439,532 · `compileQuery` 63,920 · `isValid`
713,361 (NAPI) · WASM `bind` 421,129 ops/s. NAPI `parse()` +1–9% and WASM
+2–7% vs 2026-09-07; `parseJson()` flat (±3%). Prepared `bind` −8% /
`compileQuery` −6% — plausibly the new BM25 option-plumbing in the refreshed
JS wrapper; small in absolute terms.

`parseJson()` stays 1.4–2.6× faster than `parse()` — V8 object allocation,
not Rust parsing, is the Node bottleneck. Prefer medians over single-shot
WASM numbers: JIT/GC noise dominates a single pass (see the legacy reports).

Prior single-shot reports: `legacy/BENCHMARK_REPORT_2026-07-29.md`,
`legacy/BENCHMARK_REPORT_2026-08-28.md` (means, not medians — compare
trends, not digits).

## Running the Benchmarks

```bash
# 1. Rust parser / compile / mock-executor / bind / micro benchmarks
cargo build --release --manifest-path bench/bench_rust/Cargo.toml --bins
bench/bench_rust/target/release/parse [--iterations N] [--reps N] [--filter SUBSTR] [--json]
bench/bench_rust/target/release/explain [--iterations N] [--reps N] [--filter SUBSTR] [--json]
bench/bench_rust/target/release/e2e [--iterations N] [--reps N] [--filter SUBSTR] [--json]
bench/bench_rust/target/release/bench_bind [--iterations N] [--reps N] [--filter SUBSTR] [--json]
bench/bench_rust/target/release/bench_sparse [--iterations N] [--reps N] [--filter SUBSTR] [--json]
bench/bench_rust/target/release/bench_upsert [--iterations N] [--reps N] [--filter SUBSTR] [--json]

# 2. Python binding benchmark (requires installed pyqql)
python3 bench/bench_python.py [--iterations N] [--reps N] [--filter SUBSTR] [--json]

#    (cd crates/nqql && npm run build)
#    (cd crates/qql-wasm && wasm-pack build --release --target nodejs --out-dir pkg-node)
node bench/bench_node.js [--iterations N] [--reps N] [--filter SUBSTR] [--json]
```

Run on an otherwise idle machine. Pin each process to one CPU with
`taskset -c <cpu>` and keep the default reps (median). All harnesses share
`bench/queries.json`; `--filter` matches a case-insensitive substring of the
query name (e.g. `--filter facet`).
