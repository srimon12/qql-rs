# QQL Query Examples

Golden examples for crafting complex QQL. Each example solves a real retrieval problem — study the structure, not just the syntax.

---

## 1. Multi-Stage Hybrid Retrieval with Per-Prefetch Tuning

**Problem:** You need semantic + keyword recall, but the dense leg should only search recent tech articles while the sparse leg should cast a wider net with a lower quality bar.

**Why this works:** CTEs let you define independent retrieval strategies with their own filters, limits, and score thresholds. The top-level `FUSION RRF` merges results using reciprocal rank fusion. Per-prefetch `WHERE` clauses push filters down to Qdrant — they are not post-filters.

```sql
WITH
  dense AS (
    QUERY 'vector database performance' USING dense LIMIT 200
    WHERE category = 'tech' AND published_at >= '2025-01-01'
  ),
  sparse AS (
    QUERY 'vector database performance' USING sparse LIMIT 300
  )
QUERY 'vector database performance' FROM articles LIMIT 10
  PREFETCH (
    dense SCORE THRESHOLD 0.6,
    sparse SCORE THRESHOLD 0.3
  )
  FUSION RRF
  WITH (rrf_k = 20, rrf_weights = [0.6, 0.4])
```

**Key decisions:**
- Dense prefetch is smaller (200) but filtered to recent tech — higher precision leg.
- Sparse prefetch is larger (300) with no filter — casts a wider keyword net.
- `rrf_weights = [0.6, 0.4]` favors the dense leg.
- `rrf_k = 20` (default is 60) makes the rank penalty steeper — top-ranked items dominate more.

---

## 2. Tiered Retrieval with Nested CTEs

**Problem:** You want a broad first pass, then a narrower second pass that only searches within the first pass results. This is a coarse-to-fine retrieval pattern common in RAG pipelines.

**Why this works:** CTEs can reference other CTEs in their `PREFETCH`. The inner CTE does a broad dense search. The outer CTE uses the inner results as a prefetch, effectively scoping its search to the inner CTE's candidates.

```sql
WITH
  broad AS (
    QUERY 'emergency neurological assessment' USING dense LIMIT 500
    WHERE department = 'emergency'
  ),
  narrow AS (
    QUERY 'emergency neurological assessment' USING sparse LIMIT 100
    PREFETCH (broad)
  )
QUERY 'emergency neurological assessment' FROM clinical_docs LIMIT 5
  PREFETCH (narrow)
  FUSION RRF
```

**Key decisions:**
- `broad` retrieves 500 dense candidates from the emergency department.
- `narrow` does a sparse search scoped to those 500 candidates — keyword precision within a semantic neighborhood.
- The top-level query fuses only the narrow results. This is a 2-stage pipeline: dense broad → sparse narrow → RRF.

---

## 3. Hybrid Search with Conditional Scoring via Per-Prefetch Filters

**Problem:** You want hybrid retrieval, but results from a specific category or priority level should be boosted by being in a separate, higher-weighted prefetch.

**Why this works:** Instead of a single hybrid query, you split into multiple CTEs with different filters and weights. The RRF weights control how much each leg contributes to the final ranking.

```sql
WITH
  high_priority AS (
    QUERY 'kubernetes deployment' USING dense LIMIT 50
    WHERE priority = 'critical' AND status = 'open'
  ),
  general AS (
    QUERY 'kubernetes deployment' USING dense LIMIT 200
  ),
  keyword AS (
    QUERY 'kubernetes deployment' USING sparse LIMIT 200
  )
QUERY 'kubernetes deployment' FROM incidents LIMIT 10
  PREFETCH (
    high_priority SCORE THRESHOLD 0.7,
    general SCORE THRESHOLD 0.4,
    keyword SCORE THRESHOLD 0.3
  )
  FUSION RRF
  WITH (rrf_k = 30, rrf_weights = [0.5, 0.3, 0.2])
```

