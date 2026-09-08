# `qql migrate` examples

Commands below were run against a live Qdrant **1.19.1** on `localhost:6334`
(gRPC). Binary: `target/release/qql`.

Source collections (untouched):

| collection | exact count | vectors |
|---|---:|---|
| `geosmart_berlin_stays` | 8317 | dense 384 |
| `nyayarag_legal_precedents` | 2034 | dense 384 + colbert 128 + bm25 |

## Dry-run (no writes)

```bash
./target/release/qql --url grpc://localhost:6334 \
  migrate geosmart_berlin_stays --to geosmart_copy --dry-run
```

## Same-cluster copy (durable default: `WAIT true`)

```bash
./target/release/qql --url grpc://localhost:6334 \
  migrate geosmart_berlin_stays --to geosmart_copy \
  --recreate --workers 4 --batch-size 128
```

Live: **8317 / 8317 verified in 1.784 s** (~4.7k points/s), 65 batches.
`indexing_threshold` restored to the source value (`10000`).

## Faster ingest (`WAIT false`)

```bash
./target/release/qql --url grpc://localhost:6334 \
  migrate geosmart_berlin_stays --to geosmart_copy \
  --recreate --restart --workers 4 --batch-size 128 --no-wait
```

Live: **8317 / 8317 verified in 1.635 s**. Verify polls exact COUNT until WAL
catches up (a raw COUNT immediately after `--no-wait` can lag).

## ColBERT / multivector collection

```bash
./target/release/qql --url grpc://localhost:6334 \
  migrate nyayarag_legal_precedents --to nyaya_copy \
  --recreate --workers 2 --batch-size 64
```

Live: **2034 / 2034 verified in 2.654 s**. Each point carries a ColBERT token
matrix; the gRPC client accepts scroll pages up to 64 MiB (tonic's 4 MiB
default is too small).

## Filtered extract

```bash
./target/release/qql --url grpc://localhost:6334 \
  migrate geosmart_berlin_stays --to geosmart_mitte \
  --recreate --where "neighbourhood_group = 'Mitte'"
```

Live: **1982 / 1982 verified in 0.867 s**.

## Alias cutover

```bash
./target/release/qql --url grpc://localhost:6334 \
  migrate geosmart_berlin_stays --to geosmart_copy \
  --recreate --cutover stays
```

Live: alias `stays` pointed at `geosmart_copy` after verify. `--cutover` issues
`delete_alias` + `create_alias` as one Qdrant alias batch.
`--drop-source-after-cutover` requires `--cutover` (do not use it against a
collection you still need).

## In-flight scalar quantization (plan only)

```bash
./target/release/qql --url grpc://localhost:6334 \
  migrate geosmart_berlin_stays --to geosmart_q --quantize scalar --dry-run
```

Injects `WITH QUANTIZATION (type = 'scalar', always_ram = true, quantile = 0.99)`
on CREATE. Other values: `--quantize binary|product|turbo`.

## Reshard / tenant split (clustered Qdrant only)

```bash
./target/release/qql --url grpc://localhost:6334 \
  migrate geosmart_berlin_stays --to geosmart_sharded \
  --shard-number 4 --replication-factor 1 \
  --shard-key-field neighbourhood_group \
  --on-missing-shard-key skip
```

Creates `sharding_method = 'custom'`, a `is_tenant = true` index on
`neighbourhood_group`, and routes each point with `SHARD '<group>'`. Integers
stay numeric shard keys (`SHARD 101`), strings stay keywords (`SHARD 'acme'`).

On **standalone** Qdrant this fails in the schema phase (~15 ms) with:

```
custom shard keys require Qdrant distributed mode
```

Do not wait for ingest — `CREATE SHARD KEY` is unimplemented there.

## Cross-cluster + alias cutover

```bash
./target/release/qql --url grpc://old:6334 \
  migrate docs \
  --target-url grpc://new:6334 \
  --to docs_v2 \
  --cutover docs \
  --drop-source-after-cutover
```

## Resume after a crash

Re-run the **same** command. Checkpoint files live at:

```
.qql-migrate/<source_url>__<source>__<target_url>__<target>.json
```

`--restart` discards the checkpoint. `--resume` errors if none exists.

## JSON output

Add `--json` for scripting. Fields: `written`, `source_count`, `target_count`,
`verified`, `resumed`, `create`, `indexes`, `restore_optimizers`.
