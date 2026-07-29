# QQL Gaps (agent-facing)

Use this file when a request sounds reasonable in Qdrant terms but is still outside the current QQL surface.

**Engineering punch list (IDs, fix order, config sketch):** repo root [`gaps.md`](../../../gaps.md).

## Not Supported Yet

- Offset-style pagination for grouped search
- MMR for `USING SPARSE` or `RECOMMEND`
- ReadConsistency / Timeout controls via QQL syntax (timeout is in the Executor config layer, not the language)
- `USING HYBRID` shorthand (use `QUERY HYBRID TEXT '...' DENSE ... SPARSE ...`)
- Dynamic shard routing key resolution (shard key must be explicitly provided)
- `max_selectivity` on `PARAMS (acorn = true)` -- the plan type has the field but it is not settable from QQL syntax yet
- **Cross-encoder pair rerank** (Cohere-style / fastembed `TextRerank` / bge-reranker): QQL `RERANK` is **late-interaction MaxSim** only; pair scoring is GAP-MV-008

## Vector roles — do not invent

These are **supported** (do not claim they need raw SDK work):

- `USING name` without `AS` when executing against a live collection (schema fills dense/sparse/multi)
- `USING name AS MULTI` for ColBERT-style multivector query text
- `QUERY IMAGE 'path-or-url'` for CLIP vision → dense
- `QUERY RERANK … USING colbert` with multivector schema + host `embed_multi`
- Precomputed multi-dense: `VECTOR [[...], [...]]` and upsert `vector: { colbert: [[...]] }`
- UPSERT multivector: `USING MULTI MODEL '…' VECTOR colbert` or schema auto-embed when multivector slots exist
- UPSERT image: `USING IMAGE MODEL 'clip-vision' ON FIELD image INTO image`
- Host multi config: `multi_embedding_*` / edge offline `multi_model: "bge-m3"`
- Host image config: `image_embedding_*` / edge offline `image_model: "clip-vision"` (pair dense CLIP text model)

Do **not** invent name-based heuristics (`*sparse*` → sparse). Kind comes from schema or `AS …`.

Do **not** treat CLIP as multivector. CLIP text/vision = **dense** dual-encoder space.

## FastEmbed map (hosts)

| Host API | QQL |
|---|---|
| `TextEmbedding` (MiniLM, BGE, CLIP **text**, …) | Dense (`TEXT`) |
| `ImageEmbedding` (CLIP vision, …) | Dense (`IMAGE` / `embed_image`) |
| Sparse / BM25 | Sparse |
| `Bgem3Embedding.colbert` | Multi (`MultiDense`) |
| `TextRerank` (bge-reranker, …) | Cross-encoder — **not yet in language** |

## What To Say

Prefer plain language:

- `QQL does not support this yet.`
- `This needs raw Qdrant SDK usage or a QQL extension.`
- `The closest supported QQL form is ...`

## Practical Fallbacks

- Need CLIP text→image: `QUERY TEXT '…' MODEL 'Qdrant/clip-ViT-B-32-text' FROM coll USING image LIMIT n` (collection image vectors in CLIP space)
- Need CLIP image query: `QUERY IMAGE '/path.jpg' MODEL 'Qdrant/clip-ViT-B-32-vision' FROM coll USING image LIMIT n`
- Need CLIP ingest: `UPSERT … USING IMAGE MODEL '…' ON FIELD image INTO image` (or dual DENSE text + IMAGE)
- Need late-interaction ordering: use `QUERY RERANK TEXT 'query' MODEL 'colbert-model' FROM <collection> USING colbert PREFETCH (...) LIMIT <n>`
- Need multivector nearest without schema: use `USING colbert AS MULTI`
- Need cross-encoder rerank: not in QQL yet — use external `TextRerank` / Cohere on candidate payloads
- Need keyword plus semantic retrieval: use `QUERY HYBRID TEXT 'text' DENSE dense SPARSE sparse FUSION RRF FROM <collection> LIMIT <n>`
- Need multi-tenant isolation: use `SHARD '<key>'` on QUERY, UPSERT, SCROLL, DELETE

## Reminder

Do not hide missing features behind made-up syntax. If the current CLI cannot parse and execute it, it is outside this skill.