**Key decisions:**
- Three prefetch legs: critical incidents (dense), general incidents (dense), keyword (sparse).
- `rrf_weights = [0.5, 0.3, 0.2]` — critical incidents get 50% of the RRF weight.
- This effectively boosts priority without a formula engine — the filter + weight combination achieves conditional scoring.
- Each leg has its own score threshold to prune low-quality candidates before fusion.

---

## 4. Grouped Retrieval with Cross-Collection Lookup

**Problem:** You search in collection `docs`, but the group IDs (e.g., author names) live in a separate `metadata` collection. You want top-5 results per author, but the author info comes from metadata.

**Why this works:** `WITH LOOKUP FROM` tells Qdrant to resolve group IDs from a different collection than the one being searched. This is useful when your search corpus and your grouping taxonomy are stored separately.

```sql
QUERY 'machine learning optimization' FROM research_papers LIMIT 20
  GROUP BY 'author_id'
  GROUP_SIZE 5
  WITH LOOKUP FROM author_metadata
  USING HYBRID
  WHERE year >= 2023
  WITH (rrf_k = 30, rrf_weights = [0.7, 0.3])
```

**Key decisions:**
- Search happens on `research_papers`, but `author_id` group resolution happens against `author_metadata`.
- `GROUP_SIZE 5` returns up to 5 papers per author.
- Combined with hybrid RRF — the search itself uses both dense and keyword signals.
- The `WHERE year >= 2023` filter applies to the search, not the lookup.

---

## 5. Paginated Browse with ORDER BY

**Problem:** You need a standard paginated list view (e.g., "show me the next page of articles") ordered by a payload field, not by similarity score.

**Why this works:** `QUERY ORDER BY` bypasses vector search entirely. It uses Qdrant's `OrderBy` query variant — a full scan sorted by a payload field. Combine with `OFFSET` for pagination.

```sql
-- Page 1
QUERY ORDER BY created_at DESC FROM articles
  WHERE status = 'published' AND category = 'engineering'
  LIMIT 20

-- Page 2 (offset by 20)
QUERY ORDER BY created_at DESC FROM articles
  WHERE status = 'published' AND category = 'engineering'
  LIMIT 20 OFFSET 20
```

**Key decisions:**
- No text query — this is a browse, not a search.
- `ORDER BY created_at DESC` — newest first.
- Create a payload index on `created_at` for this to be fast:
  ```sql
  CREATE INDEX ON COLLECTION articles FOR created_at TYPE integer
  ```

---

## 6. Retrieval with Payload and Vector Selection

**Problem:** You're searching a medical corpus with large payloads (full text, embeddings, metadata). You only need titles and scores for the UI, and you want the rerank vector back for downstream processing.

**Why this works:** `WITH PAYLOAD` controls which fields are returned. `WITH VECTORS` controls which stored vectors come back. This reduces network transfer and deserialization overhead — critical when payloads are large.

```sql
QUERY 'acute bronchitis treatment protocols' FROM medical_records
  USING HYBRID
  WHERE specialty = 'pulmonology' AND evidence_level IN ('A', 'B')
  LIMIT 15
  RERANK
  WITH PAYLOAD (include = ['title', 'summary', 'evidence_level', 'url'], exclude = ['raw_text', 'embedding'])
  WITH VECTORS ('colbert_rerank')
  WITH (hnsw_ef = 256)
```

**Key decisions:**
- `include` + `exclude` — include the lightweight fields, explicitly exclude the heavy ones.
- `WITH VECTORS ('colbert_rerank')` — returns the ColBERT multivector for downstream re-processing.
- `hnsw_ef = 256` — higher recall at query time since we're doing hybrid + rerank.
- `RERANK` applies ColBERT reranking after the hybrid retrieval.

---

## 7. Recommendation with Cross-Collection Lookup

**Problem:** You have example point IDs in a `user_interactions` collection, but you want to find similar items in a `product_catalog` collection.

