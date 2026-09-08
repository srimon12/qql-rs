# `qql migrate` examples

Commands below were run against a live Qdrant **1.19.1** on `localhost:6334`
(gRPC) with the `vsq_*` vs-qdrant collections. Binary: `target/release/qql`.

Source collections (untouched):

| collection | exact count | vectors |
|---|---:|---|
| `vsq_berlin_qql` | 7664 | dense 384 + bm25 |
| `vsq_legal_qql` | 2000 | dense 384 + colbert 128 + bm25 |

## Dry-run (no writes)

```bash
./target/release/qql --url grpc://localhost:6334 \
  migrate vsq_berlin_qql --to vsq_berlin_copy --dry-run
```

## Same-cluster copy (durable default: `WAIT true`)

```bash
./target/release/qql --url grpc://localhost:6334 \
  migrate vsq_berlin_qql --to vsq_berlin_copy \
  --recreate --workers 4 --batch-size 128
```

Live: **7664 / 7664 verified in 1.014 s** (~7.6k points/s), 60 batches.

## Faster ingest (`WAIT false`)

```bash
./target/release/qql --url grpc://localhost:6334 \
  migrate vsq_berlin_qql --to vsq_berlin_copy \
  --recreate --restart --workers 4 --batch-size 128 --no-wait
```

Live: **7664 / 7664 verified in 0.938 s**. Verify polls exact COUNT until WAL
catches up (a raw COUNT immediately after `--no-wait` can lag).

## ColBERT / multivector collection

```bash
./target/release/qql --url grpc://localhost:6334 \
  migrate vsq_legal_qql --to vsq_legal_copy \
  --recreate --workers 2 --batch-size 64
```

Live: **2000 / 2000 verified in 3.056 s**. Each legal point carries a ColBERT
token matrix (~55–90 vectors); batch size 64 keeps gRPC messages bounded.

## Filtered extract

```bash
./target/release/qql --url grpc://localhost:6334 \
  migrate vsq_berlin_qql --to vsq_berlin_mitte \
  --recreate --where "district = 'Mitte'"
```

Live: **593 / 593 verified in 0.468 s**.

## In-flight scalar quantization (plan only)

```bash
./target/release/qql --url grpc://localhost:6334 \
  migrate vsq_berlin_qql --to vsq_berlin_q --quantize scalar --dry-run
```

Injects `WITH QUANTIZATION (type = 'scalar', always_ram = true, quantile = 0.99)`
on CREATE. Other values: `--quantize binary|product|turbo`.

## Reshard / tenant split

```bash
./target/release/qql --url grpc://localhost:6334 \
  migrate vsq_berlin_qql --to vsq_berlin_sharded \
  --shard-number 4 --replication-factor 1 \
  --shard-key-field district \
  --on-missing-shard-key skip
```

Creates `sharding_method = 'custom'`, a `is_tenant = true` index on `district`,
and routes each point with `SHARD '<district>'`. Integers stay numeric shard
keys (`SHARD 101`), strings stay keywords (`SHARD 'acme'`).

## Cross-cluster + alias cutover

```bash
./target/release/qql --url grpc://old:6334 \
  migrate docs \
  --target-url grpc://new:6334 \
  --to docs_v2 \
  --cutover docs \
  --drop-source-after-cutover
```

`--cutover` issues `delete_alias` + `create_alias` as one Qdrant alias batch.
`--drop-source-after-cutover` requires `--cutover`.

## Resume after a crash

Re-run the **same** command. Checkpoint files live at:

```
.qql-migrate/<source_url>__<source>__<target_url>__<target>.json
```

`--restart` discards the checkpoint. `--resume` errors if none exists.

## JSON output

Add `--json` for scripting. Fields: `written`, `source_count`, `target_count`,
`verified`, `resumed`, `create`, `indexes`, `restore_optimizers`.
