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
| `GROUP BY` / query groups | **No** — `QQL-EDGE-UNSUPPORTED-GROUP-BY`; use remote Qdrant |
| `SHARD`, `ALTER COLLECTION`, ACORN | **No** — `QQL-EDGE-UNSUPPORTED-*` catalog; use remote Qdrant |
| `SHOW QUOTAS` / `SET QUOTA` | **No** — `QQL-EDGE-UNSUPPORTED-QUOTA` (cluster REST `/quotas` only) |
| `PARAMS (idf = …)` | **Yes** on qdrant-edge **0.8+** (per-query sparse IDF corpus) |
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
| Edge `GROUP BY` | Rejected offline (`QQL-EDGE-UNSUPPORTED-GROUP-BY`). | Same QQL works on remote Qdrant; offline: filter + `LIMIT`. |
| Host SDK route affinity | Only Rust `RestQdrant` / `GrpcQdrant`. | Do **not** invent `Client(route_affinity=…)` in pyqql / nqql / wasm until bindings expose it. |
| Edge quotas | Always unsupported. | Use remote Qdrant REST for quota admin. |

---

## Closed / supported (do not list as gaps)

| Area | Use this |
|---|---|
| Hybrid shorthand | `USING HYBRID` or `QUERY HYBRID TEXT …` (same expand) |
| Request timeout | `PARAMS (timeout = 30)` → REST `?timeout=30` / gRPC `timeout` (seconds) |
| Read consistency | `PARAMS (consistency = majority\|quorum\|all\|N)` → OpenAPI `ReadConsistency` |
| ACORN params (remote) | `PARAMS (acorn = true, max_selectivity = 0.4)` — not on edge |
| Cluster quotas (REST) | `SHOW QUOTAS;` / `SET QUOTA (enabled = true, max_resident_memory_percent = 80) WAIT true;` — full replace; not gRPC/edge |
| Memory placement | `memory = 'cold'\|'cached'\|'pinned'` on VECTOR / HNSW / SPARSE / QUANTIZATION / indexes; `payload_memory` in `PARAMS` (no `pinned`) |
| TurboQuant dense | `WITH VECTOR (…, datatype = 'turbo4')` |
| Keyword prefix | Index `WITH (prefix = true)`; filter `field MATCH PREFIX '…'` |
| Slice sampling | `WHERE SLICE (total, index)` |
| Sparse IDF corpus | `PARAMS (idf = 'global' \| {corpus: {must: […]}})` — remote + edge 0.8+ |
| Route affinity (Rust) | `RestQdrant::with_route_affinity` / `GrpcQdrant::with_route_affinity` → `X-Qdrant-Route-Affinity` |
| Exact count | `COUNT FROM coll WITH (exact = true)` |
| Specific payload deletion | `DELETE PAYLOAD key1, key2 FROM coll WHERE ...` |
| Multi-collection lookup | `GROUP BY ... LOOKUP FROM coll` → `QueryRequest.lookup_from` |
| Grouped pagination (OFFSET with GROUP BY) | `GROUP BY … OFFSET N` → maps to `group_offset` |
| MMR with sparse vectors | `USING … AS SPARSE` with MMR is supported |
| Filter `min_should` | Conjunction threshold on compound filters |
| Request-level shard routing | QQL `SHARD '…'` or `stmt.shard_key` → REST `shard_key` / gRPC `ShardKeySelector` (never inside Filter) |
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
| Pin reads to a replica (Rust) | `RestQdrant::…with_route_affinity("session-key")` / gRPC equivalent — not QQL |
| Multi-tenant shard | `SHARD 'tenant'` / `stmt.shard_key` + `inject_filter(…, tenant_id, …)` |
| Tenant-local sparse IDF | `PARAMS (idf = {corpus: {must: [{key: 'tenant_id', match: {value: 'acme'}}]}})` + `WHERE tenant_id = 'acme'` |
| Faceted page 2 (groups) | `GROUP BY … OFFSET N` — maps to Qdrant `group_offset` |
| Edge without groups | `WHERE` + `LIMIT`, or remote Qdrant for `GROUP BY` |
| Quota admin offline | Use remote Qdrant REST; never invent edge quota ops |

---

## Reminder

- Open gaps: do **not** invent syntax for items still listed as Open.
- Closed items: prefer the supported forms above.
- Wire shapes: always check `crates/qql-runtime/openapi.json` and `proto/`.
