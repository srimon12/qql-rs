use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use qql_core::error::QqlError;
use qql_plan::{QueryBatchRequest, UpdateBatchRequest};

use crate::backend::{SparseVectorSpec, VectorSpec};
use crate::client::{CollectionInfo, QdrantOps};
use crate::config::QqlConfig;
use crate::executor::Executor;

pub struct MockQdrantClient {
    pub exists: bool,
    pub collections: Vec<String>,
    pub info: Option<CollectionInfo>,
    pub last_planned: Arc<Mutex<Option<qql_plan::PlannedOperation>>>,
    pub batch_call_count: Arc<Mutex<usize>>,
    pub last_batch_searches_count: Arc<Mutex<usize>>,
    pub update_batch_call_count: Arc<Mutex<usize>>,
    pub last_update_batch_ops_count: Arc<Mutex<usize>>,
    pub execute_planned_call_count: Arc<Mutex<usize>>,
    pub create_collection_call_count: Arc<Mutex<usize>>,
    /// Counts `get_collection_info` calls (schema-fetch accounting for
    /// prepared-statement fast-path tests).
    pub info_call_count: Arc<Mutex<usize>>,
    pub exists_call_count: Arc<Mutex<usize>>,
    pub created_collections: Arc<Mutex<HashSet<String>>>,
    /// Per-collection mock points returned by `execute_planned` when non-empty.
    /// Key: collection name, Value: JSON object with a "points" array.
    pub point_map: Arc<Mutex<HashMap<String, serde_json::Value>>>,
    /// When true, `execute_query_batch` fails (simulating a batch RPC error).
    pub fail_query_batch: Arc<Mutex<bool>>,
    /// When non-zero, `execute_planned` fails on that call number (1-based).
    pub fail_execute_planned_call: Arc<Mutex<usize>>,
}

impl Default for MockQdrantClient {
    fn default() -> Self {
        Self {
            exists: false,
            collections: Vec::new(),
            info: None,
            last_planned: Arc::new(Mutex::new(None)),
            batch_call_count: Arc::new(Mutex::new(0)),
            last_batch_searches_count: Arc::new(Mutex::new(0)),
            update_batch_call_count: Arc::new(Mutex::new(0)),
            last_update_batch_ops_count: Arc::new(Mutex::new(0)),
            execute_planned_call_count: Arc::new(Mutex::new(0)),
            info_call_count: Arc::new(Mutex::new(0)),
            exists_call_count: Arc::new(Mutex::new(0)),
            create_collection_call_count: Arc::new(Mutex::new(0)),
            created_collections: Arc::new(Mutex::new(HashSet::new())),
            point_map: Arc::new(Mutex::new(HashMap::new())),
            fail_query_batch: Arc::new(Mutex::new(false)),
            fail_execute_planned_call: Arc::new(Mutex::new(0)),
        }
    }
}

