# QQL Canonical Query Examples

Golden examples for crafting complex QQL queries. Every example presents a real retrieval problem, explains why the approach works, lists key architectural decisions, and provides pure canonical QQL code blocks.

All examples are valid against the current QQL parser (function names are case-insensitive).

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
- `dense`: High-precision leg retrieving 200 candidates filtered to tech articles.
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

---

## 3. Hybrid Search with Per-Prefetch Filtering

**Problem:** You want hybrid retrieval, but results from a specific category or priority level should be retrieved via a dedicated high-priority prefetch stream.

**Why this works:** Instead of a single hybrid query, you split into multiple CTEs with different filters and score thresholds. RRF merges candidate streams into a single ranked list.

For the simple dense+sparse case without per-leg filters, prefer the hybrid
shorthand (same plan expand as `QUERY HYBRID`):

```sql
QUERY TEXT 'kubernetes deployment' FROM incidents
  USING HYBRID DENSE dense SPARSE sparse FUSION RRF
  LIMIT 10;

-- Defaults: schema must resolve unique dense + sparse; FUSION defaults to RRF
QUERY 'kubernetes deployment' FROM incidents USING HYBRID LIMIT 10;

-- Equivalent front-form
QUERY HYBRID TEXT 'kubernetes deployment' DENSE dense SPARSE sparse FUSION RRF
  FROM incidents LIMIT 10;
```

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

---

## 4. Grouped Retrieval with Cross-Collection Lookup

**Problem:** You search in collection `research_papers`, but the group IDs (e.g. author names) live in a separate `author_metadata` collection. You want top-5 results per author without duplicate author dominance in the result feed.

**Why this works:** `GROUP BY` partitions hits by payload field, while `LOOKUP FROM` resolves grouping metadata cross-collection. `OFFSET` is supported with `GROUP BY` (maps to `group_offset`). Edge rejects `GROUP BY` entirely — use remote Qdrant for grouped search.

```sql
QUERY TEXT 'machine learning optimization' FROM research_papers
  USING dense
  WHERE year >= 2023
  GROUP BY 'author_id' SIZE 5 LOOKUP FROM author_metadata
  LIMIT 20;
```

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
  WITH VECTOR (colbert)
  LIMIT 15;
```

---

## 6b. ColBERT Multivector Nearest and Rerank

**Problem:** Late-interaction retrieval: store token-level multivectors and either search with them directly or rerank dense candidates.

**Why this works:** Multivector is a **dense role with multi shape**, not a third sparse/dense kind. Schema marks `colbert` via `WITH MULTIVECTOR`; runtime sets `multi` before embedding so TEXT becomes `MultiDense` (`[[f32,…],…]`). Offline without schema, use `AS MULTI`.

**Dimension note:** The vector dimension of a multivector column must match the
selected model's token dimension. BGE-M3 outputs 1024-dimensional tokens; the
`edge` `multi_model: "bge-m3"` path produces 1024-d bags. Smaller
late-interaction models such as `answerai-colbert-small-v1` output 128-d tokens.
Set `VECTOR(dim, COSINE)` accordingly.

```sql
-- BGE-M3 (1024-d tokens, compatible with edge multi_model: "bge-m3"):
CREATE COLLECTION docs_bge (
  dense VECTOR(384, COSINE),
  sparse SPARSE,
  colbert VECTOR(1024, COSINE) WITH MULTIVECTOR (comparator = 'max_sim')
);

-- Smaller ColBERT model (128-d tokens):
CREATE COLLECTION docs (
  dense VECTOR(384, COSINE),
  sparse SPARSE,
  colbert VECTOR(128, COSINE) WITH MULTIVECTOR (comparator = 'max_sim')
);

-- Schema-driven: USING colbert alone is enough at execute time
QUERY TEXT 'vector database latency'
FROM docs
USING colbert
LIMIT 10;

-- Explicit offline / no schema
QUERY TEXT 'vector database latency'
FROM docs
USING colbert AS MULTI
LIMIT 10;

-- Precomputed multi-dense query vector
QUERY NEAREST VECTOR [[0.1, 0.2], [0.3, 0.4], [0.5, 0.6]]
FROM docs
USING colbert
LIMIT 10;

