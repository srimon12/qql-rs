# Canonical QQL Grammar

This is the canonical syntax implemented by `qql-core`. QQL follows Qdrant retrieval concepts rather than relational `SELECT` semantics. Keywords are case-insensitive.

## Scripts

```ebnf
script       = [ statement, { ";", statement }, [ ";" ] ] ;
statement    = query | scroll | upsert | update | delete | ddl | count
             | clear-payload | delete-vectors | create-shard-key ;
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

query-tail   = [ "USING", vector-name ],
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
```

Top-level queries require `FROM`. A CTE may omit it and inherit the outer collection. Clauses occur at most once and only in the order above.

`SHARD '<key>'` routes the query to a specific shard group. It is optional and only needed when using custom sharding in a multi-tenant collection.

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
             | rerank ;

points       = "POINTS", "(", point-id, { ",", point-id }, ")" ;
nearest      = [ "NEAREST" ], query-input ;
query-input  = "TEXT", string, [ "MODEL", string ]
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

rerank       = "RERANK", rerank-input, "MODEL", string ;
rerank-input = "TEXT", string | "VECTOR", vector-value | "POINT", point-id ;
```

`QUERY POINTS (...)` retrieves those points directly. `QUERY NEAREST POINT ...` uses a point as the similarity input. A bare integer after `QUERY` is invalid, so point retrieval and point similarity cannot be confused.

Fusion requires a non-empty `PREFETCH`. Rerank requires an explicit input, `MODEL`, `USING`, and non-empty `PREFETCH`. MMR requires both `DIVERSITY` in `[0, 1]` and positive `CANDIDATES`. Hybrid expands to two prefetches (dense + sparse) with `LIMIT * 10` candidate count, fused with RRF or DBSF.

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
                | "indexed_only", "=", boolean
                | "quantization", "=", object
                | "rrf_k", "=", positive-integer
                | "rrf_weights", "=", array ;
```

`acorn = true` enables ACORN (Adaptive Cardinality Estimator for ONgRN) which estimates filter selectivity and adapts HNSW search. When `acorn = false`, ACORN is explicitly disabled.

`quantization` accepts a JSON object with `ignore`, `rescore`, and `oversampling` fields matching Qdrant's `QuantizationSearchParams`.

`rrf_k` and `rrf_weights` control the Reciprocal Rank Fusion formula when `FUSION RRF` is used.

### Examples

```sql
QUERY TEXT 'vector database' MODEL 'nomic-embed-text'
FROM docs
USING dense
WHERE category = 'database'
PARAMS (hnsw_ef = 128, exact = false, acorn = true)
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
QUERY RERANK TEXT 'vector database' MODEL 'reranker-v1'
FROM docs
USING colbert
PREFETCH (candidates)
LIMIT 10;

-- Multi-tenant query with shard routing
QUERY 'supply chain risks'
FROM sec10k
WHERE tenant_id = 'honeywell'
SHARD 'honeywell'
LIMIT 10;"
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
embedding-options = ( dense-embed | hybrid-embed ) ;
dense-embed  = "USING", "DENSE", ( "MODEL", string | "VECTOR", string ) ;
hybrid-embed = "USING", "HYBRID",
               [ "DENSE", ( "MODEL", string | "VECTOR", string ) ],
               [ "SPARSE", ( "MODEL", string | "VECTOR", string ) ] ;
scroll       = "SCROLL", "FROM", collection,
               [ "WHERE", filter ], [ "AFTER", point-id ],
               [ "SHARD", string ],
               [ "WITH", "VECTOR", [ vector-selector ] ],
               "LIMIT", positive-integer ;
count        = "COUNT", "FROM", collection,
               [ "WHERE", filter ],
               [ "SHARD", string ] ;
delete       = "DELETE", "FROM", collection, "WHERE", filter,
               [ "SHARD", string ] ;
clear-payload = "CLEAR", "PAYLOAD", "FROM", collection,
                "WHERE", filter ;
delete-vectors = "DELETE", "VECTOR", name, { ",", name },
                 "FROM", collection, "WHERE", filter ;
update       = "UPDATE", collection, "SET",
               ( "VECTOR", [ vector-name ], "=", vector-value,
                 "WHERE", "id", "=", point-id
               | "PAYLOAD", "=", object, "WHERE", filter ) ;

vector-value = dense-vector | sparse-vector | multidense-vector ;
dense-vector = "[", number, { ",", number }, "]" ;
sparse-vector = "{", "indices", ":", integer-list, ",",
                "values", ":", number-list, "}" ;
multidense-vector = "[", dense-vector, { ",", dense-vector }, "]" ;
```

Every upsert point requires an unsigned integer or string `id`. Its optional `vector` may be one unnamed vector value or an object of named vector values. All other object entries remain arbitrary payload values.

`SHARD '<key>'` on UPSERT, SCROLL, DELETE, COUNT, or QUERY routes the operation to a specific shard group.

