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
`indexing_threshold` restored to the source value (`10000` on this collection;
the default is `20000` when the source sets none).

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
on CREATE. Other values: `--quantize binary|product|turbo`. Per-family
defaults are `binary` (`one_bit`), `product` (`x16`), `turbo` (`bits = 2`);
all default to `always_ram = true` (pass `--no-always-ram` to keep quantized
vectors on disk).

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
Keys are discovered with `FACET <field> … LIMIT 10000 EXACT true`, falling back
to a payload-only scroll when FACET truncates or the field is unindexed; an
existing index on the field is promoted with `is_tenant = true`. Use
`--shard-key <literal>` instead when every point shares one key. Only
non-empty strings and non-negative integers become shard keys. Bool, float,
empty, and null values are ignored during discovery; at ingest they follow
`--on-missing-shard-key` (`error` by default, or `skip`, or `default=<key>`).

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
.qql-migrate/<source_url>__<source>__<target_url>__<target>__<hash>.json
```

Segments are sanitized and a hash of the full endpoint identity is appended,
so URLs that sanitize identically still map to different files.

`--restart` discards the checkpoint. `--resume` errors if none exists.
Tuning flags (`--workers`, `--batch-size`, `--no-wait`, `--recreate`,
`--no-verify`, `--batch-delay-ms`) can change across a resume. They affect
speed and durability, not what the target must contain. Schema-affecting
flags (`--shard-number`, `--where`, `--quantize`, shard keys) must stay the
same. `--cutover` can be added or dropped across a resume. The alias swap
reads the current flags when the Cutover phase runs.

## JSON output

Add `--json` for scripting. Fields: `ok`, `operation`, `source`, `target`,
`written`, `skipped`, `batches`, `source_count`, `target_count`, `verified`,
`resumed`, `dry_run`, `cutover_alias`, `source_dropped`, `create`, `indexes`,
`shard_keys`, `restore_optimizers`.

## End-to-end sharded demo

`berlin_shard_migration.py` (next to these docs) drives the full loop through
the CLI — create, ingest via `UPSERT … VALUES :rows --params-file`, migrate
with `--shard-key-field district`, verify, and a sharded `dump` round-trip.
Needs multi-node Qdrant; see the script header for prerequisites.
