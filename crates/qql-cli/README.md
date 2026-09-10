# qql-cli

CLI + REPL for QQL: remote Qdrant (REST/gRPC), convert, dump, migrate, doctor, optional edge.

## Install

```bash
# Default: rest + grpc
cargo build --release -p qql-cli

# REST only (smaller)
cargo build --release -p qql-cli --no-default-features --features rest

# Edge + FastEmbed (opt-in; heavier)
cargo build --release -p qql-cli --features edge
```

Binary: `target/release/qql`.

## Commands

| Command | Role |
|---------|------|
| `qql exec "…"` | One statement (`--json`, `--quiet`, `--param`, `--params-file`) |
| `qql execute file.qql` | Script |
| `qql explain "…"` | Plan without Qdrant |
| `qql connect` | REPL |
| `qql convert [file.json]` | REST JSON → QQL |
| `qql dump <coll> out.qql` | Export collection as QQL (custom-sharded collections emit `CREATE SHARD KEY` + `SHARD`-routed batches; replay with `qql execute`) |
| `qql migrate <coll> --to <name>` | Version-agnostic collection migration (schema + points) |
| `qql doctor` | Health + embed host snapshot |
| `qql --edge …` | Use configured local edge backend |
| `qql edge optimize <coll>` | Run qdrant-edge optimizers (merge segments, build HNSW/sparse indexes) |
| `qql edge bootstrap <coll> --from <url>` | Seed a local edge collection from a remote shard snapshot |
| `qql version` | Version |

```bash
qql exec "SHOW COLLECTIONS"
qql exec --json "QUERY TEXT 'ml' FROM docs USING dense LIMIT 5"
qql exec "QUERY TEXT :q FROM docs LIMIT :lim" -p q=ml -p lim=5
qql exec "UPSERT INTO docs VALUES :rows WAIT true" --params-file rows.json
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
qql exec "SHOW QUOTAS"
qql exec "SET QUOTA (enabled = true, max_resident_memory_percent = 80, max_disk_usage_percent = 90, release_margin_percent = 5) WAIT true"
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
| `QQL_EDGE_WAL_SEGMENT_MB` | `--wal-segment-mb` | qdrant-edge 32 MiB | WAL segment capacity in MiB (> 0; also exposed by the Python/Node edge SDKs) |
| `EMBED_URL` | `--embed-url` | — | HTTP embedding endpoint |
| `EMBED_KEY` | `--embed-key` | — | HTTP Bearer token |
| `EMBED_MODEL` | `--embed-model` | `nomic-embed-text` | HTTP embedding model ID |
| `EMBED_DIM` | `--embed-dim` | `768` | HTTP embedding vector dimension |
| `MULTI_EMBED_URL` / `MULTI_EMBED_KEY` / `MULTI_EMBED_MODEL` / `MULTI_EMBED_DIM` | `--multi-embed-*` | — | Multi/ColBERT HTTP endpoint |
| `IMAGE_EMBED_URL` / `IMAGE_EMBED_KEY` / `IMAGE_EMBED_MODEL` / `IMAGE_EMBED_DIM` | `--image-embed-*` | — | Image/CLIP HTTP endpoint |

Global: `qql --url http://host:6333 exec "…"`.

### Edge

```bash
qql config edge \
  --data-dir ./qql-data \
  --embedder http \
  --embed-url http://localhost:11434/v1/embeddings \
  --embed-model all-minilm:l6-v2 \
  --embed-dim 384

qql --edge exec "QUERY TEXT 'search' FROM docs USING dense LIMIT 5"
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
| `edge` | no | In-process edge + FastEmbed |

## Docs

- [Syntax](../../docs/syntax.md) · [Install skill](../../skills/qql-skill/references/qql-install.md) · [Gaps](../../skills/qql-skill/references/qql-gaps.md)
