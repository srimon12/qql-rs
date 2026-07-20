#![allow(clippy::field_reassign_with_default)]

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;
use qql_core::error::QqlError;

use crate::config::QqlConfig;
use crate::executor::{
    CollectionInfo, CountPointsReq, CreateCollectionReq, CreateFieldIndexReq, DeletePointsReq,
    Executor, GetPointsReq, PointGroup, QdrantOps, RetrievedPoint, ScoredPoint, ScrollPointsReq,
    SetPayloadReq, UpdateVectorsReq, UpsertPointsReq,
};
use crate::pipeline::PointId;

struct MockQdrantClient {
    pub exists: bool,
    pub collections: Vec<String>,
    pub info: Option<CollectionInfo>,
    pub get_records: Vec<RetrievedPoint>,
    pub scroll_records: Vec<RetrievedPoint>,
    pub scroll_offset: Option<PointId>,
    pub last_create_collection: Arc<Mutex<Option<CreateCollectionReq>>>,
    pub last_update_collection: Arc<Mutex<Option<serde_json::Value>>>,
    pub last_upsert: Arc<Mutex<Option<UpsertPointsReq>>>,
    pub last_delete: Arc<Mutex<Option<DeletePointsReq>>>,
    pub last_update_vectors: Arc<Mutex<Option<UpdateVectorsReq>>>,
    pub last_set_payload: Arc<Mutex<Option<SetPayloadReq>>>,
    pub last_query: Arc<Mutex<Option<crate::pipeline::QueryPointsRequest>>>,
}

impl Default for MockQdrantClient {
    fn default() -> Self {
        Self {
            exists: false,
            collections: Vec::new(),
            info: None,
            get_records: Vec::new(),
            scroll_records: Vec::new(),
            scroll_offset: None,
            last_create_collection: Arc::new(Mutex::new(None)),
            last_update_collection: Arc::new(Mutex::new(None)),
            last_upsert: Arc::new(Mutex::new(None)),
            last_delete: Arc::new(Mutex::new(None)),
            last_update_vectors: Arc::new(Mutex::new(None)),
            last_set_payload: Arc::new(Mutex::new(None)),
            last_query: Arc::new(Mutex::new(None)),
        }
    }
}

fn mock_collection_info() -> CollectionInfo {
    let val = serde_json::json!({
        "status": "green",
        "optimizer_status": "ok",
        "segments_count": 0,
        "payload_schema": {},
        "config": {
            "params": {
                "vectors": {
                    "size": 384,
                    "distance": "Cosine"
                },
                "sparse_vectors": {},
                "replication_factor": 1,
                "shard_number": 1,
                "write_consistency_factor": 1
            },
            "hnsw_config": {
                "m": 16,
                "ef_construct": 100,
                "full_scan_threshold": 10000
            },
            "optimizer_config": {
                "deleted_threshold": 0.2,
                "vacuum_min_vector_number": 1000,
                "full_scan_threshold": 10000,
                "indexing_threshold": 1000,
                "max_optimization_threads": 1,
                "default_segment_number": 0,
                "flush_interval_sec": 1,
                "max_segment_size": 10000,
                "memmap_threshold": 10000
            }
        }
    });
    serde_json::from_value(val).unwrap()
}

