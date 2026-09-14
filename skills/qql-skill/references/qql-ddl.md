# QQL DDL reference

Collections, indexes, shard keys, quotas, and storage tuning. DDL flows through the same planner as DML. DDL never takes `SHARD` routing or mutation selectors.

## Create collection

```sql
CREATE COLLECTION docs (
  dense VECTOR(384, COSINE),
  sparse SPARSE,
  colbert VECTOR(128, COSINE) WITH MULTIVECTOR (comparator = 'max_sim')
) WITH HNSW (m = 16, ef_construct = 100);

CREATE COLLECTION docs HYBRID (dense VECTOR(768, COSINE), sparse SPARSE)
  WITH HNSW (m = 16)
  WITH PARAMS (replication_factor = 3, shard_number = 4);

CREATE COLLECTION sec10k HYBRID (dense VECTOR(384, COSINE), sparse SPARSE)
WITH PARAMS (
  shard_number = 8,
  sharding_method = 'custom',
  shard_keys = ['honeywell', 'ge', '3m', 'rtx']
);
```

Key decisions:

- Dense vectors need dimension plus distance. `VECTOR(384, COSINE)` is canonical. `DOT` and `EUCLID` follow the same shape.
- Sparse vectors use bare `SPARSE`. Optional sparse config uses `WITH SPARSE (modifier = 'idf', memory = 'cached')`.
- Multivector columns use `VECTOR(dim, COSINE) WITH MULTIVECTOR (comparator = 'max_sim')`. Token dimension must match the model. BGE-M3 uses 1024. Smaller ColBERT models use 128.
- `HYBRID (...)` is shorthand for one dense plus one sparse in one collection.
- Collection creation supports `shard_number`, `sharding_method`, and `shard_keys` via `WITH PARAMS`. Custom sharding requires `sharding_method = 'custom'`.
- `WITH HNSW (...)` tunes graph build. `WITH QUANTIZATION (...)` sets scalar, binary, product, or turbo quantization. `WITH VECTOR (...)` and per-vector `WITH HNSW` tune one named vector.

## Collection blocks

```sql
CREATE COLLECTION docs (v VECTOR(8, COSINE)) WITH WAL (wal_capacity_mb = 32, wal_segments_ahead = 2, wal_retain_closed = 1);

CREATE COLLECTION docs (v VECTOR(8, COSINE)) WITH STRICT_MODE (enabled = true, max_query_limit = 100);

CREATE COLLECTION docs (v VECTOR(8, COSINE)) WITH METADATA (owner = 'team', version = 3);

ALTER COLLECTION docs WITH STRICT_MODE (enabled = false);
```

Key decisions:

- `WITH WAL` is create-only. `WITH STRICT_MODE` and `WITH METADATA` work on create and alter.
- Create-time `read_fan_out_factor` and `read_fan_out_delay_ms` parse on `CREATE` and apply with a follow-up PATCH.
- Unknown block keys fail at plan time, not silently ignored.

## Memory placement and TurboQuant

```sql
CREATE COLLECTION docs (
  dense VECTOR(384, COSINE) WITH VECTOR (memory = 'cached', datatype = 'turbo4')
    WITH HNSW (memory = 'cold')
) WITH PARAMS (payload_memory = 'cold')
  WITH QUANTIZATION (type = 'scalar', memory = 'cached');

CREATE COLLECTION docs_q (
  dense VECTOR(128, DOT) WITH QUANTIZATION (type = 'scalar', memory = 'pinned')
);
```

Key decisions:

- Qdrant 1.19 memory tiers are `cold`, `cached`, `pinned` on vector, HNSW, sparse, quantization, and indexes. `payload_memory` lives in collection `PARAMS` and rejects `pinned`.
- Prefer `memory` and `payload_memory` for new scripts. Legacy `on_disk` and `always_ram` still dual-write through 1.19. Upstream plans removal around 1.21.
- `datatype = 'turbo4'` is dense 4-bit TurboQuant storage, not a distance metric.
- Edge maps `pinned` to RAM and `cached` or `cold` to mmap. Create-time per-vector config lowers onto the engine config.

## Alter and drop collection

```sql
ALTER COLLECTION docs WITH VECTOR dense (HNSW (m = 32), VECTOR (memory = 'cached'));

ALTER COLLECTION docs WITH PARAMS (replication_factor = 3);

ALTER COLLECTION docs WITH QUANTIZATION (type = 'scalar', always_ram = true);

DROP COLLECTION docs;
```

