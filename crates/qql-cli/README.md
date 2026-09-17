# qql-cli

CLI + REPL for QQL: remote Qdrant (REST/gRPC), convert, dump, migrate, doctor, optional edge.

## Install

Default binary is lean (`rest` + `grpc` only) — edge (ONNX) and record
(axum) stay opt-in. Prebuilt archives from GitHub releases are default-only;
feature builds install from crates.io with one command (no clone needed):

```bash
# Default: rest + grpc (matches the release archives)
cargo install qql-cli --locked

# REST only (smallest)
cargo install qql-cli --locked --no-default-features --features rest

# Local edge backend, no server (`--edge`, `qql edge`, `qql config edge`)
cargo install qql-cli --locked --features edge

# Local zero-server ONNX embeddings (for remote Qdrant and migrations)
cargo install qql-cli --locked --features fastembed

# In-process embedded Qdrant edge database
cargo install qql-cli --locked --features edge

# Everything at once (fastembed + edge)
cargo install qql-cli --locked --features full

# Standalone REST traffic recorder proxy
cargo install qql-record --locked

# From a local checkout instead
cargo build --release -p qql-cli
cargo build --release -p qql-cli --features fastembed
cargo build --release -p qql-record
```

Check what's installed: `qql version` reports the enabled `features` array.
Binary: `target/release/qql` (or `~/.cargo/bin/qql` for installs).

## Commands

| Command | Role |
|---------|------|
| `qql lint [path]` | Offline syntax + plan check, `--fix` autofix, `--json` for CI |
| `qql run "…"` | One statement (`--json`, `--quiet`, `--param`, `--params-file`) |
| `qql run file.qql` | Script (`--stop-on-error`) |
| `qql explain "…"` | Plan without Qdrant |
| `qql repl` | REPL (alias: `connect`) |
| `qql doctor ["…"]` | Health + embed host snapshot, or 5-stage query triage (alias: `check`) |
| `qql setup` | Connection wizard → `~/.qql/config.json` (`0600`) |
| `qql config …` | Persistent settings (`show`, `get`, `set`, `path`, `edge`) |
| `qql convert [file.json]` | REST JSON → QQL |
| `qql dump <coll> out.qql` | Export collection as QQL (custom-sharded collections emit `CREATE SHARD KEY` + `SHARD`-routed batches; replay with `qql run`) |
| `qql migrate <coll> --to <name>` | Version-agnostic collection migration (schema + points) |
| `qql record` | Transparent REST recorder → JSONL + QQL (delegates to `qql-record`) |
| `qql --edge …` | Use configured local edge backend |
| `qql edge optimize <coll>` | Run qdrant-edge optimizers (merge segments, build HNSW/sparse indexes) |
| `qql edge bootstrap <coll> --from <url>` | Seed a local edge collection from a remote shard snapshot |
| `qql version` | Version |

```bash
qql run "SHOW COLLECTIONS"
qql run --json "QUERY TEXT 'ml' FROM docs USING dense LIMIT 5"
qql run "QUERY TEXT :q FROM docs LIMIT :lim" -p q=ml -p lim=5
qql run "UPSERT INTO docs VALUES :rows WAIT true" --params-file rows.json
qql explain "QUERY TEXT 'ml' FROM docs USING HYBRID LIMIT 5"
qql doctor --json

# Guide: /docs/operations/cluster-migration/ (migrate)
# Guide: /docs/operations/backup-restore/ (dump)
# Cross-version / cross-cluster migrate (schema + points, not snapshots)
qql migrate docs --url http://old:6333 --target-url http://new:6334 --to docs
qql migrate docs --to docs_q --quantize scalar --shard-number 12 --dry-run
qql migrate docs --to docs --restart   # discard a checkpoint and start over
qql migrate docs --to docs_v2 --cutover docs --shard-key-field tenant_id --on-missing-shard-key skip

# Cluster quotas (Qdrant ≥ 1.19, REST only — use :6333, not gRPC :6334)
qql run "SHOW QUOTAS"
qql run "SET QUOTA (enabled = true, max_resident_memory_percent = 80, max_disk_usage_percent = 90, release_margin_percent = 5) WAIT true"
```

## Configuration

### Remote HTTP Embedder (Environment Variables)

