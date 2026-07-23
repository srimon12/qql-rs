#![allow(clippy::field_reassign_with_default)]

use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;
use qql_core::error::QqlError;
use qql_plan::{QueryBatchRequest, UpdateBatchRequest};

use crate::client::{CollectionInfo, CreateCollectionReq, CreateFieldIndexReq, QdrantOps};
use crate::config::QqlConfig;
use crate::executor::Executor;

struct MockQdrantClient {
    pub exists: bool,
    pub collections: Vec<String>,
    pub info: Option<CollectionInfo>,
    pub last_planned: Arc<Mutex<Option<qql_plan::PlannedOperation>>>,
    pub batch_call_count: Arc<Mutex<usize>>,
    pub last_batch_searches_count: Arc<Mutex<usize>>,
    pub update_batch_call_count: Arc<Mutex<usize>>,
    pub last_update_batch_ops_count: Arc<Mutex<usize>>,
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
        }
    }
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
        self.info
            .clone()
            .ok_or_else(|| QqlError::execution("QQL-EXECUTION", "no mock info set", None))
    }
    async fn create_collection(&self, req: CreateCollectionReq) -> Result<(), QqlError> {
        let _ = req;
        Ok(())
    }
    async fn update_collection(&self, req: serde_json::Value) -> Result<(), QqlError> {
        let _ = req;
        Ok(())
    }
    async fn delete_collection(&self, _name: &str) -> Result<(), QqlError> {
        Ok(())
    }
    async fn create_field_index(&self, _req: CreateFieldIndexReq) -> Result<(), QqlError> {
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
        let route = qql_plan::plan::to_rest_route(op);
        if route.path.contains("nonexistent") {
            return Err(QqlError::execution(
                "QQL-EXECUTION",
                "collection does not exist",
                None,
            ));
        }
        *self.last_planned.lock().unwrap() = Some(op.clone());
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
    let last_planned = client.last_planned.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));

    let query = "CREATE COLLECTION mycol WITH HNSW (m = 32, ef_construct = 100) WITH QUANTIZATION (type = 'scalar', always_ram = true, quantile = 0.99)";
    let resp = executor.execute(query).await;
    assert!(resp.is_ok(), "{:?}", resp.err());

    let op = last_planned.lock().unwrap().take().unwrap();
    let route = qql_plan::plan::to_rest_route(&op);
    assert_eq!(route.path, "/collections/mycol");
    let req = route.body_json().unwrap();
    assert_eq!(req["vectors"]["dense"]["size"], 384);

    // Check HNSW config serialization
    let hnsw = &req["hnsw_config"];
    assert_eq!(hnsw["m"], 32);
    assert_eq!(hnsw["ef_construct"], 100);

    // Check Quantization config serialization
    let quant = &req["quantization_config"];
    assert_eq!(quant["disabled"], false);
    assert_eq!(quant["quantization_config"]["type"], "scalar");
    assert_eq!(quant["quantization_config"]["always_ram"], true);
    assert_eq!(quant["quantization_config"]["quantile"], 0.99);
}

#[tokio::test]
async fn test_create_hybrid_materializes_default_schema() {
    let client = MockQdrantClient::default();
    let last_planned = client.last_planned.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));

    executor
        .execute("CREATE COLLECTION mycol HYBRID")
        .await
        .unwrap();

    let op = last_planned.lock().unwrap().take().unwrap();
    let route = qql_plan::plan::to_rest_route(&op);
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
    let resp = executor.execute(query).await;
    assert!(resp.is_ok(), "{:?}", resp.err());

    let op = last_planned.lock().unwrap().take().unwrap();
    let route = qql_plan::plan::to_rest_route(&op);
    let req = route.body_json().unwrap();

    // Check Optimizers config serialization
    let opt = &req["optimizers_config"];
    assert_eq!(opt["deleted_threshold"], 0.2);
    assert_eq!(opt["default_segment_number"], 4);
    assert_eq!(opt["max_optimization_threads"], 2);

    // Check Params serialization
    let params = &req["params"];
    assert_eq!(params["replication_factor"], 2);
    assert_eq!(params["on_disk_payload"], true);
}

