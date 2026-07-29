# QQL Implementation Gaps

Engineering source of truth. Skill-facing summary:
[`skills/qql-skill/references/qql-gaps.md`](skills/qql-skill/references/qql-gaps.md).

**Spec authority for wire shapes:**
[`crates/qql-runtime/openapi.json`](crates/qql-runtime/openapi.json) and
[`crates/qql-runtime/proto/`](crates/qql-runtime/proto/). Do not invent fields
not present there.

Status: **OPEN** | **PARTIAL** | **DONE** | **BLOCKED**

---

## 0. Edge capability matrix

| Capability | Edge status | Notes |
|---|---|---|
| Dense ANN + sparse + hybrid FUSION | **Yes** | Default offline path |
| Multivector store + query | **Yes** | Opt-in multi model for embed |
| `CROSS RERANK` / CLIP IMAGE | **Opt-in** | Models required; IMAGE = local path |
| `GROUP BY` | **No** | `QQL-EDGE-UNSUPPORTED-GROUP-BY` |
| `SHARD` / ACORN / ALTER | **No** | Stable `QQL-EDGE-UNSUPPORTED-*` catalog |
| `timeout` / `consistency` | **N/A** | Single-node; request-level opts ignored |
| Batch RPC | **No** | Fan-out only (QUAL-002 deferred) |

---

## 1. Multivector / CLIP / cross-encoder

| ID | Status | Item |
|---|---|---|
| GAP-MV / KIND | **DONE** | Host multi/image/rerank, schema-first kinds |
| GAP-EDGE-001 / UX-008 | **DONE** | CLI doctor host summary |
| GAP-EDGE-002 | **PARTIAL** | IMAGE = local path only |
| GAP-EDGE-003 | **PARTIAL** | Heavy model e2e still optional/CI-skippable |
| GAP-EDGE-004 | **OPEN** | Edge GROUP BY (engine); catalog + docs closed product decision |

---

## 2. UX / language

| ID | Status | Notes |
|---|---|---|
| **GAP-UX-001** | **BLOCKED** | Group OFFSET — not in Qdrant OpenAPI; fail-closed |
| **GAP-UX-002** | **PARTIAL** | MMR dense-only; fail-closed sparse |
| **GAP-UX-003** | **DONE** | `PARAMS (timeout, consistency)` OpenAPI/proto aligned |
| **GAP-UX-004** | **DONE** | `USING HYBRID` |
| **GAP-UX-005** | **DONE** | ACORN + max_selectivity |
| **GAP-UX-006** | **DONE** | `inject_shard_key` |
| **GAP-UX-007** | **DONE** | Edge GROUP BY: clear unsupported catalog + remote hint |
| **GAP-UX-008** | **DONE** | Doctor hosts |

---

## 3. Quality / transport

| ID | Status | Item |
|---|---|---|
| **GAP-QUAL-001** | **DONE** | Query path parity + **DDL sub-config REST OpenAPI projection**: flatten create params, nest quantization (`scalar`/`product`/`binary`/`turbo`), nest index `field_schema`, multi-step create (deferred fan-out params + shard keys) matching gRPC. gRPC accepts flat IR and nested OpenAPI quant shapes. |
| **GAP-QUAL-002** | **OPEN** | Edge batch fan-out (deferred; check fastembed batch later) |
| **GAP-QUAL-003** | **DONE** | Edge unsupported catalog in `qql-edge/src/backend/unsupported.rs` with stable codes + remote remediation |
| **GAP-QUAL-004** | **DONE** | OpenAPI contract cases for multi-dense, hybrid, formula, groups, timeout query params; CROSS RERANK is plan-only (not a fake Query body); multi REST↔gRPC shape tests |

---

## 4. Fix order (remaining)

1. **GAP-QUAL-002** — Edge / host batch (after fastembed batching review)  
2. **GAP-EDGE-002** — Optional IMAGE URL fetch  
3. **GAP-EDGE-003** — Optional heavy multi+CROSS e2e  
4. **GAP-UX-001** — Only when Qdrant groups support pagination  


---

## 5. Supported (do not re-open)

- Hybrid `USING HYBRID` / `QUERY HYBRID`
- Request timeout + read consistency
- ACORN + max_selectivity (remote)
- `inject_shard_key`
- Multi / CLIP / CROSS RERANK when hosts configured
- Fail-closed group OFFSET + MMR sparse
- Edge unsupported catalog + REST/gRPC query parity tests

---

## 6. Related docs

| Doc | Role |
|---|---|
| OpenAPI / proto | Wire truth |
| `docs/syntax.md` | Language |
| `crates/qql-edge/README.md` | Edge unsupported table |
| Skill `qql-gaps.md` | Agent rules |
