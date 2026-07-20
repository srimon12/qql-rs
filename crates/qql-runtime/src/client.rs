use async_trait::async_trait;
use qql_core::ast::Value;
use qql_core::error::QqlError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::pipeline::{QueryPointsGroupsRequest, QueryPointsRequest};

pub use crate::backend::{
    CollectionInfo, Filter as QdrantFilter, Point as PointStruct, PointGroup, PointId,
    RetrievedPoint, ScoredPoint,
};

#[derive(Debug, Clone)]
pub struct VectorTopology {
    pub dense_vector: Option<String>,
    pub sparse_vector: Option<String>,
    pub rerank_vector: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateCollectionReq {
    pub collection_name: String,
    pub vectors_config: Option<serde_json::Value>,
    pub sparse_vectors_config: Option<serde_json::Value>,
    pub hnsw_config: Option<serde_json::Value>,
    pub optimizers_config: Option<serde_json::Value>,
    pub quantization_config: Option<serde_json::Value>,
    pub params: Option<serde_json::Value>,
}

impl CreateCollectionReq {
    pub fn new(name: String) -> Self {
        CreateCollectionReq {
            collection_name: name,
            vectors_config: None,
            sparse_vectors_config: None,
            hnsw_config: None,
            optimizers_config: None,
            quantization_config: None,
            params: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UpsertPointsReq {
    pub collection_name: String,
    pub points: Vec<PointStruct>,
}

#[derive(Debug, Clone)]
pub struct DeletePointsReq {
    pub collection_name: String,
    pub filter: Option<QdrantFilter>,
    pub point_id: Option<PointId>,
}

#[derive(Debug, Clone)]
pub struct UpdateVectorsReq {
    pub collection_name: String,
    pub point_id: PointId,
    pub vector: Vec<f32>,
    pub vector_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SetPayloadReq {
    pub collection_name: String,
    pub point_id: Option<PointId>,
    pub filter: Option<QdrantFilter>,
    pub payload: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct CreateFieldIndexReq {
    pub collection_name: String,
    pub field: String,
    pub field_type: String,
    pub options: HashMap<String, Value>,
}

#[derive(Debug, Clone)]
pub struct ScrollPointsReq {
    pub collection_name: String,
    pub limit: u64,
    pub filter: Option<QdrantFilter>,
    pub after: Option<PointId>,
}

#[derive(Debug, Clone)]
pub struct CountPointsReq {
    pub collection_name: String,
    pub filter: Option<QdrantFilter>,
}

#[derive(Debug, Clone)]
pub struct GetPointsReq {
    pub collection_name: String,
    pub point_id: Value,
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
pub trait QdrantCoreOps: QdrantOpsBound {
    async fn list_collections(&self) -> Result<Vec<String>, QqlError>;
    async fn collection_exists(&self, name: &str) -> Result<bool, QqlError>;
    async fn get_collection_info(&self, name: &str) -> Result<CollectionInfo, QqlError>;
    async fn create_collection(&self, req: CreateCollectionReq) -> Result<(), QqlError>;
    async fn upsert(&self, req: UpsertPointsReq) -> Result<(), QqlError>;
    async fn query(&self, req: QueryPointsRequest) -> Result<Vec<ScoredPoint>, QqlError>;
    async fn query_groups(
        &self,
        req: QueryPointsGroupsRequest,
    ) -> Result<Vec<PointGroup>, QqlError>;
    async fn delete(&self, req: DeletePointsReq) -> Result<(), QqlError>;
    async fn update_vectors(&self, req: UpdateVectorsReq) -> Result<(), QqlError>;
    async fn set_payload(&self, req: SetPayloadReq) -> Result<(), QqlError>;
    async fn scroll(
        &self,
        req: ScrollPointsReq,
    ) -> Result<(Vec<RetrievedPoint>, Option<PointId>), QqlError>;
    async fn get(&self, req: GetPointsReq) -> Result<Vec<RetrievedPoint>, QqlError>;
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait QdrantAdminOps: QdrantOpsBound {
    async fn update_collection(&self, _req: serde_json::Value) -> Result<(), QqlError> {
        Err(QqlError::runtime("update_collection not supported"))
    }
    async fn delete_collection(&self, _name: &str) -> Result<(), QqlError> {
        Err(QqlError::runtime("delete_collection not supported"))
    }
    async fn query_batch(
        &self,
        _req: Vec<QueryPointsRequest>,
    ) -> Result<Vec<Vec<ScoredPoint>>, QqlError> {
        Err(QqlError::runtime("query_batch not supported"))
    }
    async fn create_field_index(&self, _req: CreateFieldIndexReq) -> Result<(), QqlError> {
        Err(QqlError::runtime("create_field_index not supported"))
    }
    async fn count(&self, _req: CountPointsReq) -> Result<u64, QqlError> {
        Err(QqlError::runtime("count not supported"))
    }
}

pub trait QdrantOps: QdrantCoreOps + QdrantAdminOps {}
impl<T: QdrantCoreOps + QdrantAdminOps> QdrantOps for T {}
