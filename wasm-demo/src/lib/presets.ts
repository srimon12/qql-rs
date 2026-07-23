export type PresetId =
  | "hybrid"
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
  description: string
  query: string
}

export const PRESETS: Preset[] = [
  {
    id: "hybrid",
    label: "Hybrid RRF",
    description: "Dense + sparse fusion with shard routing",
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
