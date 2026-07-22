# QQL Canonical Query Examples

Golden examples for crafting complex QQL queries. Every example presents a real retrieval problem, explains why the approach works, lists key architectural decisions, and provides pure canonical QQL code blocks.

---

## 1. Multi-Stage Hybrid Retrieval with Per-Prefetch Tuning

**Problem:** You need semantic understanding and exact keyword matching for a technical documentation search engine. The dense semantic search must focus only on recent tech articles, while the sparse keyword search casts a wider net with a lower quality bar.

**Why this works:** Common Table Expressions (CTEs) define independent candidate retrieval streams with their own filters, limits, and score thresholds. The top-level `QUERY FUSION RRF` merges candidate streams using Reciprocal Rank Fusion.

```sql
WITH
  dense AS (
    QUERY TEXT 'vector database performance' FROM articles USING dense WHERE category = 'tech' AND published_at >= 1735689600 LIMIT 200
  ),
  sparse AS (
    QUERY TEXT 'vector database performance' FROM articles USING sparse LIMIT 300
  )
QUERY FUSION RRF FROM articles
  PREFETCH (
    dense SCORE THRESHOLD 0.6,
    sparse SCORE THRESHOLD 0.3
  )
  LIMIT 10;
```

**Key decisions:**
- `dense`: High-precision leg retrieving 200 candidates filtered to tech articles published after Jan 1, 2025.
- `sparse`: Wide-net keyword leg retrieving 300 candidates with a lower score threshold (0.3).
- `QUERY FUSION RRF`: Merges rankings seamlessly without requiring raw score normalization.

---

## 2. Tiered Retrieval with Nested CTEs

**Problem:** In a clinical RAG pipeline, you want a broad first pass to retrieve 500 semantically relevant emergency department documents, followed by a narrow second pass that performs keyword matching *only* within those 500 candidates.

**Why this works:** CTEs can reference preceding CTEs inside their `PREFETCH` clause, enabling multi-stage coarse-to-fine filtering directly inside Qdrant.

```sql
WITH
  broad AS (
    QUERY TEXT 'emergency neurological assessment' FROM clinical_docs USING dense WHERE department = 'emergency' LIMIT 500
  ),
  narrow AS (
    QUERY TEXT 'emergency neurological assessment' FROM clinical_docs USING sparse PREFETCH (broad) LIMIT 100
  )
QUERY FUSION RRF FROM clinical_docs
  PREFETCH (narrow)
  LIMIT 5;
```

**Key decisions:**
- Stage 1 (`broad`): Semantic search over the emergency department.
- Stage 2 (`narrow`): Keyword search restricted to `broad` candidates.
- Final Output: Fused top 5 results delivered with microsecond latency.

---

## 3. Hybrid Search with Per-Prefetch Filtering

**Problem:** You want hybrid retrieval, but results from a specific category or priority level should be retrieved via a dedicated high-priority prefetch stream.

**Why this works:** Instead of a single hybrid query, you split into multiple CTEs with different filters and score thresholds. RRF merges candidate streams into a single ranked list.

```sql
WITH
  high_priority AS (
    QUERY TEXT 'kubernetes deployment' FROM incidents USING dense WHERE priority = 'critical' AND status = 'open' LIMIT 50
  ),
  general AS (
    QUERY TEXT 'kubernetes deployment' FROM incidents USING dense LIMIT 200
  ),
  keyword AS (
    QUERY TEXT 'kubernetes deployment' FROM incidents USING sparse LIMIT 200
  )
QUERY FUSION RRF FROM incidents
  PREFETCH (
    high_priority SCORE THRESHOLD 0.7,
    general SCORE THRESHOLD 0.4,
    keyword SCORE THRESHOLD 0.3
  )
  LIMIT 10;
```

**Key decisions:**
- Three prefetch legs: critical incidents (dense), general incidents (dense), keyword (sparse).
- Each leg has its own score threshold to prune low-quality candidates before fusion.

---

## 4. Grouped Retrieval with Cross-Collection Lookup

**Problem:** You search in collection `research_papers`, but the group IDs (e.g. author names) live in a separate `author_metadata` collection. You want top-5 results per author without duplicate author dominance in the result feed.

**Why this works:** `GROUP BY` partitions hits by payload field, while `LOOKUP FROM` resolves grouping metadata cross-collection.

```sql
QUERY TEXT 'machine learning optimization' FROM research_papers
  USING dense
  WHERE year >= 2023
  GROUP BY 'author_id' SIZE 5 LOOKUP FROM author_metadata
  LIMIT 20;
```