#[async_trait]
impl QdrantOps for MockQdrantClient {
    async fn list_collections(&self) -> Result<Vec<String>, QqlError> {
        Ok(self.collections.clone())
    }
    async fn collection_exists(&self, _name: &str) -> Result<bool, QqlError> {
        Ok(self.exists)
    }
    async fn get_collection_info(&self, _name: &str) -> Result<CollectionInfo, QqlError> {
        if let Some(ref info) = self.info {
            Ok(info.clone())
        } else {
            Ok(mock_collection_info())
        }
    }
    async fn create_collection(&self, req: CreateCollectionReq) -> Result<(), QqlError> {
        *self.last_create_collection.lock().unwrap() = Some(req);
        Ok(())
    }
    async fn update_collection(&self, req: serde_json::Value) -> Result<(), QqlError> {
        *self.last_update_collection.lock().unwrap() = Some(req);
        Ok(())
    }
    async fn delete_collection(&self, _name: &str) -> Result<(), QqlError> {
        Ok(())
    }
    async fn upsert(&self, req: UpsertPointsReq) -> Result<(), QqlError> {
        *self.last_upsert.lock().unwrap() = Some(req);
        Ok(())
    }
    async fn query(
        &self,
        req: crate::pipeline::QueryPointsRequest,
    ) -> Result<Vec<ScoredPoint>, QqlError> {
        if req.collection_name == "nonexistent" {
            return Err(QqlError::runtime("collection 'nonexistent' does not exist"));
        }
        *self.last_query.lock().unwrap() = Some(req);
        Ok(vec![])
    }
    async fn query_groups(
        &self,
        _req: crate::pipeline::QueryPointsGroupsRequest,
    ) -> Result<Vec<PointGroup>, QqlError> {
        Ok(vec![])
    }
    async fn query_batch(
        &self,
        _req: Vec<crate::pipeline::QueryPointsRequest>,
    ) -> Result<Vec<Vec<ScoredPoint>>, QqlError> {
        Ok(vec![])
    }
    async fn delete(&self, req: DeletePointsReq) -> Result<(), QqlError> {
        if req.collection_name == "nonexistent" {
            return Err(QqlError::runtime("collection 'nonexistent' does not exist"));
        }
        *self.last_delete.lock().unwrap() = Some(req);
        Ok(())
    }
    async fn update_vectors(&self, req: UpdateVectorsReq) -> Result<(), QqlError> {
        if req.collection_name == "nonexistent" {
            return Err(QqlError::runtime("collection 'nonexistent' does not exist"));
        }
        *self.last_update_vectors.lock().unwrap() = Some(req);
        Ok(())
    }
    async fn set_payload(&self, req: SetPayloadReq) -> Result<(), QqlError> {
        if req.collection_name == "nonexistent" {
            return Err(QqlError::runtime("collection 'nonexistent' does not exist"));
        }
        *self.last_set_payload.lock().unwrap() = Some(req);
        Ok(())
    }
    async fn create_field_index(&self, _req: CreateFieldIndexReq) -> Result<(), QqlError> {
        Ok(())
    }
    async fn scroll(
        &self,
        _req: ScrollPointsReq,
    ) -> Result<(Vec<RetrievedPoint>, Option<PointId>), QqlError> {
        Ok((self.scroll_records.clone(), self.scroll_offset.clone()))
    }
    async fn count(&self, _req: CountPointsReq) -> Result<u64, QqlError> {
        Ok(0)
    }
    async fn get(&self, _req: GetPointsReq) -> Result<Vec<RetrievedPoint>, QqlError> {
        Ok(self.get_records.clone())
    }
}

fn test_config() -> QqlConfig {
    QqlConfig {
        inference_mode: "cloud".to_string(),
        ..Default::default()
    }
}

fn test_local_config() -> QqlConfig {
    QqlConfig {
        inference_mode: "local".to_string(),
        ..Default::default()
    }
}

struct MockEmbedder {
    dense: Vec<f32>,
    sparse_indices: Vec<u32>,
    sparse_values: Vec<f32>,
}

#[async_trait]
impl crate::embedder::Embedder for MockEmbedder {
    async fn embed_dense(&self, _text: &str, _model: &str) -> Result<Vec<f32>, QqlError> {
        Ok(self.dense.clone())
    }
    async fn embed_sparse(&self, _text: &str) -> Result<crate::sparse::SparseVector, QqlError> {
        Ok(crate::sparse::SparseVector {
            indices: self.sparse_indices.clone(),
            values: self.sparse_values.clone(),
        })
    }
}