-- Dense first stage + ColBERT late-interaction rerank
WITH candidates AS (
  QUERY TEXT 'vector database latency' FROM docs USING dense LIMIT 100
)
QUERY RERANK TEXT 'vector database latency' MODEL 'answerai-colbert-small-v1'
FROM docs
USING colbert
PREFETCH (candidates)
LIMIT 10;

-- Precomputed multivector on upsert
UPSERT INTO docs VALUES {
  id: 1,
  text: 'chunk text',
  vector: { dense: [0.1, 0.2, 0.3], colbert: [[0.1, 0.2], [0.3, 0.4]] }
};
```

**Key decisions:**
- Kind is dense; shape is multi (`MultiDense`).
- Token dimension depends on model: BGE-M3 → 1024-d, smaller ColBERT models → 128-d. Match `VECTOR(dim, ...)` to the actual model.
- Host embedder must implement `embed_multi` for TEXT → multivector; otherwise pass `VECTOR [[...]]` or precomputed upsert vectors.
- `USING sparse` in a CTE never silently dense-embeds when schema marks sparse.

### 6c. Cross-encoder pair rerank (`CROSS RERANK`)

**Problem:** Reorder dense candidates with a pair scorer `(query, doc_text)` — not MaxSim multivector.

**Why this works:** `CROSS RERANK` runs PREFETCH, reads document text from `ON FIELD` (default `text`), scores pairs client-side. Distinct from late-interaction `RERANK … USING colbert`. Host needs `rerank_pairs` (edge `reranker_model` or HTTP `rerank_endpoint`).

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

**Problem:** Apply different scoring logic for different content tiers -- premium content gets a 2.5x boost, low priority content is untouched.

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

**Problem:** Apply non-linear score transformations -- logarithmic dampening for citation counts and square root for similarity scores.

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

## 17. Full Setup, Indexing, Ingestion, and Cleanup Script

**Problem:** Create collection, payload indexes, upsert documents with auto-embedding, count points, clear payload, delete vectors, and drop everything in a single QQL script.

```sql
CREATE COLLECTION medical (
  dense VECTOR(384, COSINE),
  colbert VECTOR(128, COSINE) WITH MULTIVECTOR (comparator = 'max_sim')
);
CREATE INDEX ON COLLECTION medical FOR specialty TYPE keyword;
UPSERT INTO medical VALUES {id: 1, text: 'stroke protocol', specialty: 'neurology'}, {id: 2, text: 'cardiac arrest', specialty: 'cardiology'} USING DENSE MODEL 'all-minilm:l6-v2';
COUNT FROM medical WHERE specialty = 'neurology';
QUERY TEXT 'emergency' FROM medical USING dense LIMIT 5;
CLEAR PAYLOAD FROM medical WHERE id = 1;
DELETE VECTOR colbert FROM medical WHERE id = 2;
DROP INDEX ON COLLECTION medical FOR specialty;
DROP COLLECTION medical;
```

---

## 18. Shard Key Lifecycle

**Problem:** Create custom shard keys for a multi-tenant collection, list them, then drop one.

```sql
CREATE COLLECTION tenants HYBRID (dense VECTOR(768, COSINE), sparse SPARSE)
WITH PARAMS (shard_number = 8, sharding_method = 'custom');

CREATE SHARD KEY 'acme' ON COLLECTION tenants WITH (shards_number = 2);
CREATE SHARD KEY 'globex' ON COLLECTION tenants WITH (shards_number = 2);

-- List all shard keys
SHOW SHARD KEYS ON COLLECTION tenants;

-- Remove a shard key
DROP SHARD KEY 'acme' ON COLLECTION tenants;

SHOW SHARD KEYS ON COLLECTION tenants;
DROP COLLECTION tenants;
```

---

## 19. Time-Based Recency Decay

**Problem:** Prioritize recent news articles using exponential decay based on publication timestamp.

```sql
QUERY FORMULA score * EXP_DECAY(published_at, 1735689600, 86400.0, 0.5)
  FROM news
  USING dense
  LIMIT 20;
```

---

## 20. Geo-Distance Radius and Bounding Box Filtering

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

## 21. Maximal Marginal Relevance (MMR) Diversification

**Problem:** Balance similarity relevance against result diversity. MMR supports both dense and sparse targets.

```sql
QUERY MMR 'emergency triage' DIVERSITY 0.5 CANDIDATES 100
  FROM docs
  USING dense
  LIMIT 10;
