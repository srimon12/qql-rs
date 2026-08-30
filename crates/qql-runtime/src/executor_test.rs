#![allow(clippy::field_reassign_with_default)]

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;
use qql_core::error::QqlError;
use qql_plan::{QueryBatchRequest, UpdateBatchRequest};

use crate::backend::{SparseVectorSpec, VectorSpec};
use crate::client::{CollectionInfo, QdrantOps};
use crate::config::QqlConfig;
use crate::executor::{Executor, OnError};

struct MockQdrantClient {
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
    pub created_collections: Arc<Mutex<HashSet<String>>>,
    /// Per-collection mock points returned by `execute_planned` when non-empty.
    /// Key: collection name, Value: JSON object with a "points" array.
    pub point_map: Arc<Mutex<HashMap<String, serde_json::Value>>>,
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
            create_collection_call_count: Arc::new(Mutex::new(0)),
            created_collections: Arc::new(Mutex::new(HashSet::new())),
            point_map: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl QdrantOps for MockQdrantClient {
    async fn list_collections(&self) -> Result<Vec<String>, QqlError> {
        Ok(self.collections.clone())
    }
    async fn collection_exists(&self, name: &str) -> Result<bool, QqlError> {
        Ok(self.exists || self.created_collections.lock().unwrap().contains(name))
    }
    async fn get_collection_info(&self, name: &str) -> Result<CollectionInfo, QqlError> {
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
        | qql_plan::PlannedOperation::QueryGroups { collection, .. } = op
        {
            if let Some(points) = self.point_map.lock().unwrap().get(collection) {
                return Ok(points.clone());
            }
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
        Ok(batch
            .searches
            .iter()
            .map(|_| serde_json::json!({"result": {"points": []}}))
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

fn collection_with_vectors(dense: &[&str], sparse: &[&str]) -> CollectionInfo {
    collection_with_vectors_multi(dense, sparse, &[])
}

fn collection_with_vectors_multi(
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

#[test]
fn test_execution_report_counts_mixed_results() {
    let report = crate::executor::ExecutionReport::from_results(vec![
        crate::executor::ExecResponse {
            ok: true,
            operation: "QUERY".to_string(),
            message: "ok".to_string(),
            data: None,
        },
        crate::executor::ExecResponse {
            ok: false,
            operation: "QUERY".to_string(),
            message: "failed".to_string(),
            data: None,
        },
    ]);

    assert!(!report.ok);
    assert_eq!(report.succeeded, 1);
    assert_eq!(report.failed, 1);
}

struct MockEmbedder {
    dense: Vec<f32>,
    sparse_indices: Vec<u32>,
    sparse_values: Vec<f32>,
    multi: Vec<Vec<f32>>,
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
        // Return descending scores: the first document gets the highest score.
        Ok((0..documents.len()).map(|i| 1.0 - i as f32 * 0.1).collect())
    }
}

#[tokio::test]
async fn show_collections_preserves_backend_data() {
    let client = MockQdrantClient {
        collections: vec!["alpha".into(), "beta".into()],
        ..Default::default()
    };
    let executor = Executor::new(Box::new(client), Some(test_config()));

    let report = executor
        .execute("SHOW COLLECTIONS", OnError::Stop)
        .await
        .unwrap();

    assert_eq!(report.results[0].operation, "SHOW_COLLECTIONS");
    assert_eq!(
        report.results[0].data.as_ref().unwrap()["result"]["collections"],
        serde_json::json!([{"name": "alpha"}, {"name": "beta"}])
    );
}

#[tokio::test]
async fn test_create_collection_with_hnsw_and_quantization() {
    let client = MockQdrantClient::default();
    let last_planned = client.last_planned.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));

    let query = "CREATE COLLECTION mycol WITH HNSW (m = 32, ef_construct = 100) WITH QUANTIZATION (type = 'scalar', always_ram = true, quantile = 0.99)";
    let resp = executor.execute(query, OnError::Stop).await;
    assert!(resp.is_ok(), "{:?}", resp.err());

    let op = last_planned.lock().unwrap().take().unwrap();
    let route = qql_plan::plan::to_rest_route(&op).expect("rest route");
    assert_eq!(route.path, "/collections/mycol");
    let req = route.body_json().unwrap();
    assert_eq!(req["vectors"]["dense"]["size"], 384);

    // Check HNSW config serialization
    let hnsw = &req["hnsw_config"];
    assert_eq!(hnsw["m"], 32);
    assert_eq!(hnsw["ef_construct"], 100);

    // OpenAPI QuantizationConfig: { "scalar": { "type": "int8", … } }
    let quant = &req["quantization_config"];
    assert_eq!(quant["scalar"]["type"], "int8");
    assert_eq!(quant["scalar"]["always_ram"], true);
    assert_eq!(quant["scalar"]["quantile"], 0.99);
}

#[tokio::test]
async fn test_create_hybrid_materializes_default_schema() {
    let client = MockQdrantClient::default();
    let last_planned = client.last_planned.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));

    executor
        .execute("CREATE COLLECTION mycol HYBRID", OnError::Stop)
        .await
        .unwrap();

    let op = last_planned.lock().unwrap().take().unwrap();
    let route = qql_plan::plan::to_rest_route(&op).expect("rest route");
    let req = route.body_json().unwrap();
    assert_eq!(req["vectors"]["dense"]["size"], 384);
    assert_eq!(req["sparse_vectors"]["sparse"]["modifier"], "idf");
}

#[tokio::test]
async fn test_create_collection_with_optimizers_and_params() {
    let client = MockQdrantClient::default();
    let last_planned = client.last_planned.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));

    let query = "CREATE COLLECTION mycol WITH OPTIMIZERS (deleted_threshold = 0.2, default_segment_number = 4, max_optimization_threads = 2) WITH PARAMS (replication_factor = 2, on_disk_payload = true)";
    let resp = executor.execute(query, OnError::Stop).await;
    assert!(resp.is_ok(), "{:?}", resp.err());

    let op = last_planned.lock().unwrap().take().unwrap();
    let route = qql_plan::plan::to_rest_route(&op).expect("rest route");
    let req = route.body_json().unwrap();

    // Check Optimizers config serialization
    let opt = &req["optimizers_config"];
    assert_eq!(opt["deleted_threshold"], 0.2);
    assert_eq!(opt["default_segment_number"], 4);
    assert_eq!(opt["max_optimization_threads"], 2);

    // OpenAPI CreateCollection: replication_factor / on_disk_payload are top-level
    assert_eq!(req["replication_factor"], 2);
    assert_eq!(req["on_disk_payload"], true);
    assert!(req.get("params").is_none());
}