| Variable | Default | Role |
|----------|---------|------|
| `QDRANT_URL` | `http://localhost:6333` | REST/gRPC URL (`:6334` selects gRPC when enabled) |
| `QDRANT_API_KEY` | — | Auth |
| `EMBED_URL` | — | OpenAI-compatible embeddings endpoint |
| `EMBED_KEY` | — | Bearer token for the embedding endpoint |
| `EMBED_MODEL` | `all-minilm:l6-v2` | Remote embedding model ID |
| `EMBED_DIM` | `384` | Remote embedding vector dimension |
| `MULTI_EMBED_URL` / `MULTI_EMBED_KEY` / `MULTI_EMBED_MODEL` / `MULTI_EMBED_DIM` | — | Multi/ColBERT embedding endpoint |
| `IMAGE_EMBED_URL` / `IMAGE_EMBED_KEY` / `IMAGE_EMBED_MODEL` / `IMAGE_EMBED_DIM` | — | Image/CLIP embedding endpoint |
| `RERANK_URL` / `RERANK_KEY` / `RERANK_MODEL` | — | Cross-encoder reranking endpoint |

Sparse document encoding is **always local** (`qdrant/bm25`-compatible), even on
the remote path. Tune it in `~/.qql/config.json` via `bm25_k1` / `bm25_b` /
`bm25_avg_len` (defaults `1.2` / `0.75` / `256`; write-path only; invalid values
fail closed with `QQL-VALIDATION-CONFIG`). It applies only when an HTTP embedder
is configured (`EMBED_URL`), since `TEXT` resolution requires an embedder.

### Local Edge Backend (`qql config edge`)

Edge-specific variables start with `QQL_EDGE_`; the `EMBED_*`, `MULTI_EMBED_*`, and
`IMAGE_EMBED_*` variables above are shared with the HTTP embedder.

| Variable | Flag | Default | Role |
|----------|------|---------|------|
| `QQL_EDGE_DATA_DIR` | `--data-dir` | `~/.qql/edge-data` | Directory for persistent edge data |
| `QQL_EDGE_EMBEDDER` | `--embedder` | `fastembed` | Embedder engine (`fastembed` or `http`) |
| `QQL_EDGE_MODEL` | `--model` | `BGESmallENV15` | Dense FastEmbed model ID/alias |
| `QQL_EDGE_SPARSE_MODEL` | `--sparse-model` | — | Offline sparse model (e.g. `splade`) |
| `QQL_EDGE_MULTI_MODEL` | `--multi-model` | — | Offline multi/ColBERT model (e.g. `bge-m3`) |
| `QQL_EDGE_IMAGE_MODEL` | `--image-model` | — | Offline CLIP vision model (e.g. `clip-vision`) |
| `QQL_EDGE_RERANKER_MODEL` | `--reranker-model` | — | Offline cross-encoder (also falls back to `RERANK_MODEL`) |
| `QQL_EDGE_CACHE_DIR` | `--cache-dir` | — | Model download cache directory |
| `QQL_EDGE_ON_DISK` | `--in-memory` | `true` | `true`/`false`/`1`/`0` — payloads on disk |
| `QQL_EDGE_WAL_SEGMENT_MB` | `--wal-segment-mb` | qdrant-edge 32 MiB | WAL segment capacity in MiB (> 0; seeds shards without a persisted capacity, a persisted value wins; also exposed by the Python/Node edge SDKs) |
| `QQL_EDGE_BM25_K1` | `--bm25-k1` | `1.2` | Client-side BM25 `k1` for the local sparse document encoder (write-path only; malformed values fail closed) |
| `QQL_EDGE_BM25_B` | `--bm25-b` | `0.75` | Client-side BM25 `b` length normalization in `[0, 1]` |
| `QQL_EDGE_BM25_AVG_LEN` | `--bm25-avg-len` | `256` | Client-side BM25 expected average document length in tokens |
| `EMBED_URL` | `--embed-url` | — | HTTP embedding endpoint |
| `EMBED_KEY` | `--embed-key` | — | HTTP Bearer token |
| `EMBED_MODEL` | `--embed-model` | `nomic-embed-text` | HTTP embedding model ID |
| `EMBED_DIM` | `--embed-dim` | `768` | HTTP embedding vector dimension |
| `MULTI_EMBED_URL` / `MULTI_EMBED_KEY` / `MULTI_EMBED_MODEL` / `MULTI_EMBED_DIM` | `--multi-embed-*` | — | Multi/ColBERT HTTP endpoint |
| `IMAGE_EMBED_URL` / `IMAGE_EMBED_KEY` / `IMAGE_EMBED_MODEL` / `IMAGE_EMBED_DIM` | `--image-embed-*` | — | Image/CLIP HTTP endpoint |

Global: `qql --url http://host:6333 run "…"`.

