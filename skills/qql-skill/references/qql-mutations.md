# QQL mutation reference

Point writes, payload writes, vector writes, and deletes. Mutations take `WHERE` selectors and optional `SHARD` routing. DDL and `SHOW` never take mutation selectors.

`WAIT` controls durability acknowledgement. `WAIT false` sends `?wait=false` explicitly on mutation routes and batch blocks. It never means omit the param.

## Upsert points

```sql
UPSERT INTO docs VALUES
  {id: 1, text: 'Qdrant vector database', category: 'tech'},
  {id: 2, text: 'Rust programming language', category: 'programming'}
  USING DENSE MODEL 'all-minilm:l6-v2';

UPSERT INTO docs VALUES
  {id: 1, text: 'primary text', title: 'Qdrant Overview', category: 'tech'}
  USING DENSE MODEL 'all-minilm' ON FIELD title INTO title_vec;

UPSERT INTO docs VALUES
  {id: 1, text: 'primary text', title: 'Qdrant Overview'}
  USING
    DENSE MODEL 'all-minilm' ON FIELD text INTO dense,
    DENSE MODEL 'all-minilm' ON FIELD title INTO title_vec;
```

Key decisions:

- Rows are inline dicts with `id` plus payload plus vector inputs. Vector inputs are literal arrays, named maps, or inference dicts.
- `USING DENSE MODEL '...'` embeds a text field into a dense vector. `ON FIELD <field>` selects the source field. `INTO <vector>` selects the destination vector. Omit both when the mapping is unambiguous.
- Multiple `USING` legs map distinct fields to distinct named vectors.
- Text and sparse and multi resolution follows `qql-embeddings.md`. Schema topology fills kinds before embedding.
- Whole-point params avoid string building. `UPSERT INTO c VALUES :rows` binds one point dict or a list of them. Pair with prepared statements and `upsert_many` helpers in SDKs.
- Shard routing appends `SHARD 'key'`. `UPSERT INTO sec10k VALUES {id: 1, tenant_id: 'honeywell'} SHARD 'honeywell'`.

## Precomputed vectors on upsert

```sql
UPSERT INTO docs VALUES {id: 1, vector: [0.1, 0.2, 0.3], category: 'tech'};

UPSERT INTO docs VALUES {id: 1, vector: {dense: [0.1, 0.2], sparse: {indices: [1, 5], values: [0.5, 0.8]}}};

UPSERT INTO docs VALUES {
  id: 1,
  text: 'chunk text',
  vector: {dense: [0.1, 0.2, 0.3], colbert: [[0.1, 0.2], [0.3, 0.4]]}
};

UPSERT INTO docs VALUES {id: 1, vector: {text: 'hello', model: 'm'}};
```

Key decisions:

- Bare `vector: [...]` targets the default vector. Named maps target named vectors.
- Sparse values use `{indices: [...], values: [...]}`.
- Multivector values are nested arrays `[[...], ...]`.
- Inference dicts use `{text: '...', model: '...'}` or `{image: '...', model: '...'}` or `{object: {...}, model: '...'}`. Only string `text` and `image` or an `object` key claim the inference shape. A dict with numeric `text` stays a named vector.
- Typed arrays bind efficiently in SDKs. `Float32Array` packs dense vectors with one copy. Integer arrays bind sparse indices. Plain `number[]` of 32 plus elements packs as `F32Array` in Node and Python. Payload values never repack.

## Conditional upsert

```sql
UPSERT INTO docs VALUES {id: 1, vector: [0.1]} UPDATE FILTER status = 'active';

UPSERT INTO docs VALUES {id: 1, vector: [0.1]} UPDATE MODE insert_only;

UPSERT INTO docs VALUES {id: 1, vector: [0.1]}
  UPDATE FILTER status = 'active'
  UPDATE MODE update_only;
```

Key decisions:

- `UPDATE FILTER <filter>` guards the write. Only matching points are touched.
- `UPDATE MODE` is `insert_only`, `update_only`, or `upsert`.
- Guards commute and each appears at most once. Unknown modes fail closed.
- Either clause can appear alone. Both together narrow insert-only or update-only to filtered points.

## Update vector by ID

```sql
UPDATE docs SET VECTOR dense = [0.1, 0.2, 0.3] WHERE id = 1;

UPDATE docs SET VECTOR VALUES {id: 1, vector: [0.1, 0.2]}, {id: 2, vector: {dense: [0.3, 0.4]}};

UPDATE docs SET VECTOR dense = {text: 'hello', model: 'm'} WHERE id = 1;
```

Key decisions:

- Single-vector form sets one named vector on filtered points.
- `VALUES` form sets per-point vectors in one statement.
- Inference dicts work as vector values with the same shapes as upsert.
- `UPDATE ... VECTOR` rejects `inject_filter` with `QQL-VALIDATION-FILTER-INJECT`. It is a vector replace path, not a policy stamp path.

## Update payload

```sql
UPDATE docs SET PAYLOAD = {status: 'reviewed'} WHERE category = 'tech';

UPDATE docs SET PAYLOAD = {a: 1} KEY 'a.b' WHERE id = 1;

BATCH { UPDATE docs SET PAYLOAD = {a: 1} OVERWRITE WHERE id = 1; UPDATE docs SET PAYLOAD = {b: 2} WHERE id = 2; };
```

Key decisions:

- Default merge patches the payload. Missing keys are added. Present keys are overwritten at that level.
- `KEY 'a.b'` merges at a nested path instead of the root.
- `OVERWRITE` replaces the full payload. A lone `OVERWRITE` outside `BATCH` fails closed with `QQL-REST-OVERWRITE-BATCH-ONLY`. There is no single-point replace route.
- `KEY` plus `OVERWRITE` commute in source. Format normalizes to `KEY` then `OVERWRITE`.
- `inject_filter` merges into the selector filter. See `inject-filter.md`.

## Delete points

```sql
DELETE FROM docs WHERE category = 'obsolete';

DELETE FROM docs WHERE id IN (1, 2, 3);
```

Key decisions:

- Selector is a full filter. ID lists use the point-ID predicate.
- Same-collection deletes batch into one RPC via `BATCH` or SDK auto-batching.
- Tenant deletes always pair `WHERE tenant_id = ...` with `inject_filter` on untrusted input. See `qql-multitenancy.md`.

## Clear payload

```sql
CLEAR PAYLOAD FROM docs WHERE status = 'archived';
```

Key decisions:

- Wipes all payload from matching points. Vectors stay.
- Selector follows the same filter grammar as delete.

## Delete payload keys

```sql
DELETE PAYLOAD status, draft FROM docs WHERE category = 'tech';
```

Key decisions:

- Key list is comma-separated bare identifiers. At least one key is required.
- Only listed keys are removed. Other payload stays.

## Delete vectors

```sql
DELETE VECTOR colbert FROM docs WHERE id = 42;

DELETE VECTOR dense, colbert FROM docs WHERE category = 'temp';
```

Key decisions:

- Name list targets stored named vectors. Points stay. Other vectors stay.
- Selector is usually an ID predicate, but any filter works.
