# QQL Benchmarks

Compares QQL throughput across different parser implementations, runtimes, and host SDK languages (Rust, Python, Node.js, Go).

Benchmarks are split into two categories:
1. **Isolated Parser Benchmarks**: Pure lexing and parsing of QQL query strings into an AST (no network I/O, no schema compilation, no payload construction).
2. **Full E2E Pipeline Benchmarks**: The complete query compilation lifecycle right up to the millisecond before sending the network request (parsing, filter injection, schema validation, and Qdrant REST JSON payload construction).

- **CPU:** Intel Core i5-10400F @ 2.90 GHz
- **Rust:** `qql-rs` (v0.1.2)
- **Go:** `qql-go` (v0.1.2)
- **Python:** `pyqql` (v0.1.2 PyO3)
- **Node.js:** `nqql` (v0.1.2 N-API)
- **Date:** July 2026 (v0.1.2 Release Verification)

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
|-------|:--------:|:--------:|:--------:|:--------:|:--------:|:--------:|
| **Simple** | **1,877,772** | 1,480,826 | 1,688,724 | 235,292 | **370,404** | 179,751 |
| **Hybrid** | **966,028** | 781,476 | 1,300,844 | 191,749 | **278,040** | 83,213 |
| **Full** | **334,284** | 315,357 | 664,517 | 92,690 | **115,894** | 41,127 |
| **CTE Prefetch** | **397,353** | 352,641 | 337,312 | 88,072 | **105,672** | 61,454 |
| **CreateCollection** | **608,408** | 553,549 | 393,101 | 137,582 | **184,659** | 80,221 |
| **Upsert** | **657,067** | 439,307 | 508,451 | 134,583 | **194,843** | 83,514 |
| **DeleteWhere** | **1,782,101** | 1,448,972 | **1,960,807** | 235,995 | **418,612** | 153,067 |
| **OrderBy** | **1,110,445** | 886,853 | 1,020,497 | 160,957 | **269,375** | 90,656 |
| **WithPayload** | **789,211** | 719,559 | 858,692 | 150,814 | **233,036** | 103,087 |

* **Python DX Win**: `pyqql` wraps the native Rust `Stmt` directly inside PyO3 memory — parser throughput matches native Rust/Go speeds almost 1-to-1 (up to **1.48M ops/s**).
* **Node.js parse()**: Returns a stable array of native `Stmt` objects. ~235K ops/s — V8 object allocation is the bottleneck.
* **Node.js parseJson()**: Returns the raw JSON string directly from Rust. Bypasses V8 object heap allocation entirely for maximum forwarding throughput — **1.55–1.75× faster** than `parse()`. Ideal for HTTP/IPC forwarding.
* **WASM parse()**: Parses QQL directly into JS AST objects inside WebAssembly without native binary dependencies.

---

## 2. Compile Pipeline Benchmarks (ops/sec)
*Historical results. `explain` measures parsing plus explanation rendering; it does not perform schema resolution, embedding, transport I/O, or a Qdrant call.*

| Query Type | Rust (Pure Sync E2E) | Node.js (`nqql` E2E) | Python (`pyqql` E2E) | Rust (Async E2E) | Go (`qql-go` E2E) |
| :--- | :---: | :---: | :---: | :---: | :---: |
| **Simple** | **1,275,059** | 968,671 | 744,265 | 808,268 | 306,741 |
| **Hybrid** | **730,040** | 679,747 | 519,031 | 545,719 | 364,957 |
| **Full** | **296,628** | 258,702 | 307,240 | 252,813 | 195,372 |
| **CTE_Prefetch** | **361,565** | 340,308 | 376,637 | 304,298 | 163,404 |
| **CreateCollection** | **581,623** | 526,987 | 563,644 | 442,965 | 262,059 |
| **Upsert** | **629,885** | 561,909 | 585,383 | 416,268 | 185,858 |
| **DeleteWhere** | **1,539,661** | 1,362,533 | 1,124,044 | 974,978 | 469,121 |
| **OrderBy** | **791,553** | 668,740 | 573,749 | 575,844 | 259,201 |
| **WithPayload** | **667,446** | 569,643 | 590,138 | 463,445 | 292,933 |

### Speed Hierarchy Physics:
$$\text{Rust Pure Sync} > \text{Node.js E2E} \ge \text{Python E2E} > \text{Rust Async (due to tokio runtime block\_on)} > \text{Go}$$

- **Rust Pure Sync**: Bypasses both FFI translation and Tokio runtime scheduling, showing the true, maximum speed of our in-memory payload compiler (up to **1.53M ops/s**!).
- **FFI E2E (Node/Python)**: Since `explain()` returns a flat string payload, there is zero object translation overhead. They match native speeds, trailing Rust Sync only by the minor FFI boundary hop cost.
- **Rust Async**: The `block_on` wrapper adds task scheduling and future state-machine polling overhead on every query, making it slightly slower than pure sync compilation.

---

## 3. BM25 Sparse Vector Benchmark (100,000 Iterations)

| Operation | Total Time | Throughput (ops/sec) |
|---|:---:|:---:|
| **Build Document Vector** | 511.32 ms | **195,571** |
| **Build Query Vector** | 132.88 ms | **752,578** |

The pipeline now matches Qdrant's `qdrant/bm25` exactly (murmur3-32 token
IDs, word tokenizer, English stopwords, snowball stemming, BM25 tf
saturation). Stemming and stopword filtering do real work per token, so
throughput is lower than the old bare hasher — still ~5 µs per document,
negligible next to dense ONNX inference.

---

## Running the Benchmarks

Run benchmarks on an otherwise idle machine. Pin each process to one available
CPU with `taskset -c <cpu>`, repeat at least three times, and report the
median. Rust results are passed to `std::hint::black_box`; Node results are
retained in a sink so the measured work is not discarded.

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
(cd crates/nqql && npx napi build --release --platform)
node bench/bench_node.js

# 4. Node-targeted WASM parse benchmark
wasm-pack build crates/qql-wasm --release --target nodejs --out-dir pkg-node
node bench/bench_node.js
```

The Node suite reports N-API `parse`, N-API `parseJson`, and WASM `parse` when
the corresponding artifacts are available. `bench_python.py` reports `parse`
and `explain`; label the latter as explanation rendering rather than E2E.
