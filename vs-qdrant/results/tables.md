### Results

#### Python — `qdrant-client 1.19.0` vs `pyqql 0.4.0` (both REST)

| scenario | official ops/s | qql ops/s | qql/official | p50 official | p50 qql | parity |
|---|---:|---:|---:|---:|---:|---|
| query_dense | 346 | 724 | 2.09x | 2.891 ms | 1.381 ms | overlap 1.0, top1 ok |
| query_dense_filtered | 224 | 287 | 1.28x | 4.455 ms | 3.479 ms | overlap 1.0, top1 ok |
| query_sparse | 725 | 2,147 | 2.96x | 1.38 ms | 0.466 ms | overlap 1.0, top1 ok |
| query_hybrid | 260 | 485 | 1.87x | 3.843 ms | 2.061 ms | overlap 0.818, top1 differs |
| scroll_pages | 86 | 128 | 1.49x | 11.562 ms | 7.841 ms | overlap 1.0, top1 ok |
| count_berlin | 613 | 1,577 | 2.57x | 1.631 ms | 0.634 ms | exact |
| facet_district | 755 | 2,081 | 2.76x | 1.325 ms | 0.48 ms | exact |
| query_colbert | 126 | 132 | 1.05x | 7.932 ms | 7.578 ms | overlap 1.0, top1 ok |
| count_legal | 808 | 3,045 | 3.77x | 1.237 ms | 0.328 ms | exact |
| prepared_rerun | 100 | 192 | 1.92x | 10.04 ms | 5.213 ms | overlap 1.0, top1 ok |
| ingest (10k pts, wait=false) | 6.757 s | 9.892 s | 0.68x | — | — | counts + sample point byte-identical |
| cold import / require | 1096.8 ms | 30.9 ms | 35.5x | — | — | median of 5 fresh processes |

#### Node — `@qdrant/js-client-rest 1.19.0` vs `nqql 0.4.0` (both REST)

| scenario | official ops/s | qql ops/s | qql/official | p50 official | p50 qql | parity |
|---|---:|---:|---:|---:|---:|---|
| query_dense | 292 | 707 | 2.42x | 3.429 ms | 1.414 ms | overlap 1, top1 ok |
| query_dense_filtered | 184 | 276 | 1.50x | 5.43 ms | 3.619 ms | overlap 1, top1 ok |
| query_sparse | 401 | 2,159 | 5.38x | 2.493 ms | 0.463 ms | overlap 1, top1 ok |
| query_hybrid | 252 | 439 | 1.74x | 3.962 ms | 2.275 ms | overlap 1, tie-break ok |
| scroll_pages | 56 | 130 | 2.32x | 17.745 ms | 7.705 ms | overlap 1, top1 ok |
| count_berlin | 371 | 1,201 | 3.24x | 2.695 ms | 0.833 ms | exact |
| facet_district | 404 | 1,776 | 4.40x | 2.477 ms | 0.563 ms | exact |
| query_colbert | 105 | 128 | 1.22x | 9.508 ms | 7.813 ms | overlap 1, top1 ok |
| count_legal | 419 | 2,639 | 6.30x | 2.385 ms | 0.379 ms | exact |
| prepared_rerun | 75 | 185 | 2.47x | 13.311 ms | 5.411 ms | overlap 1, top1 ok |
| ingest (10k pts, wait=false) | 8.803 s | 7.193 s | 1.22x | — | — | counts + sample point byte-identical |
| cold import / require | 108.1 ms | 37 ms | 2.9x | — | — | median of 5 fresh processes |

#### Rust — `qdrant-client 1.19.0` vs `qql 0.4.0` (both gRPC)

| scenario | official ops/s | qql ops/s | qql/official | p50 official | p50 qql | parity |
|---|---:|---:|---:|---:|---:|---|
| count_berlin | 1,668 | 1,633 | 0.98x | 0.599 ms | 0.612 ms | exact |
| count_legal | 3,205 | 3,252 | 1.01x | 0.312 ms | 0.307 ms | exact |
| facet_district | 2,412 | 2,286 | 0.95x | 0.414 ms | 0.437 ms | exact |
| prepared_rerun | 229 | 210 | 0.92x | 4.349 ms | 4.742 ms | overlap 1.0, top1 ok |
| query_colbert | 42 | 43 | 1.02x | 23.745 ms | 22.994 ms | overlap 1.0, top1 ok |
| query_dense | 971 | 923 | 0.95x | 1.029 ms | 1.083 ms | overlap 1.0, top1 ok |
| query_dense_filtered | 327 | 311 | 0.95x | 3.05 ms | 3.21 ms | overlap 1.0, top1 ok |
| query_hybrid | 660 | 619 | 0.94x | 1.513 ms | 1.615 ms | overlap 0.818, top1 ok |
| query_sparse | 2,700 | 2,410 | 0.89x | 0.37 ms | 0.415 ms | overlap 1.0, top1 ok |
| scroll_pages | 152 | 132 | 0.87x | 6.559 ms | 7.573 ms | overlap 1.0, top1 ok |
| ingest (10k pts, wait=false) | 0.802 s | 1.001 s | 0.80x | — | — | counts + sample point byte-identical |

### Application LOC (identical scenario set)

| language | official SDK | qql SDK | qql LOC ratio |
|---|---:|---:|---:|
| python | 170 | 134 | 0.79x |
| node | 152 | 126 | 0.83x |
| rust | 421 | 253 | 0.6x |