**Why this works:** `LOOKUP FROM` tells Qdrant where to find the example vectors. `USING` specifies which vector space to use for similarity. This decouples the "example source" from the "search target."

```sql
QUERY RECOMMEND WITH (positive = ('user-click-1', 'user-click-2', 'user-click-3'))
  NEGATIVE IDS ('user-skip-1')
  FROM product_catalog
  LOOKUP FROM user_interactions VECTOR 'dense'
  USING 'product_dense'
  LIMIT 20
  SCORE THRESHOLD 0.5
  WHERE availability = 'in_stock' AND price >= 10
```

**Key decisions:**
- Positive examples come from `user_interactions` — Qdrant looks up their vectors there.
- Similarity is computed against `product_dense` vectors in `product_catalog`.
- `NEGATIVE IDS` excludes items the user explicitly skipped.
- `WHERE` filter applies to the result set in `product_catalog`.

---

## 8. Full RAG Pipeline: Retrieve, Group, Limit

**Problem:** You're building a RAG pipeline. You want to retrieve relevant documents, group them by source (so you don't return 10 chunks from the same document), and limit per-group diversity.

**Why this works:** `GROUP BY` + `GROUP_SIZE` ensures diversity in the result set. Combined with hybrid retrieval and per-prefetch tuning, this gives you a production-grade retrieval step.

```sql
WITH
  semantic AS (
    QUERY 'how does transformer attention mechanism work' USING dense LIMIT 300
    WHERE doc_type IN ('paper', 'textbook', 'blog')
  ),
  keyword AS (
    QUERY 'transformer attention mechanism' USING sparse LIMIT 200
  )
QUERY 'how does transformer attention mechanism work' FROM knowledge_base LIMIT 20
  PREFETCH (
    semantic SCORE THRESHOLD 0.5,
    keyword SCORE THRESHOLD 0.3
  )
  FUSION RRF
  WITH (rrf_k = 20, rrf_weights = [0.65, 0.35])
  GROUP BY 'source_id'
  GROUP_SIZE 3
```

**Key decisions:**
- Dense leg searches only papers, textbooks, and blogs — excludes noise.
- Sparse leg has no filter — catches exact terminology matches.
- `GROUP BY 'source_id'` with `GROUP_SIZE 3` — max 3 chunks per source document.
- `rrf_weights = [0.65, 0.35]` — semantic understanding matters more than keyword matching.
- This is the kind of query you'd store in a config file and tune over time.

---

## 9. Multi-Collection Discovery

**Problem:** You have a set of "context pairs" (positive/negative examples) and want to explore the vector space around them. This is useful for finding items that are similar to the positives but dissimilar to the negatives.

```sql
QUERY DISCOVER TARGET 'uuid-anchor-item'
  CONTEXT PAIRS (
    ('uuid-positive-1', 'uuid-negative-1'),
    ('uuid-positive-2', 'uuid-negative-2'),
    ('uuid-positive-3', 'uuid-negative-3')
  )
  FROM product_catalog
  LIMIT 15
  WHERE category = 'electronics' AND rating >= 4.0
  WITH (hnsw_ef = 128)
```

**Key decisions:**
- The target anchors the search direction.
- Context pairs teach the algorithm what "similar" and "different" mean in this context.
- Useful for exploration: "show me items like these but not like those."

---

## 10. Complex Filter Chains

**Problem:** You need to combine multiple filter conditions with boolean logic, ranges, and null checks.

```sql
QUERY 'incident response playbook' FROM runbooks LIMIT 10
  WHERE (
    (severity >= 3 AND status = 'open')
    OR (severity >= 5 AND status = 'acknowledged')
  )
  AND assigned_team IS NOT NULL
  AND tags MATCH ANY 'kubernetes' 'docker' 'container'
  AND created_at BETWEEN '2024-01-01' AND '2025-12-31'
  AND NOT (category = 'deprecated')
```

