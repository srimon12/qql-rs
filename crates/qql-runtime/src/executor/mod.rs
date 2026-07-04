use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json;

use qql_core::ast::{Stmt, Value};
use qql_core::error::QqlError;
use qql_core::parser;

use crate::config::QqlConfig;
use crate::embedder::Embedder;
use crate::filter_conv::QdrantFilter;
use crate::pipeline::{PointId, QueryPointsGroupsRequest, QueryPointsRequest};

pub const DENSE_VECTOR_NAME: &str = "dense";
pub const SPARSE_VECTOR_NAME: &str = "sparse";
pub const RERANK_VECTOR_NAME: &str = "colbert";
pub const DENSE_MODEL_DEFAULT: &str = "sentence-transformers/all-minilm-l6-v2";
pub const SPARSE_MODEL_DEFAULT: &str = "qdrant/bm25";
pub const RERANK_MODEL_DEFAULT: &str = "answerdotai/answerai-colbert-small-v1";
pub const DENSE_VECTOR_SIZE: u64 = 384;
pub const RERANK_VECTOR_SIZE: u64 = 96;
pub const INFERENCE_MODE_DEFAULT: &str = "local";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecResponse {
    pub ok: bool,
    pub operation: String,
    pub message: String,
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub id: String,
    pub score: f32,
    pub text: Option<String>,
    pub payload: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupedSearchResult {
    pub group_id: serde_json::Value,
    pub hits: Vec<SearchHit>,
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PointStruct {
    pub id: PointId,
    pub vector: Option<serde_json::Value>,
    pub payload: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredPoint {
    pub id: PointId,
    pub score: f32,
    pub payload: Option<HashMap<String, serde_json::Value>>,
    pub vector: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PointGroup {
    pub group_id: serde_json::Value,
    pub hits: Vec<ScoredPoint>,
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
    pub options: HashMap<String, Value<'static>>,
}

#[derive(Debug, Clone)]
pub struct ScrollPointsReq {
    pub collection_name: String,
    pub limit: u64,
    pub filter: Option<QdrantFilter>,
    pub after: Option<PointId>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RetrievedPoint {
    pub id: PointId,
    pub payload: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct CountPointsReq {
    pub collection_name: String,
    pub filter: Option<QdrantFilter>,
}

#[derive(Debug, Clone)]
pub struct CollectionInfo {
    pub name: String,
    pub status: String,
    pub points_count: Option<u64>,
    pub indexed_vectors_count: Option<u64>,
    pub segments_count: u64,
    pub config: Option<CollectionConfig>,
    pub payload_schema: HashMap<String, PayloadSchemaInfo>,
}

#[derive(Debug, Clone)]
pub struct CollectionConfig {
    pub params: Option<CollectionParams>,
}

#[derive(Debug, Clone)]
pub struct CollectionParams {
    pub vectors_config: Option<VectorsConfigType>,
    pub sparse_vectors_config: Option<HashMap<String, SparseVectorConfig>>,
    pub shard_number: Option<u64>,
    pub replication_factor: Option<u64>,
    pub write_consistency_factor: Option<u64>,
    pub read_fan_out_factor: Option<u64>,
    pub read_fan_out_delay_ms: Option<u64>,
    pub on_disk_payload: Option<bool>,
}

#[derive(Debug, Clone)]
pub enum VectorsConfigType {
    Single(VectorParams),
    Multi(HashMap<String, VectorParams>),
}

#[derive(Debug, Clone)]
pub struct VectorParams {
    pub size: u64,
    pub distance: String,
    pub on_disk: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct SparseVectorConfig {
    pub modifier: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PayloadSchemaInfo {
    pub data_type: String,
    pub params: Option<serde_json::Value>,
}

#[async_trait]
pub trait QdrantOperations: Send + Sync {
    async fn list_collections(&self) -> Result<Vec<String>, QqlError>;
    async fn collection_exists(&self, name: &str) -> Result<bool, QqlError>;
    async fn get_collection_info(&self, name: &str) -> Result<CollectionInfo, QqlError>;
    async fn create_collection(&self, req: CreateCollectionReq) -> Result<(), QqlError>;
    async fn update_collection(&self, req: serde_json::Value) -> Result<(), QqlError>;
    async fn delete_collection(&self, name: &str) -> Result<(), QqlError>;
    async fn upsert(&self, req: UpsertPointsReq) -> Result<(), QqlError>;
    async fn query(&self, req: QueryPointsRequest) -> Result<Vec<ScoredPoint>, QqlError>;
    async fn query_groups(
        &self,
        req: QueryPointsGroupsRequest,
    ) -> Result<Vec<PointGroup>, QqlError>;
    async fn query_batch(
        &self,
        req: Vec<QueryPointsRequest>,
    ) -> Result<Vec<Vec<ScoredPoint>>, QqlError>;
    async fn delete(&self, req: DeletePointsReq) -> Result<(), QqlError>;
    async fn update_vectors(&self, req: UpdateVectorsReq) -> Result<(), QqlError>;
    async fn set_payload(&self, req: SetPayloadReq) -> Result<(), QqlError>;
    async fn create_field_index(&self, req: CreateFieldIndexReq) -> Result<(), QqlError>;
    async fn scroll(
        &self,
        req: ScrollPointsReq,
    ) -> Result<(Vec<RetrievedPoint>, Option<PointId>), QqlError>;
    async fn count(&self, req: CountPointsReq) -> Result<u64, QqlError>;
    async fn get(&self, req: GetPointsReq) -> Result<Vec<RetrievedPoint>, QqlError>;
}

#[derive(Debug, Clone)]
pub struct GetPointsReq {
    pub collection_name: String,
    pub point_id: Value<'static>,
}

pub struct Executor {
    pub(crate) client: Box<dyn QdrantOperations>,
    pub(crate) config: Option<QqlConfig>,
    pub(crate) embedder: Option<Arc<dyn Embedder>>,
}

impl Executor {
    pub fn new(client: Box<dyn QdrantOperations>, config: Option<QqlConfig>) -> Self {
        Executor {
            client,
            config,
            embedder: None,
        }
    }

    pub fn with_embedder(
        client: Box<dyn QdrantOperations>,
        config: Option<QqlConfig>,
        embedder: Option<Arc<dyn Embedder>>,
    ) -> Self {
        Executor {
            client,
            config,
            embedder,
        }
    }

    pub fn default_context_timeout(&self) -> u64 {
        self.config
            .as_ref()
            .and_then(|c| {
                if c.request_timeout > 0 {
                    Some(c.request_timeout)
                } else {
                    None
                }
            })
            .unwrap_or(30)
    }

    pub fn request_timeout(&self) -> Option<u64> {
        self.config.as_ref().and_then(|c| {
            if c.request_timeout > 0 {
                Some(c.request_timeout)
            } else {
                None
            }
        })
    }

    pub fn parse_query(query: &str) -> Result<Stmt<'_>, QqlError> {
        parser::Parser::parse(query)
    }

    pub async fn execute(&self, query: &str) -> Result<ExecResponse, QqlError> {
        let stmt = Self::parse_query(query)?;
        self.execute_node(stmt).await
    }

    pub async fn execute_node(&self, stmt: Stmt<'_>) -> Result<ExecResponse, QqlError> {
        match stmt {
            Stmt::ShowCollections => self.do_show_collections().await,
            Stmt::ShowCollection(collection) => self.do_show_collection(collection).await,
            Stmt::CreateCollection(n) => self.do_create_collection(*n).await,
            Stmt::AlterCollection(n) => self.do_alter_collection(*n).await,
            Stmt::DropCollection(n) => self.do_drop_collection(n.collection).await,
            Stmt::Insert(n) => self.do_insert(*n).await,
            Stmt::Select(n) => self.do_select(*n).await,
            Stmt::Scroll(n) => self.do_scroll(*n).await,
            Stmt::Query(n) => self.do_query(*n).await,
            Stmt::Delete(n) => self.do_delete(*n).await,
            Stmt::UpdateVector(n) => self.do_update_vector(*n).await,
            Stmt::UpdatePayload(n) => self.do_update_payload(*n).await,
            Stmt::CreateIndex(n) => self.do_create_index(*n).await,
        }
    }
}

pub(crate) mod ddl;
pub(crate) mod dml;
pub(crate) mod helpers;