#[tokio::test]
async fn test_create_collection_with_named_vectors_hnsw_quant() {
    let client = MockQdrantClient::default();
    let last_planned = client.last_planned.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));

    let query = "CREATE COLLECTION mycol (dense_vec VECTOR(128, Cosine) WITH HNSW (m = 16) WITH QUANTIZATION (type = 'binary', always_ram = false))";
    let resp = executor.execute(query).await;
    assert!(resp.is_ok(), "{:?}", resp.err());

    let op = last_planned.lock().unwrap().take().unwrap();
    let route = qql_plan::plan::to_rest_route(&op);
    let req = route.body_json().unwrap();

    let vectors = &req["vectors"];
    assert!(vectors.get("dense_vec").is_some());
    let v_conf = &vectors["dense_vec"];
    assert_eq!(v_conf["size"], 128);
    assert_eq!(v_conf["distance"], "Cosine");

    // Check per-vector HNSW
    let hnsw = &v_conf["hnsw_config"];
    assert_eq!(hnsw["m"], 16);

    // Check per-vector Quantization
    let quant = &v_conf["quantization_config"];
    assert_eq!(quant["type"], "binary");
    assert_eq!(quant["always_ram"], false);
}

#[tokio::test]
async fn test_alter_collection_quantization_and_hnsw() {
    let mut client = MockQdrantClient::default();
    client.exists = true;
    let last_planned = client.last_planned.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));

    let query = "ALTER COLLECTION mycol WITH HNSW (ef_construct = 150) WITH QUANTIZATION (type = 'product', always_ram = true)";
    let resp = executor.execute(query).await;
    assert!(resp.is_ok(), "{:?}", resp.err());

    let op = last_planned.lock().unwrap().take().unwrap();
    let route = qql_plan::plan::to_rest_route(&op);
    assert_eq!(route.path, "/collections/mycol");
    let req = route.body_json().unwrap();

    assert_eq!(req["hnsw_config"]["ef_construct"], 150);
    assert_eq!(
        req["quantization_config"]["quantization_config"]["type"],
        "product"
    );
    assert_eq!(
        req["quantization_config"]["quantization_config"]["always_ram"],
        true
    );
}

#[tokio::test]
async fn test_alter_collection_disable_quantization() {
    let mut client = MockQdrantClient::default();
    client.exists = true;
    let last_planned = client.last_planned.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));

    let query = "ALTER COLLECTION mycol WITH QUANTIZATION (disabled = true)";
    let resp = executor.execute(query).await;
    assert!(resp.is_ok(), "{:?}", resp.err());

    let op = last_planned.lock().unwrap().take().unwrap();
    let route = qql_plan::plan::to_rest_route(&op);
    let req = route.body_json().unwrap();

    assert_eq!(req["quantization_config"]["disabled"], true);
}

#[tokio::test]
async fn test_dml_missing_collection_errors() {
    let client = MockQdrantClient::default(); // exists = false
    let executor = Executor::new(Box::new(client), Some(test_config()));

    let resp_delete = executor
        .execute("DELETE FROM nonexistent WHERE id = 'abc'")
        .await;
    assert!(resp_delete.is_err());
    assert!(resp_delete.unwrap_err().message.contains("does not exist"));

    let resp_update = executor
        .execute("UPDATE nonexistent SET PAYLOAD = {k: 'v'} WHERE id = 'abc'")
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

    let query = "QUERY 'admin docs' FROM docs WHERE metadata.group = 'admin' LIMIT 10 OFFSET 5";
    let resp = executor.execute(query).await;
    assert!(resp.is_ok(), "{:?}", resp.err());

    let op = last_planned.lock().unwrap().take().unwrap();
    let route = qql_plan::plan::to_rest_route(&op);
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

    let query = "QUERY HYBRID TEXT 'hello' DENSE dense SPARSE sparse FUSION RRF FROM docs LIMIT 10";
    let resp = executor.execute(query).await;
    assert!(resp.is_ok(), "{:?}", resp.err());

    let op = last_planned.lock().unwrap().take().unwrap();
    let route = qql_plan::plan::to_rest_route(&op);
    assert_eq!(route.method, qql_plan::types::Method::Post);
    assert!(route.body.is_some());
}

