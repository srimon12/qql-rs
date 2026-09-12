# QQL query reference

All `QUERY` forms. Each section names the problem, the QQL shape, and the key decisions. Companion runnable files live in `examples/`.

Clause order is enforced. `SHARD` sits after `WHERE` and before `PARAMS`. `LIMIT` and `OFFSET` close the statement. See `SKILL.md` for the full order template.

Vector roles apply to every form with `USING`. `USING name` resolves kind from collection schema. Offline use `AS DENSE` or `AS SPARSE` or `AS MULTI`. Unknown kind fails closed with `QQL-VECTOR-KIND`. Full embedding rules live in `qql-embeddings.md`.

## 1. Nearest vector search

Problem: closest vectors to text, a literal vector, an image, or a point reference.

```sql
QUERY TEXT 'vector database performance' FROM articles USING dense LIMIT 10;

QUERY [0.1, 0.2, 0.3] FROM docs USING dense LIMIT 10;

QUERY TEXT 'sunset over harbor' MODEL 'Qdrant/clip-ViT-B-32-text' FROM photos USING image LIMIT 10;

QUERY IMAGE '/path.jpg' MODEL 'Qdrant/clip-ViT-B-32-vision' FROM photos USING image LIMIT 10;

QUERY POINT 42 FROM docs USING dense LIMIT 10;
```

Key decisions:

- `TEXT '...'` embeds through the host embedder. Bare `'...'` without `TEXT` is the same text path in most SDK paths. Prefer `TEXT` when a model or options follows.
- `[0.1, 0.2]` is an implicit vector literal. The `VECTOR` keyword prefix is optional and means the same shape.
- `MODEL '...'` selects the inference model for this input. `OPTIONS {...}` passes opaque inference options. See `qql-params.md`.
- `USING dense` without `AS` needs a reachable schema. Offline appends `AS DENSE`.
- Payloads return by default. Add `WITH PAYLOAD false` for minimal bandwidth. Add `WITH VECTOR (...)` to return stored vectors.
- `SCORE THRESHOLD <n>` drops low-score hits on this leg.
- `PARAMS (hnsw_ef = 128, exact = false)` tunes HNSW. `PARAMS (timeout = 30, consistency = majority)` tunes request routing on remote Qdrant.

## 2. Point fetch

Problem: fetch known IDs without vector scoring.

```sql
QUERY POINTS (1, 2, 'uuid-3') FROM docs;

QUERY POINTS (:a, :b) FROM docs;
```

Key decisions:

- IDs mix integers and UUID strings. Placeholders bind the same way as filters.
- No `USING` clause. No vector scoring. Result shape is points, not scored hits.
- SDK accessors: `report.points()` in Python and Node, `points()` in WASM dx, typed points in Rust.

## 3. Hybrid dense plus sparse

Problem: semantic recall plus exact keyword recall in one statement.

```sql
QUERY TEXT 'kubernetes deployment' FROM incidents
  USING HYBRID DENSE dense SPARSE sparse FUSION RRF
  LIMIT 10;

QUERY 'kubernetes deployment' FROM incidents USING HYBRID LIMIT 10;

QUERY HYBRID TEXT 'kubernetes deployment' DENSE dense SPARSE sparse FUSION RRF
  FROM incidents LIMIT 10;
```

Key decisions:

- All three spellings expand to the same AST. Front-form `QUERY HYBRID` equals back-form `USING HYBRID`.
- Bare `USING HYBRID` needs schema with exactly one dense and one sparse vector. Name them explicitly when the collection holds more than one of either kind.
- `FUSION` defaults to `RRF`. Use `FUSION DBSF` for distribution-based score fusion.
- RRF tuning lives in `PARAMS (rrf_k = 60, rrf_weights = [0.8, 0.2])`. Never in `WITH (...)`.
- Sparse leg embeds with local BM25. Query weights stay unit. Document weights use tf saturation. See `qql-embeddings.md`.

## 4. Fusion over prefetches

Problem: merge independent candidate streams with per-leg filters and thresholds.

```sql
WITH
  dense AS (
    QUERY TEXT 'vector database performance' FROM articles USING dense
    WHERE category = 'tech' LIMIT 200
  ),
  sparse AS (
    QUERY TEXT 'vector database performance' FROM articles USING sparse LIMIT 300
  )
QUERY FUSION RRF FROM articles
  PREFETCH (dense SCORE THRESHOLD 0.6, sparse SCORE THRESHOLD 0.3)
  LIMIT 10;
```

Key decisions:

- CTEs define independent legs with their own `WHERE`, `LIMIT`, `SCORE THRESHOLD`.
- `PREFETCH (a, b)` names the legs to fuse. Optional per-leg `WHERE` narrows after prefetch. Optional `SCORE THRESHOLD` filters before fusion.
- `QUERY FUSION RRF` merges ranks without score normalization. `QUERY FUSION DBSF` merges score distributions.
- Nested CTEs compose. A later CTE can `PREFETCH` an earlier CTE for coarse-to-fine pipelines. See `examples/hybrid-fusion.qql`.

