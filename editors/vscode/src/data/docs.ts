/**
 * Hover documentation for QQL keywords and constructs.
 * Keep entries concise — shown in Markdown hover cards.
 */

export interface KeywordDoc {
  title: string;
  summary: string;
  example?: string;
  category: string;
}

export const KEYWORD_DOCS: Record<string, KeywordDoc> = {
  QUERY: {
    title: "QUERY",
    category: "Statement",
    summary:
      "Search or retrieve points. Modes: nearest (`TEXT`/`VECTOR`/`POINT`), `HYBRID`, `RECOMMEND`, `DISCOVER`, `FORMULA`, `FUSION`, `RERANK`, `MMR`, `ORDER BY`, `SAMPLE`, `POINTS`.",
    example: "QUERY TEXT 'hello' FROM docs USING dense LIMIT 10;",
  },
  WITH: {
    title: "WITH (CTE)",
    category: "Clause",
    summary:
      "Define named Common Table Expressions used as prefetch stages. Each CTE is an independent candidate stream that can be fused or reranked.",
    example:
      "WITH candidates AS (QUERY TEXT 'q' FROM docs USING dense LIMIT 100)\nQUERY FUSION RRF FROM docs PREFETCH (candidates) LIMIT 10;",
  },
  FROM: {
    title: "FROM",
    category: "Clause",
    summary: "Target collection for the statement.",
    example: "FROM my_collection",
  },
  WHERE: {
    title: "WHERE",
    category: "Clause",
    summary:
      "Payload filter. Supports comparison, `IN`, `MATCH`, `MATCH ANY`, `IS NULL`, geo predicates, `NESTED`, and boolean combinators.",
    example: "WHERE status = 'active' AND tags MATCH ANY ('a', 'b')",
  },
  LIMIT: {
    title: "LIMIT",
    category: "Clause",
    summary: "Maximum number of results to return.",
    example: "LIMIT 10",
  },
  OFFSET: {
    title: "OFFSET",
    category: "Clause",
    summary: "Skip the first N results (pagination). Works with `ORDER BY` and `GROUP BY`.",
  },
  USING: {
    title: "USING",
    category: "Clause",
    summary:
      "Named vector to search against. Forms: `USING dense`, `USING colbert AS MULTI`, `USING HYBRID DENSE d SPARSE s FUSION RRF`.",
    example: "USING dense",
  },
  TEXT: {
    title: "TEXT",
    category: "Query input",
    summary:
      "Embed this string at execute time (dense / sparse / multi depending on schema + `USING`).",
    example: "QUERY TEXT 'semantic search' FROM docs USING dense LIMIT 10;",
  },
  IMAGE: {
    title: "IMAGE",
    category: "Query input",
    summary: "Vision embedding search (e.g. CLIP). Path or URL plus optional `MODEL`.",
    example: "QUERY IMAGE '/path/img.jpg' MODEL 'clip-vit' FROM images USING image LIMIT 10;",
  },
  HYBRID: {
    title: "HYBRID",
    category: "Query mode",
    summary:
      "Dense + sparse retrieval fused with RRF or DBSF. Front-form `QUERY HYBRID …` or tail-form `USING HYBRID`.",
    example: "QUERY TEXT 'q' FROM docs USING HYBRID DENSE dense SPARSE sparse FUSION RRF LIMIT 10;",
  },
  FUSION: {
    title: "FUSION",
    category: "Query mode",
    summary:
      "Merge prefetch candidate streams. Algorithms: `RRF` (Reciprocal Rank Fusion), `DBSF` (Distribution-Based Score Fusion).",
    example: "QUERY FUSION RRF FROM docs PREFETCH (a, b) LIMIT 10;",
  },
  RRF: {
    title: "RRF",
    category: "Fusion",
    summary: "Reciprocal Rank Fusion. Optional `RRF_K` and `RRF_WEIGHTS` tune the merge.",
  },
  DBSF: {
    title: "DBSF",
    category: "Fusion",
    summary: "Distribution-Based Score Fusion — normalizes score distributions before merging.",
  },
  PREFETCH: {
    title: "PREFETCH",
    category: "Clause",
    summary:
      "Reference CTE stages (or nested queries) as candidate inputs for fusion / rerank / formula.",
    example: "PREFETCH (dense SCORE THRESHOLD 0.5, sparse)",
  },
  RECOMMEND: {
    title: "RECOMMEND",
    category: "Query mode",
    summary:
      "Recommend points from positive/negative examples. Strategies: `average_vector`, `best_score`, `sum_scores`.",
    example:
      "QUERY RECOMMEND POSITIVE (1, 2) NEGATIVE (3) STRATEGY average_vector FROM products LIMIT 10;",
  },
  DISCOVER: {
    title: "DISCOVER",
    category: "Query mode",
    summary: "Explore vector space around a target using positive/negative context pairs.",
    example: "QUERY DISCOVER TARGET 1 CONTEXT (POSITIVE 2 NEGATIVE 3) FROM items LIMIT 10;",
  },
  FORMULA: {
    title: "FORMULA",
    category: "Query mode",
    summary:
      "Score shaping with arithmetic, `CASE WHEN`, payload fields, and decay helpers (`EXP_DECAY`, `GAUSS_DECAY`, `GEO_DISTANCE`, …).",
    example:
      "QUERY FORMULA ($score * 0.7 + popularity * 0.3) DEFAULTS (popularity = 0) FROM docs LIMIT 10;",
  },
  RERANK: {
    title: "RERANK",
    category: "Query mode",
    summary:
      "Late-interaction multivector rerank (e.g. ColBERT MaxSim) over prefetch candidates. For pair scorers use `CROSS RERANK`.",
    example: "QUERY RERANK TEXT 'q' MODEL 'colbert' FROM docs USING colbert PREFETCH (c) LIMIT 10;",
  },
  CROSS: {
    title: "CROSS RERANK",
    category: "Query mode",
    summary:
      "Cross-encoder pair scoring `(query, doc_text)` over prefetch candidates. Reads document text from `ON FIELD` (default `text`).",
    example:
      "QUERY CROSS RERANK TEXT 'q' MODEL 'bge-reranker' ON FIELD body FROM docs PREFETCH (c) LIMIT 10;",
  },
  MMR: {
    title: "MMR",
    category: "Query mode",
    summary:
      "Maximal Marginal Relevance — balance relevance vs diversity. `DIVERSITY` in `[0,1]`, `CANDIDATES` pool size.",
    example: "QUERY MMR 'q' DIVERSITY 0.5 CANDIDATES 100 FROM docs USING dense LIMIT 10;",
  },
  POINTS: {
    title: "QUERY POINTS",
    category: "Query mode",
    summary: "Retrieve points by ID (no vector search). No `LIMIT` required.",
    example: "QUERY POINTS (1, 2) FROM docs;",
  },
  ORDER: {
    title: "ORDER BY",
    category: "Query mode",
    summary: "Payload index scan ordered by a field (`ASC` / `DESC`). Good for dashboards.",
    example: "QUERY ORDER BY created_at DESC FROM docs LIMIT 20;",
  },
  SAMPLE: {
    title: "SAMPLE RANDOM",
    category: "Query mode",
    summary: "Random point sampling from a collection.",
    example: "QUERY SAMPLE RANDOM FROM docs LIMIT 10;",
  },
  SCROLL: {
    title: "SCROLL",
    category: "Statement",
    summary: "Cursor-based pagination through points (optionally filtered).",
    example: "SCROLL FROM docs WHERE status = 'active' LIMIT 100;",
  },
  COUNT: {
    title: "COUNT",
    category: "Statement",
    summary: "Count matching points. Use `WITH (exact = true)` for exact counts.",
    example: "COUNT FROM docs WHERE active = true WITH (exact = true);",
  },
  FACET: {
    title: "FACET",
    category: "Statement",
    summary:
      "Compute value counts for a payload field via Qdrant's `/collections/{collection}/facet` endpoint. Supports `WHERE`, `LIMIT`, `EXACT`, and `SHARD`.",
    example: "FACET room_type FROM stays WHERE price < 150 LIMIT 5 EXACT true;",
  },
  UPSERT: {
    title: "UPSERT INTO",
    category: "Statement",
    summary: "Insert or update points. `USING DENSE MODEL '…'` embeds text fields at execute time.",
    example: "UPSERT INTO docs VALUES {id: 1, text: 'hello'} USING DENSE MODEL 'all-MiniLM-L6-v2';",
  },
  DELETE: {
    title: "DELETE",
    category: "Statement",
    summary:
      "Delete points by filter/IDs, or `DELETE PAYLOAD` / `DELETE VECTOR` for partial deletes.",
    example: "DELETE FROM docs WHERE status = 'archived';",
  },
  UPDATE: {
    title: "UPDATE",
    category: "Statement",
    summary: "Update vectors (`UPDATE … VECTOR`) or payload fields on matching points.",
  },
  CLEAR: {
    title: "CLEAR PAYLOAD",
    category: "Statement",
    summary: "Remove all payload keys from matching points (points remain).",
  },
  CREATE: {
    title: "CREATE",
    category: "DDL",
    summary: "DDL: `CREATE COLLECTION`, `CREATE INDEX`, `CREATE SHARD KEY`.",
    example: "CREATE COLLECTION docs (dense VECTOR(384, COSINE));",
  },
  DROP: {
    title: "DROP",
    category: "DDL",
    summary: "DDL: `DROP COLLECTION`, `DROP INDEX`, `DROP SHARD KEY`.",
  },
  ALTER: {
    title: "ALTER COLLECTION",
    category: "DDL",
    summary: "Patch collection params (HNSW, optimizers, quantization, …).",
  },
  SHOW: {
    title: "SHOW",
    category: "DDL",
    summary: "`SHOW COLLECTIONS`, `SHOW COLLECTION name`, `SHOW SHARD KEYS`.",
  },
  VECTOR: {
    title: "VECTOR",
    category: "DDL / input",
    summary:
      "In DDL: declare a dense vector column `VECTOR(dim, DISTANCE)`. In queries: pass a precomputed vector `VECTOR [0.1, 0.2, …]`.",
  },
  SPARSE: {
    title: "SPARSE",
    category: "DDL / mode",
    summary: "Sparse (BM25-style) vector column or retrieval leg.",
  },
  MULTIVECTOR: {
    title: "MULTIVECTOR",
    category: "DDL",
    summary:
      "Mark a dense column as multi-vector (e.g. ColBERT). Usually `WITH MULTIVECTOR (comparator = 'max_sim')`.",
  },
  DENSE: {
    title: "DENSE",
    category: "Mode",
    summary: "Dense embedding leg — used in hybrid, upsert auto-embed, and named vectors.",
  },
  MODEL: {
    title: "MODEL",
    category: "Clause",
    summary: "Embedding or reranker model name for TEXT/IMAGE/RERANK/UPSERT paths.",
  },
  GROUP: {
    title: "GROUP BY",
    category: "Clause",
    summary:
      "Partition hits by a payload field. `SIZE n` caps per-group results; `LOOKUP FROM` resolves cross-collection metadata.",
    example: "GROUP BY 'author_id' SIZE 5 LOOKUP FROM authors",
  },
  SCORE: {
    title: "SCORE THRESHOLD",
    category: "Clause",
    summary: "Minimum score for results or per-prefetch candidate streams.",
  },
  THRESHOLD: {
    title: "THRESHOLD",
    category: "Clause",
    summary: "Part of `SCORE THRESHOLD <float>`.",
  },
  PARAMS: {
    title: "PARAMS",
    category: "Clause",
    summary:
      "Search/request params: `hnsw_ef`, `exact`, `quantization`, `acorn`, `max_selectivity`, `timeout`, `consistency`, …",
    example: "PARAMS (hnsw_ef = 128, acorn = true, max_selectivity = 0.4)",
  },
  ACORN: {
    title: "ACORN",
    category: "Search param",
    summary:
      "Adaptive filtered HNSW (remote Qdrant). Pair with `max_selectivity` in `(0, 1]`. Not supported on edge.",
  },
  HNSW: {
    title: "HNSW",
    category: "DDL",
    summary: "HNSW index config on create/alter: `m`, `ef_construct`, …",
  },
  SHARD: {
    title: "SHARD",
    category: "Multi-tenancy",
    summary:
      "`SHARD 'key'` routes a request to a custom shard. Also used in `CREATE/DROP SHARD KEY`.",
    example: "QUERY TEXT 'q' FROM tenants SHARD 'acme' USING dense LIMIT 10;",
  },
  MATCH: {
    title: "MATCH",
    category: "Filter",
    summary:
      "Full-text / keyword match on a payload field. Prefer `MATCH ANY` for multi-value sets.",
  },
  MATCH_ANY: {
    title: "MATCH ANY",
    category: "Filter",
    summary: "True if the field matches any of the given values.",
  },
  NESTED: {
    title: "NESTED",
    category: "Filter",
    summary: "Filter inside nested payload objects/arrays.",
  },
  GEO_RADIUS: {
    title: "GEO_RADIUS",
    category: "Filter",
    summary: "Points within `radius` meters of `center: {lat, lon}`.",
  },
  GEO_BBOX: {
    title: "GEO_BBOX",
    category: "Filter",
    summary: "Points inside a bounding box (`top_left` / `bottom_right`).",
  },
  GEO_POLYGON: {
    title: "GEO_POLYGON",
    category: "Filter",
    summary: "Points inside a polygon (with optional interiors / holes).",
  },
  GEO_DISTANCE: {
    title: "GEO_DISTANCE",
    category: "Formula",
    summary: "Distance in meters between two geo points — used in formula scoring/decay.",
  },
  ABS: {
    title: "ABS(x)",
    category: "Formula",
    summary: "Absolute value of a formula expression.",
    example: "QUERY FORMULA ABS($score) DEFAULTS (score = 0.0) FROM docs LIMIT 10;",
  },
  SQRT: {
    title: "SQRT(x)",
    category: "Formula",
    summary: "Square root — common non-linear score dampening.",
    example: "QUERY FORMULA SQRT($score) * 10.0 DEFAULTS (score = 0.0) FROM docs LIMIT 10;",
  },
  LOG: {
    title: "LOG(x)",
    category: "Formula",
    summary: "Base-10 logarithm (wire key `log10`) — logarithmic dampening of heavy-tailed fields.",
    example:
      "QUERY FORMULA LOG(citation_count + 1) DEFAULTS (citation_count = 0) FROM docs LIMIT 10;",
  },
  LN: {
    title: "LN(x)",
    category: "Formula",
    summary: "Natural logarithm.",
    example: "QUERY FORMULA LN($score + 1) DEFAULTS (score = 0.0) FROM docs LIMIT 10;",
  },
  EXP: {
    title: "EXP(x)",
    category: "Formula",
    summary: "Natural exponential `e^x`.",
    example: "QUERY FORMULA EXP($score) DEFAULTS (score = 0.0) FROM docs LIMIT 10;",
  },
  ACOSH: {
    title: "ACOSH(x)",
    category: "Formula",
    summary:
      "Inverse hyperbolic cosine — smooth strictly-positive score shaping. New Qdrant `Expression` variant.",
    example: "QUERY FORMULA ACOSH($score + 1.0) DEFAULTS (score = 0.0) FROM docs LIMIT 10;",
  },
  POW: {
    title: "POW(base, exponent)",
    category: "Formula",
    summary: "Raise a formula expression to a power.",
    example: "QUERY FORMULA POW(ABS($score), 2.0) DEFAULTS (score = 0.0) FROM docs LIMIT 10;",
  },
  MAX: {
    title: "MAX(a, b, …)",
    category: "Formula",
    summary:
      "Largest of n ≥ 1 formula expressions — clamp scores upward. New Qdrant `Expression` variant.",
    example: "QUERY FORMULA MAX($score * 2.0, 1.0) DEFAULTS (score = 0.0) FROM docs LIMIT 10;",
  },
  MIN: {
    title: "MIN(a, b, …)",
    category: "Formula",
    summary:
      "Smallest of n ≥ 1 formula expressions — cap scores / normalize. New Qdrant `Expression` variant.",
    example:
      "QUERY FORMULA MIN($score, bonus, popularity) DEFAULTS (score = 0.0) FROM docs LIMIT 10;",
  },
  EXP_DECAY: {
    title: "EXP_DECAY",
    category: "Formula",
    summary: "Exponential decay for recency or distance boosting in `FORMULA`.",
  },
  GAUSS_DECAY: {
    title: "GAUSS_DECAY",
    category: "Formula",
    summary: "Gaussian decay for smooth distance/time score shaping.",
  },
  LIN_DECAY: {
    title: "LIN_DECAY",
    category: "Formula",
    summary: "Linear decay helper for formula scoring.",
  },
  CASE: {
    title: "CASE WHEN",
    category: "Formula",
    summary: "Conditional expression inside `FORMULA`.",
    example: "CASE WHEN priority = 'high' THEN score * 2.5 ELSE score END",
  },
  DEFAULTS: {
    title: "DEFAULTS",
    category: "Clause",
    summary: "Default values for missing payload fields referenced in a formula.",
  },
  PAYLOAD: {
    title: "PAYLOAD",
    category: "Clause",
    summary:
      "`WITH PAYLOAD true|false|INCLUDE (…) | EXCLUDE (…)` controls returned payload fields.",
  },
  INCLUDE: {
    title: "INCLUDE",
    category: "Clause",
    summary: "Whitelist payload fields in the response.",
  },
  EXCLUDE: {
    title: "EXCLUDE",
    category: "Clause",
    summary: "Blacklist payload fields from the response.",
  },
  POSITIVE: {
    title: "POSITIVE",
    category: "Recommend / Discover",
    summary: "Positive example point IDs for recommend/discover context.",
  },
  NEGATIVE: {
    title: "NEGATIVE",
    category: "Recommend / Discover",
    summary: "Negative example point IDs for recommend/discover context.",
  },
  STRATEGY: {
    title: "STRATEGY",
    category: "Recommend",
    summary: "`average_vector` | `best_score` | `sum_scores`.",
  },
  COSINE: {
    title: "COSINE",
    category: "Distance",
    summary: "Cosine distance metric for dense vectors.",
  },
  DOT: {
    title: "DOT",
    category: "Distance",
    summary: "Dot-product similarity metric.",
  },
  EUCLID: {
    title: "EUCLID",
    category: "Distance",
    summary: "Euclidean (L2) distance metric.",
  },
  MANHATTAN: {
    title: "MANHATTAN",
    category: "Distance",
    summary: "Manhattan (L1) distance metric.",
  },
  INDEX: {
    title: "INDEX",
    category: "DDL",
    summary:
      "Payload index. Types: keyword, integer, float, geo, text, bool, datetime, uuid. Supports `is_tenant = true`.",
  },
  COLLECTION: {
    title: "COLLECTION",
    category: "DDL",
    summary: "Collection name in DDL / SHOW statements.",
  },
  MULTI: {
    title: "AS MULTI",
    category: "Clause",
    summary:
      "Treat the named vector as multivector (MultiDense) when schema is unavailable offline.",
  },
  TIMEOUT: {
    title: "TIMEOUT",
    category: "Param",
    summary: "Server-side request timeout (seconds) via `PARAMS (timeout = N)`.",
  },
  CONSISTENCY: {
    title: "CONSISTENCY",
    category: "Param",
    summary: "Read consistency on clustered Qdrant: `majority`, `quorum`, `all`, or a factor `N`.",
  },
  TRUE: {
    title: "true",
    category: "Literal",
    summary: "Boolean true.",
  },
  FALSE: {
    title: "false",
    category: "Literal",
    summary: "Boolean false.",
  },
  NULL: {
    title: "null",
    category: "Literal",
    summary: "Null literal (e.g. `IS NULL` checks).",
  },
};

export function lookupKeywordDoc(word: string): KeywordDoc | undefined {
  return KEYWORD_DOCS[word.toUpperCase()];
}

export function formatKeywordHover(doc: KeywordDoc): string {
  const parts = [`**${doc.title}** · _${doc.category}_`, "", doc.summary];
  if (doc.example) {
    parts.push("", "```qql", doc.example, "```");
  }
  return parts.join("\n");
}
