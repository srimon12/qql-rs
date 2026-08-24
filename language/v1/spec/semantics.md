# QQL 1.x semantics

This document defines the meaning and validation rules of programs accepted by
the canonical [`grammar.pest`](../grammar.pest) for the current language version
**1.4** (see [`versioning.md`](versioning.md)). The fixture suite is normative:
valid fixtures must parse and plan, invalid cases must fail with their declared
code, and canonical AST output must match `fixtures/expected`.

## 1. Lexical model

Keywords are ASCII case-insensitive. Identifiers preserve their source spelling
and are case-sensitive when used as collection, field, CTE, or vector names.
Object and config keys are unique under ASCII case-insensitive comparison.

An identifier begins with an ASCII letter, `_`, or `$`; later characters may
also be digits. A dotted path (`metadata.author`) or array path
(`items[].price`) is emitted as one identifier. A quoted string is accepted
where the grammar uses `name`, allowing keywords and punctuation in names.

Strings may use single (`'...'`) or double (`"..."`) quotes, raw quotes (`r'...'`, `r"..."`), triple-quoted multiline delimiters (`'''...'''`, `"""..."""`), or backtick delimiters (`` `...` ``). The escapes `\\`, `\'`, `\"`, `\n`, `\r`, `\t`, and `\$` are supported in standard single/double quoted strings. A doubled single quote inside a single-quoted string (`'it''s'`) represents one quote. Raw and triple-quoted strings preserve literal contents, `$PARAM` references, and internal quotes verbatim. Strings are UTF-8.

Integers are signed decimal values. Floats use a decimal fraction and/or an
`e`/`E` exponent. `≥`, `≤`, and `≠` normalize to `>=`, `<=`, and `!=`.
`--` starts a line comment; block comments do not exist.

A script contains at most 256 statements. Statements are separated by `;`; one
trailing separator is allowed. Leading and repeated separators are invalid.

Spans in reference errors are zero-based UTF-8 byte ranges `[start, end)`.

## 2. Values and identity

QQL values are `string`, signed `integer`, `float`, `boolean`, `null`, ordered
object entries, and ordered lists. Point IDs are either unsigned integers or
strings. Each UPSERT point must contain exactly one case-insensitive `id` key.

Vector values have three structural kinds:

| Kind | Form |
|---|---|
| dense | `[0.1, 0.2, 0.3]` |
| sparse | `{indices: [1, 9], values: [0.4, 0.7]}` |
| multidense | `[[0.1, 0.2], [0.3, 0.4]]` |

Dense and multidense components are finite binary32 values. Sparse indices are
unsigned integers; sparse values are finite binary32 values; `indices` and
`values` have equal length. A point may contain one unnamed vector or an object
of arbitrary named vectors.

## 3. Named vectors and roles

Named vectors are first-class language values. The strings `dense`, `sparse`,
and `colbert` are conventional defaults used when a new topology is
materialized; they are not reserved and receive no selection priority.

A query target is represented as:

```text
USING <name> [AS DENSE | AS SPARSE | AS MULTI | AS MULTIVECTOR]

Query inputs:

- `TEXT '…' [MODEL '…']` — embed text (dense / sparse / multi by USING role)
- `IMAGE 'path-or-url' [MODEL '…']` — embed image via host vision model → **dense**
  (CLIP vision; not multivector)
- `VECTOR …` / `POINT …` — no embedding

Rerank forms:

- `RERANK … MODEL '…' USING <dense|multi> PREFETCH (…)` — late-interaction MaxSim
  (query embed against candidates in Qdrant).
- `CROSS RERANK TEXT '…' MODEL '…' [ON FIELD f] PREFETCH (…)` — cross-encoder
  pair scores on payload field `f` (default `text`), reordered client-side.
  Requires host `rerank_pairs`. No `USING` vector.
```

The name answers “which vector?” and the optional role answers “what kind of
embedding/query input?”. They are independent.

`AS MULTI` / `AS MULTIVECTOR` mark a **dense multivector** target (ColBERT-style
late interaction). Multivector is not a third kind beside dense/sparse: the
role remains dense and the query input shape is multi-dense
(`[[f32, …], …]`). Collection schema may also mark named dense vectors as
multivector when they carry `multivector_config` (e.g. `max_sim`).

### 3.1 Query target resolution

Resolution occurs before text embedding:

