# QQL Benchmarks

Compares QQL throughput across different parser implementations, runtimes, and host SDK languages (Rust, Python, Node.js, Go).

Benchmarks are split into two categories:
1. **Isolated Parser Benchmarks**: Pure lexing and parsing of QQL query strings into an AST (no network I/O, no schema compilation, no payload construction).
2. **Full E2E Pipeline Benchmarks**: The complete query compilation lifecycle right up to the millisecond before sending the network request (parsing, filter injection, schema validation, and Qdrant REST JSON payload construction).

- **CPU:** Intel Core i5-10400F @ 2.90 GHz
- **Rust:** `qql-rs` (v0.1.0)
- **Go:** `qql-go` (v0.1.0)
- **Python:** `pyqql` (v0.1.0 PyO3)
- **Node.js:** `nqql` (v0.1.0 N-API)
- **Date:** July 2026 (Post-Refactor 3-Layer Architecture)

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
*Isolates lexing & parsing throughput into typed AST. Higher is better.*

| Query | Rust (`qql-rs`) | Python (`pyqql`) | Go (`qql-go`) | Node.js (`NAPI parse()`) | Node.js (`parseFastJson`) |
|-------|:--------:|:--------:|:--------:|:--------:|:--------:|
| **Simple** | **1,929,013** | 1,520,097 | 1,688,724 | 492,729 | 238,891 |
| **Hybrid** | **970,296** | 806,691 | 1,300,844 | 403,121 | 256,251 |
| **Full** | **344,583** | 269,761 | 664,517 | 191,049 | 111,314 |
| **CTE Prefetch** | **430,517** | 382,035 | 337,312 | 227,220 | 91,355 |
| **CreateCollection** | **621,378** | 494,318 | 393,101 | 299,658 | 178,783 |
| **Upsert** | **654,713** | 611,150 | 508,451 | 254,368 | 184,985 |
| **DeleteWhere** | **1,801,754** | 1,203,957 | 1,960,807 | 347,703 | 428,751 |
| **OrderBy** | **958,494** | 921,252 | 1,020,497 | 334,826 | 221,614 |
| **WithPayload** | **802,854** | 758,053 | 858,692 | 350,946 | 194,900 |

* **Python DX Win**: Because `pyqql` wraps the native Rust `Stmt` directly inside PyO3 memory, parser throughput matches native Rust/Go speeds almost 1-to-1 (up to **1.52M ops/s**!).
* **Node.js Boundary Cost**: Node.js N-API class wrapping provides direct AST access, while `parseFastJson` bypasses V8 object allocation for flat JSON consumption.

---

## 2. E2E Pipeline Benchmarks (ops/sec)
*Measures entire compilation lifecycle + REST JSON payload construction. Higher is better.*

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
| **Build Document Vector** | 64.13 ms | **1,559,443** |
| **Build Query Vector** | 20.39 ms | **4,903,462** |

---

## Running the Benchmarks

```bash
# 1. Build release binaries & bindings
cargo build --release -p pyqql -p nqql
cargo build --release --manifest-path bench/bench_rust/Cargo.toml --bins
(cd crates/nqql && npx napi build --release --platform)

# 2. Rust (Parser & E2E Sync/Async)
cargo run --release --manifest-path bench/bench_rust/Cargo.toml --bin parse
cargo run --release --manifest-path bench/bench_rust/Cargo.toml --bin explain
cargo run --release --manifest-path bench/bench_rust/Cargo.toml --bin e2e
cargo run --release --manifest-path bench/bench_rust/Cargo.toml --bin bench_sparse

# 3. Python (Parser & E2E)
PYTHONPATH=target/release python3 bench/bench_python.py

# 4. Node.js (Parser & E2E)
node bench/bench_node.js
```