#[tokio::test]
async fn test_create_collection_with_hnsw_and_quantization() {
    let client = MockQdrantClient::default();
    let last_create = client.last_create_collection.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));

    let query = "CREATE COLLECTION mycol WITH HNSW (m = 32, ef_construct = 100) WITH QUANTIZATION (type = 'scalar', always_ram = true, quantile = 0.99)";
    let resp = executor.execute(query).await;
    assert!(resp.is_ok(), "{:?}", resp.err());

    let req_opt = last_create.lock().unwrap().take();
    assert!(req_opt.is_some());
    let req = req_opt.unwrap();
    assert_eq!(req.collection_name, "mycol");

    // Check HNSW config serialization
    let hnsw = req.hnsw_config.unwrap();
    assert_eq!(hnsw["m"], 32);
    assert_eq!(hnsw["ef_construct"], 100);

    // Check Quantization config serialization
    let quant = req.quantization_config.unwrap();
    assert!(quant.get("scalar").is_some());
    let scalar = &quant["scalar"];
    assert_eq!(scalar["type"], "int8");
    assert_eq!(scalar["always_ram"], true);
    assert_eq!(scalar["quantile"], 0.99);
}

#[tokio::test]
async fn test_create_collection_with_optimizers_and_params() {
    let client = MockQdrantClient::default();
    let last_create = client.last_create_collection.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));

    let query = "CREATE COLLECTION mycol WITH OPTIMIZERS (deleted_threshold = 0.2, default_segment_number = 4, max_optimization_threads = 2) WITH PARAMS (replication_factor = 2, on_disk_payload = true)";
    let resp = executor.execute(query).await;
    assert!(resp.is_ok(), "{:?}", resp.err());

    let req_opt = last_create.lock().unwrap().take();
    assert!(req_opt.is_some());
    let req = req_opt.unwrap();

    // Check Optimizers config serialization
    let opt = req.optimizers_config.unwrap();
    assert_eq!(opt["deleted_threshold"], 0.2);
    assert_eq!(opt["default_segment_number"], 4);
    assert_eq!(opt["max_optimization_threads"], 2);

    // Check Params serialization
    let params = req.params.unwrap();
    assert_eq!(params["replication_factor"], 2);
    assert_eq!(params["on_disk_payload"], true);
}

#[tokio::test]
async fn test_create_collection_with_named_vectors_hnsw_quant() {
    let client = MockQdrantClient::default();
    let last_create = client.last_create_collection.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));

    let query = "CREATE COLLECTION mycol (dense_vec VECTOR(128, Cosine) WITH HNSW (m = 16) WITH QUANTIZATION (type = 'binary', always_ram = false))";
    let resp = executor.execute(query).await;
    assert!(resp.is_ok(), "{:?}", resp.err());

    let req_opt = last_create.lock().unwrap().take();
    assert!(req_opt.is_some());
    let req = req_opt.unwrap();

    let vectors = req.vectors_config.unwrap();
    assert!(vectors.get("dense_vec").is_some());
    let v_conf = &vectors["dense_vec"];
    assert_eq!(v_conf["size"], 128);
    assert_eq!(v_conf["distance"], "Cosine");

    // Check per-vector HNSW
    let hnsw = &v_conf["hnsw_config"];
    assert_eq!(hnsw["m"], 16);

    // Check per-vector Quantization
    let quant = &v_conf["quantization_config"];
    assert!(quant.get("binary").is_some());
    assert_eq!(quant["binary"]["always_ram"], false);
}

#[tokio::test]
async fn test_alter_collection_quantization_and_hnsw() {
    let mut client = MockQdrantClient::default();
    client.exists = true;
    let last_update = client.last_update_collection.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));

    let query = "ALTER COLLECTION mycol WITH HNSW (ef_construct = 150) WITH QUANTIZATION (type = 'product', always_ram = true)";
    let resp = executor.execute(query).await;
    assert!(resp.is_ok(), "{:?}", resp.err());

    let req_opt = last_update.lock().unwrap().take();
    assert!(req_opt.is_some());
    let req = req_opt.unwrap();

    assert_eq!(req["collection_name"], "mycol");
    assert_eq!(req["hnsw_config"]["ef_construct"], 150);
    assert_eq!(req["quantization_config"]["product"]["always_ram"], true);
    assert_eq!(req["quantization_config"]["product"]["compression"], "x4");
}