**Key decisions:**
- Nested `OR` inside `AND` — complex boolean logic.
- `IS NOT NULL` — exclude unassigned runbooks.
- `MATCH ANY` — at least one of the listed terms must appear in `tags`.
- `BETWEEN` — date range filter.
- `NOT (...)` — exclusion clause.
- Create indexes on `severity`, `status`, `assigned_team`, `tags`, `created_at`, and `category` for this to perform well.

---

## 11. Score Boosting with BOOST Formula

**Problem:** You want to re-rank search results using payload signals — boost by recency, popularity, or category relevance without an external reranker.

**Why this works:** The `BOOST` clause applies a mathematical expression to modify the similarity score. The `$score` variable holds the original score. Payload fields are accessible by name. The expression is evaluated by Qdrant's Formula engine — no client-side post-processing.

```sql
QUERY 'vector database performance' FROM articles LIMIT 20
  BOOST ($score + 0.3 * popularity + 0.1 * freshness)
  DEFAULTS (popularity = 0.0, freshness = 0.0)
```

**Key decisions:**
- `$score` preserves the original similarity signal.
- `popularity` and `freshness` are payload fields — their values are added to the score.
- `DEFAULTS` provides fallback values for missing fields (articles without these fields get 0.0).
- The weights (0.3, 0.1) control how much each factor matters — tune these based on your data.

---

## 12. Conditional Scoring with CASE WHEN

**Problem:** You want different scoring logic for different categories — premium content gets a 2x boost, deprecated content gets penalized.

**Why this works:** `CASE WHEN` in a BOOST formula uses the same filter expressions as `WHERE` clauses. Qdrant evaluates the condition per-point and applies the corresponding expression. The formula engine handles the branching internally.

```sql
QUERY 'kubernetes best practices' FROM documentation LIMIT 15
  BOOST (
    CASE WHEN category = 'premium' THEN $score * 2.0
    ELSE CASE WHEN status = 'deprecated' THEN $score * 0.5
    ELSE $score END END
  )
```

**Key decisions:**
- Nested `CASE WHEN` for multi-branch logic.
- `category = 'premium'` gets 2x score — premium content surfaces higher.
- `status = 'deprecated'` gets 0.5x — deprecated content sinks.
- Default is just `$score` — no modification for everything else.
- The filter expressions support full boolean logic: `AND`, `OR`, `NOT`, `IN`, `BETWEEN`, `MATCH`, etc.

---

## 13. Geo-Distance Decay

**Problem:** You're searching for nearby restaurants. Results closer to the user should score higher, with a smooth decay based on distance.

**Why this works:** `GAUSS_DECAY` applies a gaussian decay function to the distance. The score decreases smoothly as distance increases. `GEO_DISTANCE` computes the distance between a fixed point and a payload field containing geo coordinates.

```sql
QUERY 'italian restaurant' FROM restaurants LIMIT 10
  BOOST (
    $score * GAUSS_DECAY(
      GEO_DISTANCE(48.8566, 2.3522, location),
      0.0,
      5000.0,
      0.5
    )
  )
```

**Key decisions:**
- `GEO_DISTANCE(48.8566, 2.3522, location)` — computes distance from Paris coordinates to the `location` payload field.
- `GAUSS_DECAY(distance, target=0, scale=5000, midpoint=0.5)` — at 5km distance, the decay factor is 0.5.
- Multiplying `$score` by the decay factor preserves ranking relevance while penalizing distance.
- `target=0` means "closer is better" — the score peaks at distance 0.
- `scale=5000` is in meters — the unit depends on your geo data.

---

## 14. Mathematical Score Shaping

**Problem:** You want to apply non-linear score transformations — logarithmic dampening for high scores, square root for variance reduction, or power functions for amplification.

