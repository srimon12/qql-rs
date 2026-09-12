# QQL params reference

Placeholders, binding, prepared statements, search params, and inference inputs. One contract across Python, Node, WASM, and Rust.

## Placeholder grammar

```sql
QUERY TEXT :q FROM docs WHERE category = :cat LIMIT :lim;

QUERY TEXT ? FROM medical_records WHERE department = ? LIMIT ?;

QUERY POINT :point_id FROM events SHARD :tenant LIMIT :lim OFFSET :offset;
```

Key decisions:

- Named placeholders use `:name`. Positional placeholders use `?`. Never mix both styles in one statement. Mixed styles fail with `QQL-BIND-MIXED-STYLE`.
- `:name` binds by identifier. `?` binds sequentially one-to-one with the positional list.
- `$` is an identifier character in QQL. `$category` and `$score` are field names. Never use `$name` or `$1` as placeholders.
- Colons in compact dicts are key-value separators, not placeholders. `{a:b}` stays a dict. Write `{key: :val}` to bind a dict value.
- Comments and string literals never substitute. Placeholders inside them stay inert.
- `LIMIT 0` is rejected at parse time. Qdrant requires `limit >= 1`. The failure surfaces at the parse gate, not as a runtime 422.
- Unbound placeholders fail closed with `QQL-BIND-MISSING-PARAM` before any request leaves. Extra positional values raise `QQL-BIND-UNUSED-PARAMS`. Wrong types raise `QQL-BIND-TYPE-MISMATCH`. Re-binding an already-bound `Stmt` raises `QQL-BIND-ALREADY-BOUND`.

## Named and positional binding

Python:

```python
report = client.execute(
    "QUERY TEXT :query FROM docs WHERE category = :cat LIMIT :limit",
    params={"query": "machine learning", "cat": "tech", "limit": 10},
)
report = client.execute(
    "QUERY TEXT ? FROM docs WHERE category = ? LIMIT ?",
    params=["machine learning", "tech", 10],
)
```

Node:

```js
await client.execute(
  "QUERY TEXT :query FROM docs WHERE category = :cat LIMIT :limit",
  { params: { query: "machine learning", cat: "tech", limit: 10 } }
);
await client.execute(
  "QUERY TEXT ? FROM docs WHERE category = ? LIMIT ?",
  { params: ["machine learning", "tech", 10] }
);
```

WASM takes a JS object or array, never a JSON string. Rust keeps typed twins `bind_named` and `bind_positional`, plus `execute_with_params` and `execute_with_positional_params`.

## Nested and dotted params

```sql
QUERY 'coffee' FROM venues
WHERE location GEO_RADIUS { center: {lat: :loc.lat, lon: :loc.lon}, radius: :rad }
LIMIT 5;
```

Key decisions:

- Dotted paths bind nested dicts. `{"loc": {"lat": 1.0, "lon": 2.0}}` binds `:loc.lat` and `:loc.lon`. Flat dotted keys work identically.
- Numpy arrays and typed arrays bind directly. 1-D contiguous float buffers bind as packed `f32` with one copy. Plain `number[]` of 32 plus elements packs as `F32Array` in Node and Python. Shorter lists keep exact list semantics. Payload values never repack.
- Matrix params bind ColBERT multi-vectors on the `Stmt` path. Whole-point upsert params bind point dicts or lists of them. `UPSERT INTO c VALUES :rows` splices N points as data, not text.

## Statement-scoped batch params

```python
report = client.execute(
    ["QUERY TEXT :q FROM docs LIMIT 5", "QUERY TEXT :q FROM articles LIMIT 10"],
    params=[{"q": "quantum"}, {"q": "relativity"}],
)
```

Key decisions:

- Batch param lists bind per statement. Length must match statement count exactly. Mismatch fails with `QQL-BIND-BATCH-LENGTH`.
- A scalar list like `[1, 2]` is a shared positional list, never per-statement. Single-container rule: with one statement, `params=[[1, 2]]` unrolls as a one-element container list binding `[1, 2]` positionally.
- Node uses the same shape under `{ params: [...] }`. WASM uses `{ params: [...] }`. Rust binds each statement through the same `bind_stmt` entry point.

## Prepared statements

Problem: parse once, bind and execute many times without re-lexing.

Python:

```python
stmt = pyqql.parse("QUERY TEXT :query FROM docs WHERE category = :cat LIMIT :limit")[0]
report = client.execute(stmt, params={"query": "neural nets", "cat": "ai", "limit": 5})
route = stmt.compile_route(params={"query": "neural nets", "cat": "ai", "limit": 5})
bound = stmt.bind({"query": "search", "cat": "tech", "limit": 5})
```

Node:

```js
const [stmt] = parse("QUERY TEXT :query FROM docs WHERE category = :cat LIMIT :lim");
const bound = stmt.bind({ query: "vector databases", cat: "tech", lim: 10 });
const route = stmt.compileRoute({ query: "vector databases", cat: "tech", lim: 10 });
const res = await client.execute(stmt, { params: { query: "neural nets", cat: "ai", limit: 5 } });
```