#[tokio::test]
async fn test_alter_collection_disable_quantization() {
    let mut client = MockQdrantClient::default();
    client.exists = true;
    let last_update = client.last_update_collection.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));

    let query = "ALTER COLLECTION mycol WITH QUANTIZATION (disabled = true)";
    let resp = executor.execute(query).await;
    assert!(resp.is_ok(), "{:?}", resp.err());

    let req_opt = last_update.lock().unwrap().take();
    assert!(req_opt.is_some());
    let req = req_opt.unwrap();

    assert_eq!(req["quantization_config"]["disabled"], true);
}

#[test]
fn test_point_id_helpers() {
    use crate::executor::helpers::{point_id_string, to_point_id_static};
    use qql_core::ast::Value;

    // to_point_id_static
    let id_str = to_point_id_static(&Value::Str(String::from("abc"))).unwrap();
    assert_eq!(point_id_string(&id_str), "abc");

    let id_num_str = to_point_id_static(&Value::Str(String::from("42"))).unwrap();
    assert_eq!(point_id_string(&id_num_str), "42");

    let id_int = to_point_id_static(&Value::Int(100)).unwrap();
    assert_eq!(point_id_string(&id_int), "100");

    let id_float = to_point_id_static(&Value::Float(99.0)).unwrap();
    assert_eq!(point_id_string(&id_float), "99");

    let id_neg = to_point_id_static(&Value::Int(-5));
    assert!(id_neg.is_err());
}

#[tokio::test]
async fn test_do_select_returns_record_or_nil() {
    // found
    let mut client = MockQdrantClient::default();
    client.exists = true;
    let mut payload = HashMap::new();
    payload.insert("text".to_string(), serde_json::json!("hello"));
    payload.insert("topic".to_string(), serde_json::json!("search"));
    let record_val = serde_json::json!({
        "id": {
            "uuid": "pt-1"
        },
        "payload": payload
    });
    client.get_records = vec![serde_json::from_value(record_val).unwrap()];

    let executor = Executor::new(Box::new(client), Some(test_config()));
    let resp = executor
        .execute("SELECT * FROM docs WHERE id = 'pt-1'")
        .await;
    assert!(resp.is_ok(), "{:?}", resp.err());
    let data = resp.unwrap().data.unwrap();
    assert_eq!(data["id"], "pt-1");
    assert_eq!(data["payload"]["text"], "hello");

    // missing
    let mut client_missing = MockQdrantClient::default();
    client_missing.exists = true;
    let executor_missing = Executor::new(Box::new(client_missing), Some(test_config()));
    let resp_missing = executor_missing
        .execute("SELECT * FROM docs WHERE id = 'pt-404'")
        .await;
    assert!(resp_missing.is_ok());
    let data_missing = resp_missing.unwrap().data.unwrap();
    assert!(data_missing.is_null());
}

#[tokio::test]
async fn test_do_scroll_returns_upstream_style_payload() {
    let mut client = MockQdrantClient::default();
    client.exists = true;
    let mut payload = HashMap::new();
    payload.insert("text".to_string(), serde_json::json!("hello"));
    payload.insert("topic".to_string(), serde_json::json!("search"));
    let record_val = serde_json::json!({
        "id": {
            "num": 7
        },
        "payload": payload
    });
    client.scroll_records = vec![serde_json::from_value(record_val).unwrap()];
    client.scroll_offset = Some(PointId::Uuid("pt-next".to_string()));

    let executor = Executor::new(Box::new(client), Some(test_config()));
    let resp = executor.execute("SCROLL FROM docs LIMIT 5").await;
    assert!(resp.is_ok(), "{:?}", resp.err());
    let data = resp.unwrap().data.unwrap();
    assert_eq!(data["points"][0]["id"], 7);
    assert_eq!(data["points"][0]["payload"]["text"], "hello");
    assert_eq!(data["next_offset"], "pt-next");
}