**Key decisions:**
- `GROUP BY 'author_id' SIZE 5`: Ensures diversity by capping at 5 papers per author.
- `LOOKUP FROM author_metadata`: Pulls author collection attributes for each group header.

---

## 5. Paginated Browse with ORDER BY

**Problem:** A web dashboard needs to browse articles ordered by release timestamp with strict pagination, without performing vector search.

**Why this works:** `QUERY ORDER BY` uses Qdrant's payload index scan engine for efficient deterministic sorting.

```sql
-- Page 1: Top 20 published articles
QUERY ORDER BY created_at DESC FROM articles
  WHERE status = 'published' AND category = 'engineering'
  LIMIT 20;

-- Page 2: Next 20 articles
QUERY ORDER BY created_at DESC FROM articles
  WHERE status = 'published' AND category = 'engineering'
  LIMIT 20 OFFSET 20;
```

---

## 6. Selective Payload and Vector Projections

**Problem:** You need to retrieve high-dimensional Colbert multivectors for downstream re-ranking while excluding heavy raw text payloads from the network response.

**Why this works:** `WITH PAYLOAD` controls which fields are returned. `WITH VECTOR` controls which stored vectors come back.

```sql
QUERY TEXT 'acute bronchitis treatment protocols' FROM medical_records
  USING dense
  WHERE specialty = 'pulmonology' AND evidence_level IN ('A', 'B')
  WITH PAYLOAD INCLUDE (title, summary, evidence_level, url)
  WITH VECTOR (colbert_rerank)
  LIMIT 15;
```

---

## 7. Recommendation Search with Positive & Negative Point IDs

**Problem:** Recommend products to a user based on items they clicked (positive examples) and items they explicitly skipped or disliked (negative examples).

**Why this works:** `QUERY RECOMMEND` computes an average positive vector and subtracts negative vector directions in vector space.

```sql
QUERY RECOMMEND POSITIVE (101, 102, 103) NEGATIVE (201) STRATEGY average_vector
  FROM product_catalog
  USING product_dense
  WHERE availability = 'in_stock' AND price >= 10
  SCORE THRESHOLD 0.5
  LIMIT 20;
```

---

## 8. Full RAG Pipeline: Retrieve, Group, Limit

**Problem:** You're building a RAG pipeline. You want to retrieve relevant documents, group them by source (so you don't return 10 chunks from the same document), and limit per-group diversity.

```sql
WITH
  semantic AS (
    QUERY TEXT 'how does transformer attention mechanism work' FROM knowledge_base USING dense WHERE doc_type IN ('paper', 'textbook', 'blog') LIMIT 300
  ),
  keyword AS (
    QUERY TEXT 'transformer attention mechanism' FROM knowledge_base USING sparse LIMIT 200
  )
QUERY FUSION RRF FROM knowledge_base
  PREFETCH (
    semantic SCORE THRESHOLD 0.5,
    keyword SCORE THRESHOLD 0.3
  )
  GROUP BY 'source_id' SIZE 3
  LIMIT 20;
```

---

## 9. Multi-Collection Discovery (Target & Context Pairs)

**Problem:** You have a set of "context pairs" (positive/negative examples) and want to explore the vector space around them relative to a target anchor.

```sql
QUERY DISCOVER TARGET 'uuid-anchor-item'
  CONTEXT (
    POSITIVE 'uuid-positive-1' NEGATIVE 'uuid-negative-1',
    POSITIVE 'uuid-positive-2' NEGATIVE 'uuid-negative-2'
  )
  FROM product_catalog
  USING dense
  WHERE category = 'electronics' AND rating >= 4.0
  PARAMS (hnsw_ef = 128)
  LIMIT 15;
```

---

## 10. Complex Multi-Tenant Security Filter Chains

**Problem:** You need to combine multiple filter conditions with boolean logic, ranges, set membership, and nested document checks.

```sql
QUERY TEXT 'incident response playbook' FROM runbooks
  USING dense
  WHERE (
    (severity >= 3 AND status = 'open')
    OR (severity >= 5 AND status = 'acknowledged')
  )
  AND assigned_team IS NOT NULL
  AND tags MATCH ANY ('kubernetes', 'docker', 'container')
  AND created_at BETWEEN 1704067200 AND 1767139200
  AND NOT (category = 'deprecated')
  LIMIT 10;
```

---

## 11. Score Boosting with Formula Engine

**Problem:** Re-rank search results using payload signals (popularity, freshness) without an external reranker.

```sql
WITH candidates AS (
  QUERY TEXT 'vector database performance' FROM articles USING dense LIMIT 100
)
QUERY FORMULA (score * 0.7 + popularity * 0.3) DEFAULTS (popularity = 0.0)
  FROM articles
  PREFETCH (candidates)
  LIMIT 20;
```