### Embed directive (fine-grained embedding control)

```ebnf
upsert       = "UPSERT", "INTO", collection, "VALUES",
               point-object, { ",", point-object },
               [ embedding-options ],
               [ embed-directive, { ",", embed-directive } ],
               [ "SHARD", string ] ;
embed-directive = "EMBED", field, "INTO", vector-name,
                  "USING", ( "DENSE" | "SPARSE" ), [ "MODEL", string ] ;
```

The `EMBED` directive maps a specific payload field to a named vector. Multiple directives within one `EMBED` clause are comma-separated:
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
                    | "HYBRID", [ "RERANK" ],
                        [ "DENSE", name, "VECTOR", vector-name ],
                        [ "SPARSE", name, "VECTOR", vector-name ] ],
                    [ "(", vector-def, { ",", vector-def }, ")" ],
                    [ "(", sparse-def, { ",", sparse-def }, ")" ],
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

vector-def    = name, "VECTOR", "(", size, ",", distance, ")"
                [ "WITH", "MULTIVECTOR", "(", config-block, ")" ] ;
sparse-def    = name, "SPARSE" ;
config-blocks = "WITH", ( "HNSW" | "PARAMS" | "OPTIMIZERS" | "QUANTIZE"
                         | "VECTOR" ), config-block ;
```

`USING [DENSE] MODEL '<model>'` creates a collection with a single dense vector whose dimension is inferred from the embedding model. `HYBRID` enables dense + sparse hybrid search; add `RERANK` for a second dense vector used by the `QUERY RERANK` expression. The `VECTOR` keyword separating the vector-config name from the vector-name value is required. All three syntax forms begin with `CREATE COLLECTION <name>` followed by at most one mode keyword group; `DENSE MODEL` without a preceding `USING` is rejected.

### Collection Params

Shard configuration for multi-tenant isolation:

```sql
CREATE COLLECTION sec10k HYBRID (dense VECTOR(768, COSINE), sparse SPARSE)
WITH PARAMS (
  replication_factor = 2,
  shard_number = 8,
  sharding_method = 'custom',
  shard_keys = ['honeywell', 'ge', '3m', 'rtx']
);
```

| Param | Type | Description |
|-------|------|-------------|
| `replication_factor` | integer | Replica count per shard |
| `write_consistency_factor` | integer | Min replicas for write ack |
| `on_disk_payload` | boolean | Store payload on disk |
| `shard_number` | integer | Total shard count |
| `sharding_method` | string | `'auto'` or `'custom'` |
| `shard_keys` | string list | Tenant identifiers for custom sharding |
| `read_fan_out_factor` | integer | Read fan-out factor |
| `read_fan_out_delay_ms` | integer | Read fan-out delay |

### DDL Examples

```sql
CREATE COLLECTION docs (
  dense VECTOR(384, COSINE),
  sparse SPARSE,
  colbert VECTOR(128, COSINE) WITH MULTIVECTOR (comparator = 'max_sim')
) WITH HNSW (m = 16, ef_construct = 100);

ALTER COLLECTION docs WITH VECTOR (on_disk = true);
CREATE INDEX ON COLLECTION docs FOR title TYPE text WITH (lowercase = true);
CREATE SHARD KEY 'acme' ON COLLECTION docs WITH (shards_number = 2);
DROP INDEX ON COLLECTION docs FOR title;
DROP COLLECTION docs;
SHOW COLLECTIONS;
SHOW COLLECTION docs;
```

### Point Counting

```sql
-- Count with optional filter
COUNT FROM docs WHERE status = 'active';

-- Count with shard routing
COUNT FROM sec10k WHERE tenant_id = 'honeywell' SHARD 'honeywell';
```

### Point Mutations

```sql
-- Clear payload fields from points
CLEAR PAYLOAD FROM docs WHERE status = 'archived';

-- Delete specific named vectors from points
DELETE VECTOR colbert FROM docs WHERE id = 42;

-- Delete multiple vectors at once
DELETE VECTOR dense, sparse FROM docs WHERE status = 'deprecated';
```

### Supported field index types

| TYPE | Index variants |
|------|----------------|
| `keyword` | `is_tenant`, `on_disk`, `enable_hnsw` |
| `integer` | `lookup`, `range`, `is_principal`, `on_disk`, `enable_hnsw` |
| `float` | `on_disk`, `is_principal`, `enable_hnsw` |
| `geo` | `on_disk`, `enable_hnsw` |
| `text` | `tokenizer` (word/prefix/whitespace/multilingual), `lowercase`, `min_token_len`, `max_token_len`, `on_disk`, `stopwords`, `phrase_matching` |
| `bool` | `on_disk`, `enable_hnsw` |
| `datetime` | `on_disk`, `is_principal`, `enable_hnsw` |
| `uuid` | `is_tenant`, `on_disk`, `enable_hnsw` |
