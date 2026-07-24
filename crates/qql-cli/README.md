# qql-cli

Command-line interface and interactive REPL for QQL. Connects to Qdrant, executes queries, converts REST payloads, dumps collections, and runs an in-process qdrant-edge backend.

## Installation

```bash
# Default build (grpc + rest + edge)
cargo build --release -p qql-cli

# REST-only (smaller binary, no gRPC or edge dependencies)
cargo build --release -p qql-cli --no-default-features --features rest

# binary at target/release/qql
```

## Commands

### exec — Run a single QQL query

```bash
qql exec "SHOW COLLECTIONS"
qql exec --json "QUERY 'machine learning' FROM docs LIMIT 5"
```

### execute — Run multiple queries from a .qql script file

```bash
qql execute script.qql
qql execute --stop-on-error migrate.qql
```

### explain — Show execution plan without running

```bash
qql explain "QUERY 'search' FROM docs LIMIT 10"
```

### connect — Start interactive REPL

Opens a REPL connected to Qdrant:

```bash
qql connect --url http://localhost:6333
```

Then type QQL directly:
```
qql> SHOW COLLECTIONS;
qql> QUERY 'similar to this' FROM docs LIMIT 10;
qql> UPSERT INTO docs (id, vector, payload) VALUES ...
qql> exit
```

Built-in REPL commands: `help`, `explain <query>`, `execute <file>`, `dump <name> <file>`, `exit`/`quit`.

### convert — Convert Qdrant REST JSON payloads to QQL

Reads a REST JSON payload (from file or stdin) and outputs the equivalent QQL statement:

```bash
qql convert search_payload.json
echo '{"collection": "docs", "limit": 5, "with_payload": true}' | qql convert
```

### dump — Export a collection to .qql script

Full collection export as a replayable `.qql` script:

1. `CREATE COLLECTION` reconstructed from live vector schema (size, distance, sparse)
2. `CREATE INDEX` statements from payload indexes (when reported by Qdrant)
3. Batched `UPSERT` statements with real `vector:` values (not re-embed stubs)

Uses cursor pagination (`AFTER` / `next_page_offset`) and requests vectors on every
scroll page. Safe for multi-batch collections and streams to disk.

```bash
qql dump docs docs_export.qql
qql dump --batch-size 500 docs docs_export.qql
qql dump docs out.qql --json   # machine-readable stats
```

Reload with:

```bash
qql execute docs_export.qql
```

### doctor — Check Qdrant connection health

```bash
qql doctor
qql doctor --json
```

### edge — Run QQL against local qdrant-edge (no server)

Requires `edge` feature (included in default build). Embeddings via local ONNX (`fastembed`) or HTTP provider:

```bash
# fastembed — local ONNX inference, fully offline
qql edge "QUERY 'vector search' FROM docs USING dense LIMIT 5" --data-dir /tmp/my-edge

# HTTP embedder (Ollama, OpenAI, etc.)
qql edge "QUERY 'search' FROM docs LIMIT 5" --embedder http \
  --embed-url http://localhost:11434/v1/embeddings \
  --embed-model nomic-embed-text --embed-dim 768
```

### version — Print version info

```bash
qql version
```

## Configuration

```bash
# Global flag (overrides QDRANT_URL)
qql --url http://localhost:6333 exec "SHOW COLLECTIONS"
```

Set via environment variables:

| Variable | Default | Description |
|---|---|---|
| `QDRANT_URL` | `http://localhost:6333` | Qdrant REST/gRPC URL |
| `QDRANT_API_KEY` | — | Qdrant API key for authenticated access |
| `EMBED_URL` | — | HTTP embedder endpoint (Ollama, OpenAI, TEI, etc.) |
| `EMBED_KEY` | — | API key for HTTP embedder |
| `EMBED_MODEL` | `all-minilm:l6-v2` | Embedding model name |
| `EMBED_DIM` | `384` | Expected embedding dimension |

Persistent config loaded from `~/.qql/config.json` (auto-created on first use). Fields mirror `QqlConfig`:

```json
{
  "url": "http://localhost:6333",
  "secret": null,
  "embedding_endpoint": "http://localhost:11434/v1/embeddings",
  "embedding_model": "nomic-embed-text",
  "embedding_dimension": 768
}
```

## Feature Flags

| Feature | Default | Description |
|---------|---------|-------------|
| `rest` | yes | Qdrant REST API transport |
| `grpc` | yes | Qdrant gRPC transport (auto-selected when URL contains `:6334`) |
| `edge` | yes | In-process qdrant-edge backend (no server required) |

## .qql Script Format

Statements are separated by semicolons. Supports all QQL statements:

```qql
CREATE COLLECTION docs WITH VECTOR size 384 distance Cosine;
UPSERT INTO docs (id, vector, payload) VALUES
    (1, [0.1, 0.2, ...], {"text": "first document"}),
    (2, [0.3, 0.4, ...], {"text": "second document"});
QUERY 'search' FROM docs LIMIT 10;
```

Comments with `--` and blank lines are ignored.