---

## 12. Conditional Business Logic Scoring

**Problem:** Apply different scoring logic for different content tiers — premium content gets a 2.5x boost, low priority content is untouched.

```sql
WITH candidates AS (
  QUERY TEXT 'clinical protocols' FROM documentation USING dense LIMIT 100
)
QUERY FORMULA (CASE WHEN priority = 'high' THEN score * 2.5 ELSE score END)
  FROM documentation
  PREFETCH (candidates)
  LIMIT 15;
```

---

## 13. Geo-Distance Decay

**Problem:** Search for nearby emergency services, boosting closer providers with smooth Gaussian decay based on distance.

```sql
WITH candidates AS (
  QUERY TEXT 'emergency clinic' FROM restaurants USING dense LIMIT 100
)
QUERY FORMULA (score * GAUSS_DECAY(GEO_DISTANCE(48.8566, 2.3522, location), 0.0, 5000.0, 0.5)) DEFAULTS (location = {lat: 48.8566, lon: 2.3522})
  FROM restaurants
  PREFETCH (candidates)
  LIMIT 10;
```

---

## 14. Mathematical Score Shaping

**Problem:** Apply non-linear score transformations — logarithmic dampening for citation counts and square root for similarity scores.

```sql
WITH candidates AS (
  QUERY TEXT 'quantum computing' FROM papers USING dense LIMIT 100
)
QUERY FORMULA (SQRT(score) * LOG(citation_count + 1)) DEFAULTS (citation_count = 0)
  FROM papers
  PREFETCH (candidates)
  LIMIT 20;
```

---

## 15. Hybrid Search with Formula Boosting

**Problem:** Hybrid prefetch retrieval combined with conditional score boosting.

```sql
WITH
  dense AS (
    QUERY TEXT 'transformer attention mechanism' FROM papers USING dense WHERE year >= 2020 LIMIT 200
  ),
  sparse AS (
    QUERY TEXT 'transformer attention mechanism' FROM papers USING sparse LIMIT 200
  )
QUERY FUSION RRF FROM papers
  PREFETCH (dense SCORE THRESHOLD 0.5, sparse SCORE THRESHOLD 0.3)
  LIMIT 10;
```

---

## 16. Multi-Query Semicolon Batch Script

**Problem:** Execute multiple search statements in a single batch script separated by semicolons.

```sql
QUERY TEXT 'emergency triage' FROM docs USING dense LIMIT 5;
QUERY TEXT 'cardiac arrest protocol' FROM docs USING dense LIMIT 5;
QUERY TEXT 'neurological assessment' FROM docs USING dense LIMIT 5;
```

---

## 17. Full Setup, Indexing, and Ingestion Script

**Problem:** Create collection, payload indexes, upsert documents with auto-embedding, and perform semantic query in a single QQL script.

```sql
CREATE COLLECTION medical (dense VECTOR(384, COSINE));
CREATE INDEX ON COLLECTION medical FOR specialty TYPE keyword;
UPSERT INTO medical VALUES {id: 1, text: 'stroke protocol', specialty: 'neurology'}, {id: 2, text: 'cardiac arrest', specialty: 'cardiology'} USING DENSE MODEL 'all-minilm:l6-v2';
QUERY TEXT 'emergency' FROM medical USING dense LIMIT 5;
```

---

## 18. Time-Based Recency Decay

**Problem:** Prioritize recent news articles using exponential decay based on publication timestamp.

```sql
QUERY FORMULA score * EXP_DECAY(published_at, 1735689600, 86400.0, 0.5)
  FROM news
  USING dense
  LIMIT 20;
```

---

## 19. Geo-Distance Radius and Bounding Box Filtering

**Problem:** Filter and boost points based on geospatial bounding box and distance decay.

```sql
QUERY FORMULA score * GAUSS_DECAY(GEO_DISTANCE(48.8566, 2.3522, location), 0.0, 5000.0, 0.5)
  FROM places
  USING dense
  WHERE location GEO_BBOX {
    top_left: {lat: 48.8600, lon: 2.3400},
    bottom_right: {lat: 48.8500, lon: 2.3600}
  }
  LIMIT 10;
```

---

## 20. Maximal Marginal Relevance (MMR) Diversification

**Problem:** Balance similarity relevance against result diversity for dense queries.

```sql
QUERY MMR 'emergency triage' DIVERSITY 0.5 CANDIDATES 100
  FROM docs
  USING dense
  LIMIT 10;
```