```

---

## 22. Geo-Radius Filtering

**Problem:** Find points within a radius of a geographic center point.

```sql
QUERY TEXT 'coffee shop' FROM places
  USING dense
  WHERE location GEO_RADIUS { center: {lat: 48.8566, lon: 2.3522}, radius: 5000 }
  LIMIT 10;
```

---

## 23. Quantization-Aware Search

**Problem:** Control quantization behavior per query for faster search with optional rescore.

```sql
QUERY TEXT 'quantum computing' FROM papers
  USING dense
  PARAMS (quantization = {ignore: false, rescore: true, oversampling: 2.0})
  LIMIT 20;
```

---

## 24. ACORN Search Params (remote Qdrant)

**Problem:** Filtered HNSW search should adapt to filter selectivity via ACORN.

**Limit:** Supported on remote Qdrant REST/gRPC. **Not** supported on edge.
`max_selectivity` requires `acorn = true` and is in `(0, 1]`.

```sql
QUERY TEXT 'filtered product search' FROM products
  USING dense
  WHERE category = 'electronics' AND in_stock = true
  PARAMS (hnsw_ef = 128, acorn = true, max_selectivity = 0.4)
  LIMIT 20;
```

---

## 25. Request timeout and read consistency

**Problem:** Cap how long a query may run and control replica read consistency
on a clustered Qdrant deployment.

**Why this works:** OpenAPI puts `timeout` and `consistency` on the **request**
(query string for REST; fields on `QueryPoints` for gRPC), not inside body
`SearchParams`. QQL accepts them in `PARAMS` and lowers accordingly.

```sql
QUERY TEXT 'incident response' FROM runbooks
  USING dense
  PARAMS (timeout = 30, consistency = majority, hnsw_ef = 64)
  LIMIT 10;

-- Factor form: require agreement from N replicas
QUERY TEXT 'incident response' FROM runbooks
  USING dense
  PARAMS (consistency = 2)
  LIMIT 10;
```

**Limit:** Single-node edge does not use cluster consistency; timeout is a
server-side override on remote Qdrant (client HTTP timeout is separate).

---

## 26. Cluster resource quotas (`SHOW QUOTAS` / `SET QUOTA`)

**Problem:** You operate a shared Qdrant cluster and need to inspect global
resource limits, then cap resident memory (and optionally disk) so a noisy
workload cannot starve the node.

**Why this works:** Qdrant 1.19 exposes a cluster-wide quota API at
`GET|PUT /quotas`. QQL maps `SHOW QUOTAS` / `SET QUOTA (…)` directly. `WAIT true`
is a query param (consensus wait), not a body field.

```sql
-- Inspect current config + utilization
SHOW QUOTAS;

-- Full replace of quota config (PUT replaces the whole object)
SET QUOTA (
  enabled = true,
  max_resident_memory_percent = 80,
  max_disk_usage_percent = 90,
  release_margin_percent = 5
) WAIT true;

-- Disable quotas (still a full replace — omitted keys are unset)
SET QUOTA (enabled = false);

-- Clear a single limit with null in a replace body
SET QUOTA (max_disk_usage_percent = null);
```

**Key decisions:**
- `SET QUOTA` is a **full replace**, not a merge of previous limits.
- Percent fields must be valid ranges (`QQL-PLAN-QUOTA` on out-of-range / unknown keys).
- **REST only** — gRPC returns `QQL-GRPC-QUOTA`; edge returns `QQL-EDGE-UNSUPPORTED-QUOTA`.
- Prefer REST client for quota admin; DML can stay on gRPC.

---

## 27. Memory placement tiers + TurboQuant (`turbo4`)

**Problem:** You want cold/cached/pinned placement for vectors, HNSW, payload, and
quantization, plus 4-bit TurboQuant dense storage for a large document corpus.

**Why this works:** Qdrant 1.19 adds `memory = 'cold'|'cached'|'pinned'` on vector,
HNSW, sparse, quantization, and indexes, plus `payload_memory` in collection
`PARAMS` (payload rejects `pinned`). Dense `datatype = 'turbo4'` enables TurboQuant.

```sql
CREATE COLLECTION docs (
  dense VECTOR(384, COSINE) WITH VECTOR (memory = 'cached', datatype = 'turbo4')
    WITH HNSW (memory = 'cold')
) WITH PARAMS (payload_memory = 'cold')
  WITH QUANTIZATION (type = 'scalar', memory = 'cached');

