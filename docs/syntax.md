# QQL Syntax Guide

This guide is illustrative. The language contract lives in
[`language/v1/grammar.pest`](../language/v1/grammar.pest); production parsing
is the hand-written `AstLowerer` in `qql-core` (not a pest runtime frontend).
QQL follows Qdrant retrieval concepts rather than relational `SELECT`
semantics. Keywords are case-insensitive.

## Scripts

```ebnf
script       = [ statement, { ";", statement }, [ ";" ] ] ;
statement    = query | scroll | upsert | update | delete | ddl | count
             | clear-payload | delete-payload | delete-vectors
             | create-shard-key | drop-shard-key | show-shard-keys
             | drop-index | show | show-quotas | set-quota | facet ;
```

Multiple statements require `;`. Leading semicolons, repeated semicolons, and adjacent unseparated statements are invalid.

## Query

`QUERY` is the universal retrieval entry point.

```ebnf
query        = [ "WITH", cte, { ",", cte } ],
               "QUERY", query-expr,
               "FROM", collection, query-tail ;

cte          = name, "AS", "(", cte-query, ")" ;
cte-query    = "QUERY", query-expr, [ "FROM", collection ], query-tail ;

query-tail   = [ "USING", hybrid-using | vector-target ],
               [ "PREFETCH", "(", prefetch, { ",", prefetch }, ")" ],
               [ "WHERE", filter ],
               [ "SHARD", string ],
               [ "PARAMS", search-params ],
               [ "SCORE", "THRESHOLD", number ],
               [ "GROUP", "BY", field,
                   [ "SIZE", positive-integer ],
                   [ "LOOKUP", "FROM", collection, [ "VECTOR", vector-name ] ] ],
               [ "WITH", "PAYLOAD", payload-selector ],
               [ "WITH", "VECTOR", vector-selector ],
               [ "LIMIT", positive-integer ],
               [ "OFFSET", non-negative-integer ] ;
vector-target = vector-name, [ "AS", vector-kind ] ;
hybrid-using  = "HYBRID", [ "DENSE", vector-name ], [ "SPARSE", vector-name ],
                [ "FUSION", ( "RRF" | "DBSF" ) ] ;
vector-kind  = "DENSE" | "SPARSE" | "MULTI" | "MULTIVECTOR" ;
```

Top-level queries require `FROM`. A CTE may omit it and inherit the outer collection. Clauses occur at most once and only in the order above.

### Vector Input Forms
QQL supports three vector input forms for semantic search:
- **Explicit vector**: `QUERY VECTOR [0.1, 0.2, 0.3] FROM docs ...`
- **Implicit vector array**: `QUERY [0.1, 0.2, 0.3] FROM docs ...` (the `VECTOR` keyword can be omitted for compact array literals)
- **Point reference**: `QUERY POINT 42 FROM docs ...`

### Payload Selection Defaults
By default, queries return all point payloads (`WITH PAYLOAD true`). To minimize network payload bandwidth when payload attributes are not needed, pass explicit `WITH PAYLOAD false`:
```sql
QUERY 'search' FROM docs WITH PAYLOAD false LIMIT 10;
```

These are **two different features**. Confusing them is the most common multi-tenant mistake.

| Form | Kind | Purpose | Wire |
|------|------|---------|------|
| `CREATE SHARD KEY 'acme' ON COLLECTION c …` | **DDL** | Register a custom partition name | Create shard-key API |
| `DROP SHARD KEY` / `SHOW SHARD KEYS` | **DDL** | Manage / list keys | Admin RPCs |
| `… SHARD 'acme'` on QUERY / UPSERT / COUNT / … | **DML routing** | Send **this** request to that key | REST `shard_key` / gRPC `ShardKeySelector` |

```sql
-- 1) Define partitions (once)
CREATE COLLECTION tenants HYBRID (dense VECTOR(384, COSINE), sparse SPARSE)
WITH PARAMS (shard_number = 8, sharding_method = 'custom',
             shard_keys = ['acme', 'globex']);

CREATE SHARD KEY 'acme' ON COLLECTION tenants WITH (shards_number = 2);

-- 2) Route every DML op (and still filter for isolation)
UPSERT INTO tenants VALUES {id: 1, text: '…', tenant_id: 'acme'} SHARD 'acme';
QUERY TEXT 'risks' FROM tenants USING dense
  WHERE tenant_id = 'acme' SHARD 'acme' LIMIT 10;
```

