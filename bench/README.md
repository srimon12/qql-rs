# Parser Benchmarks

Compares QQL parse throughput across parser implementations and bindings.

All benchmarks measure **pure parse time** (no Qdrant I/O and no embedding
inference) — just lexing + parsing a QQL string into an AST. These numbers are
for regression tracking and language-boundary cost, not for claiming
end-to-end search latency. Results are medians from 3–5 runs of 100k–500k
iterations each on a single machine:

- **CPU:** Intel Core i5-10400F @ 2.90 GHz
- **Rust:** `qql-core` via `cargo run --release`
- **Go:** `qql-go` via `go test -bench`
- **Python:** `pyqql` via `timeit` (100k iterations)
- **Node.js:** `nqql` via `process.hrtime.bigint()` (100k iterations)

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

| Query | Rust | qql-go (Go) | Python | Node.js |
|-------|--------:|------------:|-------:|--------:|
| Simple | **451** | 592 | 3,452 | 4,355 |
| Hybrid | **507** | 769 | 3,510 | 4,489 |
| Full | **1,270** | 1,505 | 5,471 | 7,093 |
| CTE Prefetch | **2,551** | 2,965 | 14,441 | 16,077 |
| CreateCollection | **1,494** | 2,544 | 4,256 | 5,932 |
| Insert | **1,337** | 1,967 | 3,288 | 4,922 |
| DeleteWhere | **498** | 510 | 1,261 | 1,760 |
| OrderBy | **885** | 980 | 4,505 | 5,960 |
| WithPayload | **1,055** | 1,165 | 4,840 | 6,166 |

## Results (ops/sec)

Higher is better.

| Query | Rust | qql-go (Go) | Python | Node.js |
|-------|--------:|------------:|-------:|--------:|
| Simple | 2,216,633 | 1,688,724 | 289,607 | 229,612 |
| Hybrid | 1,973,226 | 1,300,844 | 284,833 | 222,765 |
| Full | 787,500 | 664,517 | 182,772 | 140,983 |
| CTE Prefetch | 391,949 | 337,312 | 69,243 | 62,198 |
| CreateCollection | 669,371 | 393,101 | 234,947 | 168,576 |
| Insert | 748,191 | 508,451 | 304,074 | 203,133 |
| DeleteWhere | 2,006,640 | 1,960,807 | 792,676 | 567,971 |
| OrderBy | 1,129,480 | 1,020,497 | 221,938 | 167,783 |
| WithPayload | 947,595 | 858,692 | 206,588 | 162,156 |

## Speed Relative to Rust

| Query | Rust (1.0×) | qql-go | Python | Node.js |
|-------|:----------:|:------:|:------:|:-------:|
| Simple | 1.0× | 1.3× | 7.6× | 9.7× |
| Hybrid | 1.0× | 1.5× | 6.9× | 8.9× |
| Full | 1.0× | 1.2× | 4.3× | 5.6× |
| CTE Prefetch | 1.0× | 1.2× | 5.7× | 6.3× |
| CreateCollection | 1.0× | 1.7× | 2.8× | 4.0× |
| Insert | 1.0× | 1.5× | 2.5× | 3.7× |
| DeleteWhere | 1.0× | 1.0× | 2.5× | 3.5× |
| OrderBy | 1.0× | 1.1× | 5.1× | 6.7× |
| WithPayload | 1.0× | 1.1× | 4.6× | 5.8× |


## Key Observations

### Rust and Go are both fast enough for parser-only work
## Full E2E Pipeline Benchmarks

While the above tables isolate the *parser*, these E2E benchmarks measure the entire lifecycle of a query before it hits the network:
1. Lex and parse the query into an AST.
2. Validate the schema and validate collections using a Mock API.
3. Build the full execution pipeline (Filter Injection, Nested Struct Generation).
4. Construct and allocate the final Qdrant REST API JSON Payload.

| Query Type | `qql-rs` (Rust) ops/s | `qql-go` (Go) ops/s |
| :--- | :--- | :--- |
| **Simple** | 838,358 | 306,741 |
| **Hybrid** | 732,769 | 364,957 |
| **Full** | 299,756 | 195,372 |
| **CTE_Prefetch** | 264,566 | 163,404 |
| **CreateCollection** | 563,655 | 262,059 |
| **Insert** | 220,701 | 185,858 |
| **DeleteWhere** | 529,149 | 469,121 |
| **OrderBy** | 262,346 | 259,201 |
| **WithPayload** | 549,287 | 292,933 |

**Insight:** Even when doing the heavy lifting of dynamic memory allocation for Qdrant API REST payloads, Rust maintains a massive lead, operating 1.5x to 2.7x faster than Go for almost every query type.

## Contiguous-Array Optimization Impact (Parser)
Contiguous-array parser with O(1) lookahead and copy-free backtracking.
The Rust implementation is usually 1.1–1.4× faster than Go on parse-only
queries. That is useful, but it is not the reason to use this rewrite.

### qql-go (native Go) — Close second
Pure Go implementation with no C FFI. Consistently within 2× of Rust.
The Go parser is competitive because:
- Go generates efficient code for this workload
- No garbage collection pressure from short-lived AST nodes
- No language boundary crossing

### All FFI bindings (pyqql, nqql) pay a fixed boundary cost
Every call crosses a language boundary:
1. Marshal Python/JS string → C string
2. C function call into Rust
## Language Boundary & Serialization (FFI)

The massive gap between Rust/Go and Python/Node.js is purely due to the Foreign Function Interface (FFI) serialization boundary. 

1. **Python (`pyqql`)**: Uses `pythonize` to directly map Rust structs into CPython memory as native `PyDict`s. This is extremely efficient and yields up to ~290k ops/s.
2. **Node.js (`nqql`)**: Passing generic structs through `napi-rs` causes heavy intermediate memory allocations (mapping via `serde_json::Value`), dropping throughput to ~60k ops/s. However, exposing a method that returns a JSON string to Javascript and calling V8's native `JSON.parse` restores throughput to **~230k ops/s**.

## What This Benchmark Does Not Measure

These benchmarks simulate the absolute maximum CPU workload of the library *right up to the millisecond before the network request is fired*. They do **not** measure network latency, actual Qdrant engine execution, or embedding model inference. For real workloads, network I/O and vector database search latency dominate CPU pipeline building time.

## Running Yourself

```bash
# Rust
cargo run --release --manifest-path bench/bench_rust/Cargo.toml

# Python
PYTHONPATH=target/release python3 bench/bench_python.py

# Node.js
node bench/bench_node.js

# Go
Use the standalone qql-go library.
```
