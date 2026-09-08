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

| run | result | wall |
|---|---|---:|
| `vsq_berlin_qql` → copy, wait=true, workers=4, batch=128 | 7664/7664 verified | **1.014 s** |
| same, `--no-wait --restart` | 7664/7664 verified | **0.938 s** |
| same, wait=true, workers=1 | 7664/7664 verified | 1.068 s |
| `vsq_legal_qql` ColBERT, workers=2, batch=64 | 2000/2000 verified | **3.056 s** |
| `--where "district = 'Mitte'"` | 593/593 verified | 0.468 s |

Original `vsq_*` collections were not dropped. Temporary `*_migrate*` copies
were removed after the run.

Workers=4 vs 1 barely moved on this **single-shard** node (one update worker).
Expect the `--workers` flag to matter once the **target** has multiple shards.

## Bugs found live and fixed in this pass

1. **`max_indexing_threads = 0` / `default_segment_number = 0`** from Qdrant
   schema failed QQL parse (`config_positive_u64`). Dump now omits zero values
   for those keys (`dump/schema.rs` `POSITIVE_ONLY_KEYS`).
2. **`--no-wait` verify race**: COUNT immediately after ingest saw 7424/7664;
   a moment later the collection was complete. Verify now polls (40 × 50 ms)
   when `wait` is false.

## Layout

| file | role |
|---|---|
| `mod.rs` | phase machine, Ctrl+C, optimizer restore-on-error |
| `options.rs` | flags, fingerprint, bulk threshold default 2 GB |
| `checkpoint.rs` | atomic JSON; path includes both URLs |
| `schema.rs` | overrides, tenant index, CREATE/ALTER |
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
  migrate vsq_berlin_qql --to vsq_berlin_copy --recreate --workers 4
./target/release/qql --url grpc://localhost:6334 \
  exec "DROP COLLECTION vsq_berlin_copy"
```

Qdrant must already hold `vsq_berlin_qql` / `vsq_legal_qql` (vs-qdrant harness).

## What is still not done

- **SDK surface**: engine is CLI-only. `pyqql` / `nqql` have no `client.migrate`.
- **QQL alias syntax**: cutover uses `QdrantOps::change_aliases`, not `CREATE ALIAS`.
- **True range-parallel scroll**: producer is still one cursor; workers parallelize
  upserts. Multi-shard targets will benefit more than this single-node test showed.
- **CDC / live writes**: a long migrate against a mutating source will miss
  post-cursor upserts. Snapshots remain the point-in-time tool.
- **Edge aliases**: `change_aliases` default-rejects on edge/custom backends.

## Commands that must stay green

```bash
cargo fmt --check
cargo clippy -p qql-cli -p qql-core -p qql-plan -p qql --all-targets -- -D warnings
cargo test -p qql-cli -p qql-core -p qql-plan --all-targets
```
