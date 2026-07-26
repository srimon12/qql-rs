export type PresetId =
  | "berlin_hybrid"
  | "berlin_cte_formula"
  | "berlin_radius"
  | "berlin_bbox"
  | "berlin_polygon"
  | "berlin_grouped"
  | "hybrid"
  | "multitenant"
  | "cte"
  | "formula"
  | "grouped"
  | "mmr"
  | "ddl"
  | "discover"
  | "mutation"
  | "exact_lookup"
  | "recommend"
  | "discovery"
  | "order_by"
  | "random_sample"
  | "advanced_filter"
  | "rerank"
  | "collection_ddl"

export type PresetCategory =
  | "vector-search"
  | "geo-spatial"
  | "aggregation"
  | "multi-stage"
  | "fusion"
  | "tenant-isolation"
  | "point-lifecycle"
  | "schema-ddl"
  | "advanced-filters"
  | "discovery"

export type PresetComplexity = "beginner" | "intermediate" | "advanced"

export type ExecutionImpact = {
  reads: boolean
  writes: boolean
  schema: boolean
}

export type Preset = {
  id: PresetId
  label: string
  labelBadge?: string
  description: string
  teaching: string
  query: string
  category: PresetCategory
  complexity: PresetComplexity
  dataset: string
  tags: string[]
  impact: ExecutionImpact
  featured: boolean
}

export type CategoryDef = {
  id: PresetCategory
  label: string
  description: string
}

export const PRESET_CATEGORIES: CategoryDef[] = [
  { id: "vector-search", label: "Vector Search", description: "Semantic and keyword retrieval" },
  { id: "geo-spatial", label: "Geo-Spatial", description: "Location-based filtering" },
  { id: "aggregation", label: "Aggregation", description: "Grouping and bucketing" },
  { id: "multi-stage", label: "Multi-Stage", description: "CTE pipelines and prefetch DAGs" },
  { id: "fusion", label: "Fusion", description: "Score fusion algorithms" },
  { id: "tenant-isolation", label: "Tenant Isolation", description: "Multi-tenant sharding patterns" },
  { id: "point-lifecycle", label: "Point Lifecycle", description: "Create, update, delete operations" },
  { id: "schema-ddl", label: "Schema & DDL", description: "Collection definitions and schema management" },
  { id: "advanced-filters", label: "Advanced Filters", description: "Nested, compound, and specialized conditions" },
  { id: "discovery", label: "Discovery", description: "Recommend, context, and exploratory search" },
]

const SHARED_SEC10K_TAGS = ["sec10k", "SEC filings"] as const
const SHARED_BERLIN_TAGS = ["berlin_airbnb", "Airbnb"] as const
const SHARED_GEO_TAGS = [...SHARED_BERLIN_TAGS, "geo-filter"] as const