#[async_trait]
impl QdrantOps for MockQdrantClient {
    async fn list_collections(&self) -> Result<Vec<String>, QqlError> {
        Ok(self.collections.clone())
    }
    async fn collection_exists(&self, name: &str) -> Result<bool, QqlError> {
        *self.exists_call_count.lock().unwrap() += 1;
        Ok(self.exists || self.created_collections.lock().unwrap().contains(name))
    }
    async fn get_collection_info(&self, name: &str) -> Result<CollectionInfo, QqlError> {
        *self.info_call_count.lock().unwrap() += 1;
        if let Some(info) = &self.info {
            return Ok(info.clone());
        }
        if self.created_collections.lock().unwrap().contains(name) {
            return Ok(collection_with_vectors(&["dense"], &["sparse"]));
        }
        Err(QqlError::execution(
            "QQL-EXECUTION",
            "no mock info set",
            None,
        ))
    }
    async fn create_collection(
        &self,
        collection_name: &str,
        _req: &qql_plan::CreateCollectionRequest,
    ) -> Result<(), QqlError> {
        *self.create_collection_call_count.lock().unwrap() += 1;
        self.created_collections
            .lock()
            .unwrap()
            .insert(collection_name.to_string());
        Ok(())
    }
    async fn update_collection(
        &self,
        _collection_name: &str,
        _req: &qql_plan::UpdateCollectionRequest,
    ) -> Result<(), QqlError> {
        Ok(())
    }
    async fn delete_collection(&self, _name: &str) -> Result<(), QqlError> {
        Ok(())
    }
    async fn create_field_index(
        &self,
        _collection_name: &str,
        _req: &qql_plan::CreateIndexRequest,
    ) -> Result<(), QqlError> {
        Ok(())
    }
    async fn delete_field_index(
        &self,
        _collection_name: &str,
        _field_name: &str,
    ) -> Result<(), QqlError> {
        Ok(())
    }
    async fn execute_planned(
        &self,
        op: &qql_plan::PlannedOperation,
    ) -> Result<serde_json::Value, QqlError> {
        *self.execute_planned_call_count.lock().unwrap() += 1;
        *self.last_planned.lock().unwrap() = Some(op.clone());
        let call = *self.execute_planned_call_count.lock().unwrap();
        if *self.fail_execute_planned_call.lock().unwrap() == call {
            return Err(QqlError::execution(
                "QQL-EXECUTION",
                "mock failure for this call",
                None,
            ));
        }
        if let qql_plan::PlannedOperation::CreateCollection { collection, .. } = op {
            self.created_collections
                .lock()
                .unwrap()
                .insert(collection.clone());
            return Ok(serde_json::json!({"result": true, "status": "ok", "time": 0.0}));
        }
        let route = qql_plan::plan::to_rest_route(op).expect("rest route");
        if route.path.contains("nonexistent") {
            return Err(QqlError::execution(
                "QQL-EXECUTION",
                "collection does not exist",
                None,
            ));
        }
        if matches!(op, qql_plan::PlannedOperation::ListCollections) {
            return Ok(serde_json::json!({
                "result": {
                    "collections": self
                        .collections
                        .iter()
                        .map(|name| serde_json::json!({"name": name}))
                        .collect::<Vec<_>>(),
                }
            }));
        }
        // Return per-collection mock points when configured.
        if let qql_plan::PlannedOperation::Query { collection, .. }
        | qql_plan::PlannedOperation::QueryGroups { collection, .. }
        | qql_plan::PlannedOperation::Facet { collection, .. }
        | qql_plan::PlannedOperation::GetPoints { collection, .. } = op
            && let Some(points) = self.point_map.lock().unwrap().get(collection)
        {
            return Ok(points.clone());
        }
        Ok(serde_json::json!({"result": {"points": []}}))
    }

    async fn execute_query_batch(
        &self,
        _collection: &str,
        batch: &QueryBatchRequest,
    ) -> Result<Vec<serde_json::Value>, QqlError> {
        *self.batch_call_count.lock().unwrap() += 1;
        *self.last_batch_searches_count.lock().unwrap() = batch.searches.len();
        if *self.fail_query_batch.lock().unwrap() {
            return Err(QqlError::transport(
                "QQL-BACKEND-HTTP",
                "REST 400: batch rejected",
                None,
            ));
        }
        // Real upstream shape (OpenAPI QueryResponse): each batch item
        // carries the points at its top level — no `result` wrapper.
        Ok(batch
            .searches
            .iter()
            .map(|_| serde_json::json!({"points": []}))
            .collect())
    }

    async fn execute_update_batch(
        &self,
        _collection: &str,
        batch: &UpdateBatchRequest,
    ) -> Result<Vec<serde_json::Value>, QqlError> {
        *self.update_batch_call_count.lock().unwrap() += 1;
        *self.last_update_batch_ops_count.lock().unwrap() = batch.operations.len();
        Ok(batch
            .operations
            .iter()
            .map(|op| {
                serde_json::json!({
                    "status": "completed",
                    "operation": op.operation_name(),
                })
            })
            .collect())
    }
}

