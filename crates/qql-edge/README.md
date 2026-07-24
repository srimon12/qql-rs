# qql-edge

Zero-network QQL executor. Combines `qdrant-edge` (in-process HNSW) with the
QQL runtime for fully local vector search — no external Qdrant, no network
hops, no API keys.

Three embedding strategies produce an [`Executor`] backed by [`EdgeQdrant`]:

| Constructor | Embedder | Use case |
|------------|----------|----------|
| `local_executor()` | `FastEmbedder` (ONNX, default BGE small 384-d) | Fully offline |
| `local_executor_with_options()` | `FastEmbedder` + model/cache selection | Pick ONNX model |
| `http_executor()` | `HttpEmbedder` (OpenAI-compatible) | Local model, remote API |
| `custom_executor()` | Any `Arc<dyn Embedder>` | GPU, ensemble, caching |
| `list_embedding_models()` | — | Discover ONNX models + dims |

## Quick start

```rust
use qql_edge::local_executor;
use qql::executor::OnError;

let mut executor = local_executor("/tmp/qql-edge-data", false)?;
let resp = executor.execute("CREATE COLLECTION docs HYBRID", OnError::Stop).await?;
let resp = executor.execute("UPSERT INTO docs VALUES {id: 1, text: 'hello world'}", OnError::Stop).await?;
let resp = executor.execute("QUERY 'hello' FROM docs LIMIT 5;", OnError::Stop).await?;
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
- `rest`: HTTP-based embedding via `reqwest` (for `http_executor`)

When neither feature is enabled, only `custom_executor()` is available.

## Boundaries

- No `UPDATE ... SET VECTOR` via batch — uses individual route dispatch
- gRPC is not available in edge mode (no protobuf dependency)
- Edge `qdrant-edge` does not support all Qdrant features (e.g., geo-filtering,
  advanced quantization types); operations that require these will fail at
  the `QqlError` level
- Shard keys are not supported in edge mode (no sharding in qdrant-edge)
- `GROUP BY`, `ALTER COLLECTION`, recommendation queries, and shard DDL are
  rejected explicitly in edge mode

## Verification

```bash
# Edge tests require the fastembed-local feature and ~1 GB model download
cargo test -p qql-edge --features fastembed-local -- --test-threads=2

# HTTP executor only
cargo test -p qql-edge --features http-embedding -- --test-threads=2
```