#[tokio::test]
async fn test_create_collection_with_named_vectors_hnsw_quant() {
    let client = MockQdrantClient::default();
    let last_planned = client.last_planned.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));

    let query = "CREATE COLLECTION mycol (dense_vec VECTOR(128, Cosine) WITH HNSW (m = 16) WITH QUANTIZATION (type = 'binary', always_ram = false))";
    let resp = executor.execute(query, OnError::Stop).await;
    assert!(resp.is_ok(), "{:?}", resp.err());

    let op = last_planned.lock().unwrap().take().unwrap();
    let route = qql_plan::plan::to_rest_route(&op).expect("rest route");
    let req = route.body_json().unwrap();

    let vectors = &req["vectors"];
    assert!(vectors.get("dense_vec").is_some());
    let v_conf = &vectors["dense_vec"];
    assert_eq!(v_conf["size"], 128);
    assert_eq!(v_conf["distance"], "Cosine");

    // Check per-vector HNSW
    let hnsw = &v_conf["hnsw_config"];
    assert_eq!(hnsw["m"], 16);

    // OpenAPI nested binary quantization on vector params
    let quant = &v_conf["quantization_config"];
    assert_eq!(quant["binary"]["always_ram"], false);
}

#[tokio::test]
async fn test_alter_collection_quantization_and_hnsw() {
    let mut client = MockQdrantClient::default();
    client.exists = true;
    let last_planned = client.last_planned.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));

    let query = "ALTER COLLECTION mycol WITH HNSW (ef_construct = 150) WITH QUANTIZATION (type = 'product', always_ram = true)";
    let resp = executor.execute(query, OnError::Stop).await;
    assert!(resp.is_ok(), "{:?}", resp.err());

    let op = last_planned.lock().unwrap().take().unwrap();
    let route = qql_plan::plan::to_rest_route(&op).expect("rest route");
    assert_eq!(route.path, "/collections/mycol");
    let req = route.body_json().unwrap();

    assert_eq!(req["hnsw_config"]["ef_construct"], 150);
    // OpenAPI: product quantization nested under `product`
    assert_eq!(req["quantization_config"]["product"]["always_ram"], true);
    assert_eq!(req["quantization_config"]["product"]["compression"], "x4");
}

#[tokio::test]
async fn test_alter_collection_disable_quantization() {
    let mut client = MockQdrantClient::default();
    client.exists = true;
    let last_planned = client.last_planned.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));

    let query = "ALTER COLLECTION mycol WITH QUANTIZATION (disabled = true)";
    let resp = executor.execute(query, OnError::Stop).await;
    assert!(resp.is_ok(), "{:?}", resp.err());

    let op = last_planned.lock().unwrap().take().unwrap();
    let route = qql_plan::plan::to_rest_route(&op).expect("rest route");
    let req = route.body_json().unwrap();

    // OpenAPI QuantizationConfigDiff disabled variant is the string "Disabled"
    assert_eq!(req["quantization_config"], "Disabled");
}

#[tokio::test]
async fn test_dml_missing_collection_errors() {
    let client = MockQdrantClient::default(); // exists = false
    let executor = Executor::new(Box::new(client), Some(test_config()));

    let resp_delete = executor
        .execute("DELETE FROM nonexistent WHERE id = 'abc'", OnError::Stop)
        .await;
    assert!(resp_delete.is_err());
    assert!(resp_delete.unwrap_err().message.contains("does not exist"));

    let resp_update = executor
        .execute(
            "UPDATE nonexistent SET PAYLOAD = {k: 'v'} WHERE id = 'abc'",
            OnError::Stop,
        )
        .await;
    assert!(resp_update.is_err());
    assert!(resp_update.unwrap_err().message.contains("does not exist"));
}

#[tokio::test]
async fn test_do_query_basic() {
    let mut client = MockQdrantClient::default();
    client.exists = true;
    // Simulate a collection with an unnamed default vector (no named vectors)
    client.info = Some(CollectionInfo::default());
    let last_planned = client.last_planned.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));

    let query = "QUERY TEXT 'admin docs' MODEL 'test-model' FROM docs WHERE metadata.group = 'admin' LIMIT 10 OFFSET 5";
    let resp = executor.execute(query, OnError::Stop).await;
    assert!(resp.is_ok(), "{:?}", resp.err());

    let op = last_planned.lock().unwrap().take().unwrap();
    let route = qql_plan::plan::to_rest_route(&op).expect("rest route");
    assert_eq!(route.method, qql_plan::types::Method::Post);
    assert!(route.path.contains("docs"));
    assert!(route.body.is_some());
}

#[tokio::test]
async fn test_do_query_hybrid() {
    let mut client = MockQdrantClient::default();
    client.exists = true;
    let last_planned = client.last_planned.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));

    let query = "QUERY HYBRID TEXT 'hello' MODEL 'test-model' DENSE dense SPARSE sparse FUSION RRF FROM docs LIMIT 10";
    let resp = executor.execute(query, OnError::Stop).await;
    assert!(resp.is_ok(), "{:?}", resp.err());

    let op = last_planned.lock().unwrap().take().unwrap();
    let route = qql_plan::plan::to_rest_route(&op).expect("rest route");
    assert_eq!(route.method, qql_plan::types::Method::Post);
    assert!(route.body.is_some());
}

