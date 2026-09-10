# QQL Gaps (agent-facing)

Use this file so you **do not invent syntax** for open items and **do not claim
missing** features that already ship.

---

## Edge (important)

| Capability | Status |
|---|---|
| Dense / sparse / hybrid FUSION | **Yes** (default) |
| Multivector / ColBERT (`AS MULTI`, MaxSim `RERANK`) | **Opt-in** `multi_model` / multi HTTP |
| CLIP `IMAGE` + CLIP text dense | **Opt-in** `image_model` (local **paths** only) |
| Cross-encoder `CROSS RERANK` | **Opt-in** `reranker_model` / `rerank_endpoint` |
| `GROUP BY` / query groups | **Yes** on qdrant-edge **0.8+** (`SIZE`, `LIMIT`, `OFFSET`); `LOOKUP FROM` → `QQL-EDGE-UNSUPPORTED-GROUP-LOOKUP` |
| Create-time per-vector config (`WITH VECTOR` / `WITH HNSW` / `WITH QUANTIZATION` / `WITH SPARSE`) | **Yes** — lowered onto the engine config; `memory` tiers map to the engine's RAM/mmap switch (`pinned`→RAM, `cached`/`cold`→mmap) |
| Background optimization | **No** — `qql edge optimize <collection>`; `qql --edge doctor` / `qql check --edge` report `indexed_vectors_count` lag with the same nudge |
| Remote → edge seed | **Yes (CLI)** — `qql edge bootstrap <collection> --from <url> [--shard-id N] [--force]`; single-shard auto-discovery, multi-shard fails closed (`QQL-SNAPSHOT-SHARD`) |
| Edge snapshot **creation** | **No** — qdrant-edge 0.8 exposes unpack/apply only; publish edge → remote with `qql --edge migrate … --target-url`, never by copying shard files |
| Edge → edge migration | **No** — rejected before executors start; seed devices from a server snapshot instead |
| WAL segment capacity | **Rust/CLI only** — `--wal-segment-mb` / `QQL_EDGE_WAL_SEGMENT_MB` / `LocalExecutorOptions::wal_segment_capacity`; Python/Node cannot set it in qdrant-edge 0.8 |
| `ALTER COLLECTION … WITH VECTOR <name> (…)` | **Yes (REST/gRPC)** — per-vector `HNSW` / `QUANTIZATION` / storage diffs; unnamed `WITH VECTOR (…)` targets the default vector. Edge: per-vector `HNSW` only; other fields → `QQL-EDGE-UNSUPPORTED-VECTOR-DIFF`, sparse → `QQL-EDGE-UNSUPPORTED-SPARSE-DIFF` |
| `SHARD`, shard-key DDL | **No** — `QQL-EDGE-UNSUPPORTED-*` catalog; use remote Qdrant |
| `ALTER COLLECTION` | **Partial** — remote: global + per-vector HNSW/quantization/storage; edge: global `HNSW` / `OPTIMIZERS` and per-vector `HNSW` persist, `WITH PARAMS` / `QUANTIZATION` / other per-vector fields reject per field |
| Create-time `WITH PARAMS` | **Partial** — `on_disk_payload` only; other keys → `QQL-EDGE-UNSUPPORTED-COLLECTION-PARAMS` |
| `SHOW QUOTAS` / `SET QUOTA` | **No** — `QQL-EDGE-UNSUPPORTED-QUOTA` (cluster REST `/quotas` only) |
| `PARAMS (idf = …)` | **Yes** on qdrant-edge **0.8+** (per-query sparse IDF corpus) |
| `PARAMS (acorn = …, max_selectivity = …)` | **Yes** on qdrant-edge **0.8+** |
| Batch query/update | Fan-out only (not one native batch RPC) |
| `PARAMS (timeout / consistency)` | Rejected fail-loud (`QQL-EDGE-UNSUPPORTED-TIMEOUT` / `…-CONSISTENCY`) |
| Route affinity (`X-Qdrant-Route-Affinity`) | **N/A** — single-node process; no replica pin |

Edge unsupported codes are stable (see `crates/qql-edge/README.md`).

`qql doctor` prints which hosts are loaded: dense / multi / image / cross_rerank.

---

## gRPC vs REST (quotas & affinity)

| Capability | REST | gRPC | Notes |
|---|---|---|---|
| `SHOW QUOTAS` / `SET QUOTA` | **Yes** `GET\|PUT /quotas` | **No** `QQL-GRPC-QUOTA` | Public gRPC has no quota service; use REST admin client |
| Route affinity | Header via `RestQdrant::with_route_affinity` | Metadata via `GrpcQdrant::with_route_affinity` | Transport only — **not** QQL / plan body / OpenAPI field |
| Memory / turbo4 / MATCH PREFIX / SLICE / idf | **Yes** | **Yes** (body/proto) | Quotas are the main REST-only admin gap |

---

## Open / incomplete (do not invent syntax)

| Area | Reality | Agent rule |
|---|---|---|
| Edge `GROUP BY` | Supported offline (`QQL-EDGE-*` free) except `LOOKUP FROM`. | Same QQL works on remote Qdrant; offline `SIZE`/`LIMIT`/`OFFSET` are honored. |
| Host SDK route affinity | Exposed: `pyqql.Client(route_affinity=…)`, `nqql` `{ routeAffinity }`, WASM `client.setRouteAffinity(…)`. | Route affinity is **N/A on edge** (single node). Do not add it to `localExecutor`/`httpExecutor`. |
| Edge quotas | Always unsupported. | Use remote Qdrant REST for quota admin. |
| Edge → edge migrate | Refused with a precise error (local dir is not a server). | Publish edge → remote (`--target-url`) or seed devices with `qql edge bootstrap`; no continuous sync is provided (dual-write + partial snapshots is the documented pattern). |
| Edge snapshots | qdrant-edge 0.8 can only unpack/apply snapshots, never create them. | Never tar/copy shard directories by hand; back up via a server snapshot seed flow or `qql dump`. |

