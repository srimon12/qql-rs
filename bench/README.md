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

## Results — 2026-09-07 (v0.4.0)

CPU i5-10400F @ 2.90 GHz · Rust 1.98.0 · CPython 3.14.4 (`pyqql` 0.4.0, abi3) ·
Node 24.18.0 (`nqql` 0.4.0, NAPI-RS 3) · `wasm32-unknown-unknown` release ·
release builds · `taskset` pinned · median of reps.

Scope of this run vs the prior (v0.3.2) run: prepared statements, typed
execution results, batch orchestration via `qql_plan::BatchGrouper`, and
located parameter-error spans (`Option<Box<Span>>` on parameter AST variants —
boxed so the AST enums keep their 32-byte layout; unboxed, the FFI serialization
walk cost Python 3–11%).

### Rust parser (`parse`, ops/s)

| Simple | Hybrid | Full | CTE | CreateColl | Upsert | DeleteWhere | OrderBy | WithPayload | Facet | Scroll | Count | Bound |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 1,998,717 | 955,567 | 320,815 | 392,144 | 608,958 | 631,862 | 1,858,858 | 1,066,386 | 749,676 | 1,263,238 | 1,620,974 | 1,979,281 | 916,001 |

Parse is flat vs v0.3.2 (±3%) — span capture on parameter placeholders is
absorbed.

### Rust explanation rendering (`explain`, ops/s)

| Simple | Hybrid | Full | CTE | CreateColl | Upsert | DeleteWhere | OrderBy | WithPayload | Facet | Scroll | Count | Bound |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 1,246,788 | 679,466 | 234,680 | 294,784 | 514,207 | 546,183 | 1,501,884 | 642,033 | 537,599 | 792,713 | 840,432 | 953,325 | 554,186 |

`explain` now renders parameterized pagination (`LIMIT :name` / `?`) and
located spans, which costs 1–5% on most queries (−9% on `Bound`, the
parameter-heavy case) in exchange for plans that show their placeholders.

### Rust mock E2E (`e2e`, ops/s)

| Simple | Hybrid | Full | CTE | CreateColl | Upsert | DeleteWhere | OrderBy | WithPayload | Facet | Scroll | Count | Bound |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 402,572 | 417,208 | 172,613 | 175,751 | 293,901 | 253,904 | 747,367 | 449,738 | 287,478 | 636,230 | 657,252 | 808,635 | 360,469 |

The execute path is the big winner of 0.4.0: **+0.1% to +68%** vs v0.3.2
(DeleteWhere +68%, Count +13%, Facet +11%, CTE +15%) — typed result
normalization, the `BatchGrouper` orchestration, and the zero-allocation
unbound-parameter gate.

### Rust bind/compile (`bench_bind`, ops/s)

| `bind_str` | `parse+bind_stmt` | `parse+bind+plan` | `parse_and_plan` | `compile_statement` | `format` |
|---:|---:|---:|---:|---:|---:|
| 1,763,051 | 708,964 | 582,235 | 273,029 | 157,323 | 194,532 |

AST binding + planning is **+15%** faster than v0.3.2 (the zero-alloc
`validate_no_unbound_params` visitor). Textual binding (`bind_str`) is
−18% (466 → 567 ns/op) — the price of the triple-quoted/raw-string literal
scanner and located spans in bind errors; still ~1.8M queries/s.

### Rust microbenchmarks

| Operation | Throughput |
|---|---:|
| BM25 build document | 266,339 ops/s |
| BM25 build query | 868,270 ops/s |
| UPSERT parse only | 611,078 ops/s |
| UPSERT parse + route | 330,160 ops/s |
| UPSERT parse + route + JSON body | 267,334 ops/s |

Route projection ≈ 1,445 ns/op; JSON body extraction ≈ 726 ns/op.

### Python `pyqql` (ops/s)

| Query | Parse | ParseJson | Explain |
|---|---:|---:|---:|
| Simple | 1,504,704 | 895,148 | 901,060 |
| Hybrid | 870,761 | 587,167 | 577,000 |
| Full | 322,158 | 225,268 | 212,911 |
| CTE Prefetch | 379,597 | 211,207 | 266,782 |
| CreateCollection | 530,663 | 365,405 | 453,382 |
| Upsert | 532,043 | 439,029 | 468,508 |
| DeleteWhere | 1,405,406 | 1,027,514 | 1,179,828 |
| OrderBy | 938,627 | 624,069 | 533,269 |
| WithPayload | 694,188 | 497,809 | 474,524 |
| Facet | 1,057,363 | 816,514 | 611,463 |
| Scroll | 1,317,173 | 998,317 | 691,525 |
| Count | 1,481,977 | 1,100,107 | 699,691 |
| Bound | 740,602 | 493,446 | 446,315 |

Bound prepared path: `bind` 572,497 · `compile_query` 121,664 · `is_valid`
752,111 ops/s (`is_valid` +11% vs v0.3.2). Net vs v0.3.2: parse +0.1–7.9%,
parseJson −4.2–+2.2%, explain −2.1–+6.8% — the `Box<Span>` layout keeps the
FFI walk flat while bind errors stay located.

### Node `nqql` + WASM (ops/s)

| Query | NAPI `parse()` | NAPI `parseJson()` | WASM `parse()` |
|---|---:|---:|---:|
| Simple | 390,300 | 787,613 | 323,018 |
| Hybrid | 301,038 | 509,764 | 243,140 |
| Full | 152,335 | 209,934 | 95,643 |
| CTE Prefetch | 143,002 | 191,437 | 73,033 |
| CreateCollection | 229,624 | 321,933 | 173,044 |
| Upsert | 209,953 | 400,549 | 151,888 |
| DeleteWhere | 388,899 | 891,829 | 378,145 |
| OrderBy | 298,686 | 559,663 | 227,310 |
| WithPayload | 247,678 | 438,328 | 188,107 |
| Facet | 310,676 | 704,061 | 332,495 |
| Scroll | 367,814 | 834,874 | 357,987 |
| Count | 377,842 | 978,304 | 408,613 |
| Bound | 255,283 | 431,893 | 171,175 |

Bound prepared path: `bind` 477,124 · `compileQuery` 67,973 · `isValid`
691,180 (NAPI) · WASM `bind` 377,763 ops/s.

Node/WASM parse is flat vs v0.3.2 (−7% worst case on parameter-heavy
queries, several positive) — same 32-byte AST layout behind NAPI and the
wasm-bindgen JSON serializer.

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