export const PRESETS: Preset[] = [
  {
    id: "berlin_hybrid",
    label: "Berlin Hybrid Dense + Sparse RRF",
    labelBadge: "HYBRID",
    description: "Fuse dense neural vectors and BM25 sparse keywords using RRF within a geo-radius constraint",
    teaching: "Demonstrates QQL QUERY HYBRID syntax: fuses dense neural retrieval and sparse BM25 keyword matching using Reciprocal Rank Fusion (RRF) alongside GEO_RADIUS location constraints.",
    query: `-- Berlin Airbnb — Hybrid Dense + Sparse BM25 RRF Fusion
-- Combines 384-d dense neural vectors & BM25 sparse keyword vectors
QUERY HYBRID TEXT 'cozy studio near historic landmarks and parks'
  DENSE dense SPARSE sparse
  FUSION RRF
  FROM berlin_airbnb
  WHERE location GEO_RADIUS {center: {lat: 52.5163, lon: 13.3777}, radius: 1500.0}
    AND price <= 120.0
  WITH PAYLOAD true
  LIMIT 5;`,
    category: "fusion",
    complexity: "intermediate",
    dataset: "berlin_airbnb",
    tags: [...SHARED_BERLIN_TAGS, "hybrid", "RRF", "fusion", "geo-filter"],
    impact: { reads: true, writes: false, schema: false },
    featured: true,
  },
  {
    id: "berlin_cte_formula",
    label: "Berlin CTE Multi-Stage Superhost Boost",
    labelBadge: "CTE",
    description: "CTE pipeline reranks candidate vector matches with CASE business logic",
    teaching: "Demonstrates QQL CTE pipeline & QUERY FORMULA: Stage 1 retrieves candidate dense matches via CTE; Stage 2 applies a CASE WHEN expression to multiply Superhost scores by 1.5x.",
    query: `-- Berlin Airbnb — Multi-Stage CTE Pipeline & Conditional Business Logic
-- Stage 1: Dense CTE retrieves 50 candidate vector matches
-- Stage 2: FORMULA engine applies conditional 1.5x score boost for verified Superhosts
WITH
  dense_candidates AS (
    QUERY TEXT 'spacious loft with balcony and fast wifi'
    FROM berlin_airbnb
    USING dense
    WHERE price <= 150.0
    LIMIT 50
  )
QUERY FORMULA (CASE WHEN superhost = true THEN score * 1.5 ELSE score END)
  FROM berlin_airbnb
  PREFETCH (dense_candidates)
  WITH PAYLOAD true
  LIMIT 5;`,
    category: "multi-stage",
    complexity: "advanced",
    dataset: "berlin_airbnb",
    tags: [...SHARED_BERLIN_TAGS, "CTE", "formula", "CASE", "business logic"],
    impact: { reads: true, writes: false, schema: false },
    featured: false,
  },
  {
    id: "berlin_radius",
    label: "Berlin Geo Radius Brandenburg Gate",
    labelBadge: "GEO",
    description: "Search 1.5 km radius around Brandenburg Gate combining vectors, location, and price",
    teaching: "Demonstrates QQL GEO_RADIUS payload filter: queries listings within a center point (lat/lon) radius while combining semantic vector search and price constraints.",
    query: `-- Berlin Airbnb — Geo Radius 1.5km around Brandenburg Gate
-- Combines semantic vector query with GEO_RADIUS payload filter & price cutoff
QUERY TEXT 'cozy studio near historic landmarks and parks'
  FROM berlin_airbnb
  USING dense
  WHERE location GEO_RADIUS {center: {lat: 52.5163, lon: 13.3777}, radius: 1500.0}
    AND price <= 100.0
  WITH PAYLOAD true
  LIMIT 5;`,
    category: "geo-spatial",
    complexity: "beginner",
    dataset: "berlin_airbnb",
    tags: [...SHARED_GEO_TAGS, "GEO_RADIUS", "price filter"],
    impact: { reads: true, writes: false, schema: false },
    featured: true,
  },
  {
    id: "berlin_bbox",
    label: "Berlin Geo BBox Mitte City Center",
    labelBadge: "GEO",
    description: "Bounding box search across Mitte with room-type filter",
    teaching: "Demonstrates QQL GEO_BBOX payload filter: queries listings within a rectangular bounding box defined by top_left and bottom_right lat/lon coordinates.",
    query: `-- Berlin Airbnb — Geo Bounding Box over Mitte City Center
-- Combines semantic search with GEO_BBOX coordinates & room_type filter
QUERY TEXT 'spacious loft with balcony and fast wifi'
  FROM berlin_airbnb
  USING dense
  WHERE location GEO_BBOX {top_left: {lat: 52.545, lon: 13.350}, bottom_right: {lat: 52.500, lon: 13.430}}
    AND room_type = 'Entire home apt'
  WITH PAYLOAD true
  LIMIT 5;`,
    category: "geo-spatial",
    complexity: "beginner",
    dataset: "berlin_airbnb",
    tags: [...SHARED_GEO_TAGS, "GEO_BBOX"],
    impact: { reads: true, writes: false, schema: false },
    featured: false,
  },
  {
    id: "berlin_polygon",
    label: "Berlin Geo Polygon Kreuzberg",
    labelBadge: "GEO",
    description: "Custom polygon boundary around Kreuzberg nightlife district with rating threshold",
    teaching: "Demonstrates QQL GEO_POLYGON filter: queries listings inside a custom multi-point polygon boundary with rating thresholds.",
    query: `-- Berlin Airbnb — Geo Polygon Boundary (Kreuzberg Nightlife District)
-- Arbitrary multi-point polygon boundary ring with rating >= 4.7 filter
QUERY TEXT 'artistic flat nightlife and coffee shops'
  FROM berlin_airbnb
  USING dense
  WHERE location GEO_POLYGON {exterior: [{lat: 52.500, lon: 13.370}, {lat: 52.515, lon: 13.430}, {lat: 52.485, lon: 13.450}, {lat: 52.470, lon: 13.390}, {lat: 52.500, lon: 13.370}]}
    AND rating >= 4.7
  WITH PAYLOAD true
  LIMIT 5;`,
    category: "geo-spatial",
    complexity: "intermediate",
    dataset: "berlin_airbnb",
    tags: [...SHARED_GEO_TAGS, "GEO_POLYGON", "rating"],
    impact: { reads: true, writes: false, schema: false },
    featured: false,
  },
  {
    id: "berlin_grouped",
    label: "Berlin Grouped by Neighborhood",
    labelBadge: "GROUP",
    description: "Top 3 listings per neighborhood using GROUP BY aggregation",
    teaching: "Demonstrates QQL GROUP BY aggregation: partitions search results into distinct neighborhood buckets returning top N hits per neighborhood.",
    query: `-- Berlin Airbnb — Grouped Aggregation by Neighborhood
-- Vector search grouped by neighborhood returning top 3 hits per neighborhood
QUERY TEXT 'quiet courtyard apartment'
  FROM berlin_airbnb
  USING dense
  WHERE price <= 100.0
  GROUP BY neighbourhood SIZE 3
  LIMIT 15;`,
    category: "aggregation",
    complexity: "intermediate",
    dataset: "berlin_airbnb",
    tags: [...SHARED_BERLIN_TAGS, "GROUP BY", "bucketing", "aggregation"],
    impact: { reads: true, writes: false, schema: false },
    featured: true,
  },
  {
    id: "hybrid",
    label: "Hybrid RRF Dense + Sparse Fusion",
    description: "Dense + sparse fusion with shard routing on SEC 10-K filings",
    teaching: "Combines dense semantic vectors and BM25-style sparse vectors into a single Reciprocal Rank Fusion (RRF) query with custom shard targeting.",
    query: `-- Hybrid Dense+Sparse RRF — RTX missile defense contracts
-- Embeds text → queries both dense & sparse vectors → fuses with RRF
QUERY HYBRID TEXT 'Raytheon missile defense contracts programs'
  DENSE dense
  SPARSE sparse
  FUSION RRF
  FROM sec10k
  WHERE fiscal_year >= 2024
  SHARD 'rtx'
  WITH PAYLOAD true
  LIMIT 5;`,
    category: "fusion",
    complexity: "intermediate",
    dataset: "sec10k",
    tags: [...SHARED_SEC10K_TAGS, "hybrid", "RRF", "shard"],
    impact: { reads: true, writes: false, schema: false },
    featured: true,
  },
  {
    id: "multitenant",
    label: "Multi-Tenant Isolation Defense in Depth",
    description: "Physical shard routing plus logical tenant_id filter for SEC 10-K",
    teaching: "Demonstrates defense-in-depth isolation: physical custom shard routing (SHARD 'honeywell') paired with logical payload filtering (tenant_id = 'honeywell').",
    query: `-- Multi-tenant isolation — SEC 10-K SaaS pattern (honeywell)
--
-- Three layers (see skills/qql-skill/references/qql-multitenancy.md):
--   1. SHARD 'honeywell'           physical — only that custom shard is hit
--   2. WHERE tenant_id = 'honeywell'  logical — payload filter (is_tenant index)
--   3. inject_filter() in host SDKs   programmatic — filter always present
--
-- Both SHARD + tenant_id together: hard isolation + no cross-tenant leaks.

-- Tenant-scoped hybrid search (dense + sparse RRF)
QUERY HYBRID TEXT 'supply chain disruption risk shortages'
  DENSE dense
  SPARSE sparse
  FUSION RRF
  FROM sec10k
  WHERE tenant_id = 'honeywell' AND fiscal_year >= 2024
  SHARD 'honeywell'
  WITH PAYLOAD true
  LIMIT 5;

-- Audit trail: point count for this tenant only (same dual isolation)
COUNT FROM sec10k
  WHERE tenant_id = 'honeywell'
  SHARD 'honeywell';`,
    category: "tenant-isolation",
    complexity: "advanced",
    dataset: "sec10k",
    tags: [...SHARED_SEC10K_TAGS, "multi-tenant", "shard", "isolation", "defense-in-depth"],
    impact: { reads: true, writes: false, schema: false },
    featured: true,
  },
  {
    id: "cte",
    label: "CTE Prefetch DAG + Fusion + Threshold",
    description: "Multi-stage prefetch DAG with per-stream score thresholds",
    teaching: "Multi-stage execution DAG: Stage 1 pre-fetches candidate vectors in parallel with independent filters; Stage 2 fuses with per-stream score thresholds.",
    query: `-- CTE Prefetch DAG + Fusion + Score Threshold — Honeywell
-- Stage 1: dense & sparse CTE pre-fetches with independent filters
-- Stage 2: RRF fusion with per-stream score cutoffs
WITH
  dense_candidates AS (
    QUERY TEXT 'supply chain disruption risk shortages'
    FROM sec10k USING dense
    WHERE fiscal_year >= 2024 LIMIT 100
  ),
  sparse_candidates AS (
    QUERY TEXT 'supply chain disruption risk shortages'
    FROM sec10k USING sparse
    WHERE fiscal_year >= 2024 LIMIT 100
  )
QUERY FUSION RRF FROM sec10k
  PREFETCH (
    dense_candidates SCORE THRESHOLD 0.4,
    sparse_candidates SCORE THRESHOLD 0.2
  )
  SHARD 'honeywell'
  WITH PAYLOAD true
  LIMIT 5;`,
    category: "multi-stage",
    complexity: "advanced",
    dataset: "sec10k",
    tags: [...SHARED_SEC10K_TAGS, "CTE", "prefetch", "DAG", "threshold"],
    impact: { reads: true, writes: false, schema: false },
    featured: true,
  },
  {
    id: "formula",
    label: "Formula Score Boosting",
    description: "Score multiplication with DEFAULTS fallback on SEC 10-K",
    teaching: "Programmatic score rewrite: FORMULA multiplies candidate scores by mathematical expressions with safe default fallbacks.",
    query: `-- Formula Score Boosting — RTX financial results boosted 2x
-- Stage 1: dense CTE pre-fetch finds financial chunks
-- Stage 2: FORMULA multiplies every score by 2.0 with DEFAULTS fallback
WITH
  candidates AS (
    QUERY TEXT 'financial results revenue earnings growth margins'
    FROM sec10k USING dense
    WHERE fiscal_year >= 2024 LIMIT 30
  )
QUERY FORMULA score * 2.0 DEFAULTS (score = 0.0)
  FROM sec10k
  PREFETCH (candidates)
  SHARD 'rtx'
  WITH PAYLOAD true
  LIMIT 5;`,
    category: "multi-stage",
    complexity: "advanced",
    dataset: "sec10k",
    tags: [...SHARED_SEC10K_TAGS, "formula", "score boost", "CTE", "DEFAULTS"],
    impact: { reads: true, writes: false, schema: false },
    featured: false,
  },
  {
    id: "grouped",
    label: "Grouped Aggregation by Fiscal Year",
    description: "Hybrid RRF with GROUP BY and per-bucket size on SEC 10-K",
    teaching: "Aggregates search hits into distinct buckets (e.g. per fiscal year) returning top N hits per bucket.",
    query: `-- Grouped Aggregation by Fiscal Year — RTX financials
-- Hybrid RRF query with GROUP BY — 3 top hits per fiscal year
QUERY HYBRID TEXT 'financial results revenue earnings'
  DENSE dense SPARSE sparse
  FUSION RRF
  FROM sec10k
  WHERE has_figures = true
  SHARD 'rtx'
  GROUP BY fiscal_year SIZE 3
  LIMIT 20;`,
    category: "aggregation",
    complexity: "intermediate",
    dataset: "sec10k",
    tags: [...SHARED_SEC10K_TAGS, "GROUP BY", "fiscal_year", "hybrid"],
    impact: { reads: true, writes: false, schema: false },
    featured: false,
  },
  {
    id: "mmr",
    label: "MMR Diversified Results",
    description: "Maximal Marginal Relevance diversity pruning on SEC 10-K",
    teaching: "Maximal Marginal Relevance (MMR) balances relevance vs diversity to avoid near-duplicate search hits.",
    query: `-- MMR Diversified Results — 3M manufacturing innovation
-- Maximal Marginal Relevance: DIVERSITY 0.5 avoids near-duplicates
-- CANDIDATES 100 fetches a larger pool before diversity pruning
QUERY MMR TEXT 'manufacturing operations innovation products'
  DIVERSITY 0.5 CANDIDATES 100
  FROM sec10k
  USING dense
  WHERE fiscal_year >= 2024
  SHARD '3m'
  PARAMS (hnsw_ef = 256)
  WITH PAYLOAD true
  LIMIT 5;`,
    category: "vector-search",
    complexity: "advanced",
    dataset: "sec10k",
    tags: [...SHARED_SEC10K_TAGS, "MMR", "diversity", "hnsw_ef"],
    impact: { reads: true, writes: false, schema: false },
    featured: true,
  },
  {
    id: "ddl",
    label: "Scroll Pagination + Global Count",
    description: "Cursor-based SCROLL pagination and COUNT aggregation on SEC 10-K",
    teaching: "Demonstrates multi-statement scripts: cursor-based SCROLL pagination followed by a global COUNT aggregation.",
    query: `-- SCROLL Pagination — GE filings with Cursor
-- Scroll over GE's 2024+ chunks; use AFTER <point_id> to paginate
SCROLL FROM sec10k
  WHERE fiscal_year >= 2024
  SHARD 'ge'
  LIMIT 5;

-- Count total chunks with financial figures across all companies
COUNT FROM sec10k
  WHERE has_figures = true AND fiscal_year >= 2024;`,
    category: "point-lifecycle",
    complexity: "beginner",
    dataset: "sec10k",
    tags: [...SHARED_SEC10K_TAGS, "SCROLL", "COUNT", "pagination"],
    impact: { reads: true, writes: false, schema: false },
    featured: false,
  },
  {
    id: "discover",
    label: "DBSF Distribution-Based Fusion",
    description: "Alternative distribution-based score fusion instead of RRF",
    teaching: "Alternative fusion algorithm: Distribution-Based Score Fusion (DBSF) standardizes score distributions across streams.",
    query: `-- DBSF Alternative Fusion — Honeywell supply chain
-- Distribution-Based Score Fusion instead of RRF
QUERY HYBRID TEXT 'supply chain disruption risk shortages'
  DENSE dense SPARSE sparse
  FUSION DBSF
  FROM sec10k
  WHERE fiscal_year >= 2024
  SHARD 'honeywell'
  WITH PAYLOAD true
  LIMIT 5;`,
    category: "fusion",
    complexity: "intermediate",
    dataset: "sec10k",
    tags: [...SHARED_SEC10K_TAGS, "DBSF", "hybrid", "fusion"],
    impact: { reads: true, writes: false, schema: false },
    featured: false,
  },
  {
    id: "mutation",
    label: "Upsert then Delete Point",
    description: "Point lifecycle: write a new point then clean it up",
    teaching: "Full point lifecycle in a multi-statement script: UPSERT inserts a point into a custom shard, followed by DELETE cleanup.",
    query: `-- Upsert + Cleanup — demo point lifecycle
UPSERT INTO sec10k VALUES
  {id: 9999999, text: 'QQL: vector query language for Qdrant — WASM-powered',
   tenant_id: 'rtx', company: 'demo', fiscal_year: 2026}
USING DENSE MODEL 'text-embedding-all-minilm-l6-v2-embedding'
SHARD 'rtx';

DELETE FROM sec10k WHERE id = 9999999 AND tenant_id = 'rtx';`,
    category: "point-lifecycle",
    complexity: "intermediate",
    dataset: "sec10k",
    tags: [...SHARED_SEC10K_TAGS, "UPSERT", "DELETE", "write", "lifecycle"],
    impact: { reads: false, writes: true, schema: false },
    featured: true,
  },
  {
    id: "exact_lookup",
    label: "Exact Point Lookup by ID",
    description: "Retrieve specific points by known IDs using POINTS expression",
    teaching: "Demonstrates QUERY POINTS expression: fetch exact points by their numeric or string IDs without any vector search. Useful for retrieval after discovery.",
    query: `-- Exact Point Lookup — retrieve specific SEC 10-K points by ID
-- Uses QUERY POINTS for direct ID-based retrieval without vector search
QUERY POINTS (1, 2, 3)
  FROM sec10k
  WITH PAYLOAD INCLUDE (company, fiscal_year, text)
  WITH VECTOR false;`,
    category: "vector-search",
    complexity: "beginner",
    dataset: "sec10k",
    tags: [...SHARED_SEC10K_TAGS, "POINTS", "exact lookup", "by ID"],
    impact: { reads: true, writes: false, schema: false },
    featured: false,
  },
  {
    id: "recommend",
    label: "Recommendation with Positive + Negative",
    description: "Vector recommendation using positive and negative example points",
    teaching: "Demonstrates QUERY RECOMMEND: finds points similar to positive examples and dissimilar to negative examples. STRATEGY controls how candidate vectors are combined (average_vector, best_score, or sum_scores).",
    query: `-- Vector Recommendation — find SEC 10-K chunks like positive examples
-- QUERY RECOMMEND: similar to positive, dissimilar to negative
-- Requires existing point IDs that have dense vectors
QUERY RECOMMEND POSITIVE (1, 10, 25) NEGATIVE (5)
  STRATEGY average_vector
  FROM sec10k
  USING dense
  WITH PAYLOAD true
  LIMIT 5;`,
    category: "discovery",
    complexity: "intermediate",
    dataset: "sec10k",
    tags: [...SHARED_SEC10K_TAGS, "RECOMMEND", "positive", "negative", "strategy"],
    impact: { reads: true, writes: false, schema: false },
    featured: true,
  },
  {
    id: "discovery",
    label: "Context Discovery with Target + Context",
    description: "DISCOVER query finds points near target but far from context negatives",
    teaching: "DEMONSTRATES QUERY DISCOVER: finds points relevant to a target while steering away from specified negative context. Requires dense vector index.",
    query: `-- Context Discovery — discover SEC 10-K topics near target, away from negatives
-- DISCOVER: finds points close to the target but far from negative context
QUERY DISCOVER TARGET TEXT 'manufacturing innovation and automation'
  CONTEXT (
    POSITIVE TEXT 'supply chain optimization' NEGATIVE TEXT 'workforce reductions'
  )
  FROM sec10k
  USING dense
  WITH PAYLOAD true
  LIMIT 5;`,
    category: "discovery",
    complexity: "advanced",
    dataset: "sec10k",
    tags: [...SHARED_SEC10K_TAGS, "DISCOVER", "context", "target", "exploratory"],
    impact: { reads: true, writes: false, schema: false },
    featured: true,
  },
  {
    id: "order_by",
    label: "Order By Payload Field",
    description: "Sort results by a payload field instead of vector similarity",
    teaching: "Demonstrates QUERY ORDER BY: sorts results by a payload field (fiscal_year DESC) rather than vector score. Useful for dashboard-style queries over structured data.",
    query: `-- Order By — sort SEC 10-K results by fiscal year descending
-- QUERY ORDER BY uses payload fields for sorting instead of vector score
QUERY ORDER BY fiscal_year DESC
  FROM sec10k
  USING dense
  WHERE fiscal_year >= 2024
  WITH PAYLOAD INCLUDE (company, fiscal_year, text)
  LIMIT 5;`,
    category: "vector-search",
    complexity: "beginner",
    dataset: "sec10k",
    tags: [...SHARED_SEC10K_TAGS, "ORDER BY", "sort", "payload"],
    impact: { reads: true, writes: false, schema: false },
    featured: false,
  },
  {
    id: "random_sample",
    label: "Random Sample",
    description: "Retrieve random points from the collection",
    teaching: "Demonstrates QUERY SAMPLE RANDOM: returns randomly selected points matching the filter criteria. Useful for data exploration, quality checks, or getting a representative subset.",
    query: `-- Random Sample — retrieve random SEC 10-K points from 2024
-- QUERY SAMPLE RANDOM returns random matching points for exploration
QUERY SAMPLE RANDOM
  FROM sec10k
  USING dense
  WHERE fiscal_year >= 2024
  WITH PAYLOAD true
  LIMIT 5;`,
    category: "vector-search",
    complexity: "beginner",
    dataset: "sec10k",
    tags: [...SHARED_SEC10K_TAGS, "SAMPLE", "RANDOM", "exploration"],
    impact: { reads: true, writes: false, schema: false },
    featured: false,
  },
  {
    id: "advanced_filter",
    label: "Advanced Nested + Compound Filter",
    description: "Compound WHERE with NESTED, HAS_VECTOR, and MATCH ANY conditions",
    teaching: "Demonstrates complex filter composition: NESTED for array sub-documents, HAS_VECTOR for existence checks, MATCH ANY for multi-term full-text search, and compound AND/OR logic.",
    query: `-- Advanced Filter — compound WHERE with nested and full-text conditions
-- Demonstrates NESTED, HAS_VECTOR, MATCH ANY, and compound operators
QUERY TEXT 'artificial intelligence machine learning'
  FROM sec10k
  USING dense
  WHERE fiscal_year >= 2024
    AND has_figures = true
    AND company IN ('honeywell', 'ge', 'rtx')
    AND HAS_VECTOR 'dense'
    AND content MATCH ANY ('revenue', 'growth', 'innovation')
  PARAMS (exact = false)
  WITH PAYLOAD INCLUDE (company, fiscal_year, text)
  LIMIT 5;`,
    category: "advanced-filters",
    complexity: "advanced",
    dataset: "sec10k",
    tags: [...SHARED_SEC10K_TAGS, "filter", "nested", "compound", "MATCH", "HAS_VECTOR"],
    impact: { reads: true, writes: false, schema: false },
    featured: true,
  },
  {
    id: "rerank",
    label: "Rerank with Cross-Encoder Model",
    description: "Two-stage retrieval: dense candidates then cross-encoder reranking",
    teaching: "Demonstrates QUERY RERANK: Stage 1 retrieves candidate points via dense vector search; Stage 2 applies a cross-encoder reranker model for more precise relevance scoring. Requires a rerank model and colbert vector.",
    query: `-- Rerank Pipeline — cross-encoder reranking over dense candidates
-- Stage 1: dense CTE pre-fetch finds broad candidates
-- Stage 2: RERANK applies cross-encoder model for precise relevance
WITH candidates AS (
  QUERY TEXT 'supply chain risk management strategies'
  FROM sec10k
  USING dense
  WHERE fiscal_year >= 2024
  LIMIT 50
)
QUERY RERANK TEXT 'supply chain risk management strategies'
  MODEL 'cross-encoder/ms-marco-MiniLM-L-6-v2'
  FROM sec10k
  USING colbert
  PREFETCH (candidates)
  WITH PAYLOAD true
  LIMIT 5;`,
    category: "multi-stage",
    complexity: "advanced",
    dataset: "sec10k",
    tags: [...SHARED_SEC10K_TAGS, "RERANK", "cross-encoder", "colbert", "two-stage"],
    impact: { reads: true, writes: false, schema: false },
    featured: true,
  },
  {
    id: "collection_ddl",
    label: "Collection Schema DDL Blueprint",
    description: "CREATE COLLECTION with hybrid config, payload index, and schema inspection",
    teaching: "Collection DDL blueprint: CREATE COLLECTION with named dense + sparse vectors, payload index on text fields, SHOW COLLECTIONS for schema inspection, and ALTER to modify runtime params.",
    query: `-- Collection Schema DDL Blueprint — sec10k-style hybrid collection
-- CREATE with named vectors, payload indexing, and schema inspection
CREATE COLLECTION sec10k HYBRID (dense VECTOR(768, COSINE), sparse SPARSE)
  WITH PARAMS (shard_number = 6, replication_factor = 2);

-- Payload index on frequently-filtered text fields
CREATE INDEX ON COLLECTION sec10k FOR text TYPE text
  WITH (tokenizer = 'word', lowercase = true);

CREATE INDEX ON COLLECTION sec10k FOR company TYPE keyword
  WITH (is_tenant = true);

-- Inspect collection schema
SHOW COLLECTION sec10k;`,
    category: "schema-ddl",
    complexity: "advanced",
    dataset: "sec10k",
    tags: ["DDL", "CREATE", "INDEX", "schema", "SHOW", "blueprint"],
    impact: { reads: false, writes: false, schema: true },
    featured: true,
  },
]

export const DEFAULT_PRESET_ID: PresetId = "hybrid"

export function getPreset(id: string): Preset | undefined {
  return PRESETS.find((p) => p.id === id)
}

export function getCategory(id: PresetCategory): CategoryDef | undefined {
  return PRESET_CATEGORIES.find((c) => c.id === id)
}

export function getPresetsByCategory(category: PresetCategory): Preset[] {
  return PRESETS.filter((p) => p.category === category)
}

export function getFeaturedPresets(): Preset[] {
  return PRESETS.filter((p) => p.featured)
}

export function getDatasetPresets(dataset: string): Preset[] {
  return PRESETS.filter((p) => p.dataset === dataset)
}

export function searchPresets(query: string): Preset[] {
  const q = query.toLowerCase()
  return PRESETS.filter(
    (p) =>
      p.label.toLowerCase().includes(q) ||
      p.description.toLowerCase().includes(q) ||
      p.teaching.toLowerCase().includes(q) ||
      p.tags.some((t) => t.toLowerCase().includes(q))
  )
}