---

## Closed / supported (do not list as gaps)

| Area | Use this |
|---|---|
| In-database faceting | `FACET <field> FROM <col> [WHERE ...] [LIMIT ...] [EXACT true]` → REST `/collections/{col}/facet` / gRPC `Points.Facet` |
| Implicit vector literals | `QUERY [0.1, 0.2, ...] FROM <col>` (array literal without `VECTOR` keyword) |
| Payload inclusion default | `WITH PAYLOAD` defaults to `true` when omitted; use `WITH PAYLOAD false` to explicitly omit |
| Formula decay ISO strings | `EXP_DECAY(field, TARGET = "2024-01-01T00:00:00Z", ...)` — auto-infers `datetime_key` and parses ISO string |
| Hybrid shorthand | `USING HYBRID` or `QUERY HYBRID TEXT …` (same expand) |
| Request timeout | `PARAMS (timeout = 30)` → REST `?timeout=30` / gRPC `timeout` (seconds) |
| Read consistency | `PARAMS (consistency = majority\|quorum\|all\|N)` → OpenAPI `ReadConsistency` |
| ACORN params | `PARAMS (acorn = true, max_selectivity = 0.4)` — remote Qdrant and edge 0.8+ |
| Cluster quotas (REST) | `SHOW QUOTAS;` / `SET QUOTA (enabled = true, max_resident_memory_percent = 80) WAIT true;` — full replace; not gRPC/edge |
| Memory placement | `memory = 'cold'\|'cached'\|'pinned'` on VECTOR / HNSW / SPARSE / QUANTIZATION / indexes; `payload_memory` in `PARAMS` (no `pinned`) |
| Per-vector ALTER diffs | `ALTER COLLECTION c WITH VECTOR <name> (HNSW (…), QUANTIZATION (…), VECTOR (…))` / `WITH SPARSE <name> (SPARSE (…))` → typed PATCH `vectors` / `sparse_vectors` maps (gRPC `VectorsConfigDiff`) |
| TurboQuant dense | `WITH VECTOR (…, datatype = 'turbo4')` |
| Keyword prefix | Index `WITH (prefix = true)`; filter `field MATCH PREFIX '…'` |
| Slice sampling | `WHERE SLICE (total, index)` |
| Sparse IDF corpus | `PARAMS (idf = 'global' \| WHERE <filter>)` — remote + edge 0.8+ |
| Route affinity (host SDKs) | `pyqql.Client(route_affinity=…)` · `nqql` `{ routeAffinity }` · wasm `client.setRouteAffinity(key)` · Rust `RestQdrant`/`GrpcQdrant::with_route_affinity` → `X-Qdrant-Route-Affinity` |
| Exact count | `COUNT FROM coll WITH (exact = true)` |
| Specific payload deletion | `DELETE PAYLOAD key1, key2 FROM coll WHERE ...` |
| Multi-collection lookup | `GROUP BY ... LOOKUP FROM coll` → `QueryRequest.lookup_from` |
| Grouped pagination (OFFSET with GROUP BY) | `GROUP BY … OFFSET N` → maps to `group_offset` |
| MMR with sparse vectors | `USING … AS SPARSE` with MMR is supported |
| Filter `min_should` | Conjunction threshold on compound filters |
| Request-level shard routing | QQL `SHARD '…'` / `SHARD 101` or `stmt.shard_key` → REST `shard_key` / gRPC `ShardKeySelector` (never inside Filter) |
| Schema-first vectors | `USING name` / `AS DENSE\|SPARSE\|MULTI` |
| Multivector / late interaction | `USING colbert` / `AS MULTI`; `RERANK … PREFETCH` |
| CLIP | `QUERY IMAGE '…'` / `TEXT` into same dense space |
| Cross-encoder | `CROSS RERANK TEXT '…' MODEL '…' ON FIELD text PREFETCH (…)` |
| Doctor hosts | `qql doctor` → dense/multi/image/cross_rerank |

---

## Practical fallbacks

| Need | Pattern |
|---|---|
| Hybrid | `QUERY 'q' FROM docs USING HYBRID LIMIT 10` |
| Cluster timeout | `PARAMS (timeout = 30)` on QUERY |
| Replica reads | `PARAMS (consistency = majority)` |
| Pin reads to a replica | `pyqql.Client(route_affinity=…)` / `nqql` `{ routeAffinity }` / wasm `setRouteAffinity(key)` / Rust `RestQdrant::…with_route_affinity("session-key")` — not QQL |
| Multi-tenant shard | `SHARD 'tenant'` / `stmt.shard_key` + `inject_filter(…, tenant_id, …)` |
| Tenant-local sparse IDF | `PARAMS (idf = WHERE tenant_id = 'acme')` + `WHERE tenant_id = 'acme'` |
| Faceted page 2 (groups) | `GROUP BY … OFFSET N` — maps to Qdrant `group_offset` |
| Edge group lookup | `GROUP BY district` works offline; `LOOKUP FROM` needs remote Qdrant |
| Quota admin offline | Use remote Qdrant REST; never invent edge quota ops |

---

## Reminder

- Open gaps: do **not** invent syntax for items still listed as Open.
- Closed items: prefer the supported forms above.
- Wire shapes: always check `crates/qql-runtime/openapi.json` and `proto/`.