pub fn test_config() -> QqlConfig {
    QqlConfig {
        inference_mode: "cloud".to_string(),
        ..Default::default()
    }
}

pub fn test_local_config() -> QqlConfig {
    QqlConfig {
        inference_mode: "local".to_string(),
        ..Default::default()
    }
}

pub fn collection_with_vectors(dense: &[&str], sparse: &[&str]) -> CollectionInfo {
    collection_with_vectors_multi(dense, sparse, &[])
}

pub fn collection_with_vectors_multi(
    dense: &[&str],
    sparse: &[&str],
    multivector: &[&str],
) -> CollectionInfo {
    let mut info = CollectionInfo::default();
    info.schema.vectors = dense
        .iter()
        .map(|name| {
            let is_multi = multivector.iter().any(|m| m == name);
            VectorSpec {
                name: Some((*name).to_string()),
                size: if is_multi { 128 } else { 3 },
                distance: "Cosine".to_string(),
                hnsw: None,
                quantization: None,
                multivector: is_multi.then(|| {
                    let mut m = serde_json::Map::new();
                    m.insert("comparator".into(), serde_json::json!("max_sim"));
                    m
                }),
                on_disk: None,
                datatype: None,
                memory: None,
            }
        })
        .collect();
    info.schema.dense_vectors = dense.iter().map(|name| (*name).to_string()).collect();
    info.schema.sparse_vectors = sparse
        .iter()
        .map(|name| SparseVectorSpec {
            name: (*name).to_string(),
            index: None,
            modifier: Some("idf".to_string()),
        })
        .collect();
    info
}

pub struct MockEmbedder {
    pub dense: Vec<f32>,
    pub sparse_indices: Vec<u32>,
    pub sparse_values: Vec<f32>,
    pub multi: Vec<Vec<f32>>,
}

#[async_trait]
impl crate::embedder::Embedder for MockEmbedder {
    async fn embed_dense(&self, _text: &str, _model: &str) -> Result<Vec<f32>, QqlError> {
        Ok(self.dense.clone())
    }
    async fn embed_sparse_query(
        &self,
        _text: &str,
        _model: &str,
    ) -> Result<crate::sparse::SparseVector, QqlError> {
        Ok(crate::sparse::SparseVector {
            indices: self.sparse_indices.clone(),
            values: self.sparse_values.clone(),
        })
    }
    async fn embed_sparse_document(
        &self,
        _text: &str,
        _model: &str,
    ) -> Result<crate::sparse::SparseVector, QqlError> {
        Ok(crate::sparse::SparseVector {
            indices: self.sparse_indices.clone(),
            values: self.sparse_values.clone(),
        })
    }
    async fn embed_multi(&self, _text: &str, _model: &str) -> Result<Vec<Vec<f32>>, QqlError> {
        Ok(self.multi.clone())
    }
    async fn rerank_pairs(
        &self,
        _query: &str,
        documents: &[String],
        _model: &str,
    ) -> Result<Vec<f32>, QqlError> {
        Ok((0..documents.len()).map(|i| 1.0 - i as f32 * 0.1).collect())
    }
}

pub fn point_dict(id: i64, vec: Vec<f32>, tag: &str) -> qql_core::ast::Value {
    qql_core::ast::Value::Dict(vec![
        ("id".to_string(), qql_core::ast::Value::Int(id)),
        ("vector".into(), qql_core::ast::Value::F32Array(vec)),
        ("tag".into(), qql_core::ast::Value::Str(tag.to_string())),
    ])
}

pub type PreparedUpsertHarness = (
    Executor,
    Arc<Mutex<usize>>,
    Arc<Mutex<Option<qql_plan::PlannedOperation>>>,
);

pub fn prepared_upsert_executor() -> PreparedUpsertHarness {
    let client = MockQdrantClient {
        exists: true,
        collections: vec!["docs".to_string()],
        info: Some(collection_with_vectors(&["dense"], &[])),
        ..Default::default()
    };
    let info_count = client.info_call_count.clone();
    let last_planned = client.last_planned.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));
    (executor, info_count, last_planned)
}
