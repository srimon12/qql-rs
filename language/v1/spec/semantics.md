# QQL 1.0 semantics

This document defines the meaning and validation rules of programs accepted by
the canonical [`grammar.pest`](../grammar.pest). The fixture suite is normative: valid fixtures
must parse and plan, invalid cases must fail with their declared code, and
canonical AST output must match `fixtures/expected`.

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
USING <name> [AS DENSE | AS SPARSE]
```

The name answers “which vector?” and the optional role answers “what kind of
embedding/query input?”. They are independent.

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

`RERANK` always requires a dense target. `HYBRID` resolves its dense and sparse
names independently; each omitted name requires exactly one named candidate of
the corresponding role. Mixed dense and sparse structural inputs in one query
expression are invalid.

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
- Each `EMBED field INTO name` directive targets exactly the named vector;
  `USING SPARSE` selects sparse embedding, while the default is dense.
- An UPSERT with no explicit `ON FIELD` or `EMBED` directive infers text from the payload using the deterministic priority order. If no matching text payload field exists, resolution fails with an error (`QQL-EMBEDDING`).

For a missing collection, implicit text ingestion creates the conventional
`dense` + `sparse` topology. Explicit creation/embedding names are preserved.

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
| MMR | `DIVERSITY` is finite and in `[0,1]`; `CANDIDATES` is positive. |
| hybrid | Expands to dense and sparse prefetches fused by RRF (default) or DBSF. |
| rerank | Requires `USING`, a model, and non-empty `PREFETCH`. |

`LIMIT`, group size, `hnsw_ef`, and `rrf_k` are positive integers. `OFFSET`
and `VALUES_COUNT` are non-negative. Score thresholds are finite.

A group lookup names a collection only. A prefetch lookup may additionally
name a vector because it changes the lookup input for that prefetch.

### 4.1 Selectors

`WITH PAYLOAD true|false` selects all or no payload. `INCLUDE (...)` and
`EXCLUDE (...)` preserve the listed field order. `WITH VECTOR` without a value
means all vectors; it also accepts `true`, `false`, or a non-empty name list.

### 4.2 Search parameters

`PARAMS` accepts:

| Key | Type/rule |
|---|---|
| `hnsw_ef` | positive integer |
| `exact`, `acorn`, `indexed_only` | boolean |
| `rrf_k` | positive integer |
| `rrf_weights` | list of numbers |
| `quantization` | object containing `ignore`/`rescore` booleans and positive `oversampling` |

For RRF, `rrf_weights` length must equal prefetch count. RRF-only parameters
are invalid on non-RRF expressions.

## 5. Filters and formulas

Filter precedence, highest to lowest, is primary/predicate, recursive `NOT`,
`AND`, then `OR`. Comparison against `id` only permits `=` and `IN`, and every
value must be a valid point ID. `IN` and `MATCH ANY` lists are non-empty.

`!=`, `NOT IN`, `IS NOT NULL`, and `IS NOT EMPTY` normalize to a `Not` node
around the corresponding positive node. Geo validation requires latitude in
`[-90,90]`, longitude in `[-180,180]`, positive radius, and at least three
points per polygon ring.

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
| `HNSW` | `m`, `ef_construct`, `full_scan_threshold`, `max_indexing_threads`, `on_disk`, `payload_m`, `inline_storage` |
| `VECTOR` | `on_disk` |
| `OPTIMIZERS` | `deleted_threshold`, `vacuum_min_vector_number`, `default_segment_number`, `max_segment_size`, `memmap_threshold`, `indexing_threshold`, `flush_interval_sec`, `max_optimization_threads`, `prevent_unoptimized` |
| `PARAMS` | `replication_factor`, `write_consistency_factor`, `read_fan_out_factor`, `read_fan_out_delay_ms`, `on_disk_payload`, `shard_number`, `sharding_method`, `shard_keys` |
| `QUANTIZATION` | `type`, `disabled`, `always_ram`, `quantile`, `bits`, `compression`, `encoding`, `query_encoding` |
| sparse `INDEX`/`SPARSE` | `modifier`, `full_scan_threshold`, `on_disk`, `datatype` |
| `MULTIVECTOR` | `comparator` (`max_sim`) |

`read_fan_out_factor` and `read_fan_out_delay_ms` are ALTER-only. Quantization
type is `scalar`, `binary`, `product`, or `turbo`; `disabled = true` is an
ALTER form.

Index options are limited to boolean `is_tenant`, `on_disk`, `enable_hnsw`,
`lowercase`, `ascii_folding`, `phrase_matching`, `lookup`, `range`, and
`is_principal`; non-negative `min_token_len`/`max_token_len`; string
`tokenizer`; and string-list `stopwords`.

## 7. Canonical AST (`qql.ast/v1`)

Every expected file has this envelope:

```json
{
  "schema": "qql.ast/v1",
  "statements": [
    "ShowCollections"
  ]
}
```

The generated files, not a Rust type layout, are the normative schema. The
reference adapter currently maps qql-core values as follows:

- enums use externally tagged JSON (`{"Query": {...}}`);
- unit variants serialize as strings or null-valued tags according to the
  fixture;
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
| `QQL-LEX-STRING` | Unterminated string |
| `QQL-PARSE-STATEMENT` | Unknown/legacy statement keyword |
| `QQL-PARSE-EMPTY-STATEMENT` | Empty script element |
| `QQL-PARSE-EXPECTED` | Required token missing |
| `QQL-PARSE-QUERY-INPUT` | Invalid query input form |
| `QQL-PARSE-CLAUSE-ORDER` | Duplicate or out-of-order query clause |
| `QQL-PARSE-VECTOR-KIND` | `AS` is not DENSE or SPARSE |
| `QQL-PARSE-DUPLICATE-CTE` | Duplicate CTE name |
| `QQL-PARSE-DUPLICATE-KEY` | Duplicate object/config key |
| `QQL-PARSE-POSITIVE-INTEGER` | Value must be positive |
| `QQL-PARSE-NONNEGATIVE-INTEGER` | Value must be non-negative |
| `QQL-PARSE-SYNTAX` | Production-specific syntax/range failure |
| `QQL-VALIDATION-FROM` | Top-level query lacks FROM |
| `QQL-VALIDATION-PREFETCH-CTE` | Unknown CTE reference |
| `QQL-VALIDATION-FUSION-PREFETCH` | Fusion lacks prefetch |
| `QQL-VALIDATION-RERANK-PREFETCH` | Rerank lacks prefetch |
| `QQL-VALIDATION-POINTS-CLAUSE` | Unsupported clause on POINTS |
| `QQL-VALIDATION-UPSERT-ID` | UPSERT row lacks valid ID |
| `QQL-VALIDATION-MMR` | MMR diversity is invalid |
| `QQL-PLAN-VECTOR-KIND` | Structural input and declared role disagree |
| `QQL-MISSING-USING` | Schema inference is ambiguous |
| `QQL-UNKNOWN-VECTOR` | Explicit name does not exist |
| `QQL-VECTOR-KIND` | Schema role conflicts with requested role |
| `QQL-EMBEDDING-TOPOLOGY` | UPSERT embedding inference is ambiguous |
| `QQL-EMBEDDING-TARGET` | UPSERT target is absent/wrong-role |

New error codes may refine cases in a v1 minor release. A code already asserted
by a v1 fixture cannot change before v2.

## 9. Extensions

Gateway authentication, policy, rate limiting, MCP, RPC, transport selection,
embedding-provider configuration, edge storage, and SDK bindings are outside
the language. They may not change how a valid QQL program parses or normalizes.

A host may add runtime-only options. Grammar additions require a spec version
change and conformance fixtures under the versioning policy.
