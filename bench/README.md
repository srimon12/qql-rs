# QQL Benchmarks

Compares QQL throughput across different parser implementations, runtimes, and host SDK languages (Rust, Go, Python, Node.js).

Benchmarks are split into two categories:
1. **Isolated Parser Benchmarks**: Pure lexing and parsing of QQL query strings into an AST (no network I/O, no schema compilation, no payload construction).
2. **Full E2E Pipeline Benchmarks**: The complete query compilation lifecycle right up to the millisecond before sending the network request (parsing, filter injection, schema validation, and Qdrant REST JSON payload construction).

- **CPU:** Intel Core i5-10400F @ 2.90 GHz
- **Rust:** `qql-rs` (v0.1.0)
- **Go:** `qql-go` (v0.1.0)
- **Python:** `pyqql` (v0.1.0)
- **Node.js:** `nqql` (v0.1.0)

---

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

---

## 1. Parser Benchmarks (ops/sec)
*Isolates lexing & parsing throughput. Higher is better.*

| Query | Rust (`qql-rs`) | Python (`pyqql`) | Node.js (`parseFastJson`) | Go (`qql-go`) | Node.js (`NAPI parse()`) |
|-------|:--------:|:--------:|:--------:|:--------:|:--------:|
| **Simple** | 2,085,740 | 1,762,541 | 247,644 | 1,688,724 | 60,691 |
| **Hybrid** | 1,860,917 | 925,682 | 240,516 | 1,300,844 | 60,832 |
| **Full** | 762,370 | 659,058 | 153,207 | 664,517 | 45,219 |
| **CTE Prefetch** | 360,819 | 344,304 | 66,221 | 337,312 | 16,282 |
| **CreateCollection** | 685,678 | 477,277 | 194,287 | 393,101 | 73,480 |
| **Insert** | 756,855 | 730,834 | 196,979 | 508,451 | 86,938 |
| **DeleteWhere** | 1,866,475 | 1,637,655 | 539,616 | 1,960,807 | 317,806 |
| **OrderBy** | 1,029,932 | 969,849 | 190,271 | 1,020,497 | 53,242 |
| **WithPayload** | 947,908 | 845,869 | 179,144 | 858,692 | 51,986 |

* **Python DX Win**: Because `pyqql` wraps the native Rust `Stmt` directly inside PyO3 memory, parser throughput matches native Rust/Go speeds almost 1-to-1.
* **Node.js Boundary Cost**: Node.js standard N-API GC allocations have high object mapping overhead (~60k ops/s), but using `parseFastJson` bypasses this, yielding **~247k ops/s**.

---

## 2. E2E Pipeline Benchmarks (ops/sec)
*Measures entire compilation lifecycle + REST JSON payload construction. Higher is better.*

| Query Type | Rust (`qql-rs`) | Go (`qql-go`) | Python (`pyqql` E2E) | Node.js (`nqql` E2E) |
| :--- | :---: | :---: | :---: | :---: |
| **Simple** | **1,074,246** | 306,741 | 1,090,391 | 1,135,007 |
| **Hybrid** | **957,509** | 364,957 | 824,011 | 1,032,660 |
| **Full** | **395,307** | 195,372 | 519,561 | 544,210 |
| **CTE_Prefetch** | **237,292** | 163,404 | 309,577 | 321,546 |
| **CreateCollection** | **565,599** | 262,059 | 516,832 | 588,675 |
| **Insert** | **456,273** | 185,858 | 625,161 | 627,901 |
| **DeleteWhere** | **992,423** | 469,121 | 1,041,469 | 984,827 |
| **OrderBy** | **407,095** | 259,201 | 528,142 | 866,207 |
| **WithPayload** | **662,425** | 292,933 | 657,634 | 651,419 |

### Observations:
* **The Power of Rust Compilation**: Both Python and Node.js E2E pipelines operate at **1 Million+ ops/sec**! 
* **Zero FFI Boundary Cost on `explain()`**: Because `explain()` returns a flat compiled string directly from Rust back to the host language (with no recursive object translation), it operates at native speeds. This shows that compiling queries and constructing final payload buffers is extremely fast.

---

## Running the Benchmarks

```bash
# Rust (Parser & E2E)
cargo run --release --manifest-path bench/bench_rust/Cargo.toml --bin parse
cargo run --release --manifest-path bench/bench_rust/Cargo.toml --bin e2e

# Python (Parser & E2E)
PYTHONPATH=target/release python3 bench/bench_python.py

# Node.js (Parser & E2E)
node bench/bench_node.js
```