## 5. Rerank with ColBERT MaxSim

Problem: late-interaction rescoring of dense candidates with token-level multivectors.

```sql
WITH candidates AS (
  QUERY TEXT 'vector database latency' FROM docs USING dense LIMIT 100
)
QUERY RERANK TEXT 'vector database latency' MODEL 'answerai-colbert-small-v1'
FROM docs
USING colbert
PREFETCH (candidates)
LIMIT 10;
```

Key decisions:

- `RERANK` is late interaction, not a pair scorer. It needs a multivector target. Schema marks the column with `WITH MULTIVECTOR (comparator = 'max_sim')`.
- `USING colbert` resolves multi shape from schema. Offline use `USING colbert AS MULTI`.
- Token dimension must match the model. BGE-M3 uses 1024-d tokens. Smaller ColBERT models use 128-d tokens. Set `VECTOR(dim, COSINE)` accordingly at create time.
- Host embedder must implement `embed_multi` for `TEXT` to multivector. Otherwise pass precomputed `VECTOR [[...]]`.
- Precomputed form: `QUERY NEAREST VECTOR [[0.1, 0.2], [0.3, 0.4]] FROM docs USING colbert LIMIT 10`.

## 6. Cross-encoder rerank

Problem: reorder candidates with a `(query, doc_text)` pair scorer.

```sql
WITH candidates AS (
  QUERY TEXT 'vector database latency' FROM docs USING dense LIMIT 50
)
QUERY CROSS RERANK TEXT 'vector database latency' MODEL 'bge-reranker-base'
  ON FIELD text
  FROM docs
  PREFETCH (candidates)
  LIMIT 10;
```

Key decisions:

- `CROSS RERANK` runs client-side. The executor fetches candidates, reads `ON FIELD` text (default `text`), scores pairs, reorders. It is not a Qdrant wire `Query` variant.
- Host needs `rerank_pairs`. Edge needs `reranker_model` or HTTP `rerank_endpoint`.
- Do not confuse with `RERANK ... USING colbert`. That path is MaxSim multivector. This path is pair scoring.
- `compile` and `try_route` reject bare `CROSS RERANK` as client-side-only. Execute through `Client` or `Executor`, never offline route compilation alone.

## 7. Recommend by example

Problem: recommend from liked and disliked point IDs.

```sql
QUERY RECOMMEND POSITIVE (101, 102, 103) NEGATIVE (201)
  STRATEGY average_vector
  FROM product_catalog
  USING product_dense
  WHERE availability = 'in_stock'
  SCORE THRESHOLD 0.5
  LIMIT 20;
```

Key decisions:

- Positive IDs pull the centroid toward liked items. Negative IDs push away from disliked items.
- `STRATEGY` is optional. `average_vector` is the common explicit choice. Omit for server default.
- Inputs accept point IDs, vectors, or text depending on host resolution. IDs are the canonical stable form.
- Filters and thresholds apply after candidate scoring.

## 8. Context search

Problem: search guided by positive and negative example pairs.

```sql
QUERY CONTEXT (POSITIVE POINT 1 NEGATIVE POINT 2) FROM docs USING dense LIMIT 10;

QUERY CONTEXT (
  POSITIVE POINT 1 NEGATIVE POINT 2,
  POSITIVE POINT 3 NEGATIVE POINT 4
) FROM docs USING dense WHERE category = 'tech' LIMIT 10;
```

Key decisions:

- Each pair is one positive plus one negative point. Multiple pairs widen the guidance.
- `POINT` keyword marks ID inputs explicitly. Bare IDs parse in the same positions where the grammar allows them.
- Same `USING` and filter and threshold rules as nearest search.

## 9. Discover search

Problem: explore around a target anchor steered by context pairs.

```sql
QUERY DISCOVER TARGET POINT 1
  CONTEXT (POSITIVE POINT 2 NEGATIVE POINT 3)
  FROM product_catalog
  USING dense
  WHERE category = 'electronics'
  PARAMS (hnsw_ef = 128)
  LIMIT 15;
```

Key decisions:

- `TARGET` is the anchor. `CONTEXT` pairs steer direction in vector space.
- Target accepts the same input shapes as nearest search. Point anchors are the stable form for catalogs.
- Use discover for exploration. Use recommend for liked-item centroids. Use context for pair-guided search without a separate anchor.

## 10. Relevance feedback

Problem: refine a text query with weighted judged examples.

```sql
QUERY RELEVANCE FEEDBACK TARGET TEXT 'query_text'
  FEEDBACK ((POINT 1, 0.9), (POINT 2, 0.1))
  STRATEGY NAIVE (a = 1.0, b = 0.75, c = 0.25)
  FROM docs USING dense LIMIT 10;
```

Key decisions:

- `TARGET` is query text. `FEEDBACK` pairs `(point_id, weight)` encode judgments.
- `STRATEGY NAIVE (a, b, c)` weights target versus positive versus negative feedback. Tune per corpus.
- This is server-side feedback, not client-side cross-encoder rerank.