#[tokio::test]
async fn test_delete_by_id_and_filter() {
    let mut client = MockQdrantClient::default();
    client.exists = true;
    let last_delete = client.last_delete.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));

    // by id
    let resp = executor
        .execute("DELETE FROM docs WHERE id = 'point-123'")
        .await;
    assert!(resp.is_ok(), "{:?}", resp.err());
    let req = last_delete.lock().unwrap().take().unwrap();
    assert_eq!(req.collection_name, "docs");
    assert_eq!(req.point_id, Some(PointId::Uuid("point-123".to_string())));

    // by filter
    let resp_filter = executor
        .execute("DELETE FROM docs WHERE status = 'archived'")
        .await;
    assert!(resp_filter.is_ok());
    let req_filter = last_delete.lock().unwrap().take().unwrap();
    assert_eq!(req_filter.collection_name, "docs");
    assert!(req_filter.filter.is_some());
}

#[tokio::test]
async fn test_update_by_id() {
    let mut client = MockQdrantClient::default();
    client.exists = true;
    let last_update = client.last_update_vectors.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));

    let resp = executor
        .execute("UPDATE docs SET vector = [1.0, 2.0] WHERE id = 12")
        .await;
    assert!(resp.is_ok(), "{:?}", resp.err());
    let req = last_update.lock().unwrap().take().unwrap();
    assert_eq!(req.collection_name, "docs");
    assert_eq!(req.point_id, PointId::Num(12));
    assert_eq!(req.vector, vec![1.0, 2.0]);
}

#[tokio::test]
async fn test_set_payload_by_id_and_filter() {
    let mut client = MockQdrantClient::default();
    client.exists = true;
    let last_set = client.last_set_payload.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));

    // by id
    // by id
    let resp = executor
        .execute("UPDATE docs SET PAYLOAD = {status: 'active'} WHERE id = 12")
        .await;
    assert!(resp.is_ok(), "{:?}", resp.err());
    let req = last_set.lock().unwrap().take().unwrap();
    assert_eq!(req.collection_name, "docs");
    assert_eq!(req.point_id, Some(PointId::Num(12)));
    assert_eq!(req.payload.get("status").unwrap(), "active");

    // by filter
    let resp_filter = executor
        .execute("UPDATE docs SET PAYLOAD = {status: 'active'} WHERE category = 'news'")
        .await;
    assert!(resp_filter.is_ok());
    let req_filter = last_set.lock().unwrap().take().unwrap();
    assert_eq!(req_filter.collection_name, "docs");
    assert!(req_filter.filter.is_some());
    assert_eq!(req_filter.payload.get("status").unwrap(), "active");
}

#[tokio::test]
async fn test_dml_missing_collection_errors() {
    let client = MockQdrantClient::default(); // exists = false
    let executor = Executor::new(Box::new(client), Some(test_config()));

    let resp_delete = executor
        .execute("DELETE FROM nonexistent WHERE id = 'abc'")
        .await;
    assert!(resp_delete.is_err());
    assert!(resp_delete.unwrap_err().msg.contains("does not exist"));

    let resp_update = executor
        .execute("UPDATE nonexistent SET PAYLOAD = {k: 'v'} WHERE id = 'abc'")
        .await;
    assert!(resp_update.is_err());
    assert!(resp_update.unwrap_err().msg.contains("does not exist"));
}

