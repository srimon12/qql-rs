use std::collections::HashMap;
use std::sync::Arc;

use crate::backend::CollectionInfo;
use crate::executor::Executor;
use crate::executor::response::OnError;

use super::mock::{
    MockEmbedder, MockQdrantClient, collection_with_vectors, test_config, test_local_config,
};

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
async fn test_upsert_bad_types() {
    let mut client = MockQdrantClient::default();
    client.exists = true;
    client.info = Some(CollectionInfo::default());
    let executor = Executor::new(Box::new(client), Some(test_config()));

    let query = "UPSERT INTO docs VALUES {id: 1}, {id: 2, text: 'a'}, {id: 3}";
    let resp = executor.execute(query, OnError::Stop).await;
    assert!(resp.is_ok(), "{:?}", resp.err());
}

#[tokio::test]
async fn test_upsert_vector_parameter_and_wait() {
    let client = MockQdrantClient {
        exists: true,
        collections: vec!["docs".to_string()],
        info: Some(collection_with_vectors(&["dense"], &[])),
        ..Default::default()
    };
    let last_planned = client.last_planned.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));

    let mut params = HashMap::new();
    params.insert(
        "vec".to_string(),
        qql_core::ast::Value::F32Array(vec![0.1, 0.2, 0.3]),
    );

    let report = executor
        .execute_with_params(
            "UPSERT INTO docs VALUES {id: 1, vector: :vec} WAIT true",
            &params,
            OnError::Stop,
        )
        .await
        .expect("upsert with vector param and wait should succeed");
    assert!(report.ok);

    let op = last_planned.lock().unwrap().take().expect("planned op");
    match op {
        qql_plan::PlannedOperation::Upsert { request, wait, .. } => {
            assert!(wait, "wait must be true");
            assert_eq!(request.points.len(), 1);
            let p = &request.points[0];
            match &p.vector {
                Some(qql_plan::PlanPointVectors::Named(entries)) => {
                    assert_eq!(entries.len(), 1);
                    assert_eq!(entries[0].0, "dense");
                    match &entries[0].1 {
                        qql_plan::PlanVectorValue::Dense(vec) => {
                            assert_eq!(*vec, vec![0.1, 0.2, 0.3]);
                        }
                        other => panic!("expected dense vector, got {other:?}"),
                    }
                }
                other => panic!("expected named dense vector, got {other:?}"),
            }
        }
        other => panic!("expected upsert operation, got {other:?}"),
    }
}
