use async_trait::async_trait;
use qql_core::error::QqlError;
use qql_plan::{QueryBatchRequest, UpdateBatchRequest};

pub use crate::backend::{CollectionInfo, Filter as QdrantFilter, PointId, ScoredPoint};

#[derive(Debug, Clone)]
pub struct VectorTopology {
    pub dense_vector: Option<String>,
    pub sparse_vector: Option<String>,
    pub rerank_vector: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PointGroup {
    pub id: serde_json::Value,
    pub hits: Vec<ScoredPoint>,
}

#[cfg(not(target_arch = "wasm32"))]
pub trait QdrantOpsBound: Send + Sync {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Send + Sync> QdrantOpsBound for T {}

#[cfg(target_arch = "wasm32")]
pub trait QdrantOpsBound {}
#[cfg(target_arch = "wasm32")]
impl<T> QdrantOpsBound for T {}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait QdrantOps: QdrantOpsBound {
    /// Flush and release backend resources. Network backends may use the
    /// default no-op; embedded backends should override this method.
    async fn close(&self) -> Result<(), QqlError> {
        Ok(())
    }

    async fn list_collections(&self) -> Result<Vec<String>, QqlError>;
    async fn collection_exists(&self, name: &str) -> Result<bool, QqlError>;
    async fn get_collection_info(&self, name: &str) -> Result<CollectionInfo, QqlError>;
    async fn create_collection(
        &self,
        collection_name: &str,
        req: &qql_plan::CreateCollectionRequest,
    ) -> Result<(), QqlError>;
    async fn update_collection(
        &self,
        collection_name: &str,
        req: &qql_plan::UpdateCollectionRequest,
    ) -> Result<(), QqlError>;
    async fn delete_collection(&self, name: &str) -> Result<(), QqlError>;
    async fn create_field_index(
        &self,
        collection_name: &str,
        req: &qql_plan::CreateIndexRequest,
    ) -> Result<(), QqlError>;
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
