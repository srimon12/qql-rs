# qql-runtime (crate name `qql`)

Execution engine for QQL. Parses via `qql-core`, lowers via `qql-plan`,
resolves embeddings via `qql-embed`, and dispatches to Qdrant through an
abstract `QdrantOps` trait over REST, gRPC, or edge backends.

## Canonical execution flow: prepare → plan → batch → dispatch

```
Statement string
    │
    ▼ [1] Parse (qql-core Parser → Stmt)
    │     USING name without AS → kind: null (not invented at parse time)
    │
    ▼ [2] PREPARE (order matters)
    │   ├─ configure_query_vectors: collection schema → dense/sparse/multi flags
    │   │    (USING sparse embeds sparse; multivector names set multi=true)
    │   ├─ resolve_embeddings: text → Dense | Sparse | MultiDense
    │   │    (unknown kind fails with QQL-VECTOR-KIND — no silent dense default)
    │   └─ ensure_collection_for_upsert: auto-create default schema when needed
    │
    ▼ [3] Plan (qql_plan::plan::plan → PlannedOperation)
    │   └─ Transport: to_rest_route (REST) or match PlannedOperation → protobuf (gRPC)
    │
    ▼ [4] Batch classify (same-collection adjacency)
    │   ├─ 2+ contiguous QUERY → QueryBatch (POST /points/query/batch)
    │   ├─ 2+ contiguous mutations → UpdateBatch (POST /points/batch)
    │   └─ single/other → dispatch_prepared
    │
    ▼ [5] DISPATCH
        ├─ REST: reqwest (Route → JSON)
        ├─ gRPC: tonic (PlannedOperation → protobuf)
        └─ Edge: qdrant-edge (in-process HNSW)
```

This flow is implemented in `Executor::execute_node` and `Executor::execute_batch_nodes`.

## QdrantOps Trait

The single unified backend interface:

```rust
#[async_trait]
pub trait QdrantOps: Send + Sync {
    async fn list_collections(&self) -> Result<Vec<String>, QqlError>;
    async fn collection_exists(&self, name: &str) -> Result<bool, QqlError>;
    async fn get_collection_info(&self, name: &str) -> Result<CollectionInfo, QqlError>;
    async fn create_collection(&self, req: CreateCollectionReq) -> Result<(), QqlError>;
    async fn update_collection(&self, req: serde_json::Value) -> Result<(), QqlError>;
    async fn delete_collection(&self, name: &str) -> Result<(), QqlError>;
    async fn create_field_index(&self, req: CreateFieldIndexReq) -> Result<(), QqlError>;
    async fn delete_field_index(&self, collection_name: &str, field_name: &str) -> Result<(), QqlError>;
    async fn execute_route(&self, route: Route) -> Result<serde_json::Value, QqlError>;
    async fn execute_query_batch(&self, collection: &str, batch: &QueryBatchRequest) -> Result<Vec<Value>, QqlError>;
    async fn execute_update_batch(&self, collection: &str, batch: &UpdateBatchRequest) -> Result<Vec<Value>, QqlError>;
}
```

### Provided Implementations

| Backend | Crate | Transport | Connection |
|---------|-------|-----------|------------|
| `RestQdrant` | `qql` (this crate) | HTTP REST via `reqwest` | `http://localhost:6333` |
| `GrpcQdrant` | `qql` (this crate) | raw gRPC via `tonic` 0.14 | `http://localhost:6334` |
| `EdgeQdrant` | `qql-edge` | in-process HNSW (`qdrant-edge`) | none |

### API key & timeout

```rust
// REST — 30s default
let rest = RestQdrant::new("http://localhost:6333", Some("my-api-key".into()));

// REST — explicit timeout
let rest = RestQdrant::with_timeout("http://localhost:6333", None, Duration::from_secs(60))?;

// gRPC — default timeout (sync constructor, no .await)
let grpc = GrpcQdrant::from_url("http://localhost:6334", Some("my-api-key".into()))?;

// gRPC — explicit timeout (sync constructor, no .await)
let grpc = GrpcQdrant::from_url_with_timeout(
    "http://localhost:6334", None, Some(Duration::from_secs(30)),
)?;
```

