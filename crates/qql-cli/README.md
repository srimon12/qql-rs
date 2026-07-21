# qql-cli

Command-line interface and interactive REPL for QQL. Connects to Qdrant, executes queries, converts REST payloads, and dumps collections.

## Installation

```bash
cargo build --release -p qql-cli
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

Opens a REPl connected to Qdrant:

```bash
qql connect --url http://localhost:6334
```

Then type QQL directly:
```
qql> SHOW COLLECTIONS;
qql> QUERY 'similar to this' FROM docs LIMIT 10;
qql> UPSERT INTO docs (id, vector, payload) VALUES ...
qql> exit
```

### convert — Convert Qdrant REST JSON payloads to QQL

Reads a REST JSON payload (from file or stdin) and outputs the equivalent QQL statement. Useful for translating existing SDK code or captured API calls:

```bash
# From file
qql convert search_payload.json

# From stdin
echo '{"collection": "docs", "limit": 5, "with_payload": true}' | qql convert -
```

### dump — Export a collection to .qql script

Scans all points in a collection and generates QQL UPSERT statements:

```bash
qql dump docs docs_export.qql
qql dump --batch-size 500 docs docs_export.qql
```

The output file can be replayed with `qql execute`:
```bash
qql execute docs_export.qql
```

### version — Print version info

```bash
qql version
```

## Configuration

Set via environment variables:

| Variable | Default | Description |
|---|---|---|
| `QDRANT_URL` | `http://localhost:6334` | Qdrant gRPC/HTTP URL |
| `INFERENCE_MODE` | `local` | Embedding mode: `local` or `cloud` |
| `CLOUD_MODEL_OPTIONS` | — | JSON object with cloud model params |

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