1. `USING name AS kind` declares the role. A structurally dense vector input
   cannot be declared sparse, and a sparse vector input cannot be declared
   dense (`QQL-PLAN-VECTOR-KIND`).
2. `USING name` resolves the role from the collection schema and verifies that
   the name exists.
3. If `USING` is absent and the input has a structural role, the schema must
   contain exactly one vector of that role.
4. If `USING` is absent and the input is text or a point ID, the schema must
   contain exactly one vector in total.
5. Otherwise inference is ambiguous and fails with `QQL-MISSING-USING`.

`RERANK` always requires a dense target (single-vector or multivector). When
the target is multivector, text is embedded via multi-vector embedding into
`MultiDense`. `HYBRID` resolves its dense and sparse names independently; each
omitted name requires exactly one named candidate of the corresponding role.
Mixed dense and sparse structural inputs in one query expression are invalid.

The parse-time canonical AST records only source information. Therefore an
explicit `AS` appears as `kind: "Dense"` or `"Sparse"`, an untyped explicit
target has `kind: null`, and an omitted target remains `null`. Schema-resolved
roles are execution preparation state, not canonical parse AST.

### 3.2 UPSERT embedding resolution

For an existing collection:

- `USING DENSE`, `USING SPARSE`, and each side of `USING HYBRID` may omit
  `VECTOR`; omission succeeds only when exactly one vector of that role exists.
- `VECTOR name` or `INTO name` names a destination vector of the requested role.
- `ON FIELD field` explicitly names the target payload text field to embed.
- Multiple comma-separated embedding specs may be specified (e.g. `USING DENSE MODEL 'm1' ON FIELD text INTO dense, DENSE MODEL 'm2' ON FIELD title INTO title_vec`).
- When `ON FIELD` is omitted, default payload text field resolution follows a deterministic priority order: `text` > `body` > `content` > `title` > `description` > `name` > `summary` > `document`.
- Each `EMBED field INTO name` directive targets exactly the named vector and
  selects one embedding kind:
  - `USING DENSE` (also the default when no `USING` kind is written, including
    a bare `USING MODEL`) — text → dense vector;
  - `USING SPARSE` — text → sparse vector;
  - `USING MULTI` / `USING MULTIVECTOR` — synonyms; text → multi-dense bag
    (`[[f32, …], …]`, ColBERT-style late interaction). The target must be a
    multivector slot; when the collection declares no multivector slots, a
    dense slot is accepted as the target (offline fallback). An empty or
    mis-sized bag fails with `QQL-EMBEDDING-MULTI`;
  - `USING IMAGE` — the payload field holds an image path or URL; image →
    dense vector via the host vision model (CLIP; not multivector). The target
    must be a dense slot. A mis-sized image batch fails with
    `QQL-EMBEDDING-IMAGE`.
  Each kind accepts an optional `MODEL '…'`; an omitted model defaults to the
  embedder's `default` model.
- The `USING` clause accepts the same kinds through `single_embedding_spec`:
  `USING MULTI` / `USING MULTIVECTOR` (synonyms) embed text into a multivector
  slot named `colbert` by default (or `VECTOR name` / `INTO name`); `USING
  IMAGE` embeds the image source field into a dense slot named `image` by
  default (or an explicit name). When `ON FIELD` is omitted, `USING MULTI` /
  `USING MULTIVECTOR` resolve text fields with the text priority order above,
  and `USING IMAGE` resolves image-source fields (`image` > `image_path` >
  `image_url` > `photo` > `picture` > `img` > `path` > `url`).
- An UPSERT with no explicit `ON FIELD` or `EMBED` directive infers text from the payload using the deterministic priority order. If no matching text payload field exists, resolution fails with an error (`QQL-EMBEDDING`).

For a missing collection, implicit text ingestion creates the conventional
`dense` + `sparse` topology. `USING MULTI` / `USING MULTIVECTOR` / `USING
IMAGE` alone do not auto-create a collection; the collection and its
multivector/dense slots must already exist. Explicit creation/embedding names
are preserved.

Vector spelling never determines vector role.

## 4. Query semantics

Top-level `QUERY` requires `FROM`. A CTE may omit `FROM` and inherit the outer
collection. CTE names are unique and references are ASCII case-insensitive.
Every prefetch CTE reference must resolve.

Query clauses occur at most once and in the grammar order. The expression owns
its `USING` and `PREFETCH` pipeline in the canonical AST.