#[tokio::test]
async fn text_query_resolves_arbitrary_sparse_vector_by_schema() {
    let mut client = MockQdrantClient::default();
    client.exists = true;
    client.info = Some(collection_with_vectors(&["semantic_v2"], &["lexical_v2"]));
    let last_planned = client.last_planned.clone();
    let embedder = Arc::new(MockEmbedder {
        dense: vec![0.1, 0.2, 0.3],
        sparse_indices: vec![1, 7],
        sparse_values: vec![0.4, 0.8],
        multi: vec![vec![0.1, 0.2], vec![0.3, 0.4]],
    });
    let executor =
        Executor::with_embedder(Box::new(client), Some(test_local_config()), Some(embedder));

    executor
        .execute(
            "QUERY TEXT 'hello' MODEL 'test-model' FROM docs USING lexical_v2 LIMIT 10",
            OnError::Stop,
        )
        .await
        .unwrap();

    let op = last_planned.lock().unwrap().take().unwrap();
    let route = qql_plan::plan::to_rest_route(&op).expect("rest route");
    let body = route.body_json().unwrap();
    assert_eq!(body["using"], "lexical_v2");
    assert_eq!(
        body["query"]["nearest"]["indices"],
        serde_json::json!([1, 7])
    );
    let values = body["query"]["nearest"]["values"].as_array().unwrap();
    assert!((values[0].as_f64().unwrap() - 0.4).abs() < 1e-6);
    assert!((values[1].as_f64().unwrap() - 0.8).abs() < 1e-6);
}

#[tokio::test]
async fn cte_prefetch_using_sparse_embeds_sparse_via_schema() {
    // Regression: USING sparse without AS SPARSE must not dense-embed.
    let mut client = MockQdrantClient::default();
    client.exists = true;
    client.info = Some(collection_with_vectors(&["dense"], &["sparse"]));
    let last_planned = client.last_planned.clone();
    let embedder = Arc::new(MockEmbedder {
        dense: vec![0.1, 0.2, 0.3],
        sparse_indices: vec![2, 9],
        sparse_values: vec![0.5, 0.9],
        multi: vec![vec![0.1, 0.2], vec![0.3, 0.4]],
    });
    let executor =
        Executor::with_embedder(Box::new(client), Some(test_local_config()), Some(embedder));

    executor
        .execute(
            "WITH d AS (QUERY TEXT 'x' MODEL 'test-model' USING dense LIMIT 100), \
             s AS (QUERY TEXT 'x' MODEL 'test-model' USING sparse LIMIT 100) \
             QUERY FUSION RRF FROM docs PREFETCH (d, s) LIMIT 10",
            OnError::Stop,
        )
        .await
        .unwrap();

    let op = last_planned.lock().unwrap().take().unwrap();
    let route = qql_plan::plan::to_rest_route(&op).expect("rest route");
    let body = route.body_json().unwrap();
    let prefetch = body["prefetch"].as_array().expect("fusion prefetch");
    assert_eq!(prefetch.len(), 2);

    // Prefetch 0: dense arm
    assert_eq!(prefetch[0]["using"], "dense");
    assert!(
        prefetch[0]["query"]["nearest"].is_array(),
        "dense arm must be a dense vector array, got {}",
        prefetch[0]["query"]["nearest"]
    );

    // Prefetch 1: sparse arm — indices/values, not a dense float array
    assert_eq!(prefetch[1]["using"], "sparse");
    assert_eq!(
        prefetch[1]["query"]["nearest"]["indices"],
        serde_json::json!([2, 9])
    );
    let values = prefetch[1]["query"]["nearest"]["values"]
        .as_array()
        .expect("sparse values");
    assert!((values[0].as_f64().unwrap() - 0.5).abs() < 1e-6);
    assert!((values[1].as_f64().unwrap() - 0.9).abs() < 1e-6);
}

#[tokio::test]
async fn text_query_multivector_embeds_multi_via_schema() {
    let mut client = MockQdrantClient::default();
    client.exists = true;
    client.info = Some(collection_with_vectors_multi(
        &["dense", "colbert"],
        &[],
        &["colbert"],
    ));
    let last_planned = client.last_planned.clone();
    let embedder = Arc::new(MockEmbedder {
        dense: vec![0.1, 0.2, 0.3],
        sparse_indices: vec![],
        sparse_values: vec![],
        multi: vec![vec![0.11, 0.22], vec![0.33, 0.44], vec![0.55, 0.66]],
    });
    let executor =
        Executor::with_embedder(Box::new(client), Some(test_local_config()), Some(embedder));

    executor
        .execute(
            "QUERY TEXT 'late interaction' MODEL 'test-model' FROM docs USING colbert LIMIT 10",
            OnError::Stop,
        )
        .await
        .unwrap();

    let op = last_planned.lock().unwrap().take().unwrap();
    let route = qql_plan::plan::to_rest_route(&op).expect("rest route");
    let body = route.body_json().unwrap();
    assert_eq!(body["using"], "colbert");
    // MultiDense serializes as array-of-arrays under nearest.
    let nearest = &body["query"]["nearest"];
    assert!(
        nearest.is_array(),
        "expected multi-dense array, got {nearest}"
    );
    let rows = nearest.as_array().unwrap();
    assert_eq!(rows.len(), 3);
    let first = rows[0].as_array().expect("row 0");
    assert!((first[0].as_f64().unwrap() - 0.11).abs() < 1e-5);
    assert!((first[1].as_f64().unwrap() - 0.22).abs() < 1e-5);
}