API keys are sent via `api-key` header (REST) or `ApiKeyInterceptor` (gRPC tonic metadata).
Pass `None` or `""` for unauthenticated Qdrant.

## Executor

Single entry point for parsing, embedding, batching, and dispatch:

```rust
use std::sync::Arc;
use qql::executor::{Executor, OnError};
use qql::rest::RestQdrant;
use qql::embedder::HttpEmbedder;

let rest_ops = Box::new(RestQdrant::new("http://localhost:6333", None));
let embedder = Arc::new(HttpEmbedder::new(
    "http://localhost:11434/v1/embeddings", "", "all-minilm:l6-v2", 384,
)?);

let executor = Executor::with_embedder(rest_ops, None, Some(embedder));

// DDL
executor.execute("CREATE COLLECTION docs (dense VECTOR(384, COSINE));", OnError::Stop).await?;

// Upsert with auto-embedding
executor.execute(
    "UPSERT INTO docs VALUES {id: 1, text: 'vector database'} USING DENSE MODEL 'all-minilm:l6-v2';",
    OnError::Stop,
).await?;

// Query with auto-embedding
let response = executor.execute(
    "QUERY 'semantic search' FROM docs USING semantic_v2 AS DENSE LIMIT 5;",
    OnError::Stop,
).await?;
```

### prepare_statement — shared preparation

The `prepare_statement` method (called before every `execute_node` and in
`execute_batch_nodes`) performs:

1. **vector target resolution** (`configure_query_vectors`): validates named
   targets against collection schema; fills dense/sparse kinds and multivector
   flags from `multivector_config`. Ambiguous topologies fail closed
   (`QQL-MISSING-USING`). Explicit `AS DENSE|SPARSE|MULTI` is kept when the
   name is not present on a minimal mock schema.
2. **`resolve_embeddings`**: text → dense / sparse / multi-dense (if embedder
   registered). Multivector targets call `Embedder::embed_multi`.
3. **`ensure_collection_for_upsert`**: for UPSERT — auto-creates collection with
   default dense/hybrid (or hybrid+rerank multivector) schema when needed

`CREATE COLLECTION … HYBRID RERANK` materializes conventional `dense` + `sparse`
+ `colbert` (MaxSim multivector) topology.

### Batch execution — strict cardinality

`execute_batch_nodes` groups contiguous same-collection operations into
wire-level batch calls. **Response count must exactly match operation count**:

```
3 contiguous QUERY ops → QueryBatchRequest → must return 3 results
Mismatch → QQL-BATCH-CARDINALITY error
```

This is a strict check — old silent padding behavior has been removed.
Single-statement execution via `execute_node` is unaffected.

## Response envelope normalization

All backends normalize Qdrant responses to a common envelope for the executor:

```rust
pub struct ExecResponse {
    pub ok: bool,
    pub operation: String,   // e.g. "QUERY", "UPSERT"
    pub message: String,
    pub data: Option<Value>,  // "result" from the Qdrant envelope
}
```

The raw Qdrant envelopes are validated:

| Backend | Validation |
|---------|-----------|
| REST | `validate_success_envelope()` — checks `result` present + `status == "ok"` |
| gRPC | `execute_grpc_route()` synthesizes `{ result, status: "ok", time }` from protobuf |
| Edge | `mutation_response()` produces `{ result: { status: "completed" }, status: "ok", time: 0.0 }` |

## Embedding providers

The `Embedder` trait abstracts text-to-vector generation:

- **`HttpEmbedder`**: OpenAI-compatible REST endpoint (`/v1/embeddings`). Supports Ollama,
  OpenAI, vLLM, Text Embeddings Inference (TEI). Includes dimension probing.
- **`SparseEmbedder`**: Hash-based BM25 tokenizer — pure Rust, no external dependencies.

## Features

- `default = ["grpc", "rest"]`
- `rest`: HTTP REST client via `reqwest`
- `grpc`: raw gRPC client via `tonic` 0.14 with generated Protobuf schemas

## Verification

```bash
# Unit tests (no Qdrant needed)
cargo test -p qql --lib -- --test-threads=4

# With Qdrant running on localhost:6333
cargo test -p qql --test integration_test -- --test-threads=1

# Contract tests (REST vs gRPC consistency)
cargo test -p qql contract:: -- --test-threads=1
```
