# Benchmark Report — 2026-07-29

## Environment

- CPU: Intel Core i5-10400F @ 2.90 GHz (6 cores / 12 threads)
- Rust: 1.96.1
- Python: CPython 3.14.4
- Node: 24.18.0
- Builds: fresh release builds for Rust benchmarks, PyO3 wheel, N-API binding, and Node-targeted WASM.

These are one-run throughput samples on an otherwise unpinned host. They are
useful as a baseline, not a regression threshold. Repeat with CPU affinity and
report a median before making performance claims.

## Rust parser throughput (ops/s)

| Query | ops/s |
|---|---:|
| Simple | 1,912,650 |
| Hybrid | 888,400 |
| Full | 309,933 |
| CTE prefetch | 375,094 |
| Create collection | 565,617 |
| Upsert | 563,809 |
| Delete where | 1,738,933 |
| Order by | 995,511 |
| With payload | 686,519 |

## Rust explanation rendering (ops/s)

| Query | ops/s |
|---|---:|
| Simple | 1,169,375 |
| Hybrid | 720,610 |
| Full | 283,289 |
| CTE prefetch | 328,825 |
| Create collection | 542,584 |
| Upsert | 580,154 |
| Delete where | 1,312,770 |
| Order by | 740,111 |
| With payload | 609,370 |

## Rust mock-executor pipeline (ops/s)

This includes parse, preparation, planning, and mock dispatch. It excludes a
real Qdrant request and model inference.

| Query | ops/s |
|---|---:|
| Simple | 417,358 |
| Hybrid | 420,661 |
| Full | 169,642 |
| CTE prefetch | 186,695 |
| Create collection | 264,426 |
| Upsert | 227,718 |
| Delete where | 676,796 |
| Order by | 433,990 |
| With payload | 294,031 |

## Sparse and upsert microbenchmarks

| Operation | Throughput |
|---|---:|
| BM25 document vector | 1,387,330 ops/s |
| BM25 query vector | 4,608,520 ops/s |
| UPSERT parse | 597,153 ops/s |
| UPSERT parse + route | 307,804 ops/s |
| UPSERT parse + JSON body | 245,403 ops/s |

UPSERT incremental costs: route projection 1,574 ns/op; JSON body extraction
826 ns/op.

## Python PyO3 binding (ops/s)

| Query | Parse | Explain |
|---|---:|---:|
| Simple | 1,515,825 | 730,145 |
| Hybrid | 809,145 | 564,614 |
| Full | 293,886 | 243,599 |
| CTE prefetch | 357,329 | 299,366 |
| Create collection | 526,127 | 463,031 |
| Upsert | 466,936 | 464,668 |
| Delete where | 1,397,271 | 1,011,600 |
| Order by | 909,588 | 592,075 |
| With payload | 700,022 | 470,693 |

`explain` is explanation rendering, not executor E2E.

## Node N-API and WASM parse throughput (ops/s)

| Query | N-API `parse` | N-API `parseJson` | WASM `parse` |
|---|---:|---:|---:|
| Simple | 427,569 | 785,805 | 299,271 |
| Hybrid | 322,736 | 556,264 | 228,658 |
| Full | 165,951 | 217,382 | 92,793 |
| CTE prefetch | 141,479 | 185,930 | 74,264 |
| Create collection | 233,697 | 337,270 | 177,079 |
| Upsert | 235,219 | 401,744 | 154,354 |
| Delete where | 355,270 | 903,320 | 371,109 |
| Order by | 330,229 | 537,178 | 210,065 |
| With payload | 239,784 | 449,277 | 190,906 |

## Harness corrections made before measuring

- Rust benchmarks now use `black_box`; the upsert benchmark has a warmup and
  uses fallible `try_route` rather than the deprecated panic wrapper.
- The mock-executor benchmark now has a dense/sparse collection topology and
  valid server-inference model identifiers, so it measures successful work.
- The Node suite calls the actual WASM export, `parse`, and tolerates an absent
  WASM artifact instead of dereferencing `null`.
- The Python suite labels explanation rendering correctly and resolves its
  repository-relative path robustly.
- `bench/README.md` now documents all Rust binaries, maturin, N-API, WASM, and
  reproducibility requirements.
