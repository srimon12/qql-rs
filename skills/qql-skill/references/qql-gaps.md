# QQL Gaps (agent-facing)

Engineering source of truth: repo root [`gaps.md`](../../../gaps.md).

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
| Batch query/update | Fan-out only (not one native batch RPC) |
| `PARAMS (timeout / consistency)` | No-op / N/A on single-node edge |

Edge unsupported codes are stable (see `crates/qql-edge/README.md`).

`qql doctor` prints which hosts are loaded: dense / multi / image / cross_rerank.

---

## Open / incomplete (do not invent syntax)

| Area | Reality | Agent rule |
|---|---|---|
| Grouped pagination | **No OFFSET with `GROUP BY`**. Qdrant OpenAPI `QueryGroupsRequest` has no `offset`. Fail-closed: `QQL-PLAN-GROUP-OFFSET`. | Use `LIMIT` only on groups. Do not invent group cursor syntax until Qdrant supports it. |
| MMR | **Dense nearest only**. Sparse → `QQL-PLAN-MMR-SPARSE`. | Do not use MMR on sparse/recommend. |
| Edge `GROUP BY` | Rejected offline (`QQL-EDGE-UNSUPPORTED-GROUP-BY`). | Same QQL works on remote Qdrant; offline: filter + `LIMIT`. |

---

## Closed / supported (do not list as gaps)

| Area | Use this |
|---|---|
| Hybrid shorthand | `USING HYBRID` or `QUERY HYBRID TEXT …` (same expand) |
| Request timeout | `PARAMS (timeout = 30)` → REST `?timeout=30` / gRPC `timeout` (seconds) |
| Read consistency | `PARAMS (consistency = majority\|quorum\|all\|N)` → OpenAPI `ReadConsistency` |
| ACORN params (remote) | `PARAMS (acorn = true, max_selectivity = 0.4)` — not on edge |
| Exact count | `COUNT FROM coll WITH (exact = true)` |
| Specific payload deletion | `DELETE PAYLOAD key1, key2 FROM coll WHERE ...` |
| Multi-collection lookup | `GROUP BY ... LOOKUP FROM coll` → `QueryRequest.lookup_from` |
| Filter shard & min_should | `FilterCompound.shard_key` and `min_should` threshold |
| Dynamic shard (host) | `inject_shard_key(stmt, tenant)` / `stmt.inject_shard_key(tenant)` — or literal `SHARD '…'` |
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
| Multi-tenant shard | `inject_shard_key(stmt, tenant)` + `inject_filter(…, tenant_id, …)` |
| Faceted page 2 (groups) | **Not in Qdrant** — do not invent OFFSET for groups |
| Edge without groups | `WHERE` + `LIMIT`, or remote Qdrant for `GROUP BY` |

---

## Reminder

- Open gaps: do **not** invent syntax (especially group OFFSET — blocked on Qdrant).
- Closed items: prefer the supported forms above.
- Wire shapes: always check `crates/qql-runtime/openapi.json` and `proto/`.