## 11. Order by payload value

Problem: browse in payload order without vector scoring.

```sql
QUERY ORDER BY created_at DESC FROM articles
  WHERE status = 'published'
  LIMIT 20;

QUERY ORDER BY created_at DESC START FROM '2024-01-01T00:00:00Z' FROM docs LIMIT 20;
```

Key decisions:

- Uses the payload index scan engine, not HNSW.
- Direction defaults to `ASC`. Pass `DESC` explicitly for newest-first.
- `START FROM <value>` resumes from a payload value. Accepts integer, float, datetime string, or placeholder.
- `OFFSET` pages deterministically after ordering. Prefer `START FROM` for deep pagination.

## 12. Random sample

Problem: uniform random sample for inspection or seeding.

```sql
QUERY SAMPLE RANDOM FROM docs LIMIT 10;
```

Key decisions:

- Random on every call. For stable buckets use `WHERE SLICE (total, index)` instead. See `qql-filters.md`.
- No `USING` clause. No input vector. No scoring input to tune.

## 13. Formula rescoring

Problem: shape scores with payload signals without an external reranker.

```sql
WITH candidates AS (
  QUERY TEXT 'vector database performance' FROM articles USING dense LIMIT 100
)
QUERY FORMULA (score * 0.7 + popularity * 0.3)
  DEFAULTS (popularity = 0.0)
  FROM articles
  PREFETCH (candidates)
  LIMIT 20;
```

Key decisions:

- Formula runs over prefetch candidates. Always define candidates in a CTE or `PREFETCH`, then rescore.
- `score` is the incoming rank score. Payload fields reference document values. `DEFAULTS (...)` covers missing fields.
- Datetime decay uses ISO strings. `TARGET = "2024-01-01T00:00:00Z"` auto-infers `datetime_key`. Lowercase `datetime(...)` parses and formats to canonical uppercase `DATETIME(...)`.
- Common functions: `EXP_DECAY`, `GAUSS_DECAY`, `GEO_DISTANCE`, `SQRT`, `LOG`, `MAX`, `MIN`, `ACOSH`, `CASE WHEN ... THEN ... ELSE ... END`.
- Geo decay example: `score * GAUSS_DECAY(GEO_DISTANCE(48.8566, 2.3522, location), 0.0, 5000.0, 0.5)`.
- Recency example: `score * EXP_DECAY(published_at, 1735689600, 86400.0, 0.5)`.
- Conditional boost: `CASE WHEN priority = 'high' THEN score * 2.5 ELSE score END`.
- Large integers above `i64::MAX` parse as unsigned up to `u64::MAX`.

## MMR diversification

Problem: balance relevance against diversity. MMR modifies nearest search, not a separate query type.

```sql
QUERY MMR 'emergency triage' DIVERSITY 0.5 CANDIDATES 100
  FROM docs
  USING dense
  LIMIT 10;
```

Key decisions:

- `DIVERSITY` in `[0, 1]`. Higher values diversify more aggressively.
- `CANDIDATES` sets the pre-diversification pool size. Larger pools diversify better at higher latency cost.
- Sparse targets work. `USING ... AS SPARSE` with MMR is supported.

## Prefetch pipeline

Problem: multi-stage retrieval with per-leg control.

```sql
WITH
  broad AS (
    QUERY TEXT 'emergency neurological assessment' FROM clinical_docs
    USING dense WHERE department = 'emergency' LIMIT 500
  ),
  narrow AS (
    QUERY TEXT 'emergency neurological assessment' FROM clinical_docs
    USING sparse PREFETCH (broad) LIMIT 100
  )
QUERY FUSION RRF FROM clinical_docs PREFETCH (narrow) LIMIT 5;
```

Key decisions:

- CTEs can reference earlier CTEs inside `PREFETCH` for coarse-to-fine chains.
- `PREFETCH (name WHERE filter SCORE THRESHOLD n LOOKUP FROM coll VECTOR v SHARD key)` is the full leg shape. Each modifier is optional.
- Prefetch `SHARD` routes only that leg. Statement `SHARD` routes the whole request.
- Fusion needs a non-empty prefetch list. Formula and rerank need at least one candidate leg.

## Grouped hits

Problem: top results per group without one source dominating.

```sql
QUERY TEXT 'machine learning optimization' FROM research_papers
  USING dense
  WHERE year >= 2023
  GROUP BY 'author_id' SIZE 5 LOOKUP FROM author_metadata
  LIMIT 20;
```

Key decisions:

- `GROUP BY field SIZE n` partitions hits by payload value. `SIZE` caps hits per group.
- `LOOKUP FROM coll` resolves group metadata cross-collection. Remote-only. Plain `GROUP BY` works on edge 0.8 plus.
- `OFFSET` with `GROUP BY` maps to `group_offset` for grouped page two.
- Selectors shape the join: `LOOKUP FROM topics WITH PAYLOAD INCLUDE (title) WITH VECTOR (dense)`.
- SDK accessors: `report.groups()` returns `[{id, hits}]`.
