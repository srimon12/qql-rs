# QQL Edge Demo

Showcases **qdrant-edge** — a fully in-process vector search engine — through the
`qql` CLI. No Qdrant server, no Docker, no network (unless you use an external
embedding provider).

---

## What runs where

```
┌────────────────────────────────────┐
│  your process / CLI                │
│                                    │
│  qql edge "QUERY …" --data-dir … │
│       │                            │
│  ┌────▼────────────────────────┐  │
│  │  qql-edge (in-process)      │  │
│  │  ┌──────────┐ ┌──────────┐  │  │
│  │  │ Embedder │ │  HNSW    │  │  │
│  │  │ (local / │ │  index   │  │  │
│  │  │  HTTP)   │ │ on disk  │  │  │
│  │  └──────────┘ └──────────┘  │  │
│  └─────────────────────────────┘  │
└────────────────────────────────────┘
```

---

## Embedder modes

| `--embedder` | How it works | Network? |
|---|---|---|
| `fastembed` *(default)* | Local ONNX inference via fastembed-rs. Model downloaded once and cached. | ❌ None after first download |
| `http` | Calls any **OpenAI-compatible** `/v1/embeddings` endpoint. Works with Ollama, OpenAI, Cohere, Together AI, Mistral, etc. | ✅ To your provider only |

---

## Quick start

### fastembed (fully offline)

```bash
# Run all demo steps — downloads the BGE-Small model on first run
bash run-edge-demo.sh

# Or via Python
python main.py
```

### HTTP provider — Ollama (local LLM server)

```bash
# Start Ollama and pull a model
ollama pull nomic-embed-text

# Run demo with Ollama
EMBEDDER=http \
EMBED_URL=http://localhost:11434/v1/embeddings \
EMBED_MODEL=nomic-embed-text \
EMBED_DIM=768 \
bash run-edge-demo.sh
```

### HTTP provider — OpenAI

```bash
EMBEDDER=http \
EMBED_URL=https://api.openai.com/v1/embeddings \
EMBED_KEY=sk-... \
EMBED_MODEL=text-embedding-3-small \
EMBED_DIM=1536 \
bash run-edge-demo.sh
```

---

## Environment variables

| Variable | Default | Description |
|---|---|---|
| `QQL_BIN` | `target/debug/qql` | Path to the `qql` binary |
| `EMBEDDER` | `fastembed` | Embedder backend: `fastembed` or `http` |
| `EMBED_URL` | *(required for http)* | HTTP embedder endpoint URL |
| `EMBED_KEY` | `""` | API key (empty = no auth, e.g. Ollama) |
| `EMBED_MODEL` | `nomic-embed-text` | Model name sent in request body |
| `EMBED_DIM` | `768` | Expected embedding dimension |
| `EDGE_DATA_DIR` | `/tmp/qql-edge-demo` | Where qdrant-edge stores its data |

---

## CLI usage directly

```bash
QQL=/path/to/qql

# Create a collection (no server!)
$QQL edge "CREATE COLLECTION docs HYBRID" --data-dir /tmp/mydata

# Upsert with local fastembed
$QQL edge "UPSERT INTO docs VALUES {'id': 1, 'text': 'hello world', 'tag': 'test'} USING HYBRID" \
    --data-dir /tmp/mydata

# Query
$QQL edge "QUERY 'hello' FROM docs LIMIT 3" --data-dir /tmp/mydata --json

# Use Ollama instead
$QQL edge "QUERY 'hello' FROM docs LIMIT 3" \
    --data-dir /tmp/mydata \
    --embedder http \
    --embed-url http://localhost:11434/v1/embeddings \
    --embed-model nomic-embed-text \
    --embed-dim 768
```

---

## Demo coverage

| Step | Operations |
|---|---|
| Provision | `CREATE COLLECTION HYBRID`, `CREATE INDEX` |
| Seed | `UPSERT INTO … USING HYBRID` |
| Search | Dense, Sparse BM25, Hybrid RRF, Hybrid DBSF, Parameterized RRF, MMR, Exact |
| Filters | `WHERE tag = …`, `WHERE year = …`, `WHERE tag IN (…)`, score threshold, offset |
| CTE Prefetch | `WITH a AS (…), b AS (…) … PREFETCH (a, b) FUSION RRF` |
| Recommend | `QUERY RECOMMEND WITH (positive = …, negative = …)` |
| Point Access | `SELECT * WHERE id = …`, `SCROLL`, `SCROLL WHERE …` |
| Mutations | `UPDATE … SET`, `DELETE FROM … WHERE` |
| Inspect | `SHOW COLLECTION`, `SHOW COLLECTIONS` |

---

## Async architecture

`qql-edge` is **fully async-compatible**:

- All `qdrant-edge` disk I/O is offloaded via `tokio::task::spawn_blocking`
- All `fastembed` ONNX inference runs in the blocking thread pool
- The user-facing `Executor::execute().await` is a standard async call
- WASM32 target is supported via `async_trait(?Send)`
