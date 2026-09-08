# `qql migrate` handover

For the next engineer (or agent) picking this up. Code lives in
`crates/qql-cli/src/migrate/`. Strategy background is in `README.md`.

## What shipped

`qql migrate` streams **CREATE COLLECTION + CREATE INDEX + UPSERT :rows** from a
source Qdrant to a target. It is not a snapshot tool: it rebuilds data so you
can jump minor versions, change shard count, and inject quantization.

Phases: schema → ingest (checkpointed) → restore optimizer threshold → exact
COUNT verify → optional alias cutover.

## Live proof (2026-09-09, Qdrant 1.19.1 localhost gRPC :6334)

Release binary: `cargo build --release -p qql-cli` → `target/release/qql`.
Host is the vs-qdrant box (i5-10400F, 64 GB, `geosmart-qdrant` container).
Originals left in place: `geosmart_berlin_stays`, `nyayarag_legal_precedents`.

| run | result | wall |
|---|---|---:|
| `geosmart_berlin_stays` → copy, wait=true, workers=4, batch=128 | 8317/8317 verified, optimizer restored | **1.784 s** |
| same, `--no-wait --restart` | 8317/8317 verified | **1.635 s** |
| `nyayarag_legal_precedents` ColBERT, workers=2, batch=64 | 2034/2034 verified | **2.654 s** |
| `--where "neighbourhood_group = 'Mitte'"` | 1982/1982 verified | **0.867 s** |
| `--cutover qql_e2e_alias` | alias pointed at copy | 1.793 s |
| `--shard-key-field neighbourhood_group` on standalone | schema-phase error, no hang | **0.014 s** |

Temporary `*_e2e*` copies were dropped after the run.

## Bugs found live and fixed

1. **`max_indexing_threads = 0` / `default_segment_number = 0`** from Qdrant
   schema failed QQL parse (`config_positive_u64`). Dump omits those zeros
   (`dump/schema.rs` `POSITIVE_ONLY_KEYS`).
2. **`--no-wait` verify race**: COUNT immediately after ingest can lag WAL.
   Verify polls (40 × 50 ms) when `wait` is false.
3. **Custom shard keys on standalone hung ingest.** Qdrant returns
   unimplemented for `CreateShardKey` in ~1 ms, but the mpsc producer kept
   scrolling into a full channel while the consumer never returned. Schema
   phase now probes `CREATE SHARD KEY '__qql_migrate_probe__'` with a 15 s
   timeout, maps standalone/unimplemented to a clear error, and the pipeline
   `select!`s so a failed consume cancels the producer.
4. **ColBERT scroll > 4 MiB.** `nyayarag_legal_precedents` pages were 4.23 MiB;
   tonic's default decode limit is 4 MiB. `GrpcQdrant` points/collections
   clients now allow 64 MiB encode/decode.

## Layout

| file | role |
|---|---|
| `mod.rs` | phase machine, Ctrl+C, optimizer restore-on-error |
| `options.rs` | flags, fingerprint, bulk threshold default 2 GB |
| `checkpoint.rs` | atomic JSON; path includes both URLs |
| `schema.rs` | overrides, tenant index, CREATE/ALTER, shard-key probe |
| `pipeline.rs` | mpsc prefetch + N-wide `:rows` upsert |
| `verify.rs` | exact COUNT (+ poll) |
| `cutover.rs` | `QdrantOps::change_aliases` |
| `tests.rs` | unit tests (no live cluster) |

Typed shard keys (`ShardKey::Keyword` / `Number`) go through qql-core parser,
qql-plan `PlanShardKey`, and gRPC `Keyword` vs `Number`. REST serializes
untagged string|number.

## How to re-run the live check

```bash
cargo build --release -p qql-cli
./target/release/qql --url grpc://localhost:6334 \
  migrate geosmart_berlin_stays --to geosmart_copy --recreate --workers 4
./target/release/qql --url grpc://localhost:6334 \
  exec "DROP COLLECTION geosmart_copy"
```

Qdrant must already hold `geosmart_berlin_stays` / `nyayarag_legal_precedents`.

## What is still not done

- **SDK surface**: engine is CLI-only. `pyqql` / `nqql` have no `client.migrate`.
- **QQL alias syntax**: cutover uses `QdrantOps::change_aliases`, not `CREATE ALIAS`.
- **True range-parallel scroll**: producer is still one cursor; workers parallelize
  upserts. Multi-shard targets will benefit more than this single-node test showed.
- **CDC / live writes**: a long migrate against a mutating source will miss
  post-cursor upserts. Snapshots remain the point-in-time tool.
- **Edge aliases**: `change_aliases` default-rejects on edge/custom backends.
- **Custom sharding**: requires a clustered Qdrant. Standalone is fail-closed.

## Commands that must stay green

```bash
cargo fmt --check
cargo clippy -p qql-cli -p qql-core -p qql-plan -p qql --all-targets -- -D warnings
cargo test -p qql-cli -p qql-core -p qql-plan --all-targets
```
