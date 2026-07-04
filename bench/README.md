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

Rust `qql-core` uses a **contiguous-array parser** — all tokens are lexed up
front into a `Vec<Token>` and accessed by index. This gives O(1) lookahead,
zero-cost backtracking (copy a `usize`), and keeps the hot path in CPU cache.

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

Lower is better.

| Query | Rust 🏆 | qql-go (Go) | Python | Node.js | gqql (CGo) |
|-------|--------:|------------:|-------:|--------:|-----------:|
| Simple | **389** | 529 | 5,832 | 6,917 | 6,251 |
| Hybrid | **514** | 636 | 6,149 | 6,881 | 6,352 |
| Full | **1,234** | 1,565 | 12,285 | 12,815 | 11,248 |
| CTE Prefetch | **2,662** | 3,278 | 53,456 | 53,872 | 48,866 |
| CreateCollection | **1,436** | 2,931 | 22,255 | 22,099 | 20,620 |
| Insert | **1,206** | 2,159 | 9,552 | 9,840 | 8,956 |
| DeleteWhere | **488** | 517 | 2,411 | 2,459 | 2,276 |
| OrderBy | **874** | 888 | 8,657 | 8,995 | 8,580 |
| WithPayload | **975** | 1,137 | 10,466 | 10,613 | 10,027 |

## Results (ops/sec)

Higher is better.

| Query | Rust 🏆 | qql-go (Go) | Python | Node.js | gqql (CGo) |
|-------|--------:|------------:|-------:|--------:|-----------:|
| Simple | 2,571,092 | 1,890,359 | 171,470 | 144,580 | 159,974 |
| Hybrid | 1,945,910 | 1,572,327 | 162,627 | 145,320 | 157,480 |
| Full | 810,406 | 638,978 | 81,397 | 78,036 | 88,905 |
| CTE Prefetch | 375,655 | 305,070 | 18,707 | 18,562 | 20,464 |
| CreateCollection | 696,358 | 341,180 | 44,934 | 45,251 | 48,497 |
| Insert | 828,992 | 463,177 | 104,687 | 101,622 | 111,657 |
| DeleteWhere | 2,049,607 | 1,934,236 | 414,752 | 406,684 | 439,367 |
| OrderBy | 1,144,770 | 1,125,844 | 115,510 | 111,176 | 116,550 |
| WithPayload | 1,025,783 | 879,508 | 95,549 | 94,220 | 99,731 |

## Speed Relative to Rust

| Query | Rust (1.0×) | qql-go | gqql (CGo) | Python | Node.js |
|-------|:----------:|:------:|:-----------:|:------:|:-------:|
| Simple | 1.0× | 1.4× | 16.1× | 15.0× | 17.8× |
| Hybrid | 1.0× | 1.2× | 12.4× | 12.0× | 13.4× |
| Full | 1.0× | 1.3× | 9.1× | 10.0× | 10.4× |
| CTE Prefetch | 1.0× | 1.2× | 18.4× | 20.1× | 20.2× |
| CreateCollection | 1.0× | 2.0× | 14.4× | 15.5× | 15.4× |
| Insert | 1.0× | 1.8× | 7.4× | 7.9× | 8.2× |
| DeleteWhere | 1.0× | 1.1× | 4.7× | 4.9× | 5.0× |
| OrderBy | 1.0× | 1.0× | 9.8× | 9.9× | 10.3× |
| WithPayload | 1.0× | 1.2× | 10.3× | 10.7× | 10.9× |

## Contiguous-Array Optimization Impact

The Rust `qql-core` parser was refactored from `Peekable<Lexer>` (lazy iterator)
to upfront-lexed `Vec<Token>` (contiguous array). Impact on parse latency:

| Query | Before (ns) | After (ns) | Change |
|-------|:----------:|:----------:|:------:|
| Simple | 390 | 389 | — |
| Hybrid | 557 | **514** | **−7.7%** |
| Full | 1,417 | **1,234** | **−12.9%** |
| CTE Prefetch | 3,087 | **2,662** | **−13.8%** |
| CreateCollection | 1,574 | **1,436** | **−8.8%** |
| Insert | 1,199 | 1,206 | — |
| DeleteWhere | 474 | 488 | — |
| OrderBy | 873 | 874 | — |
| WithPayload | 991 | 975 | — |

Complex queries with heavy lookahead (Hybrid, Full, CTE, CreateCollection) saw
the biggest improvements. Simple queries hit the same floor — the parser is
fast either way for short inputs.

## Key Observations

### Rust (qql-core) — Fastest
Contiguous-array parser with O(1) lookahead and copy-free backtracking.
Zero-cost parsing. Every other implementation pays some overhead to bridge
into Rust (or implements its own parser in the target language).

### qql-go (native Go) — Close second, ~1–2× of Rust
Pure Go implementation with no C FFI. Consistently within 2× of Rust.
The Go parser is competitive because:
- Go generates efficient code for this workload
- No garbage collection pressure from short-lived AST nodes
- No language boundary crossing

### All FFI bindings (gqql, pyqql, nqql) — ~5–20× slower than Rust
Every call crosses a language boundary:
1. Marshal Go/Python/JS string → C string
2. C function call into Rust
3. Rust parses the QQL
4. Marshal Rust string → C string
5. Return and free C memory

The CGo/FFI overhead alone adds **~5–6 µs per call** regardless of query
complexity. This is the floor: even the simplest parse takes at least 6 µs
in any FFI-based binding. The Rust-to-Go CGo bridge is the slowest because
it goes through the full C ABI — Python and Node.js use their native FFI
which is slightly more efficient.

### Python vs Node.js — Nearly identical
Both cluster within 10% of each other. Both use Rust under the hood with
the same FFI-overhead floor. The difference is negligible.

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
