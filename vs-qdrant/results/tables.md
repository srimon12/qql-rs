### Results

#### Python — `qdrant-client 1.19.0` vs `pyqql 0.4.0` (both REST)

| scenario | official ops/s | qql ops/s | qql/official | p50 official | p50 qql | parity |
|---|---:|---:|---:|---:|---:|---|
| query_dense | 417 | 805 | 1.93x | 2.397 ms | 1.242 ms | overlap 1.0, top1 ok |
| query_dense_filtered | 227 | 305 | 1.34x | 4.413 ms | 3.284 ms | overlap 1.0, top1 ok |
| query_sparse | 728 | 2,345 | 3.22x | 1.373 ms | 0.426 ms | overlap 1.0, top1 ok |
| query_hybrid | 266 | 526 | 1.98x | 3.763 ms | 1.902 ms | overlap 0.818, top1 ok |
| scroll_pages | 88 | 145 | 1.65x | 11.366 ms | 6.915 ms | overlap 1.0, top1 ok |
| count_berlin | 616 | 1,546 | 2.51x | 1.623 ms | 0.647 ms | exact |
| facet_district | 736 | 2,028 | 2.76x | 1.358 ms | 0.493 ms | exact |
| query_colbert | 121 | 135 | 1.12x | 8.295 ms | 7.401 ms | overlap 1.0, top1 ok |
| count_legal | 794 | 3,257 | 4.10x | 1.259 ms | 0.307 ms | exact |
| prepared_rerun | 102 | 210 | 2.06x | 9.846 ms | 4.754 ms | overlap 1.0, top1 ok |
| ingest (10k pts, wait=false) | 6.663 s | 4.315 s | 1.54x | — | — | counts + sample point byte-identical |
| cold import / require | 1149.0 ms | 17.1 ms | 67.2x | — | — | median of 5 fresh processes |

#### Node — `@qdrant/js-client-rest 1.19.0` vs `nqql 0.4.0` (both REST)

| scenario | official ops/s | qql ops/s | qql/official | p50 official | p50 qql | parity |
|---|---:|---:|---:|---:|---:|---|
| query_dense | 290 | 741 | 2.56x | 3.448 ms | 1.349 ms | overlap 1, top1 ok |
| query_dense_filtered | 190 | 280 | 1.47x | 5.258 ms | 3.572 ms | overlap 1, top1 ok |
| query_sparse | 425 | 2,060 | 4.85x | 2.353 ms | 0.485 ms | overlap 1, top1 ok |
| query_hybrid | 245 | 501 | 2.04x | 4.085 ms | 1.995 ms | overlap 1, top1 ok |
| scroll_pages | 59 | 103 | 1.75x | 17.009 ms | 9.75 ms | overlap 1, top1 ok |
| count_berlin | 420 | 1,483 | 3.53x | 2.383 ms | 0.674 ms | exact |
| facet_district | 454 | 1,967 | 4.33x | 2.205 ms | 0.508 ms | exact |
| query_colbert | 118 | 137 | 1.16x | 8.446 ms | 7.274 ms | overlap 1, top1 ok |
| count_legal | 506 | 2,968 | 5.87x | 1.976 ms | 0.337 ms | exact |
| prepared_rerun | 80 | 189 | 2.36x | 12.44 ms | 5.277 ms | overlap 1, top1 ok |
| ingest (10k pts, wait=false) | 8.982 s | 7.986 s | 1.12x | — | — | counts + sample point byte-identical |
| cold import / require | 102.4 ms | 33.9 ms | 3.0x | — | — | median of 5 fresh processes |

#### Rust — `qdrant-client 1.19.0` vs `qql 0.4.0` (both gRPC)

| scenario | official ops/s | qql ops/s | qql/official | p50 official | p50 qql | parity |
|---|---:|---:|---:|---:|---:|---|
| count_berlin | 1,649 | 1,562 | 0.95x | 0.606 ms | 0.64 ms | exact |
| count_legal | 3,547 | 3,576 | 1.01x | 0.282 ms | 0.28 ms | exact |
| facet_district | 2,414 | 2,382 | 0.99x | 0.414 ms | 0.42 ms | exact |
| prepared_rerun | 288 | 275 | 0.95x | 3.462 ms | 3.632 ms | overlap 1.0, top1 ok |
| query_colbert | 43 | 43 | 1.00x | 22.989 ms | 23.06 ms | overlap 1.0, top1 ok |
| query_dense | 1,229 | 1,154 | 0.94x | 0.814 ms | 0.866 ms | overlap 1.0, top1 ok |
| query_dense_filtered | 338 | 322 | 0.95x | 2.957 ms | 3.101 ms | overlap 1.0, top1 ok |
| query_hybrid | 693 | 675 | 0.97x | 1.443 ms | 1.48 ms | overlap 0.818, top1 differs |
| query_sparse | 2,820 | 2,686 | 0.95x | 0.354 ms | 0.372 ms | overlap 1.0, top1 ok |
| scroll_pages | 154 | 134 | 0.87x | 6.475 ms | 7.413 ms | overlap 1.0, top1 ok |
| ingest (10k pts, wait=false) | 0.784 s | 0.957 s | 0.82x | — | — | counts + sample point byte-identical |

### Application LOC (identical scenario set)

| language | official SDK | qql SDK | qql LOC ratio |
|---|---:|---:|---:|
| python | 170 | 134 | 0.79x |
| node | 152 | 126 | 0.83x |
| rust | 387 | 274 | 0.71x |