- Omit `SHARD` on auto-sharded collections (default).
- **Isolation** is still `WHERE` / `inject_filter` — routing alone is not a security boundary.
- Host code may set the same field after parse: `stmt.shard_key = 'acme'` (Python),
  `stmt.shardKey = 'acme'` (Node/WASM), `stmt.set_shard_key(Some(...))` (Rust).
  There is **no** `inject_shard_key` API.
- Supported on: `QUERY`, `SCROLL`, `COUNT`, `UPSERT`, `DELETE`, `CLEAR PAYLOAD`,
  `DELETE PAYLOAD`, `DELETE VECTOR`, `UPDATE … VECTOR`, `UPDATE … PAYLOAD`.

### Vector targets and embedding roles

Vector names are application-defined; `dense`, `sparse`, and `colbert` are
conventional defaults, **not reserved**. Kind never comes from name spelling
(e.g. a vector named `sparse` is sparse only if the collection schema says so).

| Form | Meaning |
|---|---|
| `USING name` | Executor looks up `name` on the collection schema: dense, sparse, or dense+multivector |
| `USING name AS DENSE` | Explicit single-vector dense embed (MiniLM, CLIP text, …) |
| `USING name AS SPARSE` | Explicit sparse (wire-compatible BM25) embed |
| `USING name AS MULTI` / `AS MULTIVECTOR` | Explicit dense **multivector bag** (ColBERT / BGE-M3 ColBERT) → `MultiDense` — **not** CLIP |
| `USING HYBRID [DENSE n] [SPARSE n] [FUSION RRF\|DBSF]` | Expand text nearest → dense+sparse fusion (`QueryExpr::Hybrid`, same as `QUERY HYBRID`) |

### Query inputs (modality)

| Form | Embed path | Result |
|---|---|---|
| `TEXT '…' [MODEL '…']` or bare `'…'` | dense / sparse / multi by `USING` | vector |
| `IMAGE 'path-or-url' [MODEL '…']` | **image / CLIP vision** → always single dense | `Dense` |
| `VECTOR …` / `POINT …` | none | as-is |

CLIP dual-encoder: use dense CLIP **text** model for `TEXT` queries and CLIP **vision**
for `IMAGE` / `USING IMAGE` upserts into the **same** dense named vector space (e.g. 512-d).

Without `AS`, schema resolution runs **before** embedding and sets:

- dense vs sparse role;
- multivector flag when the named dense vector has `multivector_config` (e.g. `max_sim`).

When `USING` is omitted, execution succeeds only if the schema has exactly one
compatible vector; ambiguous topologies fail with `QQL-MISSING-USING`.

Offline embedding (no collection schema) requires an explicit `AS …` role.
`USING name` alone without prep fails with `QQL-VECTOR-KIND` rather than
silently dense-embedding.

### Query Expressions

```ebnf
query-expr   = points
             | nearest
             | recommend
             | context
             | discover
             | order-query
             | sample
             | fusion
             | formula
             | feedback
             | mmr
             | hybrid
             | cross-rerank
             | rerank ;

points       = "POINTS", "(", point-id, { ",", point-id }, ")" ;
nearest      = [ "NEAREST" ], query-input ;
query-input  = "TEXT", string, [ "MODEL", string ]
             | "IMAGE", string, [ "MODEL", string ]
             | "VECTOR", vector-value
             | "POINT", point-id
             | string ;

recommend    = "RECOMMEND", "POSITIVE", point-id-list,
               [ "NEGATIVE", point-id-list ],
               [ "STRATEGY", ( "average_vector" | "best_score" | "sum_scores" ) ] ;
context      = "CONTEXT", context-pairs ;
discover     = "DISCOVER", "TARGET", query-input, "CONTEXT", context-pairs ;
context-pairs = "(", "POSITIVE", query-input, "NEGATIVE", query-input,
                { ",", "POSITIVE", query-input, "NEGATIVE", query-input }, ")" ;

order-query  = "ORDER", "BY", field, [ "ASC" | "DESC" ] ;
sample       = "SAMPLE", "RANDOM" ;
fusion       = "FUSION", ( "RRF" | "DBSF" ) ;
formula      = "FORMULA", formula-expr, [ "DEFAULTS", config-block ] ;

feedback     = "RELEVANCE", "FEEDBACK", "TARGET", query-input,
               "FEEDBACK", "(", feedback-item, { ",", feedback-item }, ")",
               "STRATEGY", "NAIVE", "(", "a", "=", number, ",",
               "b", "=", number, ",", "c", "=", number, ")" ;
feedback-item = "(", query-input, ",", number, ")" ;

mmr          = "MMR", query-input,
               "DIVERSITY", number,
               "CANDIDATES", positive-integer ;

hybrid       = "HYBRID", ( "TEXT", string, [ "MODEL", string ] | string ),
               [ "DENSE", vector-name ],
               [ "SPARSE", vector-name ],
               [ "FUSION", ( "RRF" | "DBSF" ) ] ;

cross-rerank = "CROSS", "RERANK", ( "TEXT", string | string ), "MODEL", string,
               [ "ON", "FIELD", field ] ;
rerank       = "RERANK", rerank-input, "MODEL", string ;
rerank-input = "TEXT", string | "VECTOR", vector-value | "POINT", point-id ;
```

