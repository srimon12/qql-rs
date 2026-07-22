# Canonical QQL Grammar

This is the canonical syntax implemented by `qql-core`. QQL follows Qdrant retrieval concepts rather than relational `SELECT` semantics. Keywords are case-insensitive.

## Scripts

```ebnf
script       = [ statement, { ";", statement }, [ ";" ] ] ;
statement    = query | scroll | upsert | update | delete | ddl ;
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
                   [ "LOOKUP", "FROM", collection ] ],
               [ "WITH", "PAYLOAD", payload-selector ],
               [ "WITH", "VECTOR", vector-selector ],
               [ "LIMIT", positive-integer ],
               [ "OFFSET", non-negative-integer ] ;
```

Top-level queries require `FROM`. A CTE may omit it and inherit the outer collection. Clauses occur at most once and only in the order above. Search options use `PARAMS (...)`; generic query `WITH (...)` is invalid.

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

Fusion requires a non-empty `PREFETCH`. Rerank requires an explicit input, `MODEL`, `USING`, and non-empty `PREFETCH`. MMR requires both `DIVERSITY` in `[0, 1]` and positive `CANDIDATES`. Core records hybrid intent but does not invent candidate counts or `LIMIT * 10` behavior.

### Examples

```sql
QUERY TEXT 'vector database' MODEL 'nomic-embed-text'
FROM docs
USING dense
WHERE category = 'database'
PARAMS (hnsw_ef = 128, exact = false)
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
LIMIT 10;
```

## Prefetch

```ebnf
prefetch     = ( cte-name | cte-query ),
               [ "WHERE", filter ],
               [ "SCORE", "THRESHOLD", number ],
               [ "LOOKUP", "FROM", collection, [ "VECTOR", vector-name ] ] ;
```

## Selectors And Params

```ebnf
payload-selector = "true" | "false"
                 | "INCLUDE", name-list
                 | "EXCLUDE", name-list ;
vector-selector  = "true" | "false" | name-list ;
name-list        = "(", name, { ",", name }, ")" ;
search-params    = "(", search-param, { ",", search-param }, ")" ;
search-param     = "hnsw_ef", "=", positive-integer
                 | "exact", "=", boolean
                 | "acorn", "=", boolean
                 | "indexed_only", "=", boolean
                 | "quantization", "=", object ;
```

Keys in payload objects, configuration blocks, formula defaults, and search parameters are unique case-insensitively.

## Point Data

```ebnf
upsert       = "UPSERT", "INTO", collection, "VALUES",
               point-object, { ",", point-object },
               [ embedding-options ], [ embed-directives ],
               [ "SHARD", string ] ;
scroll       = "SCROLL", "FROM", collection,
               [ "WHERE", filter ], [ "AFTER", point-id ],
               [ "SHARD", string ],
               "LIMIT", positive-integer ;
delete       = "DELETE", "FROM", collection, "WHERE", filter,
               [ "SHARD", string ] ;
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

`SHARD '<key>'` on UPSERT, SCROLL, or DELETE routes the operation to a specific shard group.

## DDL

Collection creation/alteration/drop/show and payload index creation:

```ebnf
create-collection = "CREATE", "COLLECTION", name,
                    ( "DENSE", [ "MODEL", string ]
                    | "HYBRID", [ "DENSE", name ], [ "SPARSE", name ]
                    | "RERANK" ),
                    [ "(", vector-def, { ",", vector-def }, ")" ],
                    [ "(", sparse-def, { ",", sparse-def }, ")" ],
                    [ config-blocks ] ;

vector-def    = name, "VECTOR", "(", size, ",", distance, ")" ;
sparse-def    = name, "SPARSE" ;
config-blocks = "WITH", ( "HNSW" | "PARAMS" | "OPTIMIZERS" | "QUANTIZE"
                         | "VECTOR" ), config-block ;
```

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

### DDL Examples

```sql
CREATE COLLECTION docs (
  dense VECTOR(384, COSINE),
  sparse SPARSE,
  colbert VECTOR(128, COSINE) WITH MULTIVECTOR (comparator = 'max_sim')
) WITH HNSW (m = 16, ef_construct = 100);

ALTER COLLECTION docs WITH VECTOR (on_disk = true);
CREATE INDEX ON COLLECTION docs FOR title TYPE text WITH (lowercase = true);
DROP COLLECTION docs;
SHOW COLLECTIONS;
```
