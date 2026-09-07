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

## Results — 2026-09-07 (v0.3.2)

CPU i5-10400F @ 2.90 GHz · Rust 1.98.0 · CPython 3.14.4 (`pyqql` 0.3.2) ·
Node 24.18.0 (`nqql` 0.3.2) · release builds · `taskset` pinned · median of reps.
Prior single-shot reports: `BENCHMARK_REPORT_2026-07-29.md`,
`BENCHMARK_REPORT_2026-08-28.md` (means, not medians — compare trends, not digits).

### Rust parser (`parse`, ops/s)

| Simple | Hybrid | Full | CTE | CreateColl | Upsert | DeleteWhere | OrderBy | WithPayload | Facet | Scroll | Count | Bound |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 2,022,383 | 941,182 | 318,046 | 386,075 | 590,976 | 626,684 | 1,853,692 | 1,049,437 | 748,910 | 1,243,873 | 1,598,249 | 1,939,976 | 895,270 |

### Rust explain (`explain`, ops/s)

| Simple | Hybrid | Full | CTE | CreateColl | Upsert | DeleteWhere | OrderBy | WithPayload | Facet | Scroll | Count | Bound |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 1,263,906 | 683,473 | 239,854 | 307,397 | 530,429 | 561,321 | 1,550,958 | 646,228 | 569,779 | 812,499 | 884,177 | 969,502 | 608,529 |

### Rust mock E2E (`e2e`, ops/s)

| Simple | Hybrid | Full | CTE | CreateColl | Upsert | DeleteWhere | OrderBy | WithPayload | Facet | Scroll | Count | Bound |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 384,677 | 416,591 | 166,649 | 152,311 | 282,809 | 240,739 | 444,382 | 420,200 | 260,672 | 572,018 | 612,025 | 713,553 | 319,803 |

### Rust bind/compile (`bench_bind`, ops/s)

| `bind_str` | `parse+bind_stmt` | `parse+bind+plan` | `parse_and_plan` | `compile_statement` | `format` |
|---:|---:|---:|---:|---:|---:|
| 2,143,065 | 720,751 | 506,951 | 241,075 | 151,110 | 201,697 |

String binding is ~3× cheaper than parse+AST-bind; full plan+route
(`compile_statement`, 151k ops/s) dominates the prepared-statement path.

### Rust microbenchmarks

| Operation | Throughput |
|---|---:|
| BM25 build document | 260,987 ops/s |
| BM25 build query | 831,213 ops/s |
| UPSERT parse only | 597,776 ops/s |
| UPSERT parse + route | 301,179 ops/s |
| UPSERT parse + route + JSON body | 246,867 ops/s |

Route projection ≈ 1,647 ns/op; JSON body extraction ≈ 730 ns/op.

### Python `pyqql` (ops/s)

| Query | Parse | ParseJson | Explain |
|---|---:|---:|---:|
| Simple | 1,398,693 | 881,742 | 850,654 |
| Hybrid | 821,783 | 609,219 | 558,136 |
| Full | 311,931 | 235,158 | 213,813 |
| CTE Prefetch | 378,834 | 209,157 | 264,566 |
| CreateCollection | 529,929 | 363,164 | 432,633 |
| Upsert | 544,788 | 437,174 | 438,678 |
| DeleteWhere | 1,395,251 | 1,090,558 | 1,110,749 |
| OrderBy | 870,263 | 609,557 | 523,838 |
| WithPayload | 661,632 | 496,739 | 468,947 |
| Facet | 985,833 | 841,645 | 575,683 |
| Scroll | 1,264,848 | 977,305 | 659,365 |
| Count | 1,401,399 | 1,116,090 | 696,971 |
| Bound | 741,791 | 514,485 | 455,722 |

Bound prepared path: `bind` 588,302 · `compile_query` 120,254 · `is_valid` 682,019 ops/s.

### Node `nqql` + WASM (ops/s)

| Query | NAPI `parse()` | NAPI `parseJson()` | WASM `parse()` |
|---|---:|---:|---:|
| Simple | 383,121 | 789,735 | 319,859 |
| Hybrid | 287,740 | 544,237 | 235,996 |
| Full | 159,482 | 216,128 | 98,580 |
| CTE Prefetch | 146,375 | 197,557 | 76,363 |
| CreateCollection | 231,243 | 318,237 | 178,505 |
| Upsert | 219,086 | 403,673 | 159,464 |
| DeleteWhere | 400,938 | 956,949 | 395,172 |
| OrderBy | 298,473 | 554,842 | 237,931 |
| WithPayload | 256,150 | 458,900 | 199,273 |
| Facet | 332,049 | 712,178 | 346,590 |
| Scroll | 373,731 | 885,086 | 362,182 |
| Count | 384,159 | 992,377 | 406,943 |
| Bound | 269,037 | 463,758 | 191,773 |

Bound prepared path: `bind` 542,553 · `compileQuery` 70,399 · `isValid` 598,390 · WASM `bind` 391,663 ops/s.

`parseJson()` stays 1.4–2.6× faster than `parse()` — V8 object allocation, not
Rust parsing, is the Node bottleneck. The WASM `Simple` dip seen in the
2026-09-06 single-shot run (185,964) is gone under median-of-3 (319,859):
single-shot WASM numbers carry JIT/GC noise; prefer the median.

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

# 3. Node N-API + WASM benchmark (requires built nqql; WASM optional)
#    (cd crates/nqql && npm run build)
#    wasm-pack build crates/qql-wasm --release --target nodejs --out-dir pkg-node
node bench/bench_node.js [--iterations N] [--reps N] [--filter SUBSTR] [--json]
```

Run on an otherwise idle machine. Pin each process to one CPU with
`taskset -c <cpu>` and keep the default reps (median). All harnesses share
`bench/queries.json`; `--filter` matches a case-insensitive substring of the
query name (e.g. `--filter facet`).