| Expression | Rules |
|---|---|
| nearest | A bare string is equivalent to `TEXT string`; `POINT id` means similarity by point. |
| points | Direct retrieval; only `SHARD`, payload/vector selectors, and no paging/filter/scoring clauses are allowed. |
| recommend | `POSITIVE` is non-empty; `NEGATIVE` is optional; strategy is one of the three grammar values. |
| context/discover | Every positive/negative/target item is a full `query-input`; point IDs require `POINT`. |
| order/sample | Do not accept `USING` or `PREFETCH`. |
| fusion | Requires at least one `PREFETCH`. |
| formula | May score a prefetch or payload-derived expression. |
| relevance feedback | Requires non-empty feedback and `NAIVE(a,b,c)`. |
| MMR | `DIVERSITY` is finite and in `[0,1]`; `CANDIDATES` is positive. MMR supports both dense and sparse vector targets. |
| hybrid | Expands to dense and sparse prefetches fused by RRF (default) or DBSF. Surface forms: front-form `QUERY HYBRID TEXT …` and tail-form `QUERY TEXT … USING HYBRID …` lower to the same `Hybrid` AST. `USING HYBRID` requires a text nearest expression (no MMR, no non-text inputs). Omitted dense/sparse names resolve from schema (exactly one of each role). |
| rerank | Requires `USING`, a model, and non-empty `PREFETCH`. |

`LIMIT`, group size, `hnsw_ef`, and `rrf_k` are positive integers. `OFFSET`
and `VALUES_COUNT` are non-negative. Score thresholds are finite.