`QUERY POINTS (...)` retrieves those points directly. `QUERY NEAREST POINT ...` uses a point as the similarity input. A bare integer after `QUERY` is invalid, so point retrieval and point similarity cannot be confused.

Fusion requires a non-empty `PREFETCH`. **Late-interaction** `RERANK` requires an explicit input, `MODEL`, `USING`, and non-empty `PREFETCH`; the `USING` target must be dense (single-vector or multivector). When the target is multivector, `RERANK TEXT` embeds via multi-vector embedding into `MultiDense`.

**Cross-encoder** pair scoring is a separate form:

```sql
WITH c AS (QUERY TEXT 'q' FROM docs USING dense LIMIT 50)
QUERY CROSS RERANK TEXT 'q' MODEL 'bge-reranker-base' ON FIELD text
FROM docs
PREFETCH (c)
LIMIT 10;
```

`CROSS RERANK` runs the PREFETCH stage(s), extracts document text from `ON FIELD` (default `text`), scores `(query, doc)` pairs client-side, and reorders hits. It does **not** use Qdrant MaxSim. Host needs `rerank_pairs` (edge `reranker_model` or HTTP `rerank_endpoint`).

MMR requires both `DIVERSITY` in `[0, 1]` and positive `CANDIDATES`. MMR now supports both dense and sparse vector targets.

Hybrid expands to two prefetches (dense + sparse) with `LIMIT * 10` candidate
count, fused with RRF or DBSF.

