# Benchmark Report — 2026-08-28

Comprehensive benchmark sweep following zero-allocation binder rewrites, BM25 tokenizer optimizations, and PyO3 0.29 / NAPI-RS 3 upgrades.

## Environment

- **CPU:** Intel Core i5-10400F @ 2.90 GHz (6 cores / 12 threads)
- **Rust:** 1.98.0
- **Python:** CPython 3.12 (PyO3 0.29.2, `abi3-py310`)
- **Node.js:** 24.18.0 (NAPI-RS 3.12.2, `@napi-rs/cli` 3.7.4)
- **WASM:** `wasm32-unknown-unknown` (Node.js target via `wasm-pack` / `wasm-opt`)
- **Builds:** Release profile (`--release` / `-O3`) with LTO and zero debug assertions.

---

## 1. Rust Parser Throughput (`qql-rs`)

*Measured with `bench/bench_rust/target/release/parse` (100,000 iterations each).*

| Query | Latency (ns/op) | Throughput (ops/s) |
|---|:---:|:---:|
| **Simple** | 814 ns | **1,228,994** |
| **Hybrid** | 1,912 ns | **523,014** |
| **Full** | 3,400 ns | **294,125** |
| **CTE Prefetch** | 2,665 ns | **375,234** |
| **Create Collection** | 1,711 ns | **584,513** |
| **Upsert** | 1,583 ns | **631,662** |
| **Delete Where** | 699 ns | **1,431,167** |
| **Order By** | 1,015 ns | **984,982** |
| **With Payload** | 1,727 ns | **578,997** |

---

## 2. Rust Explanation Rendering (`qql-runtime`)

*Measured with `bench/bench_rust/target/release/explain` (100,000 iterations each).*

| Query | Latency (ns/op) | Throughput (ops/s) |
|---|:---:|:---:|
| **Simple** | 1,670 ns | **598,778** |
| **Hybrid** | 3,408 ns | **293,439** |
| **Full** | 7,084 ns | **141,164** |
| **CTE Prefetch** | 4,319 ns | **231,531** |
| **Create Collection** | 2,409 ns | **415,159** |
| **Upsert** | 1,893 ns | **528,255** |
| **Delete Where** | 670 ns | **1,492,690** |
| **Order By** | 1,631 ns | **613,234** |
| **With Payload** | 2,213 ns | **451,806** |

---

## 3. Rust Mock-Executor Pipeline E2E

*Measured with `bench/bench_rust/target/release/e2e` (100,000 iterations each). Includes full parse, topology validation, query planning, filter normalization, and mock dispatch.*

| Query | Latency (ns/op) | Throughput (ops/s) |
|---|:---:|:---:|
| **Simple** | 2,274 ns | **439,788** |
| **Hybrid** | 2,286 ns | **437,396** |
| **Full** | 5,321 ns | **187,947** |
| **CTE Prefetch** | 5,407 ns | **184,961** |
| **Create Collection** | 3,135 ns | **318,956** |
| **Upsert** | 3,686 ns | **271,316** |
| **Delete Where** | 1,362 ns | **734,319** |
| **Order By** | 2,005 ns | **498,851** |
| **With Payload** | 3,251 ns | **307,591** |

---

## 4. Sparse Vector BM25 & UPSERT Microbenchmarks

### BM25 Tokenizer & Embedding (`qql-embed`)
*100,000 iterations matching Qdrant server segment format (Murmur3-32, English Snowball stemmer, compile-time static `phf` stopwords, in-place ID sorting and run-length counting).*

| Operation | Total Time | Throughput (ops/s) |
|---|:---:|:---:|
| **Build Document Vector** | 390.36 ms | **256,174** |
| **Build Query Vector** | 115.60 ms | **865,066** |

### UPSERT Pipeline Breakdown (500,000 iterations)

| Step | Latency (ns/op) | Cumulative Throughput (ops/s) |
|---|:---:|:---:|
| **Parse only** | 1,576 ns | **634,373** |
| **Parse + Route** | 3,188 ns | **313,667** |
| **Parse + Route + Body JSON** | 3,906 ns | **255,997** |

*Incremental overheads: Route projection: 1,612 ns/op; JSON body extraction: 718 ns/op.*

---

## 5. Python PyO3 SDK (`pyqql`)

*Measured with `bench/bench_python.py` (PyO3 0.29.2, `abi3-py310`, 50,000 iterations each).*

| Query | Parse (ops/s) | Explain (ops/s) |
|---|:---:|:---:|
| **Simple** | **1,438,309** | **735,903** |
| **Hybrid** | **819,801** | **472,223** |
| **Full** | **326,364** | **205,907** |
| **CTE Prefetch** | **384,043** | **235,115** |
| **Create Collection** | **561,740** | **419,124** |
| **Upsert** | **567,949** | **453,058** |
| **Delete Where** | **1,393,998** | **1,190,264** |
| **Order By** | **938,385** | **467,748** |
| **With Payload** | **635,220** | **434,761** |

---

## 6. Node.js NAPI & WASM Parse Throughput

*Measured with `bench/bench_node.js` (50,000 iterations each, Node.js v24.18.0, release NAPI-RS 3.12.2 binding).*

| Query | Node.js NAPI `parse()` | Node.js NAPI `parseJson()` | WebAssembly `qql-wasm` |
|---|:---:|:---:|:---:|
| **Simple** | **404,342 ops/s** | **715,511 ops/s** | **287,115 ops/s** |
| **Hybrid** | **247,170 ops/s** | **552,628 ops/s** | **238,740 ops/s** |
| **Full** | **161,240 ops/s** | **216,254 ops/s** | **98,047 ops/s** |
| **CTE Prefetch** | **154,960 ops/s** | **200,567 ops/s** | **78,257 ops/s** |
| **Create Collection** | **234,023 ops/s** | **337,778 ops/s** | **175,765 ops/s** |
| **Upsert** | **197,621 ops/s** | **403,572 ops/s** | **158,544 ops/s** |
| **Delete Where** | **314,419 ops/s** | **884,023 ops/s** | **404,971 ops/s** |
| **Order By** | **304,958 ops/s** | **551,846 ops/s** | **237,489 ops/s** |
| **With Payload** | **246,423 ops/s** | **458,311 ops/s** | **201,431 ops/s** |
