# qql-runtime (crate name `qql`)

Execution engine for QQL. Parses statements via `qql-core`, lowers statements to HTTP/gRPC wire routes via `qql-plan`, performs text-to-vector embedding resolution, and executes operations against Qdrant through an abstract `QdrantOps` trait.

## Three-Layer Architecture

```
qql-core (parse → typed AST → explain → inject_filter)
    ↓
qql-plan (AST → typed RequestBody → Route { method, path, query, body })
    ↓
qql-runtime (resolve_embeddings → execute_route via REST reqwest or gRPC tonic)
```

## QdrantOps Trait

The single, unified backend interface. Implement this trait to connect QQL to any Qdrant backend (REST, gRPC, local edge, or mock):

```rust
use async_trait::async_trait;
use qql_core::error::QqlError;
use qql_plan::routing::Route;
use crate::client::{CollectionInfo, CreateCollectionReq, CreateFieldIndexReq};

#[async_trait]
pub trait QdrantOps: Send + Sync {
    async fn list_collections(&self) -> Result<Vec<String>, QqlError>;
    async fn collection_exists(&self, name: &str) -> Result<bool, QqlError>;
    async fn get_collection_info(&self, name: &str) -> Result<CollectionInfo, QqlError>;
    async fn create_collection(&self, req: CreateCollectionReq) -> Result<(), QqlError>;
    async fn update_collection(&self, req: serde_json::Value) -> Result<(), QqlError>;
    async fn delete_collection(&self, name: &str) -> Result<(), QqlError>;
    async fn create_field_index(&self, req: CreateFieldIndexReq) -> Result<(), QqlError>;

    /// Executes a lowered QQL route against Qdrant (query, scroll, upsert, delete, update).
    async fn execute_route(&self, route: Route) -> Result<serde_json::Value, QqlError>;
}
```

### Provided Implementations

- **`RestQdrant`**: HTTP REST client using `reqwest`. Connects to Qdrant REST API (`http://localhost:6333`).
- **`GrpcQdrant`**: High-performance gRPC client using raw `tonic` 0.14. Connects to Qdrant gRPC API (`http://localhost:6334`).
- **`EdgeQdrant`** (in `qql-edge`): Zero-network in-process vector database backed by `qdrant-edge`.

## Executor

Parses and executes QQL statements with automated embedding resolution:

```rust
use std::sync::Arc;
use qql::executor::Executor;
use qql::rest::RestQdrant;
use qql::embedder::HttpEmbedder;

// 1. Create client and optional HTTP embedder (Ollama / OpenAI / vLLM / TEI)
let rest_ops = Box::new(RestQdrant::new("http://localhost:6333", None));
let embedder = Arc::new(HttpEmbedder::new(
    "http://localhost:11434/v1/embeddings".to_string(),
    "".to_string(),
    "all-minilm:l6-v2".to_string(),
    384,
)?);

let executor = Executor::with_embedder(rest_ops, None, Some(embedder));

// 2. Execute DDL & DML statements
executor.execute("CREATE COLLECTION docs (dense VECTOR(384, COSINE));").await?;

// Auto-embeds text payloads during upsert
executor.execute(
    "UPSERT INTO docs VALUES {id: 1, text: 'vector database'} USING DENSE MODEL 'all-minilm:l6-v2';"
).await?;

// Auto-embeds query text to dense vector
let response = executor.execute("QUERY 'semantic search' FROM docs USING dense LIMIT 5;").await?;
println!("Response: {:?}", response);
```

## Statement Execution Flow

1. **Parse**: Statement string parsed into typed `qql_core::ast::Stmt`.
2. **Embed**: If `Embedder` is registered, `resolve_embeddings` converts query text inputs into dense/sparse vectors and embeds text payloads during upserts.
3. **Route**: `qql_plan::routing::route(&stmt)` lowers the statement into a transport-neutral `Route { method, path, query, body }`.
4. **Dispatch**: `client.execute_route(route)` sends the typed request to Qdrant via HTTP REST or raw gRPC Protobuf mapping.

## Embedding Providers

The `Embedder` trait abstracts text-to-vector generation:

- **`HttpEmbedder`**: OpenAI-compatible REST embedder (`/v1/embeddings`). Supports Ollama, OpenAI, vLLM, Text Embeddings Inference (TEI), and custom local embedding servers. Includes dimension probing (`probe_dimension`).
- **`SparseEmbedder`**: Hash-based BM25 tokenizer for sparse vector generation — pure Rust, zero external dependencies.

## Features

- `default = ["grpc", "rest"]`
- `rest`: Enables HTTP REST client via `reqwest`.
- `grpc`: Enables raw gRPC client via `tonic` 0.14 and generated Protobuf schemas.
