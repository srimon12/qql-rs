### Results

#### Python — `qdrant-client 1.19.0` vs `pyqql 0.4.0` (both REST)

| scenario | official ops/s | qql ops/s | qql/official | p50 official | p50 qql | parity |
|---|---:|---:|---:|---:|---:|---|
| query_dense | 405 | 674 | 1.66x | 2.472 ms | 1.485 ms | overlap 1.0, top1 ok |
| query_dense_filtered | 223 | 282 | 1.26x | 4.483 ms | 3.549 ms | overlap 1.0, top1 ok |
| query_sparse | 745 | 1,458 | 1.96x | 1.342 ms | 0.686 ms | overlap 1.0, top1 ok |
| query_hybrid | 269 | 465 | 1.73x | 3.72 ms | 2.149 ms | overlap 1.0, top1 differs |
| scroll_pages | 89 | 92 | 1.03x | 11.246 ms | 10.829 ms | overlap 1.0, top1 ok |
| count_berlin | 626 | 1,562 | 2.50x | 1.597 ms | 0.64 ms | exact |
| facet_district | 762 | 2,178 | 2.86x | 1.312 ms | 0.459 ms | exact |
| query_colbert | 121 | 134 | 1.11x | 8.245 ms | 7.466 ms | overlap 1.0, top1 ok |
| count_legal | 833 | 3,033 | 3.64x | 1.201 ms | 0.33 ms | exact |
| prepared_rerun | 102 | 165 | 1.62x | 9.842 ms | 6.067 ms | overlap 1.0, top1 ok |
| ingest (10k pts, wait=false) | 7.931 s | 3.475 s | 0.44x | — | — | counts + sample point byte-identical |
| cold import / require | 1109.7 ms | 29.9 ms | 37.1x | — | — | median of 5 fresh processes |

#### Node — `@qdrant/js-client-rest 1.19.0` vs `nqql 0.4.0` (both REST)

| scenario | official ops/s | qql ops/s | qql/official | p50 official | p50 qql | parity |
|---|---:|---:|---:|---:|---:|---|
| query_dense | 304 | 653 | 2.15x | 3.29 ms | 1.532 ms | overlap 1, top1 ok |
| query_dense_filtered | 186 | 275 | 1.48x | 5.378 ms | 3.643 ms | overlap 1, top1 ok |
| query_sparse | 434 | 1,344 | 3.10x | 2.303 ms | 0.744 ms | overlap 1, top1 ok |
| query_hybrid | 259 | 426 | 1.64x | 3.861 ms | 2.347 ms | overlap 0.818, top1 ok |
| scroll_pages | 59 | 97 | 1.64x | 16.974 ms | 10.32 ms | overlap 1, top1 ok |
| count_berlin | 406 | 1,492 | 3.67x | 2.461 ms | 0.67 ms | exact |
| facet_district | 473 | 2,025 | 4.28x | 2.112 ms | 0.494 ms | exact |
| query_colbert | 120 | 138 | 1.15x | 8.34 ms | 7.224 ms | overlap 1, top1 ok |
| count_legal | 513 | 3,015 | 5.88x | 1.947 ms | 0.332 ms | exact |
| prepared_rerun | 83 | 167 | 2.01x | 12.049 ms | 5.975 ms | overlap 1, top1 ok |
| ingest (10k pts, wait=false) | 8.764 s | 7.109 s | 0.81x | — | — | counts + sample point byte-identical |
| cold import / require | 103.2 ms | 37.7 ms | 2.7x | — | — | median of 5 fresh processes |

#### Rust — `qdrant-client 1.19.0` vs `qql 0.4.0` (both gRPC)

| scenario | official ops/s | qql ops/s | qql/official | p50 official | p50 qql | parity |
|---|---:|---:|---:|---:|---:|---|
| count_berlin | 1,594 | 1,606 | 1.01x | 0.627 ms | 0.622 ms | exact |
| count_legal | 3,422 | 3,567 | 1.04x | 0.292 ms | 0.28 ms | exact |
| facet_district | 2,291 | 2,333 | 1.02x | 0.436 ms | 0.429 ms | exact |
| prepared_rerun | 288 | 237 | 0.82x | 3.468 ms | 4.208 ms | overlap 1.0, top1 ok |
| query_colbert | 45 | 43 | 0.96x | 21.984 ms | 23.25 ms | overlap 1.0, top1 ok |
| query_dense | 873 | 668 | 0.77x | 1.145 ms | 1.496 ms | overlap 1.0, top1 ok |
| query_dense_filtered | 336 | 280 | 0.83x | 2.971 ms | 3.568 ms | overlap 1.0, top1 ok |
| query_hybrid | 679 | 529 | 0.78x | 1.471 ms | 1.89 ms | overlap 0.818, top1 ok |
| query_sparse | 2,709 | 1,489 | 0.55x | 0.369 ms | 0.671 ms | overlap 1.0, top1 ok |
| scroll_pages | 150 | 99 | 0.66x | 6.623 ms | 10.046 ms | overlap 1.0, top1 ok |
| ingest (10k pts, wait=false) | 0.815 s | 1.666 s | 2.04x | — | — | counts + sample point byte-identical |

### Application LOC (identical scenario set)

| language | official SDK | qql SDK | qql LOC ratio |
|---|---:|---:|---:|
| python | 147 | 126 | 0.86x |
| node | 152 | 123 | 0.81x |
| rust | 421 | 293 | 0.7x |

