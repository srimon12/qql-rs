# QQL embeddings reference

How text becomes vectors. Schema topology fills `USING` kinds, then hosts embed. This file owns the full matrix. Query guides repeat only the minimum needed to run alone.

## Using roles

| Form | Behavior |
|---|---|
| `USING name` | Runtime looks up `name` on collection schema. Dense, sparse, or multivector. Names never special-case by spelling |
| `USING name AS DENSE` | One dense vector. MiniLM, CLIP text, precomputed literal |
| `USING name AS SPARSE` | Sparse vector. Wire-compatible BM25. Unit-weight queries, tf-saturated documents |
| `USING name AS MULTI` | Multivector bag `[[f32]]` via `embed_multi`. BGE-M3 ColBERT, not CLIP |
| `USING HYBRID ...` | Text expands to dense plus sparse fusion. Same AST as `QUERY HYBRID` |
| No `USING` | Schema must hold exactly one compatible vector |

Offline paths without schema require explicit `AS ...`. Unknown kind fails closed with `QQL-VECTOR-KIND`. There is no silent dense default.

## Prepare order

```text
parse into Stmt
  -> fetch collection topology: dense names, sparse names, multivector flags
  -> fill USING kinds from topology
  -> batch dense texts by model through embedder
  -> encode sparse legs locally with BM25
  -> encode multi legs through embed_multi or reject with actionable error
  -> plan into PlannedOperation
```

WASM follows the same order in-browser. It fetches topology when Qdrant is reachable, then embeds. `USING sparse` and `USING colbert` work without `AS` when topology resolves.

## Dense text

```sql
UPSERT INTO docs VALUES
  {id: 1, text: 'Qdrant vector database', category: 'tech'}
  USING DENSE MODEL 'all-minilm:l6-v2';

QUERY TEXT 'vector database latency' FROM docs USING dense LIMIT 10;
```

Key decisions:

- `MODEL '...'` names the dense model for this leg. `ON FIELD <field>` selects source text. `INTO <vector>` selects the destination vector.
- `USING DENSE MODEL '...'` without `ON FIELD` embeds the default text field. Multiple legs map distinct fields to distinct vectors.
- Single dense model per WASM client. A non-default `MODEL` clause is rejected with `QQL-EMBEDDING`. Remote hosts batch dense texts by model.
- Precomputed literals skip embedding. `QUERY [0.1, 0.2] FROM docs USING dense LIMIT 10` never calls the embedder.

## Sparse BM25

```sql
QUERY TEXT 'kubernetes deployment' FROM incidents USING sparse LIMIT 10;

QUERY TEXT 'kubernetes deployment' FROM incidents USING sparse AS SPARSE LIMIT 10;
```

Key decisions:

- Wire-compatible with Qdrant `qdrant/bm25`. Murmur3-32 token IDs, word tokenizer, English stopwords plus snowball stemming. Queries embed unit weights. Documents use tf saturation.
- Sparse document encoding is always local. HTTP embedders serve dense, multi, image, and rerank only. Document-side BM25 params tune the write path: `k1`, `b`, `avg_len`. Defaults are `1.2`, `0.75`, `256`. Invalid values raise `QQL-VALIDATION-CONFIG`. Re-ingest to apply.
- `USING bm25` defaults model to `Qdrant/bm25` when unspecified.
- Tenant IDF scoping is a search param, not embedding config. `PARAMS (idf = 'global')` or `PARAMS (idf = WHERE tenant_id = 'acme')`. See `qql-params.md`.

Host knobs:

- Rust: `Bm25Params`, `LocalExecutorOptions::bm25_*`, `FastEmbedderOptions`, `HttpEmbedderOptions`.
- Python: `Client(embedder={..., bm25_k1, bm25_b, bm25_avg_len})`, `local_executor(..., bm25_k1=, ...)`, `http_executor`.
- Node: `{ bm25K1, bm25B, bm25AvgLen }` on embedder and local executor.
- WASM: `client.setBm25Params(k1, b, avgLen)`.
- CLI: `qql config edge --bm25-*` and `QQL_EDGE_BM25_*`.

## Multivector ColBERT

```sql
CREATE COLLECTION docs (
  dense VECTOR(384, COSINE),
  sparse SPARSE,
  colbert VECTOR(128, COSINE) WITH MULTIVECTOR (comparator = 'max_sim')
);

QUERY TEXT 'vector database latency' FROM docs USING colbert LIMIT 10;

QUERY TEXT 'vector database latency' FROM docs USING colbert AS MULTI LIMIT 10;

QUERY NEAREST VECTOR [[0.1, 0.2], [0.3, 0.4], [0.5, 0.6]] FROM docs USING colbert LIMIT 10;
```

Key decisions:

- Multivector is a dense role with multi shape, not a third kind. Schema marks the column with `WITH MULTIVECTOR`. Runtime sets `multi` before embedding so text becomes `MultiDense`.
- Token dimension must match the model. BGE-M3 outputs 1024-d tokens. `answerai-colbert-small-v1` outputs 128-d tokens. Set `VECTOR(dim, COSINE)` to the actual model.
- Host needs `embed_multi` for text to multivector. Default HTTP embedders are single dense only. Pass precomputed `VECTOR [[...]]` or use a multi-capable embedder when available.
- Edge multivector is opt-in via `multi_model` or multi HTTP host.
- Upsert accepts precomputed bags. `vector: {colbert: [[0.1, 0.2], [0.3, 0.4]]}`.

## CLIP text to image

```sql
QUERY TEXT 'sunset over harbor' MODEL 'Qdrant/clip-ViT-B-32-text' FROM photos USING image LIMIT 10;

QUERY IMAGE '/path.jpg' MODEL 'Qdrant/clip-ViT-B-32-vision' FROM photos USING image LIMIT 10;

UPSERT INTO photos VALUES {id: 1, vector: {image: '/path.jpg', model: 'clip-vision'}};
```

Key decisions:

- Text and image legs share one dense image space. Query with text or image against the same `USING image` target.
- Edge image is opt-in via `image_model`. Local paths only.
- Per-point image dicts use `{image: ..., model: ...}` inside `vector`. Same inference option rules as text. See `qql-params.md`.

## Rerank models

Late-interaction `RERANK ... USING colbert` needs a ColBERT model plus multivector target. Pair-scorer `CROSS RERANK ... ON FIELD text` needs a cross-encoder model plus a host `rerank_pairs`. Edge exposes `reranker_model` or `rerank_endpoint`. HTTP hosts expose a rerank endpoint. `qql doctor` prints loaded hosts as dense, multi, image, cross_rerank.

## Embedder configuration by host

Python uses `HttpEmbedder(endpoint, model, dim)` or an embedder dict on `Client`. Node uses `{endpoint, apiKey, model, dimension}` on `Client`. WASM uses `setHttpEmbedder(url, model, dim, key)` or `setEmbedder(async (texts) => number[][])`. Rust uses `HttpEmbedder` plus `Executor::new` with an `Embedder` trait object. CLI uses `EMBED_URL`, `EMBED_MODEL`, `EMBED_DIM`. Check `hasEmbedder` on WASM before embedding paths. A missing embedder on a text path fails with `QQL-EMBEDDING`, never with silent zero vectors.
