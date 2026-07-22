# QQL Edge Demo

In-process vector search using qdrant-edge. No Qdrant server, no network.

## Build

The `qql` CLI must be built with default features (includes edge + fastembed):

```bash
cargo build --release -p qql-cli
```

## Run

```bash
# fastembed — local ONNX inference, fully offline
QQL_BIN=./target/release/qql uv run examples/edge-demo/main.py

# Ollama — HTTP embedder
QQL_BIN=./target/release/qql uv run examples/edge-demo/main.py --http

# Dry-run — print QQL statements without executing
QQL_BIN=./target/release/qql uv run examples/edge-demo/main.py --dry-run
```

## What It Demonstrates

| Section | Features |
|---------|----------|
| Schema | CREATE COLLECTION, CREATE INDEX |
| Seed | UPSERT with auto-embedding |
| Search | Dense, Sparse, Hybrid RRF/DBSF, Exact |
| Filters | WHERE eq, IN, year, score threshold, offset |
| CTE | Multi-stage prefetch RRF fusion |
| Recommend | Positive, negative, strategy |
| Access | QUERY POINTS, SCROLL, ORDER BY |
| Mutations | UPDATE payload, DELETE by filter |
| Ops | SHOW COLLECTION, SHOW COLLECTIONS |