#[tokio::test]
async fn text_query_infers_only_arbitrary_dense_vector() {
    let mut client = MockQdrantClient::default();
    client.exists = true;
    client.info = Some(collection_with_vectors(&["semantic_v2"], &[]));
    let last_planned = client.last_planned.clone();
    let embedder = Arc::new(MockEmbedder {
        dense: vec![0.1, 0.2, 0.3],
        sparse_indices: vec![],
        sparse_values: vec![],
        multi: vec![vec![0.1, 0.2], vec![0.3, 0.4]],
    });
    let executor =
        Executor::with_embedder(Box::new(client), Some(test_local_config()), Some(embedder));

    executor
        .execute(
            "QUERY TEXT 'hello' MODEL 'test-model' FROM docs LIMIT 10",
            OnError::Stop,
        )
        .await
        .unwrap();

    let op = last_planned.lock().unwrap().take().unwrap();
    let route = qql_plan::plan::to_rest_route(&op).expect("rest route");
    assert_eq!(route.body_json().unwrap()["using"], "semantic_v2");
}

#[tokio::test]
async fn text_query_rejects_ambiguous_vector_topology() {
    let mut client = MockQdrantClient::default();
    client.exists = true;
    client.info = Some(collection_with_vectors(
        &["semantic_v1", "semantic_v2"],
        &[],
    ));
    let embedder = Arc::new(MockEmbedder {
        dense: vec![0.1, 0.2, 0.3],
        sparse_indices: vec![],
        sparse_values: vec![],
        multi: vec![vec![0.1, 0.2], vec![0.3, 0.4]],
    });
    let executor =
        Executor::with_embedder(Box::new(client), Some(test_local_config()), Some(embedder));

    let error = executor
        .execute(
            "QUERY TEXT 'hello' MODEL 'test-model' FROM docs LIMIT 10",
            OnError::Stop,
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "QQL-MISSING-USING");
}

#[tokio::test]
async fn hybrid_upsert_infers_arbitrary_named_targets() {
    let mut client = MockQdrantClient::default();
    client.exists = true;
    client.info = Some(collection_with_vectors(&["semantic_v2"], &["lexical_v2"]));
    let last_planned = client.last_planned.clone();
    let embedder = Arc::new(MockEmbedder {
        dense: vec![0.1, 0.2, 0.3],
        sparse_indices: vec![1],
        sparse_values: vec![0.5],
        multi: vec![vec![0.1, 0.2], vec![0.3, 0.4]],
    });
    let executor =
        Executor::with_embedder(Box::new(client), Some(test_local_config()), Some(embedder));

    executor
        .execute(
            "UPSERT INTO docs VALUES {id: 1, text: 'hello'} USING HYBRID",
            OnError::Stop,
        )
        .await
        .unwrap();

    let op = last_planned.lock().unwrap().take().unwrap();
    let route = qql_plan::plan::to_rest_route(&op).expect("rest route");
    let vector = &route.body_json().unwrap()["points"][0]["vector"];
    assert!(vector.get("semantic_v2").is_some());
    assert!(vector.get("lexical_v2").is_some());
}

#[tokio::test]
async fn upsert_rejects_ambiguous_inferred_embedding_target() {
    let mut client = MockQdrantClient::default();
    client.exists = true;
    client.info = Some(collection_with_vectors(
        &["semantic_v1", "semantic_v2"],
        &[],
    ));
    let embedder = Arc::new(MockEmbedder {
        dense: vec![0.1, 0.2, 0.3],
        sparse_indices: vec![],
        sparse_values: vec![],
        multi: vec![vec![0.1, 0.2], vec![0.3, 0.4]],
    });
    let executor =
        Executor::with_embedder(Box::new(client), Some(test_local_config()), Some(embedder));

    let error = executor
        .execute(
            "UPSERT INTO docs VALUES {id: 1, text: 'hello'} USING DENSE",
            OnError::Stop,
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "QQL-EMBEDDING-TOPOLOGY");
}

#[tokio::test]
async fn test_do_select_returns_record_or_nil() {
    let mut client = MockQdrantClient::default();
    client.exists = true;
    let last_planned = client.last_planned.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));

    let resp = executor
        .execute("QUERY POINTS ('pt-1') FROM docs", OnError::Stop)
        .await;
    assert!(resp.is_ok(), "{:?}", resp.err());

    let op = last_planned.lock().unwrap().take().unwrap();
    let route = qql_plan::plan::to_rest_route(&op).expect("rest route");
    assert_eq!(route.method, qql_plan::types::Method::Post);
    assert!(route.path.contains("docs/points"));
}

#[tokio::test]
async fn test_delete_by_id_and_filter() {
    let mut client = MockQdrantClient::default();
    client.exists = true;
    let last_planned = client.last_planned.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));

    let resp = executor
        .execute("DELETE FROM docs WHERE id = 12", OnError::Stop)
        .await;
    assert!(resp.is_ok(), "{:?}", resp.err());

    let op = last_planned.lock().unwrap().take().unwrap();
    let route = qql_plan::plan::to_rest_route(&op).expect("rest route");
    assert_eq!(route.method, qql_plan::types::Method::Post);
    assert!(route.path.contains("delete"));
}

#[tokio::test]
async fn test_set_payload_by_id_and_filter() {
    let mut client = MockQdrantClient::default();
    client.exists = true;
    let last_planned = client.last_planned.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));

    let resp = executor
        .execute(
            "UPDATE docs SET PAYLOAD = {status: 'active'} WHERE id = 12",
            OnError::Stop,
        )
        .await;
    assert!(resp.is_ok(), "{:?}", resp.err());

    let op = last_planned.lock().unwrap().take().unwrap();
    let route = qql_plan::plan::to_rest_route(&op).expect("rest route");
    assert_eq!(route.method, qql_plan::types::Method::Post);
    assert!(route.path.contains("payload"));
}

