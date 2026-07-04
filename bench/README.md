# Parser Benchmarks

Compares QQL parse throughput across all language SDKs.

All benchmarks measure **pure parse time** (no Qdrant I/O) — just lexing + parsing
a QQL string into an AST. Results are medians from 3–5 runs of 100k–500k
iterations each on a single machine:

- **CPU:** Intel Core i5-10400F @ 2.90 GHz
- **Rust:** `qql-core` via `cargo run --release`
- **Go:** `qql-go` via `go test -bench`
- **Python:** `pyqql` via `timeit` (100k iterations)
- **Node.js:** `nqql` via `process.hrtime.bigint()` (100k iterations)
- **gqql (CGo):** Rust C FFI called from Go via `go test -bench`

## Queries

| # | Label | QQL |
|---|-------|-----|
| 1 | Simple | `QUERY 'search' FROM docs LIMIT 10` |
| 2 | Hybrid | `QUERY 'search' FROM docs LIMIT 10 USING HYBRID` |
| 3 | Full | `QUERY 'vector search' FROM docs LIMIT 10 OFFSET 5 USING HYBRID RERANK WHERE topic = 'search' WITH (hnsw_ef = 128, exact = true)` |
| 4 | CTE Prefetch | `WITH a AS (QUERY 'search' USING dense LIMIT 100 WHERE category = 'tech'), b AS (QUERY 'search' USING sparse LIMIT 100) QUERY 'search' FROM docs LIMIT 10 PREFETCH (a WHERE priority = 'high' SCORE THRESHOLD 0.8, b SCORE THRESHOLD 0.5) FUSION RRF` |
| 5 | CreateCollection | `CREATE COLLECTION docs HYBRID WITH HNSW (m = 32, ef_construct = 100) WITH QUANTIZATION (type = 'scalar', quantile = 0.95)` |
| 6 | Insert | `INSERT INTO docs VALUES {id: 1, text: 'hello world', category: 'tech'}, {id: 2, text: 'second document', category: 'science'}` |
| 7 | DeleteWhere | `DELETE FROM docs WHERE category = 'archived'` |
| 8 | OrderBy | `QUERY ORDER BY created_at DESC FROM docs LIMIT 20 WHERE status = 'active'` |
| 9 | WithPayload | `QUERY 'search' FROM docs LIMIT 10 WITH PAYLOAD (include = ['title', 'body']) WITH VECTORS ('dense')` |

## Results (ns/op)

| Query | Rust 🏆 | qql-go (Go) | Python | Node.js | gqql (CGo) |
|-------|--------:|------------:|-------:|--------:|-----------:|
| Simple | 390 | 720 | 5,833 | 7,026 | 6,284 |
| Hybrid | 557 | 780 | 6,725 | 7,049 | 6,407 |
| Full | 1,417 | 1,435 | 12,033 | 13,536 | 11,439 |
| CTE Prefetch | 3,087 | 2,999 | 56,793 | 58,740 | 50,673 |
| CreateCollection | 1,574 | 2,667 | 22,280 | 23,406 | 20,906 |
| Insert | 1,199 | 2,041 | 9,647 | 10,096 | 8,950 |
| DeleteWhere | 474 | 512 | 2,116 | 2,567 | 2,265 |
| OrderBy | 873 | 887 | 9,037 | 9,172 | 8,269 |
| WithPayload | 991 | 1,135 | 10,587 | 11,483 | 9,976 |

## Results (ops/sec)

Higher is better.

| Query | Rust 🏆 | qql-go (Go) | Python | Node.js | gqql (CGo) |
|-------|--------:|------------:|-------:|--------:|-----------:|
| Simple | 2,560,823 | 1,388,889 | 171,442 | 142,326 | 159,135 |
| Hybrid | 1,795,876 | 1,282,051 | 148,705 | 141,862 | 156,006 |
| Full | 705,809 | 697,000 | 83,107 | 73,875 | 87,437 |
| CTE Prefetch | 323,920 | 333,444 | 17,608 | 17,024 | 19,734 |
| CreateCollection | 635,171 | 375,000 | 44,883 | 42,725 | 47,847 |
| Insert | 833,886 | 490,000 | 103,659 | 99,051 | 111,731 |
| DeleteWhere | 2,108,317 | 1,953,125 | 472,563 | 389,509 | 441,501 |
| OrderBy | 1,145,370 | 1,127,395 | 110,654 | 109,031 | 120,934 |
| WithPayload | 1,009,163 | 881,057 | 94,457 | 87,087 | 100,240 |

## Speed Relative to Rust

| Query | Rust (1.0×) | qql-go | gqql (CGo) | Python | Node.js |
|-------|:----------:|:------:|:-----------:|:------:|:-------:|
| Simple | 1.0× | 1.8× | 16.1× | 15.0× | 18.0× |
| Hybrid | 1.0× | 1.4× | 11.5× | 12.1× | 12.7× |
| Full | 1.0× | 1.0× | 8.1× | 8.5× | 9.6× |
| CTE Prefetch | 1.0× | 1.0× | 16.4× | 18.4× | 19.0× |
| CreateCollection | 1.0× | 1.7× | 13.3× | 14.2× | 14.9× |
| Insert | 1.0× | 1.7× | 7.5× | 8.0× | 8.4× |
| DeleteWhere | 1.0× | 1.1× | 4.8× | 4.5× | 5.4× |
| OrderBy | 1.0× | 1.0× | 9.5× | 10.4× | 10.5× |
| WithPayload | 1.0× | 1.1× | 10.1× | 10.7× | 11.6× |

## Key Observations

### Rust (qql-core) — Fastest
Native Rust with no FFI boundary. Zero-cost parsing. Every other implementation
pays some overhead to bridge into Rust (or implements its own parser in the
target language).

### qql-go (native Go) — Close second, ~1–1.8× Rust
Pure Go implementation with no C FFI. Within 2× of Rust on every query type.
On complex queries (Full, CTE Prefetch, OrderBy, WithPayload) it's within 1–1.1×
of Rust. The Go parser is competitive because:
- Go's compiler generates efficient code for this workload
- No garbage collection pressure from short-lived AST nodes
- No language boundary crossing

### All FFI bindings (gqql, pyqql, nqql) — ~8–19× slower than Rust
Every call crosses a language boundary:
1. Marshal Go/Python/JS string → C string
2. C function call into Rust
3. Rust parses the QQL
4. Marshal Rust string → C string
5. Return and free C memory

The CGo/FFI overhead alone adds **~5–6 µs per call** regardless of query
complexity. This is the floor: even the simplest parse takes at least 6 µs
in any FFI-based binding.

### Python vs Node.js — Nearly identical
Both pyqql and nqql cluster within 10% of each other. Both use Rust under the
hood with the same FFI-overhead floor. The difference is negligible.

## Running Yourself

```bash
# Rust
cargo run --release --manifest-path bench/bench_rust/Cargo.toml

# Python
PYTHONPATH=target/release python3 bench/bench_python.py

# Node.js
node bench/bench_node.js

# Go (qql-go + gqql)
cd bench && CGO_LDFLAGS="-L../target/release -l:libgqql.a -lm" go test -bench=. -benchmem
```