**Why this works:** The formula engine supports standard mathematical functions: `ABS`, `SQRT`, `LOG`, `LN`, `EXP`, `POW`. These transform the score in place, useful for normalizing skewed distributions or amplifying small differences.

```sql
QUERY 'machine learning' FROM papers LIMIT 20
  BOOST (SQRT($score) * LOG(citation_count + 1))
  DEFAULTS (citation_count = 0)
```

**Key decisions:**
- `SQRT($score)` — dampens high similarity scores, reducing the gap between top results.
- `LOG(citation_count + 1)` — logarithmic scaling of citations. The `+1` prevents `LOG(0)`.
- Multiplying combines both signals — similarity quality and paper influence.
- `DEFAULTS (citation_count = 0)` — papers without citation data get `LOG(1) = 0`, effectively no boost.

---

## 15. Hybrid Search with Score Boosting

**Problem:** You want hybrid retrieval (dense + sparse) with a formula that boosts results based on payload signals — combining the best of semantic search, keyword matching, and business logic.

**Why this works:** The BOOST formula applies after the hybrid retrieval pipeline. The formula receives the fused RRF score and can modify it using payload fields. This is a single-pass operation — no post-processing needed.

```sql
WITH
  dense AS (
    QUERY 'transformer attention mechanism' USING dense LIMIT 200
    WHERE year >= 2020
  ),
  sparse AS (
    QUERY 'transformer attention mechanism' USING sparse LIMIT 200
  )
QUERY 'transformer attention mechanism' FROM papers LIMIT 10
  PREFETCH (dense SCORE THRESHOLD 0.5, sparse SCORE THRESHOLD 0.3)
  FUSION RRF
  WITH (rrf_k = 20, rrf_weights = [0.6, 0.4])
  BOOST (
    $score + 0.2 * CASE WHEN venue IN ('NeurIPS', 'ICML', 'ICLR') THEN 1.0 ELSE 0.0 END
  )
```

**Key decisions:**
- Hybrid retrieval with CTEs — dense for semantics, sparse for keywords.
- `rrf_weights = [0.6, 0.4]` — semantic slightly preferred.
- `BOOST` adds a flat bonus for top venue papers.
- The `CASE WHEN venue IN (...)` checks if the paper is from a top venue — conditional boost.
- The formula applies to the fused score, not individual prefetch scores.

---

## 16. Batch Query — Single Round-Trip

**Problem:** You need to run multiple independent queries (e.g., for different search facets or A/B testing) but want to minimize network overhead.

**Why this works:** `BatchQuery` uses Qdrant's native `QueryBatch` API — all queries are sent in a single `QueryBatchPoints` call and return together. This is 3-5x faster than sequential execution for pure QUERY batches.

```go
results, _ := qql.BatchQuery(ctx, client, []string{
    "QUERY 'emergency triage' FROM docs LIMIT 5 USING HYBRID",
    "QUERY 'cardiac arrest protocol' FROM docs LIMIT 5 USING HYBRID",
    "QUERY 'neurological assessment' FROM docs LIMIT 5 USING HYBRID",
})
// All 3 queries executed in one round-trip
```

**Key decisions:**
- Use `BatchQuery` for pure QUERY batches — single round-trip to Qdrant.
- Use `ExecBatch` for mixed statements (INSERT + QUERY + CREATE) — sequential execution.
- All queries in a `BatchQuery` must target the same collection.

---

## 17. Batch Ingest + Query Pipeline

**Problem:** You need to set up a collection, insert data, and query it in a single pipeline.

**Why this works:** `ExecBatch` handles mixed statement types sequentially. Each statement is executed in order, and errors can be caught per-statement.

