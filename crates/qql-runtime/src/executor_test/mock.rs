use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use qql_core::error::QqlError;
use qql_plan::{QueryBatchRequest, UpdateBatchRequest};

use crate::backend::{SparseVectorSpec, VectorSpec};
use crate::client::{CollectionInfo, QdrantOps};
use crate::config::QqlConfig;
use crate::executor::Executor;
use crate::executor::response::{BackendResponse, ExecData};
use crate::executor::telemetry::{
    HardwareUsage, InferenceUsage, ModelUsage, ServerTelemetry, ServerUsage,
};

pub struct MockQdrantClient {
    pub exists: bool,
    pub collections: Vec<String>,
    pub info: Option<CollectionInfo>,
    pub last_planned: Arc<Mutex<Option<qql_plan::PlannedOperation>>>,
    pub batch_call_count: Arc<Mutex<usize>>,
    pub last_batch_searches_count: Arc<Mutex<usize>>,
    pub last_batch_timeout: Arc<Mutex<Option<u64>>>,
    pub last_batch_consistency: Arc<Mutex<Option<String>>>,
    pub update_batch_call_count: Arc<Mutex<usize>>,
    pub last_update_batch_ops_count: Arc<Mutex<usize>>,
    pub last_update_batch_wait: Arc<Mutex<Option<bool>>>,
    pub execute_planned_call_count: Arc<Mutex<usize>>,
    pub create_collection_call_count: Arc<Mutex<usize>>,
    /// Counts `get_collection_info` calls (schema-fetch accounting for
    /// prepared-statement fast-path tests).
    pub info_call_count: Arc<Mutex<usize>>,
    pub exists_call_count: Arc<Mutex<usize>>,
    pub created_collections: Arc<Mutex<HashSet<String>>>,
    /// Per-collection typed payload returned by `execute_planned` for reads
    /// when present. Key: collection name, Value: the typed [`ExecData`] the
    /// backend "returned" (hits, groups, facet, …).
    pub point_map: Arc<Mutex<HashMap<String, ExecData>>>,
    /// When true, `execute_query_batch` fails (simulating a batch RPC error).
    pub fail_query_batch: Arc<Mutex<bool>>,
    /// When non-zero, `execute_planned` fails on that call number (1-based).
    pub fail_execute_planned_call: Arc<Mutex<usize>>,
    /// When set, `execute_planned` returns it directly instead of the mock's
    /// default per-op typed response (backend-response injection).
    pub typed_response: Arc<Mutex<Option<BackendResponse>>>,
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
            last_batch_timeout: Arc::new(Mutex::new(None)),
            last_batch_consistency: Arc::new(Mutex::new(None)),
            update_batch_call_count: Arc::new(Mutex::new(0)),
            last_update_batch_ops_count: Arc::new(Mutex::new(0)),
            last_update_batch_wait: Arc::new(Mutex::new(None)),
            execute_planned_call_count: Arc::new(Mutex::new(0)),
            info_call_count: Arc::new(Mutex::new(0)),
            exists_call_count: Arc::new(Mutex::new(0)),
            create_collection_call_count: Arc::new(Mutex::new(0)),
            created_collections: Arc::new(Mutex::new(HashSet::new())),
            point_map: Arc::new(Mutex::new(HashMap::new())),
            fail_query_batch: Arc::new(Mutex::new(false)),
            fail_execute_planned_call: Arc::new(Mutex::new(0)),
            typed_response: Arc::new(Mutex::new(None)),
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
    ) -> Result<BackendResponse, QqlError> {
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
        if let Some(response) = self.typed_response.lock().unwrap().clone() {
            return Ok(response);
        }
        if let qql_plan::PlannedOperation::CreateCollection { collection, .. } = op {
            self.created_collections
                .lock()
                .unwrap()
                .insert(collection.clone());
            return Ok(BackendResponse {
                data: ExecData::Mutation { affected: None },
                telemetry: Some(mock_telemetry()),
            });
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
            return Ok(BackendResponse {
                data: ExecData::Collections(self.collections.clone()),
                telemetry: None,
            });
        }
        // Typed per-collection payload when configured.
        if let qql_plan::PlannedOperation::Query { collection, .. }
        | qql_plan::PlannedOperation::QueryGroups { collection, .. }
        | qql_plan::PlannedOperation::Facet { collection, .. }
        | qql_plan::PlannedOperation::GetPoints { collection, .. } = op
            && let Some(data) = self.point_map.lock().unwrap().get(collection)
        {
            return Ok(BackendResponse {
                data: data.clone(),
                telemetry: None,
            });
        }
        // Default typed payload carries server telemetry so the typed
        // telemetry path is exercised end-to-end; the `point_map` path above
        // stays bare on purpose (None-where-absent coverage).
        Ok(BackendResponse {
            data: self.default_data(op),
            telemetry: Some(mock_telemetry()),
        })
    }

    async fn execute_query_batch(
        &self,
        _collection: &str,
        batch: &QueryBatchRequest,
        timeout: Option<u64>,
        consistency: Option<qql_plan::types::ReadConsistencyParam>,
    ) -> Result<Vec<BackendResponse>, QqlError> {
        *self.batch_call_count.lock().unwrap() += 1;
        *self.last_batch_searches_count.lock().unwrap() = batch.searches.len();
        *self.last_batch_timeout.lock().unwrap() = timeout;
        *self.last_batch_consistency.lock().unwrap() = consistency.map(|c| c.to_query_value());
        if *self.fail_query_batch.lock().unwrap() {
            return Err(QqlError::transport(
                "QQL-BACKEND-HTTP",
                "REST 400: batch rejected",
                None,
            ));
        }
        // Empty typed batch results: the mock has no per-search fixtures.
        Ok(batch
            .searches
            .iter()
            .map(|_| BackendResponse {
                data: ExecData::Hits(Vec::new()),
                telemetry: None,
            })
            .collect())
    }

    async fn execute_update_batch(
        &self,
        _collection: &str,
        batch: &UpdateBatchRequest,
        wait: bool,
    ) -> Result<Vec<BackendResponse>, QqlError> {
        *self.update_batch_call_count.lock().unwrap() += 1;
        *self.last_update_batch_ops_count.lock().unwrap() = batch.operations.len();
        *self.last_update_batch_wait.lock().unwrap() = Some(wait);
        Ok(batch
            .operations
            .iter()
            .map(|_| BackendResponse {
                data: ExecData::Mutation { affected: None },
                telemetry: None,
            })
            .collect())
    }
}

