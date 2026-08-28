# QQL Benchmarks

Compares QQL throughput across different parser implementations, runtimes, and host SDK languages (Rust, Python, Node.js, WebAssembly, Go).

Benchmarks are split into two categories:
1. **Isolated Parser Benchmarks**: Pure lexing and parsing of QQL query strings into an AST (no network I/O, no schema compilation, no payload construction).
2. **Full E2E Pipeline Benchmarks**: The complete query compilation lifecycle right up to the millisecond before sending the network request (parsing, filter injection, schema validation, and Qdrant REST JSON payload construction).

- **CPU:** Intel Core i5-10400F @ 2.90 GHz (6 cores / 12 threads)
- **Rust:** `qql-rs` (v0.2.2)
- **Go:** `qql-go` (v0.1.2)
- **Python:** `pyqql` (v0.2.2 PyO3 0.29.2, `abi3-py310`)
- **Node.js:** `nqql` (v0.2.2 N-API 3.12.2)
- **WASM:** `qql-wasm` (v0.2.2 `wasm32-unknown-unknown`)
- **Date:** August 2026 (v0.2.2 Release Verification)

---

## Queries

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

---

## 1. Parser Benchmarks (ops/sec)
*Isolates lexing & parsing throughput. Higher is better.*

| Query | Rust (`qql-rs`) | Python (`pyqql`) | Go (`qql-go`) | Node.js `parse()` | Node.js `parseJson()` | WASM (`qql-wasm`) |
|---|:---:|:---:|:---:|:---:|:---:|:---:|
| **Simple** | **1,228,994** | 1,438,309 | 1,688,724 | 404,342 | **715,511** | 287,115 |
| **Hybrid** | **523,014** | 819,801 | 1,300,844 | 247,170 | **552,628** | 238,740 |
| **Full** | **294,125** | 326,364 | 664,517 | 161,240 | **216,254** | 98,047 |
| **CTE Prefetch** | **375,234** | 384,043 | 337,312 | 154,960 | **200,567** | 78,257 |
| **CreateCollection** | **584,513** | 561,740 | 393,101 | 234,023 | **337,778** | 175,765 |
| **Upsert** | **631,662** | 567,949 | 508,451 | 197,621 | **403,572** | 158,544 |
| **DeleteWhere** | **1,431,167** | 1,393,998 | 1,960,807 | 314,419 | **884,023** | 404,971 |
| **OrderBy** | **984,982** | 938,385 | 1,020,497 | 304,958 | **551,846** | 237,489 |
| **WithPayload** | **578,997** | 635,220 | 858,692 | 246,423 | **458,311** | 201,431 |

* **Python DX Win**: `pyqql` wraps native Rust `Stmt` handles inside PyO3 0.29 memory — parser throughput sustains native Rust speeds (up to **1.44M ops/s**).
* **Node.js parse()**: Returns a stable array of native `Stmt` objects (**150k–404k ops/s**).
* **Node.js parseJson()**: Returns the raw JSON string directly from Rust. Bypasses V8 object heap allocation entirely for maximum forwarding throughput — **1.8–2.8× faster** than `parse()` (up to **884k ops/s**). Ideal for HTTP/IPC forwarding.
* **WASM parse()**: Parses QQL directly into JS AST objects inside WebAssembly without native binary dependencies (up to **404k ops/s**).

---

## 2. Compile Pipeline & Mock E2E Benchmarks (ops/sec)

| Query Type | Rust (Pure Sync E2E) | Node.js (`nqql` Explain) | Python (`pyqql` Explain) | Rust (Mock E2E Dispatch) | Go (`qql-go` E2E) |
|---|:---:|:---:|:---:|:---:|:---:|
| **Simple** | **598,778** | 968,671 | 735,903 | 439,788 | 306,741 |
| **Hybrid** | **293,439** | 679,747 | 472,223 | 437,396 | 364,957 |
| **Full** | **141,164** | 258,702 | 205,907 | 187,947 | 195,372 |
| **CTE Prefetch** | **231,531** | 340,308 | 235,115 | 184,961 | 163,404 |
| **CreateCollection** | **415,159** | 526,987 | 419,124 | 318,956 | 262,059 |
| **Upsert** | **528,255** | 561,909 | 453,058 | 271,316 | 185,858 |
| **DeleteWhere** | **1,492,690** | 1,362,533 | 1,190,264 | 734,319 | 469,121 |
| **OrderBy** | **613,234** | 668,740 | 467,748 | 498,851 | 259,201 |
| **WithPayload** | **451,806** | 569,643 | 434,761 | 307,591 | 292,933 |

---

## 3. BM25 Sparse Vector Benchmark (100,000 Iterations)

| Operation | Total Time | Throughput (ops/sec) |
|---|:---:|:---:|
| **Build Document Vector** | 390.36 ms | **256,174** |
| **Build Query Vector** | 115.60 ms | **865,066** |

The pipeline matches Qdrant's `qdrant/bm25` exactly (murmur3-32 token IDs, word tokenizer, English stopwords, snowball stemming, BM25 tf saturation). With zero-alloc ASCII token scanning, static compile-time `phf` stopword lookups, and run-length counting on sorted ID slices, document generation runs at ~3.9 µs per document and queries at ~1.15 µs per query.

---

## Running the Benchmarks

Run benchmarks on an otherwise idle machine. Pin each process to one available CPU with `taskset -c <cpu>`, repeat at least three times, and report the median. Rust results are passed to `std::hint::black_box`; Node results are retained in a sink so the measured work is not discarded.

```bash
# 1. Rust parser / compile / mock-executor benchmarks
cargo build --release --manifest-path bench/bench_rust/Cargo.toml --bins
bench/bench_rust/target/release/parse
bench/bench_rust/target/release/explain
bench/bench_rust/target/release/e2e
bench/bench_rust/target/release/bench_sparse
bench/bench_rust/target/release/bench_upsert

# 2. Python binding + parse/explain benchmark (requires maturin)
(cd crates/pyqql && maturin develop --release)
python3 bench/bench_python.py

# 3. Node N-API binding + parse benchmark (requires npm dependencies)
(cd crates/nqql && npm run build)
node bench/bench_node.js

# 4. Node-targeted WASM parse benchmark
wasm-pack build crates/qql-wasm --release --target nodejs --out-dir pkg-node
node bench/bench_node.js
```