```go
results, _ := qql.ExecBatch(ctx, client, []string{
    "CREATE COLLECTION medical HYBRID WITH HNSW (m = 32) WITH QUANTIZATION (type = 'turbo', bits = 2)",
    "CREATE INDEX ON COLLECTION medical FOR specialty TYPE keyword",
    "INSERT INTO medical VALUES {'text': 'stroke protocol', 'specialty': 'neurology'} USING HYBRID",
    "INSERT INTO medical VALUES {'text': 'cardiac arrest', 'specialty': 'cardiology'} USING HYBRID",
    "QUERY 'emergency' FROM medical LIMIT 5 USING HYBRID",
}, true) // stopOnError = true
```

**Key decisions:**
- `stopOnError = true` — stops at first failure (useful for setup scripts).
- Each result has `ok`, `operation`, `message`, `data` fields.
- Use comma-separated VALUES for bulk insert within a single INSERT statement.

---

## 18. Time-Based Freshness Boosting

**Problem:** You want to prioritize recent articles over older ones, with exponential decay based on how old the content is.

**Why this works:** `datetime_key` tells Qdrant to parse the payload value as a datetime string. `datetime` parses a literal datetime string. The `exp_decay` function clamps the time difference into a 0-1 range, with newer items scoring closer to 1.

```sql
QUERY 'kubernetes deployment' FROM articles LIMIT 10
  BOOST (
    $score + exp_decay(
      datetime_key('published_at'),
      target=datetime('2026-06-17T00:00:00Z'),
      scale=86400,
      midpoint=0.5
    )
  )
```

**Key decisions:**
- `datetime_key('published_at')` — reads the `published_at` payload field as a datetime.
- `datetime('2026-06-17T00:00:00Z')` — the "now" reference point.
- `scale=86400` — 1 day in seconds. After 1 day, the decay factor is 0.5.
- `midpoint=0.5` — the decay reaches 0.5 at `target ± scale`.
- Kwargs (`target=`, `scale=`, `midpoint=`) make decay functions readable.

---

## 19. Geo-Distance Boosting with Dict Syntax

**Problem:** You want to boost results closer to a user's location, with smooth gaussian decay based on distance.

**Why this works:** `geo_distance` computes haversine distance between a point and a payload field. `gauss_decay` converts that distance into a 0-1 score factor. The dict syntax `{'lat': x, 'lon': y}` makes coordinates readable.

```sql
QUERY 'restaurant' FROM places LIMIT 10
  BOOST (
    $score * gauss_decay(
      geo_distance({'lat': 48.8566, 'lon': 2.3522}, location),
      scale=5000,
      midpoint=0.5
    )
  )
```

**Key decisions:**
- `geo_distance({'lat': 48.8566, 'lon': 2.3522}, location)` — Paris coordinates, `location` is the payload field.
- `gauss_decay(..., scale=5000)` — at 5km, the decay factor is 0.5.
- Multiplying `$score` by the decay preserves ranking while penalizing distance.
- Dict syntax `{'lat': x, 'lon': y}` is clearer than positional `geo_distance(lat, lon, field)`.

---

## 20. Combined: Hybrid + MMR + Time Decay + Conditional Boost

**Problem:** You want the full power of QQL — hybrid retrieval, MMR diversity, time-based freshness, and conditional scoring — in a single query.

**Why this works:** QQL composes all features into one declarative statement. The query planner handles the execution order automatically.

```sql
QUERY 'emergency triage' FROM docs LIMIT 10
  USING HYBRID
  WITH (mmr_diversity = 0.5, mmr_candidates = 100)
  BOOST (
    $score
    + exp_decay(datetime_key('updated_at'), target=datetime('2026-06-17T00:00:00Z'), scale=86400)
    + CASE WHEN priority = 'critical' THEN 0.5 ELSE 0 END
  )
```

**Key decisions:**
- `USING HYBRID` — dense + sparse retrieval.
- `WITH (mmr_diversity = 0.5)` — diverse results before boosting.
- `exp_decay(...)` — fresh content gets a score boost.
- `CASE WHEN priority = 'critical'` — critical items get a flat bonus.
- All three signals (similarity, freshness, priority) are combined in one pass.