-- Sparse index can also take memory placement
CREATE COLLECTION docs_sparse (
  sparse SPARSE WITH SPARSE (modifier = 'idf', memory = 'cached')
);

-- Quantization-only placement (pinned allowed on quantization / vectors / HNSW)
CREATE COLLECTION docs_q (
  dense VECTOR(128, DOT) WITH QUANTIZATION (type = 'scalar', memory = 'pinned')
);
```

**Key decisions:**
- Prefer `memory` / `payload_memory` for new scripts. Legacy `on_disk` /
  `on_disk_payload` / `always_ram` still dual-write through Qdrant 1.19; upstream
  plans removal around 1.21.
- `payload_memory` cannot be `'pinned'`.
- `datatype = 'turbo4'` is a dense storage datatype (TurboQuant 4-bit), not a distance metric.

---

## 28. Keyword prefix index + `MATCH PREFIX`

**Problem:** Autocomplete / prefix filters on company names (`Comp…`) must hit an
indexed keyword field without full-text tokenization.

**Why this works:** Keyword indexes support `prefix = true` (and optional
`memory`). Filters use `field MATCH PREFIX '…'` (Qdrant match-prefix condition).

```sql
CREATE INDEX ON COLLECTION docs FOR title
  TYPE keyword WITH (prefix = true, memory = 'cached');

QUERY TEXT 'x' FROM docs
  WHERE title MATCH PREFIX 'Comp'
  LIMIT 10;
```

**Key decisions:**
- `MATCH PREFIX` is for **keyword** (or similar exact) fields with a prefix-capable index — not a substitute for `MATCH PHRASE` on text indexes.
- Combine with tenant filters as usual: `WHERE title MATCH PREFIX 'Comp' AND tenant_id = 'acme'`.

---

## 29. Deterministic `SLICE` sampling filter

**Problem:** You need a stable, hash-based subset of points (e.g. 1 of 4 shards of
the ID space) for canary ranking, A/B sampling, or tenant-agnostic load tests —
without random `SAMPLE` each time.

**Why this works:** `WHERE SLICE (total, index)` is a Qdrant filter condition:
points are partitioned into `total` buckets (`total >= 1`); only bucket `index`
(`0 <= index < total`) matches. Validation fails closed (`QQL-VALIDATION-SLICE`).

```sql
-- 25% of the collection (bucket 1 of 4)
QUERY TEXT 'x' FROM docs
  WHERE SLICE (4, 1)
  LIMIT 100;

-- Combine with payload predicates
QUERY TEXT 'x' FROM docs
  WHERE SLICE (4, 1) AND status = 'active'
  LIMIT 100;
```

**Key decisions:**
- Deterministic across queries for the same point set (unlike `QUERY SAMPLE RANDOM`).
- Multi-tenant: combine with `tenant_id` / `inject_filter` when sampling inside one tenant, or use alone for cluster-wide bucket experiments.
- `total` / `index` are integers; invalid pairs are rejected at validation.

---

## 30. Per-query sparse IDF corpus

**Problem:** Sparse / BM25-style retrieval should use either global IDF stats or a
tenant-scoped corpus so term rarity reflects only that tenant’s documents.

**Why this works:** Search `PARAMS (idf = …)` lowers to Qdrant’s per-query IDF
options: `'global'` or `{corpus: <Filter>}`. Malformed corpus objects fail with
`QQL-PLAN-IDF` (not a panic). Supported on remote Qdrant and **qdrant-edge 0.8+**.

```sql
-- Cluster / collection-global IDF
QUERY TEXT 'hello' FROM docs USING sparse
  PARAMS (idf = 'global')
  LIMIT 5;

-- Tenant-scoped IDF corpus (OpenAPI-style filter object)
QUERY TEXT 'hello' FROM docs USING sparse
  PARAMS (idf = {corpus: {must: [{key: 'tenant', match: {value: 'acme'}}]}})
  LIMIT 5;
```

**Key decisions:**
- Collection sparse vectors often use `modifier = 'idf'` at create time; query
  `PARAMS (idf = …)` overrides / scopes the corpus for **this** request.
- Corpus filter shape matches Qdrant Filter JSON (`must` / `should` / …), not QQL
  `WHERE` syntax inside the object.
- Pair with `WHERE tenant = 'acme'` (and `SHARD 'acme'` when custom-sharded) so
  both **retrieval isolation** and **IDF stats** stay tenant-local.
