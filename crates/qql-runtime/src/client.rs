#[cfg(any(feature = "rest", feature = "grpc"))]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(any(feature = "rest", feature = "grpc"))]
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use qql_core::error::QqlError;
use qql_plan::{QueryBatchRequest, UpdateBatchRequest};

pub use crate::backend::{CollectionInfo, Filter as QdrantFilter, PointId, ScoredPoint};

/// HTTP header / gRPC metadata key Qdrant echoes into its logs for request
/// correlation (`x-request-id`). QQL generates one per request so a
/// misbehaving server response can be traced in Qdrant's own log lines.
pub const REQUEST_ID_HEADER: &str = "x-request-id";

#[cfg(any(feature = "rest", feature = "grpc"))]
static REQUEST_SEQ: AtomicU64 = AtomicU64::new(0);

/// Generate a process-unique request id: `qql-<nanos><seq>` — no external
/// uuid dependency, unique enough for log correlation within a boot.
#[cfg(any(feature = "rest", feature = "grpc"))]
pub(crate) fn next_request_id() -> String {
    let seq = REQUEST_SEQ.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("qql-{nanos:08x}{seq:04x}")
}

/// Named vectors a collection exposes for dense, sparse, and rerank usage.
#[derive(Debug, Clone)]
pub struct VectorTopology {
    /// Default dense vector name, when the collection has named dense vectors.
    pub dense_vector: Option<String>,
    /// Sparse vector name, when the collection defines sparse vectors.
    pub sparse_vector: Option<String>,
    /// Multivector (ColBERT) vector name used for rerank, when present.
    pub rerank_vector: Option<String>,
}

/// Grouped query result: one group key and its ordered hits.
#[derive(Debug, Clone)]
pub struct PointGroup {
    /// Group key value as returned by Qdrant (JSON-typed).
    pub id: serde_json::Value,
    /// Scored points in this group, in backend order.
    pub hits: Vec<ScoredPoint>,
}

#[cfg(not(target_arch = "wasm32"))]
/// Send/Sync bound helper for `QdrantOps` backends on native targets.
pub trait QdrantOpsBound: Send + Sync {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Send + Sync> QdrantOpsBound for T {}

#[cfg(target_arch = "wasm32")]
/// Single-threaded bound helper for `QdrantOps` backends on wasm32.
pub trait QdrantOpsBound {}
#[cfg(target_arch = "wasm32")]
impl<T> QdrantOpsBound for T {}

/// Backend contract every transport adapter implements: DDL and metadata
/// methods, single planned-operation dispatch, and the two batch entry points.
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait QdrantOps: QdrantOpsBound {
    /// Flush and release backend resources. Network backends may use the
    /// default no-op; embedded backends should override this method.
    async fn close(&self) -> Result<(), QqlError> {
        Ok(())
    }

    /// List all collection names on the backend.
    async fn list_collections(&self) -> Result<Vec<String>, QqlError>;
    /// Whether a collection with this name exists.
    async fn collection_exists(&self, name: &str) -> Result<bool, QqlError>;
    /// Fetch typed collection metadata, including the vector/index schema.
    async fn get_collection_info(&self, name: &str) -> Result<CollectionInfo, QqlError>;
    /// Create a collection from a planned DDL request.
    async fn create_collection(
        &self,
        collection_name: &str,
        req: &qql_plan::CreateCollectionRequest,
    ) -> Result<(), QqlError>;
    /// Update collection parameters from a planned DDL request.
    async fn update_collection(
        &self,
        collection_name: &str,
        req: &qql_plan::UpdateCollectionRequest,
    ) -> Result<(), QqlError>;
    /// Drop a collection and all of its points.
    async fn delete_collection(&self, name: &str) -> Result<(), QqlError>;
    /// Create a payload field index on a collection.
    async fn create_field_index(
        &self,
        collection_name: &str,
        req: &qql_plan::CreateIndexRequest,
    ) -> Result<(), QqlError>;
    /// Drop a payload field index from a collection.
    async fn delete_field_index(
        &self,
        collection_name: &str,
        field_name: &str,
    ) -> Result<(), QqlError>;

    /// Execute a pre-planned operation.
    ///
    /// REST backends: `PlannedOperation` → `to_rest_route` → HTTP.
    /// gRPC backends: `PlannedOperation` → protobuf directly.
    async fn execute_planned(
        &self,
        op: &qql_plan::PlannedOperation,
    ) -> Result<serde_json::Value, QqlError>;

    /// Send multiple `QueryRequest`s to the same collection in one network call
    /// via Qdrant's `/points/query/batch` (REST) or `QueryBatch` (gRPC) endpoint.
    async fn execute_query_batch(
        &self,
        collection: &str,
        batch: &QueryBatchRequest,
    ) -> Result<Vec<serde_json::Value>, QqlError>;

    /// Apply a series of point mutations in one network call via Qdrant's
    /// `POST /points/batch` (REST) or `UpdateBatch` (gRPC) endpoint.
    /// Returns one result per operation, in order.
    async fn execute_update_batch(
        &self,
        collection: &str,
        batch: &UpdateBatchRequest,
    ) -> Result<Vec<serde_json::Value>, QqlError>;
}