#[tokio::test]
async fn test_update_by_id() {
    let mut client = MockQdrantClient::default();
    client.exists = true;
    let last_planned = client.last_planned.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));

    let resp = executor
        .execute(
            "UPDATE docs SET VECTOR dense = [1.0, 2.0] WHERE id = 'p1'",
            OnError::Stop,
        )
        .await;
    assert!(resp.is_ok(), "{:?}", resp.err());

    let op = last_planned.lock().unwrap().take().unwrap();
    let route = qql_plan::plan::to_rest_route(&op).expect("rest route");
    assert_eq!(route.method, qql_plan::types::Method::Put);
    assert!(route.path.contains("vectors"));
}

#[tokio::test]
async fn test_upsert_into_collection_creates_missing() {
    let client = MockQdrantClient::default();
    let last_planned = client.last_planned.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));

    let resp = executor
        .execute(
            "UPSERT INTO docs VALUES {id: 'pt-1', text: 'hello'}",
            OnError::Stop,
        )
        .await;
    assert!(resp.is_ok(), "{:?}", resp.err());

    let op = last_planned.lock().unwrap().take().unwrap();
    let route = qql_plan::plan::to_rest_route(&op).expect("rest route");
    assert_eq!(route.method, qql_plan::types::Method::Put);
    assert!(route.path.contains("docs"));
}

#[tokio::test]
async fn test_do_scroll_returns_upstream_style_payload() {
    let mut client = MockQdrantClient::default();
    client.exists = true;
    let last_planned = client.last_planned.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));

    let resp = executor
        .execute("SCROLL FROM docs LIMIT 10", OnError::Stop)
        .await;

    assert!(resp.is_ok(), "{:?}", resp.err());
    let op = last_planned.lock().unwrap().take().unwrap();
    let route = qql_plan::plan::to_rest_route(&op).expect("rest route");
    assert_eq!(route.method, qql_plan::types::Method::Post);
    assert!(route.path.contains("scroll"));
}

#[tokio::test]
async fn test_query_missing_collection_errors() {
    let mut client = MockQdrantClient::default(); // exists = false
                                                  // Provide an empty schema so the vector-name check passes; the actual
                                                  // "not found" error comes from execute_route which checks the path.
    client.info = Some(CollectionInfo::default());
    let mock_embedder = Arc::new(MockEmbedder {
        dense: vec![0.1, 0.2],
        sparse_indices: vec![],
        sparse_values: vec![],
        multi: vec![vec![0.1, 0.2], vec![0.3, 0.4]],
    });
    let executor = Executor::with_embedder(
        Box::new(client),
        Some(test_local_config()),
        Some(mock_embedder),
    );

    let query = "QUERY TEXT 'hello' MODEL 'test-model' FROM nonexistent LIMIT 10";
    let resp = executor.execute(query, OnError::Stop).await;
    assert!(resp.is_err());
    assert!(resp.unwrap_err().message.contains("does not exist"));
}

#[tokio::test]
async fn test_upsert_bad_types() {
    let mut client = MockQdrantClient::default();
    client.exists = true;
    client.info = Some(CollectionInfo::default());
    let executor = Executor::new(Box::new(client), Some(test_config()));

    // Wait, the parser catches syntax errors. But logic errors?
    // E.g., UPSERT with mismatching value lengths
    let query = "UPSERT INTO docs VALUES {id: 1}, {id: 2, text: 'a'}, {id: 3}";
    let resp = executor.execute(query, OnError::Stop).await;
    // Actually, qql parser allows this since schema is flexible.
    assert!(resp.is_ok(), "{:?}", resp.err());
}

#[tokio::test]
async fn test_batch_query_groups_same_collection() {
    let mut client = MockQdrantClient::default();
    client.info = Some(CollectionInfo::default()); // unnamed vector → passes check
    let batch_count = client.batch_call_count.clone();
    let searches_count = client.last_batch_searches_count.clone();

    let executor = Executor::new(Box::new(client), Some(test_config()));

    let resp = qql_core::parser::Parser::parse_all(
        "QUERY TEXT 'a' MODEL 'test-model' FROM docs USING dense AS DENSE LIMIT 1;\
         QUERY TEXT 'b' MODEL 'test-model' FROM docs USING dense AS DENSE LIMIT 1;\
         QUERY TEXT 'c' MODEL 'test-model' FROM docs USING dense AS DENSE LIMIT 1;",
    )
    .unwrap();
    let results = executor.execute_batch_nodes(resp, false).await.unwrap();

    // 3 queries, 3 results, 1 batch call
    assert_eq!(results.len(), 3, "expected 3 results");
    for r in &results {
        assert!(r.ok, "result should be ok: {:?}", r);
    }

    let calls = *batch_count.lock().unwrap();
    assert_eq!(calls, 1, "expected 1 batch call, got {calls}");

    let count = *searches_count.lock().unwrap();
    assert_eq!(count, 3, "expected 3 searches in batch, got {count}");
}

#[tokio::test]
async fn test_batch_mutations_same_collection() {
    let client = MockQdrantClient::default();
    let update_count = client.update_batch_call_count.clone();
    let ops_count = client.last_update_batch_ops_count.clone();
    let route_count = client.last_planned.clone();

    let executor = Executor::new(Box::new(client), Some(test_config()));

    let stmts = qql_core::parser::Parser::parse_all(
        "UPSERT INTO docs VALUES {id: 1, title: 'a'};\
         UPSERT INTO docs VALUES {id: 2, title: 'b'};\
         DELETE FROM docs WHERE id = 3;",
    )
    .unwrap();
    let results = executor.execute_batch_nodes(stmts, false).await.unwrap();

    assert_eq!(
        results.len(),
        3,
        "expected 3 results, got {}",
        results.len()
    );
    for r in &results {
        assert!(r.ok, "result should be ok: {:?}", r);
    }

    let calls = *update_count.lock().unwrap();
    assert_eq!(calls, 1, "expected 1 update-batch call, got {calls}");

    let count = *ops_count.lock().unwrap();
    assert_eq!(count, 3, "expected 3 ops in batch, got {count}");

    // Individual routes should not have been used for these mutations
    assert!(
        route_count.lock().unwrap().is_none(),
        "mutations should go through update batch, not execute_route"
    );
}