#[tokio::test]
async fn test_do_select_returns_record_or_nil() {
    let mut client = MockQdrantClient::default();
    client.exists = true;
    let last_planned = client.last_planned.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));

    let resp = executor.execute("QUERY POINTS ('pt-1') FROM docs").await;
    assert!(resp.is_ok(), "{:?}", resp.err());

    let op = last_planned.lock().unwrap().take().unwrap();
    let route = qql_plan::plan::to_rest_route(&op);
    assert_eq!(route.method, qql_plan::types::Method::Post);
    assert!(route.path.contains("docs/points"));
}

#[tokio::test]
async fn test_delete_by_id_and_filter() {
    let mut client = MockQdrantClient::default();
    client.exists = true;
    let last_planned = client.last_planned.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));

    let resp = executor.execute("DELETE FROM docs WHERE id = 12").await;
    assert!(resp.is_ok(), "{:?}", resp.err());

    let op = last_planned.lock().unwrap().take().unwrap();
    let route = qql_plan::plan::to_rest_route(&op);
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
        .execute("UPDATE docs SET PAYLOAD = {status: 'active'} WHERE id = 12")
        .await;
    assert!(resp.is_ok(), "{:?}", resp.err());

    let op = last_planned.lock().unwrap().take().unwrap();
    let route = qql_plan::plan::to_rest_route(&op);
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
        .execute("UPDATE docs SET VECTOR dense = [1.0, 2.0] WHERE id = 'p1'")
        .await;
    assert!(resp.is_ok(), "{:?}", resp.err());

    let op = last_planned.lock().unwrap().take().unwrap();
    let route = qql_plan::plan::to_rest_route(&op);
    assert_eq!(route.method, qql_plan::types::Method::Put);
    assert!(route.path.contains("vectors"));
}

#[tokio::test]
async fn test_upsert_into_collection_creates_missing() {
    let client = MockQdrantClient::default();
    let last_planned = client.last_planned.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));

    let resp = executor
        .execute("UPSERT INTO docs VALUES {id: 'pt-1', text: 'hello'}")
        .await;
    assert!(resp.is_ok(), "{:?}", resp.err());

    let op = last_planned.lock().unwrap().take().unwrap();
    let route = qql_plan::plan::to_rest_route(&op);
    assert_eq!(route.method, qql_plan::types::Method::Put);
    assert!(route.path.contains("docs"));
}

#[tokio::test]
async fn test_do_scroll_returns_upstream_style_payload() {
    let mut client = MockQdrantClient::default();
    client.exists = true;
    let last_planned = client.last_planned.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));

    let resp = executor.execute("SCROLL FROM docs LIMIT 10").await;

    assert!(resp.is_ok(), "{:?}", resp.err());
    let op = last_planned.lock().unwrap().take().unwrap();
    let route = qql_plan::plan::to_rest_route(&op);
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
    });
    let executor = Executor::with_embedder(
        Box::new(client),
        Some(test_local_config()),
        Some(mock_embedder),
    );

    let query = "QUERY 'hello' FROM nonexistent LIMIT 10";
    let resp = executor.execute(query).await;
    assert!(resp.is_err());
    assert!(resp.unwrap_err().message.contains("does not exist"));
}

#[tokio::test]
async fn test_upsert_bad_types() {
    let mut client = MockQdrantClient::default();
    client.exists = true;
    let executor = Executor::new(Box::new(client), Some(test_config()));

    // Wait, the parser catches syntax errors. But logic errors?
    // E.g., UPSERT with mismatching value lengths
    let query = "UPSERT INTO docs VALUES {id: 1}, {id: 2, text: 'a'}, {id: 3}";
    let resp = executor.execute(query).await;
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
        "QUERY TEXT 'a' FROM docs USING dense LIMIT 1;\
         QUERY TEXT 'b' FROM docs USING dense LIMIT 1;\
         QUERY TEXT 'c' FROM docs USING dense LIMIT 1;",
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
         QUERY TEXT 'a' FROM docs USING dense LIMIT 1;\
         QUERY TEXT 'b' FROM docs USING dense LIMIT 1;",
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
