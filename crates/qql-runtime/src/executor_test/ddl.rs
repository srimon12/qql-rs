use super::mock::{MockQdrantClient, collection_with_vectors, test_config};
use crate::executor::{Executor, OnError};

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

    let hnsw = &req["hnsw_config"];
    assert_eq!(hnsw["m"], 32);
    assert_eq!(hnsw["ef_construct"], 100);

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

    let opt = &req["optimizers_config"];
    assert_eq!(opt["deleted_threshold"], 0.2);
    assert_eq!(opt["default_segment_number"], 4);
    assert_eq!(opt["max_optimization_threads"], 2);

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

    let hnsw = &v_conf["hnsw_config"];
    assert_eq!(hnsw["m"], 16);

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

    assert_eq!(req["quantization_config"], "Disabled");
}

#[tokio::test]
async fn test_schema_cache_reuses_and_invalidates_on_ddl() {
    let client = MockQdrantClient {
        exists: true,
        collections: vec!["docs".to_string()],
        info: Some(collection_with_vectors(&["dense"], &["bm25"])),
        ..Default::default()
    };
    let info_count = client.info_call_count.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));

    let sql = "QUERY [0.1, 0.2, 0.3] FROM docs USING dense LIMIT 5";
    assert!(executor.execute(sql, OnError::Stop).await.unwrap().ok);
    assert_eq!(*info_count.lock().unwrap(), 1);

    assert!(executor.execute(sql, OnError::Stop).await.unwrap().ok);
    assert_eq!(
        *info_count.lock().unwrap(),
        1,
        "schema cache must skip the second GET /collections"
    );

    assert!(
        executor
            .execute(
                "CREATE INDEX ON COLLECTION docs FOR district TYPE keyword",
                OnError::Stop,
            )
            .await
            .unwrap()
            .ok
    );
    assert!(executor.execute(sql, OnError::Stop).await.unwrap().ok);
    assert_eq!(
        *info_count.lock().unwrap(),
        2,
        "DDL must invalidate the schema cache"
    );
}
