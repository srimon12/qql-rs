# QQL Gaps (agent-facing)

Use this file when a request sounds reasonable in Qdrant terms but is still outside the current QQL surface.

**Engineering punch list:** repo root [`gaps.md`](../../../gaps.md).

## Not Supported Yet

- Offset-style pagination for grouped search
- MMR for `USING SPARSE` or `RECOMMEND`
- ReadConsistency / Timeout controls via QQL syntax (timeout is in the Executor config layer, not the language)
- `USING HYBRID` shorthand (use `QUERY HYBRID TEXT '...' DENSE ... SPARSE ...`)
- Dynamic shard routing key resolution (shard key must be explicitly provided)
- `max_selectivity` on `PARAMS (acorn = true)` -- the plan type has the field but it is not settable from QQL syntax yet

## Vector roles — do not invent

These are **supported**:

- `USING name` without `AS` against a live collection (schema fills dense/sparse/multi)
- `USING name AS MULTI` for ColBERT-style multivector
- `QUERY IMAGE 'path-or-url'` for CLIP vision → dense
- Late-interaction: `QUERY RERANK … USING colbert PREFETCH (…)`
- Cross-encoder: `QUERY CROSS RERANK TEXT '…' MODEL 'bge-reranker-base' ON FIELD text PREFETCH (…)`
- UPSERT multivector / image / hybrid embedding specs
- Edge offline: `multi_model`, `image_model`, `reranker_model`

Do **not** invent name-based heuristics. Kind comes from schema or `AS …`.

Do **not** conflate:

| Form | Meaning |
|---|---|
| `RERANK … USING colbert` | Late-interaction MaxSim (multivector) |
| `CROSS RERANK … MODEL 'bge-reranker…'` | Pair scorer on payload text (client-side) |
| CLIP `IMAGE` | Dense dual-encoder, not multi |

## FastEmbed map

| Host API | QQL |
|---|---|
| `TextEmbedding` | Dense `TEXT` |
| `ImageEmbedding` | Dense `IMAGE` |
| Sparse / BM25 | Sparse |
| `Bgem3Embedding.colbert` | Multi |
| `TextRerank` | `CROSS RERANK` |

## Practical Fallbacks

- Need late interaction: `QUERY RERANK … USING colbert PREFETCH (…)`
- Need pair rerank: `QUERY CROSS RERANK TEXT 'q' MODEL 'bge-reranker-base' ON FIELD text PREFETCH (c) LIMIT n`
- Need CLIP: dense named vectors + `TEXT` / `IMAGE` as above

## Reminder

Do not hide missing features behind made-up syntax.
