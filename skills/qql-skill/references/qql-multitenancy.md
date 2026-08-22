# QQL Multi-Tenancy Guide

One collection, many tenants. Isolation is a **filter**. Custom sharding is optional **routing** for performance and blast-radius.

## Two layers (do not conflate them)

```
┌─────────────────────────────────────────────────────────────┐
│  Layer A — logical isolation (required)                     │
│  WHERE tenant_id = 'honeywell'                              │
│  → OpenAPI/gRPC Filter (must/should/must_not)               │
│  → Host: inject_filter(stmt, "tenant_id", "=", tenant)      │
└─────────────────────────────────────────────────────────────┘
┌─────────────────────────────────────────────────────────────┐
│  Layer B — physical routing (optional, custom sharding)     │
│  SHARD 'honeywell'                                          │
│  → REST body: shard_key (ShardKeySelector)                  │
│  → gRPC: ShardKeySelector on QueryPoints / CountPoints / …│
│  → Host: write SHARD in QQL, or stmt.shard_key = tenant     │
└─────────────────────────────────────────────────────────────┘
```

| Concern | QQL | Wire (REST) | Wire (gRPC) |
|---------|-----|-------------|-------------|
| Isolation | `WHERE tenant_id = '…'` | `filter` | `Filter` |
| Routing | `SHARD '…'` | `shard_key` | `shard_key_selector` |
| Define partitions | `CREATE SHARD KEY '…'` | create shard key API | `CreateShardKey` |

**Never put routing inside `Filter`.** Neither OpenAPI `Filter` nor proto `Filter` accept a shard field.

---

## SHARD KEY (DDL) vs SHARD (routing)

### DDL — manage partition names on a custom-sharded collection

```sql
CREATE COLLECTION sec10k
  HYBRID (dense VECTOR(384, COSINE), sparse SPARSE)
  WITH PARAMS (
    shard_number = 8,
    sharding_method = 'custom',
    shard_keys = ['honeywell', 'ge', '3m', 'rtx']
  );

-- or create keys after the collection exists
CREATE SHARD KEY 'honeywell' ON COLLECTION sec10k WITH (shards_number = 2);
SHOW SHARD KEYS ON COLLECTION sec10k;
DROP SHARD KEY 'honeywell' ON COLLECTION sec10k;
```

This is **admin** vocabulary (Qdrant “shard key” resource). It does **not** route a query.

### DML — route one statement

```sql
UPSERT INTO sec10k VALUES { id: 1, text: '…', tenant_id: 'honeywell' }
  SHARD 'honeywell';

QUERY TEXT 'supply chain risks'
  FROM sec10k
  USING dense
  WHERE tenant_id = 'honeywell'
  SHARD 'honeywell'
  LIMIT 10;

COUNT FROM sec10k
  WHERE tenant_id = 'honeywell'
  SHARD 'honeywell'
  WITH (exact = true);
```

`SHARD '…'` is the **only** language form for request-level routing. It is first-class in the grammar — no separate inject API.

---

## Tenant index

```sql
CREATE INDEX ON COLLECTION sec10k FOR tenant_id
  TYPE keyword WITH (is_tenant = true);
```

---

## Per-tenant sparse IDF corpus (Qdrant 1.19 / QQL 1.4)

Sparse scores that use IDF should not mix term statistics across tenants when
each tenant’s vocabulary is private. Scope the **IDF corpus** with a filter object
in `PARAMS`, and keep **isolation** via `WHERE` / `inject_filter` (and `SHARD` when
custom-sharded).

```sql
-- Global IDF (shared stats across the collection)
QUERY TEXT 'supply chain risks' FROM sec10k USING sparse
  WHERE tenant_id = 'honeywell'
  SHARD 'honeywell'
  PARAMS (idf = 'global')
  LIMIT 10;

-- Tenant-scoped IDF: rarity computed only on points matching the corpus filter
QUERY TEXT 'supply chain risks' FROM sec10k USING sparse
  WHERE tenant_id = 'honeywell'
  SHARD 'honeywell'
  PARAMS (idf = {corpus: {must: [{key: 'tenant_id', match: {value: 'honeywell'}}]}})
  LIMIT 10;
```