`GROUP BY` uses Qdrant’s query/groups API. **`OFFSET` is now valid with
`GROUP BY`** (maps to Qdrant's `group_offset`). Page groups with `LIMIT`
and `OFFSET`, or constrain group keys with `WHERE`.

**Hybrid shorthand** has two equivalent surface forms that lower to the same
`QueryExpr::Hybrid` AST (and the same dense+sparse fusion plan):

```sql
-- Front-form (expression keyword)
QUERY HYBRID TEXT 'vector database' DENSE dense SPARSE bm25 FUSION RRF
FROM docs LIMIT 10;

-- Tail-form (USING clause) — natural when starting from a nearest query
QUERY TEXT 'vector database' FROM docs
USING HYBRID DENSE dense SPARSE bm25 FUSION RRF
LIMIT 10;

-- Defaults: omit DENSE/SPARSE names (schema must resolve unique dense + sparse),
-- FUSION defaults to RRF
QUERY 'vector database' FROM docs USING HYBRID LIMIT 10;
```

`USING HYBRID` requires a text nearest expression (`QUERY TEXT '…'` or bare
`QUERY '…'`). It cannot combine with `MMR`, non-text inputs, or a second
`QUERY HYBRID` expression.

### Formula expressions

```ebnf
formula-expr = constant
             | variable
             | "(", formula-expr, ")"
             | formula-expr, ("+" | "-" | "*" | "/"), formula-expr
             | "-", formula-expr
             | "ABS", "(", formula-expr, ")"
             | "SQRT", "(", formula-expr, ")"
             | "LOG", "(", formula-expr, ")"
             | "LN", "(", formula-expr, ")"
             | "EXP", "(", formula-expr, ")"
             | "POW", "(", formula-expr, ",", formula-expr, ")"
             | "GEO_DISTANCE", "(", lat, ",", lon, ",", field, ")"
             | decay-function ;
```

The formula parser supports standard arithmetic operators with precedence and parentheses. The `$score` variable represents the query score. Decay functions:

```ebnf
decay-function = ("EXP_DECAY" | "GAUSS_DECAY" | "LIN_DECAY"),
                 "(", formula-expr, [ ",", "TARGET", "=", formula-expr ],
                 [ ",", "SCALE", "=", number ],
                 [ ",", "MIDPOINT", "=", number ], ")" ;
```

The formula parser also supports `CASE WHEN ... THEN ... ELSE ... END` syntax and inline `MATCH` conditions:
```sql
QUERY FORMULA CASE WHEN tags MATCH ANY ('premium') THEN score * 2 ELSE score END
DEFAULTS (score = 0.0) FROM docs LIMIT 10;
```

### Search params

`PARAMS (...)` configures search execution:

```ebnf
search-params   = "(", search-param, { ",", search-param }, ")" ;
search-param    = "hnsw_ef", "=", positive-integer
                | "exact", "=", boolean
                | "acorn", "=", boolean
                | "max_selectivity", "=", number
                | "indexed_only", "=", boolean
                | "quantization", "=", object
                | "rrf_k", "=", positive-integer
                | "rrf_weights", "=", array
                | "idf", "=", ( string | "global" | "WHERE", filter )
                | "timeout", "=", positive-integer
                | "consistency", "=", ( positive-integer | "majority" | "quorum" | "all" | string ) ;
```

**Body vs request-level** (from Qdrant OpenAPI / proto):

| Param | Wire | Notes |
|---|---|---|
| `hnsw_ef`, `exact`, `acorn`, `max_selectivity`, `indexed_only`, `quantization`, `idf` | JSON body `params` | OpenAPI `SearchParams` (Qdrant ≥ 1.19 for `idf`) |
| `rrf_k`, `rrf_weights` | Fusion body when RRF | |
| `timeout` | REST **query string** `?timeout=N` / gRPC `timeout` field | Seconds, min 1; overrides global server timeout for this request |
| `consistency` | REST **query string** `?consistency=` / gRPC `read_consistency` | Factor `N`, or `majority` \| `quorum` \| `all` (OpenAPI `ReadConsistency`) |

`acorn = true` enables ACORN which estimates filter selectivity and adapts HNSW search. When `acorn = false`, ACORN is explicitly disabled. Optional `max_selectivity` is a number in `(0, 1]` and **requires** `acorn = true` (e.g. `PARAMS (acorn = true, max_selectivity = 0.4)`). **Not supported on edge.**

`quantization` accepts a JSON object with `ignore`, `rescore`, and `oversampling` fields matching Qdrant's `QuantizationSearchParams`.

`rrf_k` and `rrf_weights` control the Reciprocal Rank Fusion formula when `FUSION RRF` is used.

#### Sparse IDF corpus (Qdrant ≥ 1.19)

Per-query Inverse Document Frequency for sparse vectors:

```sql
-- Collection-wide / global IDF
QUERY TEXT 'vector database' FROM docs USING sparse
  PARAMS (idf = 'global')
  LIMIT 10;

-- Restrict the IDF corpus with a QQL filter
QUERY TEXT 'vector database' FROM docs USING sparse
  PARAMS (idf = WHERE status = 'active')
  LIMIT 10;

-- Tenant-scoped IDF: scoring corpus, not isolation
QUERY TEXT 'vector database' FROM docs USING sparse
  WHERE tenant_id = 'acme'
  SHARD 'acme'
  PARAMS (idf = WHERE tenant_id = 'acme')
  LIMIT 10;
```

`idf` is `'global'` or `WHERE <filter>`. JSON corpus objects are rejected at
parse (`QQL-VALIDATION-IDF`). Supported on remote Qdrant and on **qdrant-edge ≥ 0.8**.

```sql
-- Request timeout 30s + majority read consistency (remote Qdrant)
QUERY TEXT 'search' FROM docs USING dense
  PARAMS (timeout = 30, consistency = majority, hnsw_ef = 64)
  LIMIT 10;
```

Edge rejects request-level `timeout` / `consistency` with
`QQL-EDGE-UNSUPPORTED-TIMEOUT` / `QQL-EDGE-UNSUPPORTED-CONSISTENCY` (fail-loud).

### Examples

```sql
QUERY TEXT 'vector database' MODEL 'all-minilm:l6-v2'
FROM docs
USING dense
WHERE category = 'database'
PARAMS (hnsw_ef = 128, exact = false, acorn = true, max_selectivity = 0.4)
LIMIT 10;

QUERY POINTS (1, 2, 'point-a')
FROM docs
WITH PAYLOAD INCLUDE (title, url)
WITH VECTOR false;

WITH
  dense AS (QUERY TEXT 'vector database' USING dense LIMIT 100),
  sparse AS (QUERY TEXT 'vector database' USING sparse LIMIT 100)
QUERY FUSION RRF
FROM docs
PREFETCH (dense, sparse)
LIMIT 10;

WITH candidates AS (QUERY TEXT 'vector database' USING dense LIMIT 100)
QUERY RERANK TEXT 'vector database' MODEL 'answerai-colbert-small-v1'
FROM docs
USING colbert
PREFETCH (candidates)
LIMIT 10;

-- CLIP: text-to-image (dense CLIP text model → same space as stored image vectors)
QUERY TEXT 'a red running shoe' MODEL 'Qdrant/clip-ViT-B-32-text'
FROM products
USING image
LIMIT 10;

-- CLIP: image-to-image / image query (vision encoder)
QUERY IMAGE '/data/query.jpg' MODEL 'Qdrant/clip-ViT-B-32-vision'
FROM products
USING image
LIMIT 10;

-- Multivector nearest (schema has colbert WITH MULTIVECTOR, or use AS MULTI offline)
QUERY TEXT 'late interaction query'
FROM docs
USING colbert
LIMIT 10;

QUERY NEAREST VECTOR [[0.1, 0.2], [0.3, 0.4], [0.5, 0.6]]
FROM docs
USING colbert AS MULTI
LIMIT 10;

-- Multi-tenant query with shard routing
QUERY 'supply chain risks'
FROM sec10k
WHERE tenant_id = 'honeywell'
SHARD 'honeywell'
LIMIT 10;
```

## Prefetch

```ebnf
prefetch     = ( cte-name | cte-query ),
               [ "WHERE", filter ],
               [ "SCORE", "THRESHOLD", number ],
               [ "LOOKUP", "FROM", collection, [ "VECTOR", vector-name ] ] ;
```

CTE references are case-insensitive. Prefetch-level `WHERE` and `SCORE THRESHOLD` override the underlying CTE/query values when set.

## Selectors And Params

```ebnf
payload-selector = "true" | "false"
                 | "INCLUDE", name-list
                 | "EXCLUDE", name-list ;
vector-selector  = "true" | "false" | name-list ;
name-list        = "(", name, { ",", name }, ")" ;
```

Keys in payload objects, configuration blocks, formula defaults, and search parameters are unique case-insensitively.

## Point Data

```ebnf
upsert       = "UPSERT", "INTO", collection, "VALUES",
               point-object, { ",", point-object },
               [ embedding-options ],
               [ "SHARD", string ] ;
embedding-options = ( dense-embed | sparse-embed | hybrid-embed ) ;
dense-embed  = "USING",
               ( "DENSE", [ "MODEL", string ], [ "VECTOR", vector-name ]
               | "MODEL", string, [ "VECTOR", vector-name ]
               | "VECTOR", vector-name ) ;
sparse-embed = "USING", "SPARSE",
               [ "MODEL", string ], [ "VECTOR", vector-name ] ;
hybrid-embed = "USING", "HYBRID",
               [ "DENSE", [ "MODEL", string ], [ "VECTOR", vector-name ] ],
               [ "SPARSE", [ "MODEL", string ], [ "VECTOR", vector-name ] ] ;
scroll       = "SCROLL", "FROM", collection,
               [ "WHERE", filter ], [ "AFTER", point-id ],
               [ "SHARD", string ],
               [ "WITH", "VECTOR", [ vector-selector ] ],
               "LIMIT", positive-integer ;
count        = "COUNT", "FROM", collection,
               [ "WHERE", filter ],
               [ "SHARD", string ] ;
facet        = "FACET", ( field, "FROM", collection | "FROM", collection, [ "KEY" ], field ),
                [ "WHERE", filter ],
                [ "LIMIT", positive-integer ],
                [ "EXACT", boolean ],
                [ "SHARD", string ],
                [ "WITH", "(", facet-config, ")" ] ;
delete       = "DELETE", "FROM", collection, "WHERE", filter,
               [ "SHARD", string ] ;
clear-payload = "CLEAR", "PAYLOAD", "FROM", collection,
                "WHERE", filter,
                [ "SHARD", string ] ;
delete-vectors = "DELETE", "VECTOR", name, { ",", name },
                 "FROM", collection, "WHERE", filter,
                 [ "SHARD", string ] ;
update       = "UPDATE", collection, "SET",
               ( "VECTOR", [ vector-name ], "=", vector-value,
                 "WHERE", "id", "=", point-id, [ "SHARD", string ]
               | "PAYLOAD", "=", object, "WHERE", filter,
                 [ "SHARD", string ] ) ;

vector-value = dense-vector | sparse-vector | multidense-vector ;
dense-vector = "[", number, { ",", number }, "]" ;
sparse-vector = "{", "indices", ":", integer-list, ",",
                "values", ":", number-list, "}" ;
multidense-vector = "[", dense-vector, { ",", dense-vector }, "]" ;
```

Every upsert point requires an unsigned integer or string `id`. Its optional `vector` may be one unnamed vector value or an object of named vector values. All other object entries remain arbitrary payload values.

`SHARD '<key>'` on QUERY, SCROLL, COUNT, UPSERT, DELETE, CLEAR PAYLOAD, DELETE VECTOR, UPDATE … VECTOR, or UPDATE … PAYLOAD routes the operation to a specific shard group. It is a clustered-Qdrant feature; `qql-edge` rejects it explicitly because edge storage is single-node.

### Embed directive (fine-grained embedding control)

```ebnf
upsert       = "UPSERT", "INTO", collection, "VALUES",
               point-object, { ",", point-object },
               [ embedding-options ],
               [ embed-directive, { ",", embed-directive } ],
               [ "SHARD", string ] ;
embed-directive = "EMBED", field, "INTO", vector-name,
                  [ "USING",
                    ( ( "DENSE" | "SPARSE" ), [ "MODEL", string ]
                    | "MODEL", string ) ] ;
```

The `EMBED` directive maps a specific payload field to a named vector. Its
default role is dense; `USING SPARSE` selects sparse embedding, while
`USING MODEL '<name>'` is shorthand for a dense model. Multiple directives
within one `EMBED` clause are comma-separated:
```sql
UPSERT INTO docs VALUES {id: 1, title: 'doc title', body: 'doc body'}
  EMBED title INTO title_vec USING MODEL 'small',
         body INTO body_vec USING MODEL 'large';
```

## DDL

Collection creation/alteration/drop/show and payload index management:

```ebnf
create-collection = "CREATE", "COLLECTION", name,
                    [ "USING", [ "DENSE" ], "MODEL", string
                    | "USING", "HYBRID"
                    | "HYBRID",
                        [ "RERANK"
                        | [ "DENSE", "VECTOR", vector-name ],
                          [ "SPARSE", "VECTOR", vector-name ] ] ],
                    [ "(", collection-vector-def,
                        { ",", collection-vector-def }, ")" ],
                    [ config-blocks ] ;

alter-collection = "ALTER", "COLLECTION", name, config-blocks ;

create-index    = "CREATE", "INDEX", "ON", "COLLECTION", name,
                  "FOR", field, [ "TYPE", field-type ],
                  [ "WITH", config-block ] ;

drop-index      = "DROP", "INDEX", "ON", "COLLECTION", name,
                  "FOR", field ;

create-shard-key = "CREATE", "SHARD", "KEY", string,
                   "ON", "COLLECTION", name,
                   [ "WITH", config-block ] ;

drop-shard-key  = "DROP", "SHARD", "KEY", string,
                  "ON", "COLLECTION", name ;

show-shard-keys = "SHOW", "SHARD", "KEYS", "ON", "COLLECTION", name ;

drop-collection = "DROP", "COLLECTION", name ;

show            = "SHOW", "COLLECTIONS"
                | "SHOW", "COLLECTION", name ;

show-quotas     = "SHOW", "QUOTAS" ;
set-quota       = "SET", "QUOTA", config-block, [ "WAIT", boolean ] ;

vector-def    = name, "VECTOR", "(", size, ",", distance, ")"
                [ "WITH", "MULTIVECTOR", "(", config-block, ")" ]
                [ "WITH", "VECTOR", config-block ]
                [ "WITH", "QUANTIZATION", config-block ] ;
sparse-def    = name, "SPARSE", [ "WITH", "SPARSE", config-block ] ;
collection-vector-def = vector-def | sparse-def ;
config-blocks = "WITH", ( "HNSW" | "PARAMS" | "OPTIMIZERS" | "QUANTIZATION"
                         | "VECTOR" ), config-block ;
```

### Cluster quotas (Qdrant ≥ 1.19, REST only)

```sql
SHOW QUOTAS;

SET QUOTA (
  enabled = true,
  max_resident_memory_percent = 80,
  max_disk_usage_percent = 90,
  release_margin_percent = 5
) WAIT true;

-- Full replace of the cluster config — omitted keys are unset in the new body
SET QUOTA (enabled = false);

-- Clear a limit in the replacement body
SET QUOTA (max_disk_usage_percent = null);
```

| Key | Range | Notes |
|-----|-------|--------|
| `enabled` | bool | Master switch |
| `max_resident_memory_percent` | 1–100 or `null` | Resident memory cap |
| `max_disk_usage_percent` | 1–100 or `null` | Disk usage cap |
| `release_margin_percent` | 0–100 or `null` | Hysteresis margin |
| `WAIT` | bool (optional clause) | REST query `?wait=` |

`SET QUOTA` is a **full replace** (`PUT /quotas`), not a merge. Restate any
limits you want to keep. Invalid keys/ranges → `QQL-PLAN-QUOTA`.

| Backend | Behavior |
|---------|----------|
| REST | `GET /quotas`, `PUT /quotas` |
| gRPC | Fail-loud `QQL-GRPC-QUOTA` (no public quota service) |
| edge | Fail-loud `QQL-EDGE-UNSUPPORTED-QUOTA` |

### Memory placement & vector datatype (Qdrant ≥ 1.19)

Data remains on disk; `memory` only controls how a component is held in RAM:

| Placement | Meaning |
|-----------|---------|
| `cold` | Prefer disk; load on demand |
| `cached` | Cache in RAM when hot |
| `pinned` | Keep in RAM (not valid for payload) |

```sql
CREATE COLLECTION docs (
  dense VECTOR(384, COSINE)
    WITH VECTOR (memory = 'cached', datatype = 'turbo4')
) WITH HNSW (memory = 'cold')
  WITH PARAMS (payload_memory = 'cold');

CREATE COLLECTION sparse_docs (
  sparse SPARSE WITH SPARSE (modifier = 'idf', memory = 'cached')
);

CREATE COLLECTION qdocs (
  dense VECTOR(128, DOT)
    WITH QUANTIZATION (type = 'scalar', memory = 'pinned')
);
```

| Config block | `memory` | Other 1.19 keys |
|--------------|----------|-----------------|
| `WITH HNSW` | cold \| cached \| pinned | legacy `on_disk` still accepted |
| `WITH VECTOR` | cold \| cached \| pinned | `datatype = 'float32'\|'float16'\|'uint8'\|'turbo4'` (aliases `f32`/`f16`/`u8`/`t4`) |
| `WITH SPARSE` | cold \| cached \| pinned | `modifier`, `datatype` (no `turbo4` for sparse) |
| `WITH QUANTIZATION` | cold \| cached \| pinned | legacy `always_ram` still accepted |
| `WITH PARAMS` | use **`payload_memory`** | cold \| cached only (`pinned` rejected) |
| `CREATE INDEX … WITH` | cold \| cached \| pinned | keyword `prefix = true` |

Legacy `on_disk` / `on_disk_payload` / `always_ram` still parse and dual-write
with `memory` through Qdrant 1.19; prefer `memory` / `payload_memory` for new
scripts (upstream plans removal around 1.21).

`USING [DENSE] MODEL '<model>'` creates a collection with a single dense vector whose dimension is inferred from the embedding model. `USING HYBRID` creates the default dense+sparse topology. `HYBRID DENSE VECTOR semantic_v2 SPARSE VECTOR lexical_v2` assigns arbitrary names to those roles; `HYBRID RERANK` materializes conventional dense + sparse + `colbert` multivector (MaxSim) topology. All forms begin with `CREATE COLLECTION <name>` followed by at most one mode keyword group; `DENSE MODEL` without a preceding `USING` is rejected.

When an UPSERT contains text but no embedding clause, the executor inspects the existing collection schema and emits the compatible vector types. `USING DENSE`, `USING SPARSE`, and `USING HYBRID` can also omit target names and rely on schema inference. A role is inferred only when exactly one matching target exists; ambiguous schemas require `VECTOR <name>` or an explicit `EMBED` directive. The executor never infers a role merely from a vector being named `dense` or `sparse`.

Pre-computed multivectors use nested arrays:

```sql
UPSERT INTO docs VALUES {
  id: 1,
  text: 'chunk',
  vector: { colbert: [[0.1, 0.2], [0.3, 0.4]] }
};
```

### Collection Params

Shard configuration for multi-tenant isolation:

```sql
CREATE COLLECTION sec10k HYBRID (dense VECTOR(768, COSINE), sparse SPARSE)
WITH PARAMS (
  replication_factor = 2,
  shard_number = 8,
  sharding_method = 'custom',
  shard_keys = ['honeywell', 'ge', '3m', 'rtx'],
  payload_memory = 'cached'
);
```

| Param | Type | Description |
|-------|------|-------------|
| `replication_factor` | integer | Replica count per shard |
| `write_consistency_factor` | integer | Min replicas for write ack |
| `on_disk_payload` | boolean | Store payload on disk (legacy; prefer `payload_memory`) |
| `payload_memory` | string | `'cold'` or `'cached'` (Qdrant ≥ 1.19; `pinned` rejected) |
| `shard_number` | integer | Total shard count |
| `sharding_method` | string | `'auto'` or `'custom'` |
| `shard_keys` | string list | Tenant identifiers for custom sharding |
| `read_fan_out_factor` | integer | Read fan-out factor |
| `read_fan_out_delay_ms` | integer | Read fan-out delay |

### DDL Examples

```sql
CREATE COLLECTION docs (
  dense VECTOR(384, COSINE)
    WITH VECTOR (memory = 'cached', datatype = 'float16'),
  sparse SPARSE,
  colbert VECTOR(128, COSINE) WITH MULTIVECTOR (comparator = 'max_sim')
) WITH HNSW (m = 16, ef_construct = 100, memory = 'cold')
  WITH PARAMS (payload_memory = 'cached');

ALTER COLLECTION docs WITH VECTOR (memory = 'pinned');
CREATE INDEX ON COLLECTION docs FOR title TYPE text WITH (lowercase = true);
CREATE INDEX ON COLLECTION docs FOR tenant TYPE keyword
  WITH (prefix = true, memory = 'cached', is_tenant = true);
CREATE SHARD KEY 'acme' ON COLLECTION docs WITH (shards_number = 2);
DROP INDEX ON COLLECTION docs FOR title;
DROP COLLECTION docs;
SHOW COLLECTIONS;
SHOW COLLECTION docs;
SHOW QUOTAS;
```

### Point Counting

```sql
-- Count with optional filter
COUNT FROM docs WHERE status = 'active';

-- Count with exact match option
COUNT FROM docs WHERE status = 'active' WITH (exact = true);

-- Count with shard routing
COUNT FROM sec10k WHERE tenant_id = 'honeywell' SHARD 'honeywell';
```

### Facet Aggregations

Computes value counts for payload fields via Qdrant's `/collections/{collection}/facet` endpoint:

```sql
-- Basic facet counting unique categories
FACET category FROM docs;

-- Filtered facet with limit and exact counting
FACET room_type FROM stays
WHERE price < 150
LIMIT 5
EXACT true;

-- Shard-routed facet with key front syntax
FACET FROM catalog KEY tags
SHARD 'tenant_1'
LIMIT 10;
```

### Point Mutations

```sql
-- Clear all payload fields from matching points
CLEAR PAYLOAD FROM docs WHERE status = 'archived';

-- Delete specific payload keys from matching points
DELETE PAYLOAD draft, temp_token FROM docs WHERE status = 'archived';

-- Delete specific named vectors from points
DELETE VECTOR colbert FROM docs WHERE id = 42;

-- Delete multiple vectors at once
DELETE VECTOR dense, sparse FROM docs WHERE status = 'deprecated';
```

### Supported field index types

All types accept optional `memory = 'cold'|'cached'|'pinned'` (Qdrant ≥ 1.19)
alongside the legacy `on_disk` boolean where applicable.

| TYPE | Index variants |
|------|----------------|
| `keyword` | `is_tenant`, `on_disk`, `enable_hnsw`, **`prefix`** (bool — enables `MATCH PREFIX`) |
| `integer` | `lookup`, `range`, `is_principal`, `on_disk`, `enable_hnsw` |
| `float` | `on_disk`, `is_principal`, `enable_hnsw` |
| `geo` | `on_disk`, `enable_hnsw` |
| `text` | `tokenizer` (word/prefix/whitespace/multilingual), `lowercase`, `min_token_len`, `max_token_len`, `on_disk`, `stopwords`, `phrase_matching`, `ascii_folding`, `stemmer` (e.g. `'english'`) |
| `bool` | `on_disk`, `enable_hnsw` |
| `datetime` | `on_disk`, `is_principal`, `enable_hnsw` |
| `uuid` | `is_tenant`, `on_disk`, `enable_hnsw` |

```sql
-- Keyword prefix index for MATCH PREFIX filters
CREATE INDEX ON COLLECTION docs FOR title TYPE keyword
  WITH (prefix = true, memory = 'cached');

QUERY TEXT 'search' FROM docs USING dense
WHERE title MATCH PREFIX 'Comp'
LIMIT 10;
```