#[tokio::test]
async fn test_delete_payload_in_mutation_batch_is_deliberately_isolated() {
    // Regression (B-1): `DELETE PAYLOAD` has no `UpdateOperation` batch form.
    // Mixed same-collection mutation batches must not abort the whole script
    // with QQL-BATCH-INVARIANT; DELETE PAYLOAD is dispatched through the
    // single-op path in statement order and every statement succeeds.
    let client = MockQdrantClient::default();
    let update_count = client.update_batch_call_count.clone();
    let individual_calls = client.execute_planned_call_count.clone();

    let executor = Executor::new(Box::new(client), Some(test_config()));

    // Two DELETE PAYLOAD statements, no other mutations.
    let stmts = qql_core::parser::Parser::parse_all(
        "DELETE PAYLOAD draft FROM docs WHERE id = 1;\
         DELETE PAYLOAD final FROM docs WHERE id = 2;",
    )
    .unwrap();
    let results = executor.execute_batch_nodes(stmts, false).await.unwrap();
    assert_eq!(
        results.len(),
        2,
        "expected 2 results, got {}",
        results.len()
    );
    for r in &results {
        assert!(r.ok, "DELETE PAYLOAD should succeed: {:?}", r);
        assert_eq!(r.operation, "DELETE_PAYLOAD");
    }
    assert_eq!(
        *update_count.lock().unwrap(),
        0,
        "DELETE PAYLOAD must not use update batch"
    );
    assert_eq!(
        *individual_calls.lock().unwrap(),
        2,
        "each DELETE PAYLOAD should dispatch singly"
    );

    // UPSERT + DELETE PAYLOAD in one same-collection group.
    let client = MockQdrantClient::default();
    let update_count = client.update_batch_call_count.clone();
    let individual_calls = client.execute_planned_call_count.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));
    let stmts = qql_core::parser::Parser::parse_all(
        "UPSERT INTO docs VALUES {id: 1, title: 'a'};\
         DELETE PAYLOAD draft FROM docs WHERE id = 1;",
    )
    .unwrap();
    let results = executor.execute_batch_nodes(stmts, false).await.unwrap();
    assert_eq!(results.len(), 2);
    assert!(results[0].ok, "UPSERT should succeed: {:?}", results[0]);
    assert_eq!(results[0].operation, "UPSERT");
    assert!(
        results[1].ok,
        "DELETE PAYLOAD should succeed: {:?}",
        results[1]
    );
    assert_eq!(results[1].operation, "DELETE_PAYLOAD");
    assert_eq!(
        *update_count.lock().unwrap(),
        0,
        "single batchable run is dispatched singly, not via update batch"
    );
    assert_eq!(
        *individual_calls.lock().unwrap(),
        2,
        "UPSERT + DELETE PAYLOAD both dispatch through execute_planned"
    );

    // DELETE PAYLOAD sandwiched between batchable mutations: the surrounding
    // run is still batched, the DELETE PAYLOAD is isolated, and statement
    // order is preserved in the response.
    let client = MockQdrantClient::default();
    let update_count = client.update_batch_call_count.clone();
    let ops_count = client.last_update_batch_ops_count.clone();
    let individual_calls = client.execute_planned_call_count.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));
    let stmts = qql_core::parser::Parser::parse_all(
        "UPSERT INTO docs VALUES {id: 1, title: 'a'};\
         DELETE PAYLOAD draft FROM docs WHERE id = 1;\
         DELETE FROM docs WHERE id = 2;\
         UPSERT INTO docs VALUES {id: 3, title: 'c'};",
    )
    .unwrap();
    let results = executor.execute_batch_nodes(stmts, false).await.unwrap();
    assert_eq!(results.len(), 4);
    assert_eq!(results[0].operation, "UPSERT");
    assert_eq!(results[1].operation, "DELETE_PAYLOAD");
    assert_eq!(results[2].operation, "DELETE");
    assert_eq!(results[3].operation, "UPSERT");
    for r in &results {
        assert!(r.ok, "all statements should succeed: {:?}", r);
    }
    // run = [UPSERT, DELETE] batched as one update batch; the leading UPSERT
    // and the DELETE PAYLOAD dispatch singly.
    assert_eq!(
        *update_count.lock().unwrap(),
        1,
        "batchable run after DELETE PAYLOAD should batch"
    );
    assert_eq!(*ops_count.lock().unwrap(), 2);
    assert_eq!(
        *individual_calls.lock().unwrap(),
        2,
        "leading UPSERT and DELETE PAYLOAD dispatch singly"
    );
}

#[tokio::test]
async fn test_grouped_offset_applied_exactly_once() {
    // Regression (task 5): GROUP BY with OFFSET must apply the offset exactly
    // once. The planner sends `limit = user_limit + offset` and the executor
    // trims the returned groups client-side; `group_offset` has no wire
    // representation (serde-skipped, not in the gRPC QueryPointGroups proto),
    // so the backend never applies it. Simulate a server returning `limit`
    // groups and assert the result is exactly `user_limit` groups starting at
    // the offset — a double application would drop or duplicate groups.
    let client = MockQdrantClient {
        info: Some(collection_with_vectors(&["dense"], &["sparse"])),
        point_map: Arc::new(Mutex::new(HashMap::from([(
            "docs".to_string(),
            serde_json::json!({
                "result": {
                    "groups": [
                        {"id": "a", "hits": [{"id": 1, "score": 1.0}]},
                        {"id": "b", "hits": [{"id": 2, "score": 0.9}]},
                        {"id": "c", "hits": [{"id": 3, "score": 0.8}]},
                    ]
                }
            }),
        )]))),
        ..Default::default()
    };
    let executor = Executor::new(Box::new(client), Some(test_config()));
    let stmts = qql_core::parser::Parser::parse_all(
        "QUERY TEXT 'x' MODEL 'test-model' FROM docs USING dense AS DENSE GROUP BY category LIMIT 2 OFFSET 1;",
    )
    .unwrap();
    let results = executor.execute_batch_nodes(stmts, false).await.unwrap();
    assert_eq!(results.len(), 1);
    let r = &results[0];
    assert!(r.ok, "grouped query should succeed: {:?}", r);
    assert_eq!(r.operation, "QUERY_GROUPS");
    assert_eq!(r.message, "Found 2 group(s)");
    let data = r.data.as_ref().expect("data should be present");
    let groups = data["result"]["groups"].as_array().expect("groups array");
    assert_eq!(
        groups.len(),
        2,
        "must return exactly user_limit groups, got {}",
        groups.len()
    );
    assert_eq!(groups[0]["id"], "b", "groups must start at the offset");
    assert_eq!(groups[1]["id"], "c");
}

