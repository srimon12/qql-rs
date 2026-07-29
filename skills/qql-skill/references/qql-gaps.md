# QQL Gaps (agent-facing)

Engineering detail: repo root [`gaps.md`](../../../gaps.md).

## Edge (important)

Edge supports dense/sparse/hybrid by default. **Multivector, CLIP vision, and cross-encoder need opt-in models** (`multi_model`, `image_model`, `reranker_model`). Edge does **not** support `GROUP BY`, shard keys, `ALTER COLLECTION`, or ACORN.

## Not supported / incomplete (do not invent syntax)

| Area | Gap |
|---|---|
| Pagination | No OFFSET for **grouped** search |
| MMR | Not for sparse / recommend (dense nearest only) |
| Timeout / consistency | Not in QQL syntax (executor config only) |
| Hybrid shorthand | No `USING HYBRID` — use `QUERY HYBRID TEXT …` |
| ACORN | `acorn = true` but no `max_selectivity` from syntax |
| Shard | Key must be explicit string (no dynamic resolution) |
| Edge | No GROUP BY, no SHARD, batch is fan-out |

## Supported (do not claim missing)

- Schema-first `USING name` / `AS DENSE|SPARSE|MULTI`
- Multivector ColBERT path + late-interaction `RERANK … USING colbert PREFETCH`
- CLIP: `TEXT` + `IMAGE` (dense), not multi
- Cross-encoder: `CROSS RERANK TEXT '…' MODEL '…' ON FIELD text PREFETCH (…)`
- Offline edge multi/image/rerank when models configured

## Practical fallbacks

- Late interaction: `QUERY RERANK … USING colbert PREFETCH (…)`
- Pair rerank: `QUERY CROSS RERANK TEXT 'q' MODEL 'bge-reranker-base' ON FIELD text PREFETCH (c) LIMIT n`
- CLIP: dense vectors + `QUERY IMAGE '/path.jpg' …` / `QUERY TEXT '…' MODEL 'clip-…'`
- Edge without groups: filter + LIMIT, or use remote Qdrant for `GROUP BY`

## Reminder

Do not invent syntax for open gaps.
