# QQL Filter Reference

Metadata filter expressions in `WHERE` clauses for `QUERY`, `COUNT`, `SCROLL`, `DELETE`, `CLEAR PAYLOAD`, `DELETE VECTOR`, and `UPDATE ... SET PAYLOAD` statements. (The `UPDATE ... SET VECTOR` form uses point-ID equality only — general filters do not apply.)

---

## 1. Comparison Operators

```sql
WHERE field = 'value'          -- string equality
WHERE field != 'value'         -- string inequality (parsed as NOT (field = 'value'))
WHERE field > 10               -- integer greater than
WHERE field >= 10              -- integer greater than or equal
WHERE field < 100              -- integer less than
WHERE field <= 100             -- integer less than or equal
WHERE field = 3.14             -- float equality
WHERE field = true             -- boolean equality
```

---

## 2. Ranges

```sql
WHERE field BETWEEN 10 AND 100
WHERE score BETWEEN 0.5 AND 1.0
```

---

## 3. Set Membership

```sql
-- Value matches any in the list
WHERE status IN ('active', 'pending', 'reviewed')
WHERE year IN (2024, 2025, 2026)

-- Value matches none in the list (parsed as NOT (field IN (...)))
WHERE status NOT IN ('deleted', 'archived')
```

---

## 4. Null & Empty Checks

```sql
WHERE field IS NULL
WHERE field IS NOT NULL
WHERE field IS EMPTY
WHERE field IS NOT EMPTY
```

---

## 5. Text & Token Matching

```sql
WHERE content MATCH 'hello world'           -- full-text match
WHERE content MATCH ANY ('hello', 'world')   -- match any terms in list
WHERE content MATCH PHRASE 'hello world'    -- exact phrase matching
```

---

## 6. Logical Operators

```sql
WHERE a = 1 AND b = 2
WHERE a = 1 OR b = 2
WHERE NOT a = 1
WHERE (a = 1 OR b = 2) AND c = 3
```

Operator precedence (highest to lowest): comparisons/BETWEEN/IN/IS/MATCH > NOT > AND > OR.

---

## 7. Nested Object Filtering

Filter elements inside nested arrays using `NESTED('path', filter)`:

```sql
-- Filter documents having at least one review with rating > 4
WHERE NESTED('reviews', rating > 4)

-- Compound nested query
WHERE NESTED('overwritten_in', by = 'root' AND seq <= 2)

-- Combined with top-level attributes
WHERE status = 'published' AND NOT NESTED('history', action = 'reject')
```

---

## 8. Advanced Filters

### Point ID filter

```sql
WHERE id = 42
WHERE id IN (42, 'uuid-here', 100)
```

Compiled as `has_id` conditions for efficient point-specific lookups.

### Has vector filter

```sql
-- Check if a specific named vector exists
WHERE HAS_VECTOR 'dense'
```

### Values count filter

```sql
-- Filter by the number of values in an array field
WHERE tags VALUES_COUNT >= 2
WHERE categories VALUES_COUNT = 0
```

### Geospatial filters

```sql
-- Geospatial filtering with a bounding box
WHERE location GEO_BBOX {
  top_left: {lat: 52.5200, lon: 13.4050},
  bottom_right: {lat: 52.5100, lon: 13.4150}
}

-- Geospatial filtering within a radius (center, radius in meters)
WHERE location GEO_RADIUS {
  center: {lat: 52.5200, lon: 13.4050},
  radius: 1000.0
}
```

> **Note**: `GEO_POLYGON` is defined in the plan-layer types (`GeoPolygon` in
> `qql-plan/src/types.rs`) but is **not yet parsed from QQL syntax**. It can
> be reached by programmatic AST construction or future parser extension.

---

## 9. Round-trip example

These filters all lower from QQL syntax to the plan layer and serialize to
Qdrant's OpenAPI filter format. For instance:

```sql
WHERE status = 'active' AND (score >= 0.5 OR tags IS NOT EMPTY)
```

Produces the JSON condition:

```json
{
  "must": [
    { "key": "status", "match": { "value": "active" } },
    {
      "should": [
        { "key": "score", "range": { "gte": 0.5 } },
        { "is_empty": { "key": "tags" } }
      ]
    }
  ]
}
```

All filter variants are supported across REST, gRPC, and edge backends.
