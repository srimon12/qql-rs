# QQL read reference

`SCROLL`, `COUNT`, `FACET`, `GROUP BY` paging, projections, and `BATCH`. Query search lives in `qql-query.md`. Filters live in `qql-filters.md`.

## Scroll points

```sql
SCROLL FROM docs LIMIT 10;

SCROLL FROM docs WHERE status = 'active' AFTER 7 LIMIT 10;

SCROLL FROM docs ORDER BY created_at DESC LIMIT 10;

SCROLL FROM docs ORDER BY score DESC START FROM 100 LIMIT 10;

SCROLL FROM docs
  WHERE status = 'active'
  AFTER 7
  ORDER BY created_at DESC START FROM '2024-01-01T00:00:00Z'
  SHARD 'acme'
  WITH PAYLOAD INCLUDE (title)
  WITH VECTOR (dense)
  LIMIT 10;
```

Key decisions:

- `AFTER <id>` resumes cursor iteration after a point ID.
- `ORDER BY <key> ASC|DESC` sorts by payload value through the index scan engine.
- `START FROM <value>` resumes ordering from a payload value. Accepts integer, float, datetime string, or placeholder.
- Clause order is `AFTER`, `ORDER BY`, `SHARD`, `WITH PAYLOAD`, `WITH VECTOR`, `LIMIT`.
- `WITH PAYLOAD` accepts `false`, `INCLUDE (...)`, or `EXCLUDE (...)`. `WITH VECTOR` selects returned vectors.
- Node SDK offers `scrollCursor` and `scrollStream` over this path with backpressure. Python pages with explicit `AFTER` loops.

## Count points

```sql
COUNT FROM docs WHERE status = 'active';

COUNT FROM docs WITH (exact = true);

COUNT FROM sec10k WHERE tenant_id = 'honeywell' SHARD 'honeywell' WITH (exact = true);
```

Key decisions:

- Default count may be approximate on large collections. `WITH (exact = true)` forces exact counting.
- `SHARD` routes the count to one partition on custom-sharded collections.
- SDK accessors: `report.count()` returns the integer. Batch counts index per statement.

## Facet aggregations

```sql
FACET category FROM docs LIMIT 10;

FACET room_type FROM stays WHERE price < 150 LIMIT 5 EXACT true;

FACET tag FROM products WHERE in_stock = true SHARD 'tenant_1' LIMIT 10;
```

Key decisions:

- `FACET <field> FROM <collection>` aggregates value counts in-engine. No point fetch.
- `WHERE` scopes aggregation. `LIMIT` caps returned values. `EXACT true` computes exact shard counts instead of approximate.
- `SHARD` routes aggregation to one tenant partition.
- REST uses `POST /collections/{c}/facet`. gRPC uses `Points.Facet`.
- SDK accessors: `report.facet()` returns normalized `[{value, count}]`.

## Grouped retrieval paging

```sql
QUERY TEXT 'news' FROM docs GROUP BY topic SIZE 5 LOOKUP FROM topics LIMIT 20;

QUERY TEXT 'news' FROM docs
  GROUP BY topic SIZE 5
  LOOKUP FROM topics WITH PAYLOAD INCLUDE (title) WITH VECTOR (dense)
  LIMIT 20 OFFSET 10;
```

Key decisions:

- `SIZE n` caps hits per group. `LIMIT n` caps total hits. `OFFSET n` with `GROUP BY` maps to `group_offset` for grouped page two.
- `LOOKUP FROM` resolves group metadata cross-collection. Remote-only. Plain `GROUP BY` works on edge 0.8 plus.
- Lookup selectors shape the join payload. `WITH PAYLOAD INCLUDE (...)` and `WITH VECTOR (...)` narrow joined docs.
- SDK accessors: `report.groups()` returns `[{id, hits}]`.

## Payload and vector projections

```sql
QUERY TEXT 'acute bronchitis treatment protocols' FROM medical_records
  USING dense
  WHERE specialty = 'pulmonology'
  WITH PAYLOAD INCLUDE (title, summary, evidence_level, url)
  WITH VECTOR (colbert)
  LIMIT 15;
```

Key decisions:

- `QUERY` includes payloads by default. Omit the clause for full payloads. Use `WITH PAYLOAD false` to strip for bandwidth. Use `INCLUDE` or `EXCLUDE` for field selection.
- `WITH VECTOR` is off by default. Pass `true`, `false`, or an explicit vector list. Explicit lists return only named vectors.
- Scroll supports the same projection shapes with scroll clause order.

## Single-RPC batch blocks

```sql
BATCH { QUERY [0.1] FROM docs LIMIT 1; QUERY [0.2] FROM docs LIMIT 3; };

BATCH { QUERY [0.1] FROM docs LIMIT 1; QUERY [0.2] FROM docs LIMIT 3; }
  PARAMS (timeout = 30, consistency = majority);

BATCH { UPSERT INTO docs VALUES {id: 1, vector: [0.1]}; DELETE FROM docs WHERE id = 2; } WAIT false;
```

Key decisions:

- One collection and one family per block. All queries or all mutations.
- Mixed collections, mixed query and mutation members, DDL, `SHOW`, `COUNT`, `SCROLL`, `FACET`, and nested `BATCH` fail closed.
- `WAIT` belongs on mutation batches. `PARAMS (timeout, consistency)` belongs on query batches.
- `WAIT false` sends `?wait=false` explicitly. It never means omit the param.
- Remote Qdrant sends one native batch RPC. Edge fans out per member.
- SDKs also auto-batch adjacent compatible statements without explicit `BATCH` syntax. Explicit `BATCH` guarantees one RPC in QQL source.
