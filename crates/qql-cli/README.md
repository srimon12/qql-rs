# qql-cli

CLI + REPL for QQL: remote Qdrant (REST/gRPC), convert, dump, doctor, optional edge.

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
| `qql exec "…"` | One statement (`--json`, `--quiet`) |
| `qql execute file.qql` | Script |
| `qql explain "…"` | Plan without Qdrant |
| `qql connect` | REPL |
| `qql convert [file.json]` | REST JSON → QQL |
| `qql dump <coll> out.qql` | Export collection as QQL |
| `qql doctor` | Health + embed host snapshot |
| `qql --edge …` | Use configured local edge backend |
| `qql version` | Version |

```bash
qql exec "SHOW COLLECTIONS"
qql exec --json "QUERY TEXT 'ml' FROM docs USING dense LIMIT 5"
qql explain "QUERY TEXT 'ml' FROM docs USING HYBRID LIMIT 5"
qql doctor --json
```

## Configuration

### Remote HTTP Embedder (Environment Variables)

| Variable | Default | Role |
|----------|---------|------|
| `QDRANT_URL` | `http://localhost:6333` | REST/gRPC URL (`:6334` selects gRPC when enabled) |
| `QDRANT_API_KEY` | — | Auth |
| `EMBED_URL` | — | OpenAI-compatible embeddings endpoint |
| `EMBED_MODEL` | `all-minilm:l6-v2` | Remote embedding model ID |
| `EMBED_DIM` | `384` | Remote embedding vector dimension |

### Local Edge Backend (`qql config edge`)

| Setting / Variable | Default (FastEmbed) | Default (HTTP) | Role |
|--------------------|---------------------|----------------|------|
| `EDGE_DATA_DIR` / `--data-dir` | `/tmp/qql-edge` | `/tmp/qql-edge` | Directory for persistent edge data |
| `EMBEDDER` / `--embedder` | `fastembed` | — | Embedder engine (`fastembed` or `http`) |
| `EMBED_MODEL` / `--model` | `BGESmallENV15` | `nomic-embed-text` | Dense model ID/alias |
| `EMBED_DIM` / `--embed-dim` | `384` | `768` | Dense vector dimension |

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
```

Config: `~/.qql/edge.json`. Edge does **not** support custom `SHARD` / `CREATE SHARD KEY`
or `GROUP BY` — use remote Qdrant for those.

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
CREATE COLLECTION docs (dense VECTOR(384, COSINE));
UPSERT INTO docs VALUES {id: 1, text: 'first document'}
  USING DENSE MODEL 'all-minilm:l6-v2';
QUERY TEXT 'search' FROM docs USING dense LIMIT 10;
```

## Features

| Feature | Default | Role |
|---------|---------|------|
| `rest` | yes | REST |
| `grpc` | yes | gRPC |
| `edge` | no | In-process edge + FastEmbed |

## Docs

- [Syntax](../../docs/syntax.md) · [Install skill](../../skills/qql-skill/references/qql-install.md) · [Gaps](../../skills/qql-skill/references/qql-gaps.md)
