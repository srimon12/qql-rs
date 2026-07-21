use async_trait::async_trait;
use qql_core::ast::Value;
use qql_core::error::QqlError;
use qql_plan::routing::Route;
use std::collections::HashMap;

pub use crate::backend::{CollectionInfo, Filter as QdrantFilter, PointId, ScoredPoint};

#[derive(Debug, Clone)]
pub struct CollectionSchema {
    pub vector_configs: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone)]
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
    pub fn new(collection_name: String) -> Self {
        Self {
            collection_name,
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
pub struct VectorTopology {
    pub dense_vector: Option<String>,
    pub sparse_vector: Option<String>,
    pub rerank_vector: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CreateFieldIndexReq {
    pub collection_name: String,
    pub field: String,
    pub field_type: String,
    pub options: HashMap<String, Value>,
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
    async fn list_collections(&self) -> Result<Vec<String>, QqlError>;
    async fn collection_exists(&self, name: &str) -> Result<bool, QqlError>;
    async fn get_collection_info(&self, name: &str) -> Result<CollectionInfo, QqlError>;
    async fn create_collection(&self, req: CreateCollectionReq) -> Result<(), QqlError>;
    async fn update_collection(&self, req: serde_json::Value) -> Result<(), QqlError>;
    async fn delete_collection(&self, name: &str) -> Result<(), QqlError>;
    async fn create_field_index(&self, req: CreateFieldIndexReq) -> Result<(), QqlError>;
    async fn execute_route(&self, route: Route) -> Result<serde_json::Value, QqlError>;
}
