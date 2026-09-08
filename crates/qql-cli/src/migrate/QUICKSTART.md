# `qql migrate` quickstart

Copy a collection as **schema + points** (not a Qdrant snapshot). The target can
be another cluster, another minor version, another shard count, or a quantized
schema.

## 1. Build

```bash
cargo build --release -p qql-cli
# binary: target/release/qql
```

Use gRPC (`:6334` / `grpc://`) for the ingest path. REST (`:6333`) works too.

## 2. Inspect the plan (no writes)

```bash
./target/release/qql --url grpc://localhost:6334 \
  migrate geosmart_berlin_stays --to geosmart_copy --dry-run
```

You should see `CREATE COLLECTION`, `CREATE INDEX` (payload indexes first), and
the `ALTER COLLECTION` that restores `indexing_threshold` after bulk load.

## 3. Run

Same cluster, new collection:

```bash
./target/release/qql --url grpc://localhost:6334 \
  migrate geosmart_berlin_stays --to geosmart_copy --recreate
```

Cross cluster:

```bash
./target/release/qql --url grpc://old-host:6334 \
  migrate docs \
  --target-url grpc://new-host:6334 \
  --to docs \
  --target-api-key "$QDRANT_TARGET_API_KEY"
```

## 4. Confirm

The command prints `written` / `source_count` / `verified`. Default verify is
exact `COUNT … WITH (exact = true)`.

Crash recovery: a checkpoint is written under `.qql-migrate/` (includes both
URLs). Re-run the same command to resume; `--restart` starts over.

## Flags you will actually use

| Flag | Why |
|---|---|
| `--dry-run` | Print CREATE/INDEX/ALTER only |
| `--recreate` | DROP target first |
| `--workers 4` `--batch-size 128` | Parallel upsert window |
| `--quantize scalar` | Inject quantization on CREATE |
| `--where "neighbourhood_group = 'Mitte'"` | Filtered copy |
| `--cutover docs` | Point alias `docs` at the new collection |
| `--no-wait` | Faster ingest; verify polls until counts match |
| `--bulk-threshold-kb` | Cap unindexed RAM during bulk load (default 2 000 000) |

`--shard-key` / `--shard-key-field` need **Qdrant distributed mode**. Standalone
nodes reject `CREATE SHARD KEY`; migrate fails in the schema phase instead of
hanging.