`GROUP BY` supports `OFFSET` (maps to Qdrant's `group_offset` field on query/groups requests).

A group lookup names a collection only. A prefetch lookup may additionally
name a vector because it changes the lookup input for that prefetch.

### 4.1 Selectors

`WITH PAYLOAD true|false` selects all or no payload. `INCLUDE (...)` and
`EXCLUDE (...)` preserve the listed field order. `WITH VECTOR` without a value
means all vectors; it also accepts `true`, `false`, or a non-empty name list.

### 4.2 Search parameters

Request-level options (not body `SearchParams` on the wire):

| Key | Rule | Wire (OpenAPI / proto) |
|---|---|---|
| `timeout` | Positive integer seconds | REST query `timeout`; gRPC `timeout` |
| `consistency` | Non-negative integer factor, or `majority` / `quorum` / `all` | REST query `consistency`; gRPC `read_consistency` |

Body search parameters (OpenAPI `SearchParams`):

`PARAMS` accepts:

| Key | Type/rule |
|---|---|
| `hnsw_ef` | positive integer |
| `exact`, `acorn`, `indexed_only` | boolean |
| `rrf_k` | positive integer |
| `rrf_weights` | list of numbers |
| `quantization` | object containing `ignore`/`rescore` booleans and positive `oversampling` |
| `idf` | `'global'` / bare `global` (collection-wide), or `WHERE <filter>` (corpus is that QQL filter) |

`idf` selects the inverse-document-frequency corpus used for sparse scoring on
this request. `'global'` means the whole collection. A `WHERE` form uses the
same filter grammar as query `WHERE` — isolation stays on the statement
`WHERE` / `inject_filter`; IDF only scopes term statistics. JSON corpus
objects are not accepted. Malformed values fail at parse with
`QQL-VALIDATION-IDF`. An empty lowered corpus fails at plan with
`QQL-PLAN-IDF`.

```sql
QUERY TEXT 'search' MODEL 'e5' FROM docs
PARAMS (idf = 'global')
LIMIT 10;

QUERY TEXT 'search' MODEL 'e5' FROM docs
PARAMS (idf = WHERE status = 'active')
LIMIT 10;

QUERY TEXT 'search' MODEL 'e5' FROM docs
WHERE tenant_id = 'acme'
SHARD 'acme'
PARAMS (idf = WHERE tenant_id = 'acme')
LIMIT 10;
```

For RRF, `rrf_weights` length must equal prefetch count. RRF-only parameters
are invalid on non-RRF expressions.

## 5. Filters and formulas

Filter precedence, highest to lowest, is primary/predicate, recursive `NOT`,
`AND`, then `OR`. Comparison against `id` permits `=`, `IN`, and — via `NOT`
normalization — `!=` and `NOT IN`; every value must be a valid point ID. `IN`
and `MATCH ANY` lists are non-empty.

`!=`, `NOT IN`, `IS NOT NULL`, and `IS NOT EMPTY` normalize to a `Not` node
around the corresponding positive node. Geo validation requires latitude in
`[-90,90]`, longitude in `[-180,180]`, positive radius, and at least three
points per polygon ring.

### 5.1 MATCH PREFIX

`field MATCH PREFIX 'prefix'` is keyword/text prefix matching (OpenAPI
`Match.prefix`). The prefix string is required.

```sql
QUERY TEXT 'search' MODEL 'e5' FROM docs
WHERE title MATCH PREFIX 'Comp'
LIMIT 10;
```

### 5.2 SLICE

`SLICE (total, index)` is a deterministic sampling filter with no field. Both
arguments are non-negative integers; validation requires `total >= 1` and
`index < total` (`QQL-VALIDATION-SLICE` otherwise). It may combine with other
predicates via `AND` / `OR` / `NOT`.

```sql
QUERY TEXT 'search' MODEL 'e5' FROM docs
WHERE SLICE (4, 1) AND status = 'active'
LIMIT 10;
```

Field-schema compatibility (for example, MATCH on a text index) is checked by
the backend at execution time.

Formula precedence is unary `-`, multiplication/division, then addition/
subtraction. Operators are left-associative. Division may carry
`[DEFAULT = number]`. Function names are case-insensitive. Decay `scale` and
`midpoint`/`decay` are numeric constants. `CASE WHEN` uses a QQL filter as its
condition.

## 6. Point operations and DDL

`SCROLL` requires a positive `LIMIT`. `DELETE`, `CLEAR PAYLOAD`, and
`DELETE VECTOR` require `WHERE`. `UPDATE ... SET VECTOR` targets exactly one
point ID; payload updates accept any filter.

Collection modes:

- no mode: dense topology without a model hint;
- `USING [DENSE] MODEL string`: dense topology with dimension inference;
- `USING HYBRID`: conventional hybrid topology;
- `HYBRID [DENSE VECTOR name] [SPARSE VECTOR name]`: hybrid topology with
  arbitrary role names;
- `HYBRID RERANK`: conventional rerank topology.

Explicit dense and sparse vector definitions may coexist. Dense size is
positive and distance is `COSINE`, `DOT`, `EUCLID`, or `MANHATTAN`.

Collection config keys are case-insensitive and unique:

| Block | Accepted keys |
|---|---|
| `HNSW` | `m`, `ef_construct`, `full_scan_threshold`, `max_indexing_threads`, `on_disk`, `payload_m`, `inline_storage`, `memory` |
| `VECTOR` | `on_disk`, `memory`, `datatype` |
| `OPTIMIZERS` | `deleted_threshold`, `vacuum_min_vector_number`, `default_segment_number`, `max_segment_size`, `memmap_threshold`, `indexing_threshold`, `flush_interval_sec`, `max_optimization_threads`, `prevent_unoptimized` |
| `PARAMS` | `replication_factor`, `write_consistency_factor`, `read_fan_out_factor`, `read_fan_out_delay_ms`, `on_disk_payload`, `payload_memory`, `shard_number`, `sharding_method`, `shard_keys` |
| `QUANTIZATION` | `type`, `disabled`, `always_ram`, `quantile`, `bits`, `compression`, `encoding`, `query_encoding`, `memory` |
| sparse `INDEX`/`SPARSE` | `modifier`, `full_scan_threshold`, `on_disk`, `datatype`, `memory` |
| `MULTIVECTOR` | `comparator` (`max_sim`) |

`read_fan_out_factor` and `read_fan_out_delay_ms` are ALTER-only. Quantization
type is `scalar`, `binary`, `product`, or `turbo`; `disabled = true` is an
ALTER form. `sharding_method` accepts the string `'auto'` or `'custom'`;
`shard_keys` is a list of strings.

### 6.1 Memory placement

`memory` controls how a component is held in RAM while data remains on disk
(Qdrant 1.19 `Memory`). Accepted string values (case-insensitive):

| Value | Meaning |
|---|---|
| `'cold'` | Load on demand |
| `'cached'` | Keep hot in cache |
| `'pinned'` | Pin in RAM (not valid for payload) |

Applies on `WITH HNSW (…)`, `WITH VECTOR (…)`, `WITH SPARSE` / sparse `INDEX`,
`WITH QUANTIZATION (…)`, and payload field indexes. Collection
`WITH PARAMS (payload_memory = 'cold' | 'cached')` sets payload storage
placement; `pinned` is rejected for payload (`payload_memory`).

Legacy dual-write: `on_disk` / `on_disk_payload` / `always_ram` remain accepted
and may be emitted alongside `memory` / `payload_memory` for Qdrant 1.19
compatibility. Prefer `memory` / `payload_memory` in new scripts.

```sql
CREATE COLLECTION docs (
  dense VECTOR(384, COSINE) WITH VECTOR (memory = 'cached', datatype = 'turbo4')
) WITH HNSW (memory = 'cold') WITH PARAMS (payload_memory = 'cold');
```

### 6.2 Vector datatype

Dense `WITH VECTOR (datatype = …)` accepts `float32`, `float16`, `uint8`,
`turbo4` (TurboQuant 4-bit), and short aliases `f32`, `f16`, `u8`, `t4`.
Sparse index `datatype` accepts float32 / float16 / uint8 only (not `turbo4`).

### 6.3 Payload indexes

Index options are limited to boolean `is_tenant`, `on_disk`, `enable_hnsw`,
`lowercase`, `ascii_folding`, `phrase_matching`, `lookup`, `range`,
`is_principal`, and `prefix` (keyword prefix index); non-negative
`min_token_len`/`max_token_len`; string `tokenizer`, `stemmer`, and `memory`;
and string-list `stopwords`. The parser accepts these options and forwards them
to the backend's `field_schema`.

```sql
CREATE INDEX ON COLLECTION docs FOR tenant TYPE keyword
  WITH (prefix = true, memory = 'cached', is_tenant = true);
```

### 6.4 Quotas

Cluster-wide resource quotas (REST `GET|PUT /quotas` only; no public gRPC
service; edge rejects with `QQL-EDGE-UNSUPPORTED-QUOTA`):

```sql
SHOW QUOTAS;

SET QUOTA (
  enabled = true,
  max_resident_memory_percent = 80,
  max_disk_usage_percent = 90,
  release_margin_percent = 5
) WAIT true;

SET QUOTA (enabled = false);
SET QUOTA (max_disk_usage_percent = null);
```

`SET QUOTA` is a **full replace** of the cluster config (`PUT /quotas`).
Omitted keys (including `key = null`) are unset in the replacement body, not
merged with the previous limits. Accepted keys:

| Key | Rule |
|---|---|
| `enabled` | boolean |
| `max_resident_memory_percent` | integer in `[1, 100]`, or `null` to clear |
| `max_disk_usage_percent` | integer in `[1, 100]`, or `null` to clear |
| `release_margin_percent` | integer in `[0, 100]`, or `null` to clear |

Optional `WAIT true|false` waits for consensus after the update. Unknown keys
or out-of-range percents fail with `QQL-PLAN-QUOTA`.

## 7. Canonical AST (`qql.ast/v1`)

Every expected file has this envelope:

```json
{
  "schema": "qql.ast/v1",
  "statements": [
    {
      "ShowCollections": {}
    }
  ]
}
```

The generated files, not a Rust type layout, are the normative schema. The
reference adapter currently maps qql-core values as follows:

- enums use externally tagged JSON (`{"Query": {...}}`);
- unit variants serialize as strings when the derived serializer applies
  (e.g. `"SampleRandom"`) or as an empty-object tag for the custom `Stmt`
  serializer (e.g. `{"ShowCollections": {}}`); the `Stmt` deserializer
  accepts both representations, so every statement round-trips;
- optional fields are present as `null`;
- ordered maps/payloads remain arrays of `[key, value]` pairs;
- identifiers and decoded strings preserve spelling;
- integer values remain integers;
- floating values are rounded to six decimal places to remove host binary32
  noise while retaining language-level fixture precision.

JSON object key order is ignored. Array order is significant. Implementations
may use any internal AST but must emit JSON structurally equal to the expected
snapshot after the same number normalization.

Expected files are generated only after every valid statement parses and passes
planner validation. Generation is deterministic:

```bash
cargo run -p qql-conformance -- generate language/v1
```

## 8. Error contract

Errors have `kind`, stable `code`, human-readable `message`, and an optional
byte `span`. Message text is not normative. The exact codes declared by
invalid fixtures are normative for those cases.

| Code | Meaning |
|---|---|
| `QQL-LEX-CHAR` | A character cannot start any token |
| `QQL-LEX-NUMBER` | Malformed numeric literal (a trailing decimal point, or an exponent without digits) |
| `QQL-LEX-STRING` | Unterminated string literal |
| `QQL-PARSE-STATEMENT` | Unknown or legacy statement keyword (`SELECT`, `INSERT`, `BOOST`) |
| `QQL-PARSE-EMPTY-STATEMENT` | Empty script element, such as a leading or repeated separator |
| `QQL-PARSE-EXPECTED` | A required token is missing (for example an unmatched parenthesis) |
| `QQL-PARSE-QUERY-INPUT` | Invalid query input form (a bare number is not a query input) |
| `QQL-PARSE-CLAUSE-ORDER` | Duplicate or out-of-order query clause |
| `QQL-PARSE-VECTOR-KIND` | `AS` is not `DENSE`, `SPARSE`, `MULTI`, or `MULTIVECTOR` |
| `QQL-PARSE-DUPLICATE-CTE` | Duplicate CTE name in one script |
| `QQL-PARSE-DUPLICATE-KEY` | Duplicate object or config key (ASCII case-insensitive) |
| `QQL-PARSE-POSITIVE-INTEGER` | Value must be a positive integer (for example `LIMIT`, `CANDIDATES`, vector size) |
| `QQL-PARSE-NONNEGATIVE-INTEGER` | Value must be non-negative (for example `OFFSET`, `VALUES_COUNT`) |
| `QQL-PARSE-SYNTAX` | Production-specific syntax or range failure |
| `QQL-PARSE-COMPARISON` | Expected a comparison operator |
| `QQL-PARSE-CONTEXT` | `CONTEXT` requires at least one positive/negative pair |
| `QQL-PARSE-COUNT-CONFIG` | `COUNT … WITH (…)` accepts only `exact = true` / `exact = false` |
| `QQL-PARSE-CROSS-RERANK` | `CROSS RERANK` requires `TEXT '…'` or a string query input |
| `QQL-PARSE-EMBED` | `EMBED USING` requires `DENSE`, `SPARSE`, `MULTI`, `IMAGE`, or `MODEL` |
| `QQL-PARSE-EMBEDDING` | Duplicate clause in a HYBRID embedding spec |
| `QQL-PARSE-ESCAPE` | Unterminated escape sequence |
| `QQL-PARSE-FEEDBACK-STRATEGY` | `STRATEGY NAIVE` requires exactly `a`, `b`, `c` in order |
| `QQL-PARSE-FIELD` | Expected a field name |
| `QQL-PARSE-FILTER` | Expected a filter operator (for example `IS` requires `NULL`/`EMPTY`) |
| `QQL-PARSE-FLOAT` | Invalid float literal |
| `QQL-PARSE-IDENTIFIER` | Expected an identifier or quoted name |
| `QQL-PARSE-IN` | `IN` / `NOT IN` requires a non-empty value list |
| `QQL-PARSE-INDEX-TYPE` | Unsupported `CREATE INDEX` field type |
| `QQL-PARSE-INTEGER` | Invalid integer literal |
| `QQL-PARSE-LITERAL` | Expected a scalar literal |
| `QQL-PARSE-MATCH-ANY` | `MATCH ANY` requires a non-empty exact-value list |
| `QQL-PARSE-NUMBER` | Expected a number |
| `QQL-PARSE-OBJECT-KEY` | Expected an object key |
| `QQL-PARSE-PAYLOAD-SELECTOR` | `WITH PAYLOAD` requires `true`, `false`, `INCLUDE (...)`, or `EXCLUDE (...)` |
| `QQL-PARSE-POINT-ID` | A point ID must be an unsigned integer or a string |
| `QQL-PARSE-POINT-IDS` | A point ID list cannot be empty |
| `QQL-PARSE-PREFETCH` | `PREFETCH` cannot be empty |
| `QQL-PARSE-RERANK` | `RERANK` input requires `TEXT '…'`, `VECTOR […]`, or `POINT <id>` |
| `QQL-PARSE-SAMPLE` | `SAMPLE` requires `RANDOM` |
| `QQL-PARSE-SELECTOR` | A selector list cannot be empty |
| `QQL-PARSE-SEPARATOR` | Multiple statements must be separated by a semicolon |
| `QQL-PARSE-SHARD-KEY-CONFIG` | `CREATE SHARD KEY … WITH (…)` accepts only positive-integer `shards_number` / `replication_factor` |
| `QQL-PARSE-STATEMENT-LIMIT` | A script may contain at most 256 statements |
| `QQL-PARSE-TRAILING` | Unexpected trailing token |
| `QQL-PARSE-UPDATE` | Expected `VECTOR` or `PAYLOAD` after `SET` |
| `QQL-PARSE-VALUE` | Unexpected value token |
| `QQL-VALIDATION-FROM` | A top-level query lacks `FROM` |
| `QQL-VALIDATION-PREFETCH-CTE` | A `PREFETCH` name does not resolve to a CTE |
| `QQL-VALIDATION-FUSION-PREFETCH` | `QUERY FUSION` has no `PREFETCH` |
| `QQL-VALIDATION-RERANK-PREFETCH` | `QUERY RERANK` has no `PREFETCH` |
| `QQL-VALIDATION-POINTS-CLAUSE` | `QUERY POINTS` uses a clause it cannot accept |
| `QQL-VALIDATION-UPSERT-ID` | An UPSERT point lacks a valid `id` key |
| `QQL-VALIDATION-MMR` | MMR `DIVERSITY` is outside `[0, 1]` or not finite |
| `QQL-VALIDATION-HYBRID` | Invalid `USING HYBRID` / `QUERY HYBRID` combination |
| `QQL-VALIDATION-FILTER-INJECT` | `inject_filter` does not apply to this statement type |
| `QQL-VALIDATION-ID-PREDICATE` | A point ID predicate uses an operator other than `=`, `!=`, `IN`, or `NOT IN` |
| `QQL-VALIDATION-POINT-ID` | A value used as a point ID is neither an unsigned integer nor a string |
| `QQL-VALIDATION-ACORN-SELECTIVITY` | `max_selectivity` requires `PARAMS (acorn = true, …)` |
| `QQL-VALIDATION-CONFIG` | Invalid collection configuration block |
| `QQL-VALIDATION-CONSISTENCY` | `consistency` must be a non-negative integer factor or `majority` / `quorum` / `all` |
| `QQL-VALIDATION-CREATE-MODEL` | `CREATE COLLECTION … HYBRID` rejects a single dense `MODEL` |
| `QQL-VALIDATION-CROSS-RERANK-PREFETCH` | `CROSS RERANK` requires `PREFETCH` |
| `QQL-VALIDATION-FUSION` | The fusion method must be `RRF` or `DBSF` |
| `QQL-VALIDATION-GEO` | Invalid geo coordinates, radius, or polygon ring |
| `QQL-VALIDATION-LIMIT-OVERFLOW` | `LIMIT` + `OFFSET` (or hybrid candidate scaling) overflows `u64` |
| `QQL-VALIDATION-PREFETCH` | This query expression does not accept `PREFETCH` |
| `QQL-VALIDATION-RECOMMEND-STRATEGY` | Unknown `RECOMMEND STRATEGY` |
| `QQL-VALIDATION-RERANK-USING` | `RERANK` requires `USING <vector>` |
| `QQL-VALIDATION-SCORE` | A score threshold must be finite |
| `QQL-VALIDATION-SEARCH-PARAM` | Unknown search parameter |
| `QQL-VALIDATION-USING` | This query expression does not accept `USING` |
| `QQL-VALIDATION-VECTOR` | Invalid vector value |
| `QQL-PLAN-VECTOR-KIND` | Structural vector input and the declared `AS` role disagree |
| `QQL-MISSING-USING` | Schema inference is ambiguous; add `USING <vector>` |
| `QQL-UNKNOWN-VECTOR` | The explicit vector name does not exist in the collection |
| `QQL-VECTOR-KIND` | The schema role conflicts with the requested role, or the kind is unresolved before embedding |
| `QQL-PLAN-COLLECTION` | A query collection name must not be empty |
| `QQL-PLAN-CROSS-RERANK-CANDIDATE` | A `CROSS RERANK` prefetch must plan as a search query |
| `QQL-PLAN-CROSS-RERANK-CTE` | `PREFETCH` references an unknown CTE |
| `QQL-PLAN-CROSS-RERANK-MODEL` | `CROSS RERANK MODEL` must not be empty |
| `QQL-PLAN-CROSS-RERANK-PREFETCH` | `CROSS RERANK` requires at least one `PREFETCH` |
| `QQL-PLAN-CROSS-RERANK-QUERY` | `CROSS RERANK` query text must not be empty |
| `QQL-PLAN-FUSION-PREFETCH` | `FUSION` requires at least one prefetch |
| `QQL-PLAN-PREFETCH-CTE` | `PREFETCH` references an unknown CTE |
| `QQL-PLAN-PREFETCH-GROUP` | `GROUP BY` is not supported inside a `PREFETCH` source |
| `QQL-PLAN-RERANK-PREFETCH` | `RERANK` requires at least one `PREFETCH` |
| `QQL-PLAN-RERANK-USING` | `RERANK` requires a non-empty `USING` vector name |
| `QQL-PLAN-RRF-PARAMS` | `rrf_k` and `rrf_weights` are valid only with `RRF` fusion |
| `QQL-PLAN-RRF-WEIGHTS` | `rrf_weights` length must equal the prefetch count |
| `QQL-PLAN-UNSUPPORTED-PREFETCH` | `POINTS` / `CROSS RERANK` are not supported inside `PREFETCH` |
| `QQL-REST-CLIENT-SIDE` | The operation is executed client-side and has no single Qdrant REST route |
| `QQL-BACKEND` | Generic backend or transport failure |
| `QQL-JSON-NONFINITE` | A non-finite float cannot be serialized to JSON |
| `QQL-JSON-NUMBER` | A value cannot be represented as a JSON number |
| `QQL-EMBEDDING-TOPOLOGY` | UPSERT embedding inference is ambiguous across the collection topology |
| `QQL-EMBEDDING-TARGET` | The UPSERT embedding target is absent or has the wrong role |
| `QQL-EMBEDDING-MULTI` | Multi-vector embedding returned an empty or mis-sized bag for the requested text batch |
| `QQL-EMBEDDING-IMAGE` | Image embedding returned an empty or mis-sized batch for the requested image sources |
| `QQL-EDGE-UNSUPPORTED-GROUP-BY` | `GROUP BY` / query groups are not available offline |
| `QQL-EDGE-UNSUPPORTED-SHARD` | `SHARD` routing or collection sharding options are not available offline |
| `QQL-EDGE-UNSUPPORTED-SHARD-KEY` | `CREATE` / `DROP SHARD KEY` are not available offline |
| `QQL-EDGE-UNSUPPORTED-ALTER` | `ALTER COLLECTION` is not available offline |
| `QQL-EDGE-UNSUPPORTED-COLLECTION-PARAMS` | Collection `WITH PARAMS` is not available offline |
| `QQL-EDGE-UNSUPPORTED-ACORN` | `PARAMS (acorn = ...)` is not available offline |
| `QQL-EDGE-UNSUPPORTED-TIMEOUT` | `PARAMS (timeout = ...)` is not available offline |
| `QQL-EDGE-UNSUPPORTED-CONSISTENCY` | `PARAMS (consistency = ...)` is not available offline |
| `QQL-EDGE-UNSUPPORTED-QUOTA` | `SHOW QUOTAS` / `SET QUOTA` require cluster REST `/quotas` |
| `QQL-EDGE-UNSUPPORTED-RECOMMEND-STRATEGY` | `RECOMMEND STRATEGY average_vector`; offline supports `best_score` and `sum_scores` only |
| `QQL-EDGE-UNSUPPORTED-POINT-REF` | Point-ID query inputs need materialized vectors offline |
| `QQL-EDGE-UNSUPPORTED-FIELD-TYPE` | The index field type is not available offline |
| `QQL-EDGE-UNSUPPORTED-ROUTE` | The planned operation has no edge route implementation (defensive fallback) |
| `QQL-PLAN-QUOTA` | Invalid `SET QUOTA` key or out-of-range percent |
| `QQL-PLAN-IDF` | `PARAMS (idf = WHERE …)` lowered to an empty Qdrant filter |
| `QQL-GRPC-QUOTA` | Quotas have no public gRPC service; use REST |
| `QQL-VALIDATION-SLICE` | `SLICE (total, index)` with `total < 1` or `index >= total` |
| `QQL-VALIDATION-IDF` | Malformed `idf` search param at parse time |
| `QQL-EDGE-INVALID-POINT-ID` | Offline point IDs accept unsigned integers or UUIDs only |

New error codes may refine cases in a v1 minor release. A code already asserted
by a v1 fixture cannot change before v2.

## 9. Extensions

Gateway authentication, policy, rate limiting, MCP, RPC, transport selection,
embedding-provider configuration, edge storage, and SDK bindings are outside
the language. They may not change how a valid QQL program parses or normalizes.

A host may add runtime-only options. Grammar additions require a spec version
change and conformance fixtures under the versioning policy.