**Notes:**
- Corpus filter uses Qdrant Filter **JSON** shape (`must` / `should` / …), not nested QQL `WHERE`.
- Malformed corpus → `QQL-PLAN-IDF`. Supported on remote Qdrant and **edge 0.8+**.
- `idf` scopes statistics; it does **not** replace `inject_filter` for security.

---

## Deterministic sampling with `SLICE`

`WHERE SLICE (total, index)` partitions the point-ID space into `total` stable
buckets and keeps bucket `index`. Useful for canary ranking or load tests
**inside** a tenant (or cluster-wide when unfiltered).

```sql
-- 1/4 of honeywell’s points (stable bucket), not random SAMPLE
QUERY TEXT 'risks' FROM sec10k USING dense
  WHERE tenant_id = 'honeywell' AND SLICE (4, 1)
  SHARD 'honeywell'
  LIMIT 100;
```

Validation: `total >= 1` and `0 <= index < total` (`QQL-VALIDATION-SLICE`).

---

## Host SDKs

### Preferred: put `SHARD` in the QQL string

When the tenant is known when you build the template:

```python
qql = f"""
  QUERY TEXT 'supply chain risks' FROM sec10k USING dense
  WHERE tenant_id = '{tenant}'
  SHARD '{tenant}'
  LIMIT 10
"""
client.execute(qql)
```

### Always: `inject_filter` for isolation

User-supplied QQL must not be trusted for tenant isolation:

```python
stmt = parse(user_qql)[0]
inject_filter(stmt, "tenant_id", "=", tenant)  # recursive into CTEs / prefetches
client.execute(stmt)
```

### Optional: `stmt.shard_key` when the host resolves routing after parse

There is **no** `inject_shard_key`. Same AST field as the `SHARD` clause:

```python
stmt = parse("QUERY TEXT 'risks' FROM sec10k USING dense LIMIT 10")[0]
inject_filter(stmt, "tenant_id", "=", "honeywell")
stmt.shard_key = "honeywell"   # Python
# stmt.shardKey = "honeywell"  # Node / WASM
# stmt.set_shard_key(Some("honeywell".into()));  // Rust
```

| API | Purpose |
|-----|---------|
| `inject_filter` | Security — always |
| `SHARD '…'` in QQL | Routing in the language — preferred |
| `stmt.shard_key` / `set_shard_key` | Routing after parse — same wire field |

---

## OpenAPI vs proto (same plan, two projections)

From Qdrant:

- **REST** `QueryRequest` / `CountRequest` / …: optional `shard_key` → `ShardKeySelector`
- **gRPC** `QueryPoints` / `CountPoints` / …: optional `shard_key_selector` → `ShardKeySelector`
- **Filter** (both): only must / should / must_not / min_should — **no shard field**

Plan IR mirrors that: `filter` and `shard_key` are siblings on the request, never nested.

---

## Checklist

1. Custom collection: `sharding_method = 'custom'` + `CREATE SHARD KEY` / `shard_keys`
2. Upserts: `tenant_id` payload **and** `SHARD 'tenant'`
3. Queries: `WHERE tenant_id = …` **and** (if custom-sharded) `SHARD 'tenant'`
4. Host gate: always `inject_filter`; never trust client-only filters
5. Do not invent a second inject API for shards — use QQL `SHARD` or `stmt.shard_key`
6. Sparse multi-tenant: consider `PARAMS (idf = {corpus: …})` so IDF stats match the tenant
7. Canary / sample inside a tenant: `WHERE tenant_id = … AND SLICE (n, k)` (not a substitute for isolation)
