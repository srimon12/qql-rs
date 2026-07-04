# qql-runtime

Execution engine for QQL. Parses statements via `qql-core`, resolves semantic embeddings (local/cloud), and executes operations against Qdrant through an abstract `QdrantOperations` trait.

## QdrantOperations Trait

The core abstraction. Implement this to connect QQL to any Qdrant client (REST, gRPC, mock):

```rust
#[async_trait]
pub trait QdrantOperations: Send + Sync {
    // Collection operations
    async fn list_collections(&self) -> Result<Vec<String>, QqlError>;
    async fn collection_exists(&self, name: &str) -> Result<bool, QqlError>;
    async fn get_collection_info(&self, name: &str) -> Result<CollectionInfo, QqlError>;
    async fn create_collection(&self, req: CreateCollectionReq) -> Result<(), QqlError>;
    async fn update_collection(&self, req: serde_json::Value) -> Result<(), QqlError>;
    async fn delete_collection(&self, name: &str) -> Result<(), QqlError>;

    // Point operations
    async fn upsert(&self, req: UpsertPointsReq) -> Result<(), QqlError>;
    async fn query(&self, req: QueryPointsRequest) -> Result<Vec<ScoredPoint>, QqlError>;
    async fn query_groups(&self, req: QueryPointsGroupsRequest) -> Result<Vec<PointGroup>, QqlError>;
    async fn query_batch(&self, req: Vec<QueryPointsRequest>) -> Result<Vec<Vec<ScoredPoint>>, QqlError>;
    async fn delete(&self, req: DeletePointsReq) -> Result<(), QqlError>;
    async fn update_vectors(&self, req: UpdateVectorsReq) -> Result<(), QqlError>;
    async fn set_payload(&self, req: SetPayloadReq) -> Result<(), QqlError>;
    async fn create_field_index(&self, req: CreateFieldIndexReq) -> Result<(), QqlError>;
    async fn scroll(&self, req: ScrollPointsReq) -> Result<(Vec<Record>, Option<PointId>), QqlError>;
    async fn count(&self, req: CountPointsReq) -> Result<u64, QqlError>;
    async fn get(&self, req: GetPointsReq) -> Result<Vec<Record>, QqlError>;
}
```

## Executor

Parses and executes QQL statements. The main entry point:

```rust
use qql::executor::Executor;
use qql::config::QqlConfig;

// With your QdrantOperations implementation
let executor = Executor::new(Box::new(my_client), Some(config));

// Execute a single statement
let response = executor.execute("QUERY 'search' FROM docs LIMIT 5").await?;

// Execute a batch (stop on error or continue)
let responses = executor.execute_batch(&["INSERT ...", "QUERY ..."], false).await?;

// Batch multiple QUERY statements into a single query_batch call
let responses = executor.query_batch(&[
    "QUERY 'ml' FROM docs LIMIT 10",
    "QUERY 'nlp' FROM docs LIMIT 10",
]).await?;
```

## Filter Conversion (WHERE Clauses)

The `filter_conv` module translates QQL AST `FilterExpr` nodes into the Qdrant REST `Filter` type using the generated OpenAPI types. Supports the full filter grammar: comparisons, set membership, null/empty, text matching, geo, nested conditions, has_vector, values_count, and boolean logic (AND/OR/NOT).

```rust
use qql::filter_conv::FilterConverter;
use qql_core::ast::FilterExpr;

let converter = FilterConverter;
let expr = FilterExpr::Compare { field: "age", op: ">=", value: Value::Int(21) };
let filter = converter.build_filter(&expr)?; // -> Option<crate::qdrant::Filter>
```

## Query Pipeline

The pipeline handles complex QUERY execution: embedding resolution, prefetch DAGs, fusion (RRF/DBSF), reranking, MMR diversification, scoring formulas, and grouped results. Pipeline nodes include:

| Node | Purpose |
|---|---|
| `DenseEmbedNode` | Embed query text via local/cloud model |
| `SparseEmbedNode` | BM25-style sparse embedding |
| `RawVectorNode` | Use pre-provided dense vector |
| `RecommendNode` | Build recommend query from positive/negative IDs |
| `ContextNode` | Build context search from positive/negative pairs |
| `DiscoverNode` | Build discover query (target + context) |
| `OrderByNode` | Build order-by query on payload field |
| `SampleNode` | Build random sample query |
| `FusionNode` | Merge prefetch results (RRF or DBSF) |
| `RerankNode` | Apply ColBERT reranking |
| `RelevanceFeedbackNode` | Build relevance feedback query |

## Embedding

The `Embedder` trait abstracts text-to-vector generation:

- **HttpEmbedder**: Calls any HTTP embedding API endpoint with configurable model, API key, and dimension. Used for both local embedding servers and cloud inference APIs.
- **SparseEmbedder**: BM25-style hash-based tokenizer for sparse vectors — pure Rust, no dependencies.

## Generated Types from OpenAPI

All Qdrant REST API types are generated at compile time from `Qdrant openapi.json` via `typify`:

- `crate::qdrant::Filter`, `Condition`, `FieldCondition`, `Match`, `Range`, `GeoBoundingBox`, etc.
- `crate::qdrant::PointStruct`, `ScoredPoint`, `Record`, `PointGroup`
- `crate::qdrant::Query`, `Prefetch`, `SearchParams`, `WithPayloadInterface`, `WithVector`
- `crate::qdrant::CollectionInfo`, `CollectionConfig`, `VectorsConfig`, `VectorParams`
- `crate::qdrant::SparseVector`, `ExtendedPointId`, `FormulaQuery`

Type aliases are re-exported from the executor module:
- `PointStruct`, `ScoredPoint`, `PointGroup` = generated equivalents
- `RetrievedPoint` = `crate::qdrant::Record`
- `CollectionInfo`, `VectorsConfigType`, `VectorParams`, etc. = generated equivalents

## Sparse Vector Tokenizer

Implements a BM25-style hash-based tokenizer for sparse vector generation without external dependencies. Used by `SparseEmbedNode` and the `SparseEmbedder`.

## Configuration

```rust
pub struct QqlConfig {
    pub url: String,                                              // Qdrant server URL
    pub secret: Option<String>,                                   // API key
    pub inference_mode: String,                                   // "cloud" → Qdrant inference API, anything else → HttpEmbedder
    pub embedding_endpoint: Option<String>,                       // HTTP embedder endpoint (for local/external mode)
    pub embedding_api_key: Option<String>,                        // HTTP embedder API key
    pub embedding_model: Option<String>,                          // HTTP embedder model name
    pub embedding_dimension: usize,                               // Vector dimension
    pub inference_model: Option<String>,                          // Dense model for Qdrant cloud inference
    pub sparse_inference_model: Option<String>,                   // Sparse model for Qdrant cloud inference
    pub cloud_model_options: HashMap<String, String>,             // Extra params for cloud inference
    pub request_timeout: u64,                                     // Request timeout in seconds
    pub bm25_k1: Option<f64>,                                     // BM25 parameter (default: 1.5)
    pub bm25_b: Option<f64>,                                      // BM25 parameter (default: 0.75)
    pub bm25_avg_dl: Option<f64>,                                 // BM25 average doc length
}
```
