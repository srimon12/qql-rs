use std::sync::Arc;

use super::mock::{
    MockEmbedder, MockQdrantClient, collection_with_vectors, collection_with_vectors_multi,
    test_config, test_local_config,
};
use crate::client::CollectionInfo;
use crate::executor::{Executor, OnError};

#[tokio::test]
async fn test_do_query_basic() {
    let mut client = MockQdrantClient::default();
    client.exists = true;
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

    assert_eq!(prefetch[0]["using"], "dense");
    assert!(
        prefetch[0]["query"]["nearest"].is_array(),
        "dense arm must be a dense vector array, got {}",
        prefetch[0]["query"]["nearest"]
    );

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
    let mut client = MockQdrantClient::default();
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
async fn cross_rerank_preserves_same_id_different_collections() {
    let mut client = MockQdrantClient::default();
    client.exists = true;
    client.info = Some(collection_with_vectors(&["dense"], &[]));
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

    assert_eq!(
        hits.len(),
        2,
        "expected 2 hits (one per collection), got {}: {hits:?}",
        hits.len()
    );

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

    let ids: Vec<&str> = hits
        .iter()
        .map(|h| h["id"].as_str().unwrap_or(""))
        .collect();
    assert_eq!(
        ids.iter().filter(|id| **id == "1").count(),
        2,
        "both hits should have id '1'"
    );

    for hit in hits {
        let score = hit["score"]
            .as_f64()
            .expect("hit should have a numeric score");
        assert!(score > 0.0, "score should be positive, got {score}");
    }
}

#[tokio::test]
async fn numeric_and_string_ids_preserve_json_types() {
    let mut client = MockQdrantClient::default();
    client.exists = true;
    client.info = Some(collection_with_vectors(&["dense"], &[]));
    client.point_map.lock().unwrap().insert(
        "test_coll".to_string(),
        serde_json::json!({"result": {"points": [
            {"id": 42, "score": 0.9, "payload": {"title": "numeric id"}},
            {"id": "b3e0c0ea-52aa-4ebc-bd89-e137b0196ce2", "score": 0.8, "payload": {"title": "uuid id"}}
        ]}}),
    );

    let executor = Executor::new(Box::new(client), Some(test_local_config()));
    let report = executor
        .execute("QUERY [0.1, 0.2] FROM test_coll LIMIT 2", OnError::Stop)
        .await
        .expect("query should succeed");

    assert!(report.ok);
    let hits = report.results[0].data.as_ref().unwrap().as_array().unwrap();
    assert_eq!(hits[0]["id"].as_u64(), Some(42));
    assert_eq!(
        hits[1]["id"].as_str(),
        Some("b3e0c0ea-52aa-4ebc-bd89-e137b0196ce2")
    );
}

#[tokio::test]
async fn facet_response_normalizes_hits_in_data() {
    let mut client = MockQdrantClient::default();
    client.exists = true;
    client.info = Some(collection_with_vectors(&["dense"], &[]));
    client.point_map.lock().unwrap().insert(
        "test_coll".to_string(),
        serde_json::json!({
            "result": {
                "hits": [
                    {"value": "electronics", "count": 12},
                    {"value": "clothing", "count": 5}
                ]
            },
            "status": "ok",
            "time": 0.002
        }),
    );

    let executor = Executor::new(Box::new(client), Some(test_local_config()));
    let report = executor
        .execute("FACET category FROM test_coll LIMIT 10", OnError::Stop)
        .await
        .expect("facet should succeed");

    assert!(report.ok);
    let data = report.results[0]
        .data
        .as_ref()
        .expect("data should be present");
    let hits = data
        .as_array()
        .expect("facet data must be a normalized array of hits");
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0]["value"], "electronics");
    assert_eq!(hits[0]["count"], 12);
}

#[tokio::test]
async fn get_points_bare_array_result_yields_hits() {
    let client = MockQdrantClient {
        info: Some(collection_with_vectors(&["dense"], &[])),
        ..Default::default()
    };
    *client
        .point_map
        .lock()
        .unwrap()
        .entry("docs".to_string())
        .or_default() = serde_json::json!({
        "result": [
            {"id": 1483, "payload": {"text": "first"}},
            {"id": 1787, "payload": {"text": "second"}},
        ]
    });
    let executor = Executor::new(Box::new(client), Some(test_config()));
    let report = executor
        .execute("QUERY POINTS (1483, 1787) FROM docs", OnError::Stop)
        .await
        .expect("point lookup should succeed");
    assert!(report.ok, "{report:?}");
    let hits = report.results[0].data.as_ref().expect("data present");
    assert_eq!(hits.as_array().unwrap().len(), 2);
    assert_eq!(hits[0]["id"], 1483);
    assert_eq!(hits[1]["id"], 1787);
}
