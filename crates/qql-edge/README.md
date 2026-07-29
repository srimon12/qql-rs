# qql-edge

Zero-network QQL executor. Combines `qdrant-edge` (in-process HNSW) with the
QQL runtime for fully local vector search — no external Qdrant, no network
hops, no API keys.

Three embedding strategies produce an [`Executor`] backed by [`EdgeQdrant`]:

| Constructor | Embedder | Use case |
|------------|----------|----------|
| `local_executor()` | `FastEmbedder` (ONNX, default BGE small 384-d) | Fully offline dense |
| `local_executor_with_options()` | Dense ONNX + optional **multi** (`multi_model: "bge-m3"`) | Offline dense + ColBERT |
| `http_executor()` / `http_executor_with_multi()` | `HttpEmbedder` (OpenAI-compatible) | Remote dense / multi APIs |
| `custom_executor()` | Any `Arc<dyn Embedder>` | GPU, ensemble, caching |
| `list_embedding_models()` | — | Dense ONNX + multi (BGE-M3) + image (CLIP vision) catalog |

### FastEmbed roles (do not conflate)

| FastEmbed | QQL |
|---|---|
| `TextEmbedding` (BGE, MiniLM, CLIP **text**, …) | Dense (`TEXT`) |
| `ImageEmbedding` (CLIP vision, …) | Dense (`IMAGE` / `image_model`) |
| `Bgem3Embedding.colbert` | **Multi** (`MultiDense`) — offline late interaction / MaxSim `RERANK` |
| `TextRerank` (bge-reranker, …) | **`CROSS RERANK`** pair scorer (client-side; not late-interaction `RERANK`) |

CLIP is dual-encoder dense. Multivector is ColBERT bags only. Late-interaction
`RERANK` uses multi; cross-encoder uses `CROSS RERANK` + `reranker_model`.

```rust
// Offline CLIP text + vision
let mut clip = local_executor_with_options(
    "/tmp/qql-clip",
    LocalExecutorOptions {
        model: Some("ClipVitB32".into()),           // Qdrant/clip-ViT-B-32-text
        image_model: Some("clip-vision".into()),    // Qdrant/clip-ViT-B-32-vision
        ..Default::default()
    },
)?;
// CREATE COLLECTION products (image VECTOR(512, COSINE));
// UPSERT … USING IMAGE MODEL 'clip-vision' ON FIELD image INTO image;
// QUERY TEXT 'red shoe' FROM products USING image LIMIT 10;
// QUERY IMAGE '/query.jpg' FROM products USING image LIMIT 10;
```

## Quick start

```rust
use qql_edge::{local_executor, local_executor_with_options, LocalExecutorOptions};
use qql::executor::OnError;

// Dense-only offline
let mut executor = local_executor("/tmp/qql-edge-data", false)?;
let resp = executor.execute("CREATE COLLECTION docs HYBRID", OnError::Stop).await?;
let resp = executor.execute("UPSERT INTO docs VALUES {id: 1, text: 'hello world'}", OnError::Stop).await?;
let resp = executor.execute("QUERY 'hello' FROM docs LIMIT 5;", OnError::Stop).await?;

// Dense + offline ColBERT multi + cross-encoder (downloads on first use)
let mut multi_exec = local_executor_with_options(
    "/tmp/qql-edge-multi",
    LocalExecutorOptions {
        multi_model: Some("bge-m3".into()),
        reranker_model: Some("bge-reranker-base".into()),
        ..Default::default()
    },
)?;
// CREATE … colbert VECTOR(1024, COSINE) WITH MULTIVECTOR (comparator = 'max_sim')
// QUERY … USING colbert / QUERY RERANK … USING colbert PREFETCH (…)
// QUERY CROSS RERANK TEXT 'q' MODEL 'bge-reranker-base' ON FIELD text PREFETCH (c) …

executor.close().await?; // flush before deleting the data directory
```

## EdgeQdrant backend

[`EdgeQdrant`] implements `QdrantOps` (the unified backend trait from `qql-runtime`)
using `qdrant-edge`'s in-memory HNSW index. Collection data persists to disk at the
configured `base_path`.

### Supported operations

The supported point, mutation, collection, index, query, and batch operations
use the same response envelope as REST and gRPC: `{ "result": ..., "status": "ok", "time": 0.0 }`.
Unsupported clustered or server-only operations return an explicit
`QqlError`; they are never acknowledged as no-ops.

### Response normalization

Mutations return:
```json
{ "result": { "status": "completed" }, "status": "ok", "time": 0.0 }
```

Queries return normalized hit arrays with `id`, `payload`, and `vector` keys.
Batch operations return arrays under `"result"` — cardinality is verified
against the operation count.

### Features

- `fastembed-local`: ONNX-based local embedding via `fastembed-rs` (default)
- `http-embedding`: HTTP-based embedding via `reqwest` (for `http_executor`)

When neither feature is enabled, only `custom_executor()` is available.

`fastembed-local` supports Linux x86-64, Windows x86-64, and Apple Silicon
macOS. ONNX Runtime does not publish the required macOS Intel artifact, so
Intel Mac users should disable default features and use `http-embedding` or
`custom_executor()`.

## Boundaries

- No `UPDATE ... SET VECTOR` via batch — uses individual route dispatch
- gRPC is not available in edge mode (no protobuf dependency)
- Edge uses qdrant-edge's native query engine for nearest, sparse, hybrid,
  MMR (dense), recommendation (`best_score`/`sum_scores`), context, discover,
  sample, formula, relevance-feedback, and order-by queries; point-reference
  and text inputs that cannot be embedded locally are rejected
- Multivector + `CROSS RERANK` work only when the matching models are opted in
- `IMAGE` expects local filesystem paths (no remote URL fetch)
- Query/update “batch” is fan-out, not a single native batch RPC

### Unsupported product surface (stable codes)

Offline rejects use a fixed catalog (`backend/unsupported.rs`). Messages include
**why** and **use remote Qdrant** when applicable:

| Code | Feature |
|---|---|
| `QQL-EDGE-UNSUPPORTED-GROUP-BY` | `GROUP BY` / query groups |
| `QQL-EDGE-UNSUPPORTED-SHARD` | `SHARD` routing or collection sharding options |
| `QQL-EDGE-UNSUPPORTED-SHARD-KEY` | `CREATE`/`DROP SHARD KEY` |
| `QQL-EDGE-UNSUPPORTED-ALTER` | `ALTER COLLECTION` |
| `QQL-EDGE-UNSUPPORTED-COLLECTION-PARAMS` | collection `WITH PARAMS` (replication, …) |
| `QQL-EDGE-UNSUPPORTED-ACORN` | `PARAMS (acorn = …)` |
| `QQL-EDGE-UNSUPPORTED-RECOMMEND-STRATEGY` | `RECOMMEND STRATEGY average_vector` (use `best_score` / `sum_scores`) |
| `QQL-EDGE-UNSUPPORTED-POINT-REF` | point-id query inputs without embedded vectors |
| `QQL-EDGE-UNSUPPORTED-ROUTE` | unmapped REST projection |

Operational errors (`QQL-EDGE-SPAWN`, filter convert, etc.) stay separate.

## Verification

```bash
# Edge tests require the fastembed-local feature and ~1 GB model download
cargo test -p qql-edge --features fastembed-local -- --test-threads=2

# HTTP executor only
cargo test -p qql-edge --features http-embedding -- --test-threads=2
```