impl MockQdrantClient {
    /// Typed default payload per operation, mirroring what a well-formed
    /// backend would answer when no `point_map` fixture is set.
    fn default_data(&self, op: &qql_plan::PlannedOperation) -> ExecData {
        use qql_plan::PlannedOperation;
        match op {
            PlannedOperation::Query { .. }
            | PlannedOperation::Scroll { .. }
            | PlannedOperation::GetPoints { .. } => ExecData::Hits(Vec::new()),
            PlannedOperation::QueryGroups { .. } => ExecData::Groups(Vec::new()),
            PlannedOperation::Count { .. } => ExecData::Count(0),
            PlannedOperation::Facet { .. } => ExecData::Facet(Vec::new()),
            PlannedOperation::ListCollections => ExecData::Collections(self.collections.clone()),
            PlannedOperation::GetCollection { .. } => {
                ExecData::Collection(self.info.clone().unwrap_or_default())
            }
            PlannedOperation::ListShardKeys { .. } => ExecData::ShardKeys(Vec::new()),
            PlannedOperation::GetQuotas => ExecData::Quotas(qql_plan::QuotaConfig::default()),
            PlannedOperation::SetQuotas { request } => ExecData::Quotas(request.config.clone()),
            _ => ExecData::Mutation { affected: None },
        }
    }
}

/// Default mock server telemetry (`time` + hardware/inference usage).
fn mock_telemetry() -> ServerTelemetry {
    ServerTelemetry {
        time_s: Some(0.0025),
        usage: Some(ServerUsage {
            hardware: Some(HardwareUsage {
                cpu: 10,
                payload_io_read: 1,
                payload_io_write: 2,
                payload_index_io_read: 3,
                payload_index_io_write: 4,
                vector_io_read: 5,
                vector_io_write: 6,
            }),
            inference: Some(InferenceUsage {
                models: HashMap::from([("mock-model".to_string(), ModelUsage { tokens: 7 })]),
            }),
        }),
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
