export type PresetId =
  | "hybrid"
  | "berlin_radius"
  | "berlin_bbox"
  | "berlin_polygon"
  | "berlin_formula"
  | "berlin_superhost"
  | "berlin_grouped"
  | "multitenant"
  | "cte"
  | "formula"
  | "grouped"
  | "mmr"
  | "ddl"
  | "discover"
  | "mutation"

export type Preset = {
  id: PresetId
  label: string
  labelBadge?: string
  description: string
  teaching: string
  query: string
}

export const PRESETS: Preset[] = [
  {
    id: "berlin_radius",
    label: "Berlin Geo Radius (Brandenburg)",
    labelBadge: "GEO",
    description: "Search 1.5km radius around Brandenburg Gate (Berlin)",
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
  },
  {
    id: "berlin_bbox",
    label: "Berlin Geo BBox (Mitte Center)",
    labelBadge: "GEO",
    description: "Bounding box search across Mitte city center",
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
  },
  {
    id: "berlin_polygon",
    label: "Berlin Geo Polygon (Kreuzberg)",
    labelBadge: "GEO",
    description: "Custom polygon boundary around Kreuzberg district",
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
  },
  {
    id: "berlin_formula",
    label: "Berlin Score Boost (Rating Formula)",
    labelBadge: "FORMULA",
    description: "Re-rank semantic vector search using payload rating signal",
    teaching: "Demonstrates QQL QUERY FORMULA engine: multiplies candidate vector scores with payload metadata ratings using safe default fallbacks.",
    query: `-- Berlin Airbnb — Formula Score Boosting (60% Vector + 40% Review Rating)
-- Stage 1: Dense CTE retrieves candidate vector matches
-- Stage 2: FORMULA engine re-ranks candidate scores by multiplying rating signals
WITH
  candidates AS (
    QUERY TEXT 'cozy studio apartment near public transit'
    FROM berlin_airbnb
    USING dense
    LIMIT 50
  )
QUERY FORMULA (score * 0.6 + rating * 0.4) DEFAULTS (rating = 4.5)
  FROM berlin_airbnb
  PREFETCH (candidates)
  WITH PAYLOAD true
  LIMIT 5;`,
  },
  {
    id: "berlin_superhost",
    label: "Berlin Superhost CASE Business Logic",
    labelBadge: "CASE",
    description: "Conditional 1.5x score boost for verified Superhosts",
    teaching: "Demonstrates QQL conditional business logic: applies a CASE WHEN expression to boost Superhost listing scores by 1.5x during retrieval.",
    query: `-- Berlin Airbnb — Conditional Business Logic (Superhost 1.5x Boost)
-- Boosts vector match scores by 1.5x if host_is_superhost = true
WITH
  candidates AS (
    QUERY TEXT 'spacious loft with balcony and fast wifi'
    FROM berlin_airbnb
    USING dense
    LIMIT 50
  )
QUERY FORMULA (CASE WHEN superhost = true THEN score * 1.5 ELSE score END)
  FROM berlin_airbnb
  PREFETCH (candidates)
  WITH PAYLOAD true
  LIMIT 5;`,
  },
  {
    id: "berlin_grouped",
    label: "Berlin Grouped by Neighborhood",
    labelBadge: "GROUP",
    description: "Top 3 listings per Berlin neighborhood bucket",
    teaching: "Demonstrates QQL GROUP BY aggregation: partitions search results into distinct neighborhood buckets returning the top N hits per neighborhood.",
    query: `-- Berlin Airbnb — Grouped Aggregation by Neighborhood
-- Vector search grouped by neighborhood returning top 3 hits per neighborhood
QUERY TEXT 'quiet courtyard apartment'
  FROM berlin_airbnb
  USING dense
  WHERE price <= 100.0
  GROUP BY neighbourhood SIZE 3
  LIMIT 15;`,
  },
  {
    id: "hybrid",
    label: "Hybrid RRF",
    description: "Dense + sparse fusion with shard routing",
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
  },
  {
    id: "multitenant",
    label: "Multi-Tenant Isolation",
    description: "Shard routing + tenant_id payload filter (defense in depth)",
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
  },
  {
    id: "cte",
    label: "CTE Prefetch + Fusion",
    description: "Multi-stage prefetch DAG with score thresholds",
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
  },
  {
    id: "formula",
    label: "Formula Boost",
    description: "Score multiplication with DEFAULTS fallback",
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
  },
  {
    id: "grouped",
    label: "Grouped Aggregation",
    description: "GROUP BY with per-bucket size",
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
  },
  {
    id: "mmr",
    label: "MMR Diversified",
    description: "Maximal Marginal Relevance diversity pruning",
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
  },
  {
    id: "ddl",
    label: "SCROLL + COUNT",
    description: "Pagination and aggregation across tenants",
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
  },
  {
    id: "discover",
    label: "DBSF Fusion",
    description: "Distribution-Based Score Fusion alternative",
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
  },
  {
    id: "mutation",
    label: "Upsert + Delete",
    description: "Point lifecycle: write then cleanup",
    teaching: "Full point lifecycle in a multi-statement script: UPSERT inserts a point into a custom shard, followed by DELETE cleanup.",
    query: `-- Upsert + Cleanup — demo point lifecycle
UPSERT INTO sec10k VALUES
  {id: 9999999, text: 'QQL: vector query language for Qdrant — WASM-powered',
   tenant_id: 'rtx', company: 'demo', fiscal_year: 2026}
USING DENSE MODEL 'text-embedding-all-minilm-l6-v2-embedding'
SHARD 'rtx';

DELETE FROM sec10k WHERE id = 9999999 AND tenant_id = 'rtx';`,
  },
]

export const DEFAULT_PRESET_ID: PresetId = "hybrid"

export function getPreset(id: string): Preset | undefined {
  return PRESETS.find((p) => p.id === id)
}
