# QQL Benchmarks

Compares QQL parser, explain, and E2E throughput for Rust. The queries use the current (post-refactor) syntax with strict clause ordering and `QueryExpr`-based grammar.

**These numbers are NOT directly comparable to the previous benchmark results** because the query syntax changed significantly (strict clause order, `PARAMS` replacing `WITH`, `HYBRID TEXT` replacing `USING HYBRID`, etc.) and bench queries were updated to match valid current grammar.

- **CPU:** Intel Core i5-10400F @ 2.90 GHz
- **Rust:** `qql-rs` (v0.1.0)
- **Date:** 2026-07-22 (post-refactor)

---

## Queries

| # | Label | QQL |
|---|-------|-----|
| 1 | Simple | `QUERY 'search' FROM docs LIMIT 10` |
| 2 | Hybrid | `QUERY HYBRID TEXT 'search' DENSE dense SPARSE sparse FUSION RRF FROM docs LIMIT 10` |
| 3 | Full | `QUERY TEXT 'x' FROM docs USING dense WHERE active = true PARAMS (hnsw_ef = 64, exact = false) SCORE THRESHOLD 0.2 GROUP BY category SIZE 3 LOOKUP FROM categories WITH PAYLOAD INCLUDE (title, url) WITH VECTOR (dense) LIMIT 10 OFFSET 2` |
| 4 | CTE Prefetch | `WITH d AS (QUERY TEXT 'x' USING dense LIMIT 100), s AS (QUERY TEXT 'x' USING sparse LIMIT 100) QUERY FUSION RRF FROM docs PREFETCH (d, s) LIMIT 10` |
| 5 | CreateCollection | `CREATE COLLECTION docs HYBRID WITH HNSW (m = 32, ef_construct = 100) WITH QUANTIZATION (type = 'scalar', quantile = 0.95)` |
| 6 | Upsert | `UPSERT INTO docs VALUES {id: 1, text: 'hello world', category: 'tech'}, {id: 2, text: 'second document', category: 'science'}` |
| 7 | DeleteWhere | `DELETE FROM docs WHERE category = 'archived'` |
| 8 | OrderBy | `QUERY ORDER BY created_at DESC FROM docs WHERE status = 'active' LIMIT 20` |
| 9 | WithPayload | `QUERY 'search' FROM docs WITH PAYLOAD INCLUDE (title, body) WITH VECTOR (dense) LIMIT 10` |

---

## Rust Benchmarks (100k iterations, release mode)

| Query | Parse (ops/s) | Explain (ops/s) | E2E (ops/s) |
|-------|:----------:|:-------------:|:---------:|
| Simple | 2,091,144 | 1,321,717 | 915,390 |
| Hybrid | 977,886 | 758,098 | 537,460 |
| Full | 340,416 | 273,785 | 244,065 |
| CTE Prefetch | 431,582 | 352,446 | 342,284 |
| CreateCollection | 556,756 | 582,723 | 471,631 |
| Upsert | 576,298 | 618,178 | 411,258 |
| DeleteWhere | 1,501,050 | 1,586,279 | 1,001,126 |
| OrderBy | 952,654 | 771,678 | 565,801 |
| WithPayload | 801,167 | 658,371 | 525,192 |

### BM25 Sparse Benchmark (100k iterations)

| Operation | Time | ops/sec |
|-----------|------|---------|
| Build Document | 66.92ms | 1,494,330 |
| Build Query | 21.81ms | 4,586,083 |

### Pipeline Breakdown

- **Parse**: Pure lexing + parsing into typed AST. No serialization, no routing.
- **Explain**: Parse + explain formatting (produces a human-readable plan string). Heavy for complex queries due to AST introspection.
- **E2E**: Parse + route + `QdrantOps::execute_route()` with a mock backend. Full execution pipeline right up to (but not including) the HTTP call. Heavy for upsert due to collection auto-creation logic.

---

## Running the Benchmarks

```bash
# Rust
cargo build --release --manifest-path bench/bench_rust/Cargo.toml --bin parse
cargo build --release --manifest-path bench/bench_rust/Cargo.toml --bin explain
cargo build --release --manifest-path bench/bench_rust/Cargo.toml --bin e2e
cargo build --release --manifest-path bench/bench_rust/Cargo.toml --bin bench_sparse

./bench/bench_rust/target/release/parse
./bench/bench_rust/target/release/explain
./bench/bench_rust/target/release/e2e
./bench/bench_rust/target/release/bench_sparse

# Python (requires pyqql built)
PYTHONPATH=target/release python3 bench/bench_python.py

# Node.js (requires nqql built)
node bench/bench_node.js
```
