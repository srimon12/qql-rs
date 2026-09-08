### Results

#### Python — `qdrant-client 1.19.0` vs `pyqql 0.4.0` (both REST)

| scenario | official ops/s | qql ops/s | qql/official | p50 official | p50 qql | parity |
|---|---:|---:|---:|---:|---:|---|
| query_dense | 418 | 828 | 1.98x | 2.395 ms | 1.207 ms | overlap 1.0, top1 ok |
| query_dense_filtered | 234 | 322 | 1.38x | 4.271 ms | 3.104 ms | overlap 1.0, top1 ok |
| query_sparse | 764 | 2,402 | 3.14x | 1.308 ms | 0.416 ms | overlap 1.0, top1 ok |
| query_hybrid | 264 | 530 | 2.01x | 3.792 ms | 1.885 ms | overlap 0.818, top1 differs |
| scroll_pages | 95 | 134 | 1.41x | 10.495 ms | 7.475 ms | overlap 1.0, top1 ok |
| count_berlin | 648 | 1,545 | 2.38x | 1.543 ms | 0.647 ms | exact |
| facet_district | 784 | 2,255 | 2.88x | 1.276 ms | 0.444 ms | exact |
| query_colbert | 129 | 136 | 1.05x | 7.77 ms | 7.364 ms | overlap 1.0, top1 ok |
| count_legal | 858 | 3,096 | 3.61x | 1.165 ms | 0.323 ms | exact |
| prepared_rerun | 101 | 207 | 2.05x | 9.938 ms | 4.83 ms | overlap 1.0, top1 ok |
| ingest (10k pts, wait=false) | 7.229 s | 9.392 s | 0.77x | — | — | counts + sample point byte-identical |
| cold import / require | 1048.0 ms | 31.8 ms | 33.0x | — | — | median of 5 fresh processes |

#### Node — `@qdrant/js-client-rest 1.19.0` vs `nqql 0.4.0` (both REST)

| scenario | official ops/s | qql ops/s | qql/official | p50 official | p50 qql | parity |
|---|---:|---:|---:|---:|---:|---|
| query_dense | 293 | 850 | 2.90x | 3.417 ms | 1.176 ms | overlap 1, top1 ok |
| query_dense_filtered | 203 | 309 | 1.52x | 4.919 ms | 3.238 ms | overlap 1, top1 ok |
| query_sparse | 450 | 2,339 | 5.20x | 2.222 ms | 0.427 ms | overlap 1, top1 ok |
| query_hybrid | 247 | 556 | 2.25x | 4.045 ms | 1.798 ms | overlap 1, top1 differs |
| scroll_pages | 59 | 140 | 2.37x | 16.863 ms | 7.16 ms | overlap 1, top1 ok |
| count_berlin | 410 | 1,497 | 3.65x | 2.436 ms | 0.668 ms | exact |
| facet_district | 484 | 2,144 | 4.43x | 2.068 ms | 0.466 ms | exact |
| query_colbert | 118 | 145 | 1.23x | 8.49 ms | 6.916 ms | overlap 1, top1 ok |
| count_legal | 505 | 3,077 | 6.09x | 1.98 ms | 0.325 ms | exact |
| prepared_rerun | 82 | 206 | 2.51x | 12.254 ms | 4.856 ms | overlap 1, top1 ok |
| ingest (10k pts, wait=false) | 8.803 s | 7.223 s | 1.22x | — | — | counts + sample point byte-identical |
| cold import / require | 107.3 ms | 38.3 ms | 2.8x | — | — | median of 5 fresh processes |

#### Rust — `qdrant-client 1.19.0` vs `qql 0.4.0` (both gRPC)

| scenario | official ops/s | qql ops/s | qql/official | p50 official | p50 qql | parity |
|---|---:|---:|---:|---:|---:|---|
| count_berlin | 1,595 | 1,595 | 1.00x | 0.627 ms | 0.627 ms | exact |
| count_legal | 3,842 | 3,732 | 0.97x | 0.26 ms | 0.268 ms | exact |
| facet_district | 2,474 | 2,335 | 0.94x | 0.404 ms | 0.428 ms | exact |
| prepared_rerun | 191 | 266 | 1.39x | 5.235 ms | 3.753 ms | overlap 1.0, top1 ok |
| query_colbert | 44 | 46 | 1.05x | 22.639 ms | 21.691 ms | overlap 1.0, top1 ok |
| query_dense | 834 | 841 | 1.01x | 1.199 ms | 1.189 ms | overlap 1.0, top1 ok |
| query_dense_filtered | 315 | 313 | 0.99x | 3.17 ms | 3.194 ms | overlap 1.0, top1 ok |
| query_hybrid | 667 | 640 | 0.96x | 1.499 ms | 1.56 ms | overlap 0.818, top1 differs |
| query_sparse | 2,627 | 2,339 | 0.89x | 0.381 ms | 0.427 ms | overlap 1.0, top1 ok |
| scroll_pages | 155 | 130 | 0.84x | 6.424 ms | 7.688 ms | overlap 1.0, top1 ok |
| ingest (10k pts, wait=false) | 0.781 s | 0.992 s | 0.79x | — | — | counts + sample point byte-identical |

### Application LOC (identical scenario set)

| language | official SDK | qql SDK | qql LOC ratio |
|---|---:|---:|---:|
| python | 170 | 134 | 0.79x |
| node | 152 | 126 | 0.83x |
| rust | 421 | 253 | 0.6x |

