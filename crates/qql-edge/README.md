# qql-edge

Zero-network QQL executor. Combines `qdrant-edge` (in-process HNSW) with the
QQL runtime for fully local vector search — no external Qdrant, no network
hops, no API keys.

Three embedding strategies produce an [`Executor`] backed by [`EdgeQdrant`]:

| Constructor | Embedder | Use case |
|------------|----------|----------|
| `local_executor()` | `FastEmbedder` (ONNX, local CPU) | Fully offline |
| `http_executor()` | `HttpEmbedder` (OpenAI-compatible) | Local model, remote API |
| `custom_executor()` | Any `Arc<dyn Embedder>` | GPU, ensemble, caching |

## Quick start

```rust
use qql_edge::local_executor;

let mut executor = local_executor("/tmp/qql-edge-data", false)?;
let resp = executor.execute("CREATE COLLECTION docs HYBRID").await?;
let resp = executor.execute("UPSERT INTO docs VALUES {id: 1, text: 'hello world'}").await?;
let resp = executor.execute("QUERY 'hello' FROM docs LIMIT 5;").await?;
```

## EdgeQdrant backend

[`EdgeQdrant`] implements `QdrantOps` (the unified backend trait from `qql-runtime`)
using `qdrant-edge`'s in-memory HNSW index. Collection data persists to disk at the
configured `base_path`.

### Supported operations

All 21 `PlannedOperation` variants, plus `QueryBatchRequest` and
`UpdateBatchRequest`. The edge backend follows the same response envelope
convention as REST and gRPC: `{ "result": ..., "status": "ok", "time": 0.0 }`.

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

## Verification

```bash
# Edge tests require the fastembed-local feature and ~1 GB model download
cargo test -p qql-edge --features fastembed-local -- --test-threads=2

# HTTP executor only
cargo test -p qql-edge --features rest -- --test-threads=2
```