#[tokio::test]
async fn test_insert_into_collection_creates_missing() {
    let client = MockQdrantClient::default();
    let last_create = client.last_create_collection.clone();
    let last_upsert = client.last_upsert.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));

    let query =
        "INSERT INTO docs VALUES {id: '550e8400-e29b-41d4-a716-446655440000', text: 'hello'}";
    let resp = executor.execute(query).await;
    assert!(resp.is_ok(), "{:?}", resp.err());

    // Should create collection
    let create_req = last_create.lock().unwrap().take().unwrap();
    assert_eq!(create_req.collection_name, "docs");

    // Should upsert point
    let upsert_req = last_upsert.lock().unwrap().take().unwrap();
    assert_eq!(upsert_req.collection_name, "docs");
    assert_eq!(upsert_req.points.len(), 1);
    assert_eq!(
        upsert_req.points[0].id.clone(),
        PointId::Uuid("550e8400-e29b-41d4-a716-446655440000".to_string())
    );
}

#[tokio::test]
async fn test_do_query_basic() {
    let mut client = MockQdrantClient::default();
    client.exists = true;
    let last_query = client.last_query.clone();
    let mock_embedder = Arc::new(MockEmbedder {
        dense: vec![0.1, 0.2],
        sparse_indices: vec![],
        sparse_values: vec![],
    });
    let executor = Executor::with_embedder(
        Box::new(client),
        Some(test_local_config()),
        Some(mock_embedder),
    );

    let query = "QUERY 'admin docs' FROM docs LIMIT 10 OFFSET 5 WHERE metadata.group = 'admin'";
    let resp = executor.execute(query).await;
    assert!(resp.is_ok(), "{:?}", resp.err());

    let query_req = last_query.lock().unwrap().take().unwrap();
    assert_eq!(query_req.collection_name, "docs");
    assert_eq!(query_req.limit, 10);
    assert_eq!(query_req.offset, 5);
    assert!(query_req.filter.is_some()); // filter is mapped
}

#[tokio::test]
async fn test_do_query_hybrid() {
    let mut client = MockQdrantClient::default();
    client.exists = true;
    let last_query = client.last_query.clone();
    let mock_embedder = Arc::new(MockEmbedder {
        dense: vec![0.1, 0.2],
        sparse_indices: vec![1, 2],
        sparse_values: vec![0.5, 0.6],
    });
    let executor = Executor::with_embedder(
        Box::new(client),
        Some(test_local_config()),
        Some(mock_embedder),
    );

    let query = "QUERY 'hello' FROM docs LIMIT 10 USING HYBRID";
    let resp = executor.execute(query).await;
    assert!(resp.is_ok(), "{:?}", resp.err());

    let query_req = last_query.lock().unwrap().take().unwrap();
    assert_eq!(query_req.collection_name, "docs");
    // Verify prefetch was constructed
    assert!(!query_req.prefetch.is_empty());
    let prefetches = &query_req.prefetch;
    assert_eq!(prefetches.len(), 2);
}

#[tokio::test]
async fn test_query_missing_collection_errors() {
    let client = MockQdrantClient::default(); // exists = false
    let mock_embedder = Arc::new(MockEmbedder {
        dense: vec![0.1, 0.2],
        sparse_indices: vec![],
        sparse_values: vec![],
    });
    let executor = Executor::with_embedder(
        Box::new(client),
        Some(test_local_config()),
        Some(mock_embedder),
    );

    let query = "QUERY 'hello' FROM nonexistent LIMIT 10";
    let resp = executor.execute(query).await;
    assert!(resp.is_err());
    assert!(resp.unwrap_err().msg.contains("does not exist"));
}

#[tokio::test]
async fn test_insert_bad_types() {
    let mut client = MockQdrantClient::default();
    client.exists = true;
    let executor = Executor::new(Box::new(client), Some(test_config()));

    // Wait, the parser catches syntax errors. But logic errors?
    // E.g., INSERT with mismatching value lengths
    let query = "INSERT INTO docs VALUES {id: 1}, {id: 2, text: 'a'}, {id: 3}";
    let resp = executor.execute(query).await;
    // Actually, qql parser allows this since schema is flexible.
    assert!(resp.is_ok(), "{:?}", resp.err());
}