#[tokio::test]
async fn test_batch_preserves_order_mixed_query_and_mutation() {
    let client = MockQdrantClient {
        info: Some(CollectionInfo::default()),
        ..Default::default()
    };
    let query_batch = client.batch_call_count.clone();
    let update_batch = client.update_batch_call_count.clone();

    let executor = Executor::new(Box::new(client), Some(test_config()));

    // Two mutations, then two queries — should batch each group separately
    let stmts = qql_core::parser::Parser::parse_all(
        "UPSERT INTO docs VALUES {id: 1};\
         DELETE FROM docs WHERE id = 2;\
         QUERY TEXT 'a' MODEL 'test-model' FROM docs USING dense AS DENSE LIMIT 1;\
         QUERY TEXT 'b' MODEL 'test-model' FROM docs USING dense AS DENSE LIMIT 1;",
    )
    .unwrap();
    let results = executor.execute_batch_nodes(stmts, false).await.unwrap();

    assert_eq!(results.len(), 4);
    assert_eq!(results[0].operation, "UPSERT");
    assert_eq!(results[1].operation, "DELETE");
    assert_eq!(results[2].operation, "QUERY");
    assert_eq!(results[3].operation, "QUERY");

    assert_eq!(*update_batch.lock().unwrap(), 1);
    assert_eq!(*query_batch.lock().unwrap(), 1);
}

#[tokio::test]
async fn test_single_mutation_not_batched() {
    let client = MockQdrantClient::default();
    let update_count = client.update_batch_call_count.clone();

    let executor = Executor::new(Box::new(client), Some(test_config()));
    let stmts = qql_core::parser::Parser::parse_all("DELETE FROM docs WHERE id = 1;").unwrap();
    let results = executor.execute_batch_nodes(stmts, false).await.unwrap();

    assert_eq!(results.len(), 1);
    assert!(results[0].ok);
    assert_eq!(
        *update_count.lock().unwrap(),
        0,
        "single mutation must not use update batch"
    );
}

#[tokio::test]
async fn test_continue_preserves_failure_position_and_batch_boundary() {
    let client = MockQdrantClient::default();
    let update_batches = client.update_batch_call_count.clone();
    let individual_calls = client.execute_planned_call_count.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));
    let stmts = qql_core::parser::Parser::parse_all(
        "DELETE FROM docs WHERE id = 1;\
         QUERY TEXT 'missing schema' MODEL 'test-model' FROM docs LIMIT 1;\
         DELETE FROM docs WHERE id = 2;",
    )
    .unwrap();

    let results = executor.execute_batch_nodes(stmts, false).await.unwrap();

    assert_eq!(results.len(), 3);
    assert_eq!(results[0].operation, "DELETE");
    assert!(results[0].ok);
    assert_eq!(results[1].operation, "PREPARE");
    assert!(!results[1].ok);
    assert_eq!(results[2].operation, "DELETE");
    assert!(results[2].ok);
    assert_eq!(*update_batches.lock().unwrap(), 0);
    assert_eq!(*individual_calls.lock().unwrap(), 2);
}

#[tokio::test]
async fn test_stop_dispatches_prior_statement_before_later_prepare_failure() {
    let client = MockQdrantClient::default();
    let individual_calls = client.execute_planned_call_count.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));
    let stmts = qql_core::parser::Parser::parse_all(
        "DELETE FROM docs WHERE id = 1;\
         QUERY TEXT 'missing schema' MODEL 'test-model' FROM docs LIMIT 1;",
    )
    .unwrap();

    let error = executor
        .execute_batch_nodes(stmts, true)
        .await
        .expect_err("the second statement should fail preparation");

    assert!(error.message.contains("no mock info set"));
    assert_eq!(*individual_calls.lock().unwrap(), 1);
}

#[tokio::test]
async fn test_batch_upserts_keep_single_statement_auto_create_semantics() {
    let client = MockQdrantClient::default();
    let creates = client.execute_planned_call_count.clone();
    let update_batches = client.update_batch_call_count.clone();
    let embedder = Arc::new(MockEmbedder {
        dense: vec![0.1, 0.2, 0.3],
        sparse_indices: Vec::new(),
        sparse_values: Vec::new(),
        multi: vec![vec![0.1, 0.2], vec![0.3, 0.4]],
    });
    let mut config = test_config();
    config.embedding_dimension = 3;
    let executor = Executor::with_embedder(Box::new(client), Some(config), Some(embedder));
    let stmts = qql_core::parser::Parser::parse_all(
        "UPSERT INTO docs VALUES {id: 1, text: 'a'} USING DENSE MODEL 'mock';\
         UPSERT INTO docs VALUES {id: 2, text: 'b'} USING DENSE MODEL 'mock';",
    )
    .unwrap();

    let results = executor.execute_batch_nodes(stmts, true).await.unwrap();

    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|result| result.ok));
    assert_eq!(*creates.lock().unwrap(), 1);
    assert_eq!(*update_batches.lock().unwrap(), 1);
}