### Edge

```bash
qql config edge \
  --data-dir ./qql-data \
  --embedder http \
  --embed-url http://localhost:11434/v1/embeddings \
  --embed-model all-minilm:l6-v2 \
  --embed-dim 384

qql --edge run "QUERY TEXT 'search' FROM docs USING dense LIMIT 5"
qql --edge doctor
qql edge optimize docs
qql edge bootstrap docs --from http://localhost:6333
```

Config: `~/.qql/edge.json`. Edge does **not** support custom `SHARD` / `CREATE SHARD KEY` or
**`SHOW QUOTAS` / `SET QUOTA`** — use remote Qdrant (REST) for those. `GROUP BY` (without
`LOOKUP FROM`), sparse `PARAMS (idf = …)`, and ACORN are available offline (qdrant-edge 0.8+).

Edge → edge `migrate` is rejected before any executor starts; publish edge data to a remote
target with `qql --edge migrate <coll> --target-url <url>` (or `--source-edge`), and seed other
devices with `qql edge bootstrap`. Continuous sync is not provided (dual-write + partial
snapshots per the edge sync guide). Run `qql edge optimize` after bulk writes — the engine has
no background optimizer.

## Multitenancy examples

```sql
CREATE SHARD KEY 'acme' ON COLLECTION docs WITH (shards_number = 2);

QUERY TEXT 'risks' FROM docs USING dense
WHERE tenant_id = 'acme'
SHARD 'acme'
LIMIT 10;
```

## Recorder (`qql-record`, standalone tool)

Zero-code-change capture for migration: Qdrant keeps its address, point the
app at the recorder instead, change nothing else. Every request is forwarded
to `--target` (status, headers, body — auth included; repeated headers are
preserved); collection and quota routes are appended as wrapped
`{"method","path","query"?,"body"?}` JSONL for later `qql convert` use.
Bodyless `SHOW` / `DROP` routes are recorded with no `body`.

`qql-record` is available as a standalone tool (`cargo install qql-record`), and
`qql record` will automatically delegate to it if installed on your `$PATH`.

```bash
cargo install qql-record --locked
# Qdrant stays on :6333, the app now points at the recorder on :6334:
qql-record --listen 127.0.0.1:6334 --target http://127.0.0.1:6333 \
  --out capture.jsonl --qql-out capture.qql
# ... run the app ...
qql convert --collection docs capture.jsonl   # replay/migrate later
```

Bare `qql-record` uses those defaults. Notes: query strings are forwarded
upstream and recorded as a `"query"` object (`wait`, `timeout`,
`consistency`) so `qql convert` recovers `WAIT` and `PARAMS`; a trailing `/`
is stripped from the recorded path only; non-JSON bodies are forwarded, not
recorded; bodies are buffered in RAM (multi-hundred-MB single upserts sit in
memory — fine for ColBERT-size batches); `--qql-out` converts at record time
and failures become `-- ERROR <file:line> <error>` comments (valid QQL
comments, so the capture replays without hand-editing). Ctrl-C stops
the recorder; files are fsynced per line so nothing is lost.

## Script format

Semicolon-separated statements. `--` comments OK.

```qql
CREATE COLLECTION docs (
  dense VECTOR(384, COSINE)
    WITH VECTOR (memory = 'cached', datatype = 'float16')
) WITH HNSW (memory = 'cold')
  WITH PARAMS (payload_memory = 'cached');

CREATE INDEX ON COLLECTION docs FOR title TYPE keyword
  WITH (prefix = true, memory = 'cached');

UPSERT INTO docs VALUES {id: 1, text: 'first document'}
  USING DENSE MODEL 'all-minilm:l6-v2';

QUERY TEXT 'search' FROM docs USING dense
WHERE title MATCH PREFIX 'fir'
LIMIT 10;
```

## Features

| Feature | Default | Role |
|---------|---------|------|
| `rest` | yes | REST |
| `grpc` | yes | gRPC |
| `fastembed` | no | Local zero-server ONNX embeddings (for remote Qdrant and migrations) |
| `edge` | no | In-process embedded Qdrant edge database |
| `full` | no | Convenience alias for `fastembed,edge` |

## Docs

- [Syntax](https://github.com/srimon12/qql-rs/blob/main/docs/syntax.md) · [Install skill](https://github.com/srimon12/qql-rs/blob/main/skills/qql-skill/references/qql-install.md) · [Gaps](https://github.com/srimon12/qql-rs/blob/main/skills/qql-skill/references/qql-gaps.md)