Key decisions:

- Per-vector alter uses `WITH VECTOR <name> (...)`. Unnamed `WITH VECTOR (...)` targets the default vector.
- Remote supports global plus per-vector HNSW, quantization, and storage diffs. Edge persists global `HNSW` and `OPTIMIZERS` plus per-vector `HNSW`. Other per-vector fields reject per field on edge.
- `WITH PARAMS` on edge supports `on_disk_payload` only. Other keys reject with `QQL-EDGE-UNSUPPORTED-COLLECTION-PARAMS`.

## Payload indexes

```sql
CREATE INDEX ON COLLECTION docs FOR title TYPE text WITH (lowercase = true);

CREATE INDEX ON COLLECTION docs FOR tenant_id TYPE keyword WITH (is_tenant = true);

CREATE INDEX ON COLLECTION docs FOR rating TYPE integer WITH (range = true);

CREATE INDEX ON COLLECTION docs FOR title TYPE keyword WITH (prefix = true, memory = 'cached');

CREATE INDEX ON COLLECTION docs FOR body TYPE text WITH (stopwords = 'english');

CREATE INDEX ON COLLECTION docs FOR body TYPE text WITH (stopwords = {languages: ['english'], custom: ['foo']});

DROP INDEX ON COLLECTION docs FOR title;
```

Key decisions:

- Tenant fields use `is_tenant = true` for Qdrant-native tenant optimization.
- Keyword prefix search needs `prefix = true` at index time plus `MATCH PREFIX` at query time.
- Text stopwords accept a bare language string, a list, or a set with `languages` plus `custom`. Unknown languages fail at plan. Unknown set keys fail at parse.
- Supported index types follow Qdrant payload indexing. Keyword, integer, float, bool, text, geo, datetime, and uuid cover the matrix.

## Shard key lifecycle

```sql
CREATE COLLECTION tenants HYBRID (dense VECTOR(768, COSINE), sparse SPARSE)
WITH PARAMS (shard_number = 8, sharding_method = 'custom');

CREATE SHARD KEY 'acme' ON COLLECTION tenants WITH (shards_number = 2);

CREATE SHARD KEY 'acme' ON COLLECTION tenants
  WITH (shards_number = 2, placement = [1, 2], initial_state = 'Active');

SHOW SHARD KEYS ON COLLECTION tenants;

DROP SHARD KEY 'acme' ON COLLECTION tenants;
```

Key decisions:

- Shard keys are admin vocabulary. They define partitions. They do not route one request. Routing is `SHARD '...'` on DML. See `qql-multitenancy.md`.
- Numeric keys stay numeric end to end. `SHARD 101` reaches the numeric partition. `SHARD '101'` reaches the keyword partition. They hash differently on the wire.
- `placement` is a peer-ID list. `initial_state` is a replica-state string.
- Edge has no shard-key admin. Shard DDL returns `QQL-EDGE-UNSUPPORTED-*`. Use remote Qdrant.

## Show metadata

```sql
SHOW COLLECTIONS;

SHOW COLLECTION docs;

SHOW SHARD KEYS ON COLLECTION docs;
```

Key decisions:

- `SHOW COLLECTIONS` lists names. `SHOW COLLECTION <name>` returns collection info including status and point counts.
- `SHOW SHARD KEYS` lists strings and non-negative integers with preserved types.
- SDK accessors expose these shapes natively. `collections()`, `collection()`, `shard_keys()`.

## Cluster quotas

```sql
SHOW QUOTAS;

SET QUOTA (enabled = true, max_resident_memory_percent = 80, max_disk_usage_percent = 90, release_margin_percent = 5) WAIT true;

SET QUOTA (enabled = false);

SET QUOTA (max_disk_usage_percent = null);
```

Key decisions:

- Quotas are cluster-wide REST only. `GET` and `PUT /quotas`. gRPC returns `QQL-GRPC-QUOTA`. Edge returns `QQL-EDGE-UNSUPPORTED-QUOTA`. Use a REST admin client.
- `SET QUOTA` is a full replace, not a merge. Omitted keys are unset. `null` clears one limit.
- Percent fields validate ranges at plan time with `QQL-PLAN-QUOTA`.
- `WAIT true` is a query param, not a body field.