#[tokio::test]
async fn test_execute_batch_continue_collects_parse_errors_in_order() {
    let client = MockQdrantClient::default();
    let executor = Executor::new(Box::new(client), Some(test_config()));

    let report = executor
        .execute_batch(
            &[
                "DELETE FROM docs WHERE id = 1",
                "not qql",
                "DELETE FROM docs WHERE id = 2",
            ],
            OnError::Continue,
        )
        .await
        .unwrap();

    assert!(!report.ok);
    assert_eq!(report.succeeded, 2);
    assert_eq!(report.failed, 1);
    assert_eq!(report.results[0].operation, "DELETE");
    assert_eq!(report.results[1].operation, "PARSE");
    assert_eq!(report.results[2].operation, "DELETE");
}

#[tokio::test]
async fn test_execute_batch_stop_dispatches_prior_entries_before_parse_failure() {
    let client = MockQdrantClient::default();
    let individual_calls = client.execute_planned_call_count.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));

    let error = executor
        .execute_batch(&["DELETE FROM docs WHERE id = 1", "not qql"], OnError::Stop)
        .await
        .expect_err("the second entry should fail parsing");

    assert_eq!(error.code, "QQL-PARSE-STATEMENT");
    assert_eq!(*individual_calls.lock().unwrap(), 1);
}

#[tokio::test]
async fn test_execute_continue_returns_parse_failure_report() {
    let executor = Executor::new(Box::new(MockQdrantClient::default()), Some(test_config()));

    let report = executor
        .execute("not qql", OnError::Continue)
        .await
        .expect("continue mode should report parse failures");

    assert!(!report.ok);
    assert_eq!(report.succeeded, 0);
    assert_eq!(report.failed, 1);
    assert_eq!(report.results[0].operation, "PARSE");
}

#[tokio::test]
async fn test_execute_continue_returns_single_preparation_failure_report() {
    let executor = Executor::new(Box::new(MockQdrantClient::default()), Some(test_config()));

    let report = executor
        .execute(
            "QUERY TEXT 'missing schema' MODEL 'test-model' FROM docs LIMIT 1",
            OnError::Continue,
        )
        .await
        .expect("continue mode should report preparation failures");

    assert!(!report.ok);
    assert_eq!(report.succeeded, 0);
    assert_eq!(report.failed, 1);
    assert_eq!(report.results[0].operation, "PREPARE");
}

/// RT-04: CROSS RERANK with same-ID candidates from different collections
/// must preserve both hits using (collection, id) identity, not just point id.
#[tokio::test]
async fn cross_rerank_preserves_same_id_different_collections() {
    let mut client = MockQdrantClient::default();
    client.exists = true;
    // Both collections use a simple dense-only topology.
    client.info = Some(collection_with_vectors(&["dense"], &[]));
    // Same point id "1" exists in both collections with different body text.
    client.point_map.lock().unwrap().insert(
        "coll_a".to_string(),
        serde_json::json!({"result": {"points": [
            {"id": "1", "score": 0.9, "payload": {"body": "alpha body text"}}
        ]}}),
    );
    client.point_map.lock().unwrap().insert(
        "coll_b".to_string(),
        serde_json::json!({"result": {"points": [
            {"id": "1", "score": 0.8, "payload": {"body": "beta body text"}}
        ]}}),
    );

    let embedder = Arc::new(MockEmbedder {
        dense: vec![0.1, 0.2, 0.3],
        sparse_indices: vec![],
        sparse_values: vec![],
        multi: vec![vec![0.1, 0.2], vec![0.3, 0.4]],
    });
    let executor =
        Executor::with_embedder(Box::new(client), Some(test_local_config()), Some(embedder));

    let report = executor
        .execute(
            "WITH c1 AS (QUERY TEXT 'hello' MODEL 'test-model' FROM coll_a USING dense AS DENSE LIMIT 5), \
             c2 AS (QUERY TEXT 'hello' MODEL 'test-model' FROM coll_b USING dense AS DENSE LIMIT 5) \
             QUERY CROSS RERANK TEXT 'hello' MODEL 'mock' ON FIELD body \
             FROM coll_a PREFETCH (c1, c2) LIMIT 10",
            OnError::Stop,
        )
        .await
        .expect("CROSS RERANK should succeed");

    assert!(report.ok, "report should be ok: {report:?}");
    assert_eq!(report.results.len(), 1);
    assert_eq!(report.results[0].operation, "CROSS_RERANK");
    let data = report.results[0].data.as_ref().expect("should have data");
    let hits = data.as_array().expect("data should be an array of hits");

    // Both same-ID candidates from different collections survive dedup.
    assert_eq!(
        hits.len(),
        2,
        "expected 2 hits (one per collection), got {}: {hits:?}",
        hits.len()
    );

    // Verify each hit carries the correct source collection.
    let collections: Vec<&str> = hits
        .iter()
        .map(|h| h["collection"].as_str().unwrap_or(""))
        .collect();
    assert!(
        collections.contains(&"coll_a"),
        "missing coll_a in results: {collections:?}"
    );
    assert!(
        collections.contains(&"coll_b"),
        "missing coll_b in results: {collections:?}"
    );

    // Verify the id is "1" for both.
    let ids: Vec<&str> = hits
        .iter()
        .map(|h| h["id"].as_str().unwrap_or(""))
        .collect();
    assert_eq!(
        ids.iter().filter(|id| **id == "1").count(),
        2,
        "both hits should have id '1'"
    );

    // Verify the reranker scored them (scores come from MockEmbedder::rerank_pairs).
    for hit in hits {
        let score = hit["score"]
            .as_f64()
            .expect("hit should have a numeric score");
        assert!(score > 0.0, "score should be positive, got {score}");
    }
}
