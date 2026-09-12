# QQL filter reference

All 20 `FilterExpr` forms. Filters scope retrieval, counting, faceting, scrolling, and mutation selectors. Routing stays out of filters. `SHARD` is a sibling of `WHERE`, never nested inside it.

## Comparison and range

```sql
WHERE category = 'tech';
WHERE rating >= 4.5;
WHERE rating != 3;
WHERE created_at BETWEEN 1704067200 AND 1767139200;
WHERE status IN ('open', 'acknowledged');
WHERE status NOT IN ('deprecated', 'archived');
```

Key decisions:

- Operators are `=`, `!=`, `>`, `>=`, `<`, `<=`. Typed `ComparisonOp` in Rust. String operators convert via `ComparisonOp::parse_inject_op` in bindings.
- `BETWEEN low AND high` is inclusive on both ends.
- `IN` and `NOT IN` take non-empty literal lists. Placeholders bind list elements the same way as scalars.
- Point IDs filter with `WHERE id = 1` or `WHERE id IN (1, 2)`. This is the `PointId` predicate, not a payload comparison.

## Null and empty

```sql
WHERE assigned_team IS NOT NULL;
WHERE deleted_at IS NULL;
WHERE tags IS NOT EMPTY;
WHERE tags IS EMPTY;
```

Key decisions:

- `IS NULL` matches missing fields and explicit nulls.
- `IS EMPTY` matches present but valueless fields. Empty arrays count as empty.
- Negations are `IS NOT NULL` and `IS NOT EMPTY`. No `!= NULL` spelling.

## Text match

```sql
WHERE title MATCH 'vector database';
WHERE title MATCH ANY ('kubernetes', 'docker');
WHERE title MATCH PHRASE 'exact phrase';
WHERE title MATCH TOKENS 'red shoes';
WHERE tags MATCH EXCEPT ('archived', 'private');
WHERE title MATCH PREFIX 'Comp';
```

Key decisions:

- `MATCH 'term'` is full-text match on a text index.
- `MATCH ANY (...)` matches any listed value. `MATCH PHRASE '...'` matches ordered tokens.
- `MATCH TOKENS '...'` matches any token of the text. Lowers to Qdrant `MatchTextAny`.
- `MATCH EXCEPT (...)` requires none of the values to match. List must be non-empty.
- `MATCH PREFIX '...'` needs a keyword index with `prefix = true`. Create with `CREATE INDEX ... TYPE keyword WITH (prefix = true)`. Not a substitute for phrase search on text indexes.

## At-least-N

```sql
WHERE MIN SHOULD 2 (status = 'active', priority = 'high', category = 'tech');
```

Key decisions:

- `MIN SHOULD n (...)` requires at least `n` operands to hold. `n >= 1`.
- Operands are full filters. Nesting and text match inside works.
- Lowers to Qdrant `MinShould` with `min_count` on REST, gRPC, and edge.

## Logic

```sql
WHERE (severity >= 3 AND status = 'open') OR (severity >= 5 AND status = 'acknowledged');
WHERE NOT (category = 'deprecated');
WHERE category = 'tech' AND priority = 'high' AND NOT (status = 'closed');
```

Key decisions:

- `AND` conjoins. `OR` disjoins. `NOT` negates one operand. Parentheses control precedence.
- `must` converts to `AND`, `should` to `OR`, `must_not` to `NOT` in `qql convert`. See `convert-migration.md`.
- Combine freely with match, range, geo, nested, slice, and vector predicates.

## Nested documents

```sql
WHERE NESTED('reviews', rating > 4);
```

Key decisions:

- First argument is the object path. Second argument is the sub-filter applied under that path.
- Path is a string literal. Sub-filter uses the same grammar as top-level filters.

## Array and vector presence

```sql
WHERE tags VALUES_COUNT >= 2;
WHERE HAS_VECTOR 'dense';
```

Key decisions:

- `VALUES_COUNT <op> n` compares the stored value count for the field.
- `HAS_VECTOR 'name'` matches points holding the named vector. Name is a string literal.

## Deterministic slice

```sql
WHERE SLICE (4, 1);
WHERE SLICE (4, 1) AND status = 'active';
```

Key decisions:

- `SLICE (total, index)` hashes point IDs into `total` buckets and keeps bucket `index`. Deterministic across queries. Unlike `QUERY SAMPLE RANDOM`.
- `total >= 1` and `0 <= index < total`. Violations fail closed with `QQL-VALIDATION-SLICE`.
- Combine with `tenant_id` for per-tenant sampling. Use alone for cluster-wide bucket experiments.

## Geo

```sql
WHERE location GEO_RADIUS { center: {lat: 48.8566, lon: 2.3522}, radius: 5000 };

WHERE location GEO_BBOX {
  top_left: {lat: 48.86, lon: 2.34},
  bottom_right: {lat: 48.85, lon: 2.36}
};

WHERE area GEO_POLYGON {
  exterior: [{lat: 48.86, lon: 2.34}, {lat: 48.85, lon: 2.36}, {lat: 48.85, lon: 2.34}],
  interiors: []
};
```

Key decisions:

- `GEO_RADIUS` takes a center plus radius in meters.
- `GEO_BBOX` takes northwest `top_left` plus southeast `bottom_right`.
- `GEO_POLYGON` takes an outer ring plus hole rings. Holes default to empty.
- Dotted placeholders bind inside geo shapes. `center: {lat: :loc.lat, lon: :loc.lon}` binds from `{"loc": {"lat": 1.0, "lon": 2.0}}`. See `qql-params.md`.
- Geo filters combine with formula `GEO_DISTANCE` scoring. Filter narrows candidates. Formula boosts by distance.

## Full predicate list

For completeness, the 20 wire-backed forms are point ID, compare, between, in, is null, is empty, match text, match any, match phrase, match prefix, match tokens, match except, min should, and, or, not, nested, has vector, slice, values count, geo bbox, geo radius, geo polygon. The last three geo shapes count as three forms. `NOT IN` lowers through the same `In` shape with negation. All 20 validate against the OpenAPI `Filter` schema in contract tests.
