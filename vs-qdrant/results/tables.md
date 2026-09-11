### Results

#### Python — `qdrant-client 1.19.0` vs `pyqql 0.4.0` (both REST)

| scenario | official ops/s | qql ops/s | qql/official | p50 official | p50 qql | parity |
|---|---:|---:|---:|---:|---:|---|
| query_dense | 444 | 964 | 2.17x | 2.25 ms | 1.037 ms | overlap 1.0, top1 ok |
| query_dense_filtered | 229 | 315 | 1.38x | 4.368 ms | 3.17 ms | overlap 1.0, top1 ok |
| query_sparse | 713 | 2,502 | 3.51x | 1.402 ms | 0.4 ms | overlap 1.0, top1 ok |
| query_hybrid | 281 | 581 | 2.07x | 3.553 ms | 1.722 ms | overlap 1.0, top1 ok |
| scroll_pages | 94 | 162 | 1.72x | 10.684 ms | 6.156 ms | overlap 1.0, top1 ok |
| count_berlin | 651 | 1,578 | 2.42x | 1.535 ms | 0.634 ms | exact |
| facet_district | 782 | 2,262 | 2.89x | 1.279 ms | 0.442 ms | exact |
| query_colbert | 136 | 156 | 1.15x | 7.328 ms | 6.42 ms | overlap 1.0, top1 ok |
| count_legal | 853 | 3,257 | 3.82x | 1.172 ms | 0.307 ms | exact |
| prepared_rerun | 111 | 242 | 2.18x | 8.996 ms | 4.13 ms | overlap 1.0, top1 ok |
| ingest (10k pts, wait=false) | 6.445 s | 4.121 s | 1.56x | — | — | counts + sample point byte-identical |
| cold import / require | 1106.1 ms | 16.6 ms | 66.6x | — | — | median of 5 fresh processes |

#### Node — `@qdrant/js-client-rest 1.19.0` vs `nqql 0.4.0` (both REST)

| scenario | official ops/s | qql ops/s | qql/official | p50 official | p50 qql | parity |
|---|---:|---:|---:|---:|---:|---|
| query_dense | 311 | 802 | 2.58x | 3.212 ms | 1.247 ms | overlap 1, top1 ok |
| query_dense_filtered | 193 | 298 | 1.54x | 5.193 ms | 3.353 ms | overlap 1, top1 ok |
| query_sparse | 447 | 2,129 | 4.76x | 2.235 ms | 0.47 ms | overlap 1, top1 ok |
| query_hybrid | 270 | 504 | 1.87x | 3.701 ms | 1.982 ms | overlap 0.818, tie-break ok |
| scroll_pages | 61 | 108 | 1.77x | 16.4 ms | 9.227 ms | overlap 1, top1 ok |
| count_berlin | 423 | 1,526 | 3.61x | 2.363 ms | 0.655 ms | exact |
| facet_district | 484 | 2,146 | 4.43x | 2.067 ms | 0.466 ms | exact |
| query_colbert | 122 | 144 | 1.18x | 8.189 ms | 6.925 ms | overlap 1, top1 ok |
| count_legal | 511 | 3,075 | 6.02x | 1.955 ms | 0.325 ms | exact |
| prepared_rerun | 86 | 207 | 2.41x | 11.691 ms | 4.823 ms | overlap 1, top1 ok |
| ingest (10k pts, wait=false) | 8.72 s | 7.884 s | 1.11x | — | — | counts + sample point byte-identical |
| cold import / require | 102.7 ms | 39.5 ms | 2.6x | — | — | median of 5 fresh processes |

#### Rust — `qdrant-client 1.19.0` vs `qql 0.4.0` (both gRPC)

| scenario | official ops/s | qql ops/s | qql/official | p50 official | p50 qql | parity |
|---|---:|---:|---:|---:|---:|---|
| count_berlin | 1,652 | 1,646 | 1.00x | 0.605 ms | 0.607 ms | exact |
| count_legal | 3,750 | 3,676 | 0.98x | 0.267 ms | 0.272 ms | exact |
| facet_district | 2,213 | 2,134 | 0.96x | 0.452 ms | 0.469 ms | exact |
| prepared_rerun | 307 | 314 | 1.02x | 3.249 ms | 3.179 ms | overlap 1.0, top1 ok |
| query_colbert | 44 | 46 | 1.05x | 22.568 ms | 21.48 ms | overlap 1.0, top1 ok |
| query_dense | 1,227 | 1,261 | 1.03x | 0.815 ms | 0.793 ms | overlap 1.0, top1 ok |
| query_dense_filtered | 346 | 359 | 1.04x | 2.887 ms | 2.779 ms | overlap 1.0, top1 ok |
| query_hybrid | 733 | 733 | 1.00x | 1.364 ms | 1.363 ms | overlap 0.818, top1 ok |
| query_sparse | 2,985 | 2,760 | 0.92x | 0.335 ms | 0.362 ms | overlap 1.0, top1 ok |
| scroll_pages | 158 | 143 | 0.91x | 6.297 ms | 6.992 ms | overlap 1.0, top1 ok |
| ingest (10k pts, wait=false) | 0.79 s | 0.951 s | 0.83x | — | — | counts + sample point byte-identical |

### Application LOC (identical scenario set)

| language | official SDK | qql SDK | qql LOC ratio |
|---|---:|---:|---:|
| python | 171 | 134 | 0.78x |
| node | 152 | 126 | 0.83x |
| rust | 387 | 274 | 0.71x |