WASM uses `new Stmt(...)` plus `stmt.bind(...)` plus `stmt.compileRoute(...)` plus `client.executeStmt(stmt)`. Rust uses `Parser::parse` plus `bind_stmt` plus `compile_statement` and `try_route`.

Key decisions:

- `bind` returns a new bound statement. The template stays reusable.
- `compile_route` lowers to `{method, path, payload}` without I/O. Useful for proxies and offline validation.
- `explain` renders the plan tree without execution. `explain_analyze` runs once and returns plan plus measured timings.

## Vector truncation for logs

```python
print(bind("QUERY :vec FROM docs LIMIT 5", {"vec": [0.1] * 384}, truncate_vectors=True))
```

Renders `QUERY [0.10, 0.10, ... (384 dims)] FROM docs LIMIT 5`. Node uses `{ truncateVectors: true }`. WASM uses `{ truncateVectors: true }`. Rust uses `bind_named_readable(source, lookup, max_dims)`. Use previews in logs. Never truncate vectors on the execute path.

## Search params

```sql
QUERY TEXT 'filtered product search' FROM products
  USING dense
  WHERE category = 'electronics' AND in_stock = true
  PARAMS (hnsw_ef = 128, acorn = true, max_selectivity = 0.4)
  LIMIT 20;

QUERY TEXT 'incident response' FROM runbooks
  USING dense
  PARAMS (timeout = 30, consistency = majority, hnsw_ef = 64)
  LIMIT 10;

QUERY TEXT 'quantum computing' FROM papers
  USING dense
  PARAMS (quantization = {ignore: false, rescore: true, oversampling: 2.0})
  LIMIT 20;

QUERY TEXT 'hello' FROM docs USING sparse PARAMS (idf = 'global') LIMIT 5;

QUERY TEXT 'hello' FROM docs USING sparse
  WHERE tenant_id = 'acme' SHARD 'acme'
  PARAMS (idf = WHERE tenant_id = 'acme')
  LIMIT 5;
```

Key decisions:

- `hnsw_ef` sizes the HNSW candidate list. `exact = true` forces brute force. `indexed_only = true` restricts to indexed points.
- `acorn = true` enables filter-aware search. `max_selectivity` in `(0, 1]` requires `acorn = true`. Supported on remote Qdrant and edge 0.8 plus.
- `quantization` overrides per query with `ignore`, `rescore`, `oversampling`.
- `rrf_k` and `rrf_weights` tune fusion. They live in `PARAMS`, never in `WITH (...)`.
- `timeout` and `consistency` are request-level. REST lowers to query string. gRPC lowers to fields. Edge rejects both with `QQL-EDGE-UNSUPPORTED-TIMEOUT` and `QQL-EDGE-UNSUPPORTED-CONSISTENCY`. Client HTTP timeouts are a separate layer.
- `consistency` accepts `majority`, `quorum`, `all`, or a replica count.
- `idf = 'global'` uses collection-wide sparse stats. `idf = WHERE <filter>` scopes stats to the corpus filter. Write the filter in QQL, never as JSON. `idf` scopes statistics. It never replaces `inject_filter` for isolation.

## Shard binding

```sql
QUERY TEXT :q FROM sec10k USING sparse
WHERE tenant_id = :tenant
SHARD :tenant
PARAMS (idf = WHERE tenant_id = :tenant)
LIMIT 10;
```

Key decisions:

- `SHARD :tenant` binds like any placeholder. Strings become keyword keys. Non-negative integers become numeric keys. Other types fail with `QQL-BIND-TYPE-MISMATCH`.
- Numeric and keyword keys hash differently. `SHARD 101` and `SHARD '101'` reach different partitions.
- Host post-parse routing sets the same field. Python uses `stmt.shard_key`. Node and WASM use `stmt.shardKey`. Rust uses `stmt.set_shard_key(...)`.

## Inference inputs

```sql
QUERY TEXT 'hi' MODEL 'm' OPTIONS {temperature: 0.5} FROM docs USING dense LIMIT 1;

QUERY IMAGE 'https://x/y.jpg' MODEL 'clip' OPTIONS {size: 512} FROM docs USING img LIMIT 1;

QUERY OBJECT {a: 1} MODEL 'm' OPTIONS {k: true} FROM docs USING dense LIMIT 5;

UPSERT INTO docs VALUES {id: 1, vector: {text: 'hello', model: 'm'}};
```

Key decisions:

- `MODEL '...'` selects the inference model for this input. `OPTIONS {...}` passes an opaque dict. `OBJECT {...}` is the custom inference payload.
- `OPTIONS` needs no `MODEL`. `OBJECT` needs no `MODEL`. The wire serializes an empty model placeholder when omitted.
- Parenthesized `OBJECT ({...})` parses but formats to the bare object.
- Per-point dicts reuse the same shapes inside `vector`. Mixed inference and sparse keys fail closed.
