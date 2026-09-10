use std::sync::Arc;

use super::mock::{
    MockEmbedder, MockQdrantClient, collection_with_vectors, collection_with_vectors_multi,
    test_config, test_local_config,
};
use crate::client::CollectionInfo;
use crate::executor::{ExecData, Executor, FacetHit, OnError, SearchHit};
use qql_plan::{PlanFacetValue, PlanPointId};

/// Typed hit fixture with string payload values.
fn hit(id: PlanPointId, score: f32, payload: &[(&str, &str)]) -> SearchHit {
    SearchHit {
        id,
        score,
        payload: if payload.is_empty() {
            None
        } else {
            Some(
                payload
                    .iter()
                    .map(|(k, v)| (k.to_string(), serde_json::json!(v)))
                    .collect(),
            )
        },
        collection: None,
        vector: None,
    }
}

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
        ExecData::Hits(vec![hit(
            PlanPointId::String("1".into()),
            0.9,
            &[("body", "alpha body text")],
        )]),
    );
    client.point_map.lock().unwrap().insert(
        "coll_b".to_string(),
        ExecData::Hits(vec![hit(
            PlanPointId::String("1".into()),
            0.8,
            &[("body", "beta body text")],
        )]),
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
    let hits = report.results[0]
        .hits_ref()
        .expect("cross rerank should yield typed hits");

    assert_eq!(
        hits.len(),
        2,
        "expected 2 hits (one per collection), got {}: {hits:?}",
        hits.len()
    );

    let collections: Vec<&str> = hits
        .iter()
        .map(|h| h.collection.as_deref().unwrap_or(""))
        .collect();
    assert!(
        collections.contains(&"coll_a"),
        "missing coll_a in results: {collections:?}"
    );
    assert!(
        collections.contains(&"coll_b"),
        "missing coll_b in results: {collections:?}"
    );

    let ids: Vec<String> = hits.iter().map(|h| h.id.to_string()).collect();
    assert_eq!(
        ids.iter().filter(|id| id.as_str() == "1").count(),
        2,
        "both hits should have id '1'"
    );

    for hit in hits {
        assert!(
            hit.score > 0.0,
            "score should be positive, got {}",
            hit.score
        );
    }
}

#[tokio::test]
async fn cross_rerank_empty_candidates_returns_zero_hits() {
    // No `point_map` entries: every candidate stage returns zero points, so
    // the client-side scorer short-circuits before touching the embedder.
    let mut client = MockQdrantClient::default();
    client.exists = true;
    client.info = Some(collection_with_vectors(&["dense"], &[]));
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
            "WITH c1 AS (QUERY TEXT 'hello' MODEL 'test-model' FROM empty_coll USING dense AS DENSE LIMIT 5) \
             QUERY CROSS RERANK TEXT 'hello' MODEL 'mock' ON FIELD body \
             FROM empty_coll PREFETCH (c1) LIMIT 10",
            OnError::Stop,
        )
        .await
        .expect("empty CROSS RERANK should succeed");

    assert!(report.ok, "{report:?}");
    assert_eq!(report.results.len(), 1);
    assert_eq!(report.results[0].operation, "CROSS_RERANK");
    assert_eq!(report.results[0].message, "Found 0 hits");
    assert_eq!(report.results[0].hits(), Some(Vec::new()));
}

#[tokio::test]
async fn cross_rerank_missing_payload_field_errors() {
    // Candidates exist but none carry the rerank field (and it is not the
    // `text` fallback): fail closed with QQL-RERANK-CROSS-FIELD.
    let mut client = MockQdrantClient::default();
    client.exists = true;
    client.info = Some(collection_with_vectors(&["dense"], &[]));
    client.point_map.lock().unwrap().insert(
        "docs".to_string(),
        ExecData::Hits(vec![hit(
            PlanPointId::Number(1),
            0.9,
            &[("title", "no body here")],
        )]),
    );
    let embedder = Arc::new(MockEmbedder {
        dense: vec![0.1, 0.2, 0.3],
        sparse_indices: vec![],
        sparse_values: vec![],
        multi: vec![vec![0.1, 0.2], vec![0.3, 0.4]],
    });
    let executor =
        Executor::with_embedder(Box::new(client), Some(test_local_config()), Some(embedder));

    let err = executor
        .execute(
            "WITH c1 AS (QUERY TEXT 'hello' MODEL 'test-model' FROM docs USING dense AS DENSE LIMIT 5) \
             QUERY CROSS RERANK TEXT 'hello' MODEL 'mock' ON FIELD body \
             FROM docs PREFETCH (c1) LIMIT 10",
            OnError::Stop,
        )
        .await
        .expect_err("missing rerank field must fail");

    assert_eq!(err.code, "QQL-RERANK-CROSS-FIELD");
}

/// Test-only embedder returning a fixed (possibly wrong-cardinality) score
/// list for cross-rerank pair scoring.
struct FixedScoreEmbedder {
    scores: Vec<f32>,
}

#[async_trait::async_trait]
impl crate::embedder::Embedder for FixedScoreEmbedder {
    async fn embed_dense(
        &self,
        _text: &str,
        _model: &str,
    ) -> Result<Vec<f32>, qql_core::error::QqlError> {
        Ok(vec![0.1, 0.2, 0.3])
    }

    async fn rerank_pairs(
        &self,
        _query: &str,
        _documents: &[String],
        _model: &str,
    ) -> Result<Vec<f32>, qql_core::error::QqlError> {
        Ok(self.scores.clone())
    }
}

#[tokio::test]
async fn cross_rerank_score_cardinality_mismatch_errors() {
    // Two scored documents but one returned score: fail closed with
    // QQL-RERANK-CROSS (the embedder broke the same-order contract).
    let mut client = MockQdrantClient::default();
    client.exists = true;
    client.info = Some(collection_with_vectors(&["dense"], &[]));
    client.point_map.lock().unwrap().insert(
        "docs".to_string(),
        ExecData::Hits(vec![
            hit(PlanPointId::Number(1), 0.9, &[("body", "first document")]),
            hit(PlanPointId::Number(2), 0.8, &[("body", "second document")]),
        ]),
    );
    let embedder = Arc::new(FixedScoreEmbedder { scores: vec![0.5] });
    let executor =
        Executor::with_embedder(Box::new(client), Some(test_local_config()), Some(embedder));

    let err = executor
        .execute(
            "WITH c1 AS (QUERY TEXT 'hello' MODEL 'test-model' FROM docs USING dense AS DENSE LIMIT 5) \
             QUERY CROSS RERANK TEXT 'hello' MODEL 'mock' ON FIELD body \
             FROM docs PREFETCH (c1) LIMIT 10",
            OnError::Stop,
        )
        .await
        .expect_err("score cardinality mismatch must fail");

    assert_eq!(err.code, "QQL-RERANK-CROSS");
    assert!(
        err.message.contains("1 scores for 2 documents"),
        "unexpected message: {}",
        err.message
    );
}

#[tokio::test]
async fn numeric_and_string_ids_preserve_json_types() {
    let mut client = MockQdrantClient::default();
    client.exists = true;
    client.info = Some(collection_with_vectors(&["dense"], &[]));
    client.point_map.lock().unwrap().insert(
        "test_coll".to_string(),
        ExecData::Hits(vec![
            hit(PlanPointId::Number(42), 0.9, &[("title", "numeric id")]),
            hit(
                PlanPointId::String("b3e0c0ea-52aa-4ebc-bd89-e137b0196ce2".into()),
                0.8,
                &[("title", "uuid id")],
            ),
        ]),
    );

    let executor = Executor::new(Box::new(client), Some(test_local_config()));
    let report = executor
        .execute("QUERY [0.1, 0.2] FROM test_coll LIMIT 2", OnError::Stop)
        .await
        .expect("query should succeed");

    assert!(report.ok);
    let hits = report.results[0].hits_ref().expect("hits present");
    assert_eq!(hits[0].id, qql_plan::PlanPointId::Number(42));
    assert_eq!(
        hits[1].id,
        qql_plan::PlanPointId::String("b3e0c0ea-52aa-4ebc-bd89-e137b0196ce2".to_string())
    );
}

#[tokio::test]
async fn facet_response_normalizes_hits_in_data() {
    let mut client = MockQdrantClient::default();
    client.exists = true;
    client.info = Some(collection_with_vectors(&["dense"], &[]));
    client.point_map.lock().unwrap().insert(
        "test_coll".to_string(),
        ExecData::Facet(vec![
            FacetHit {
                value: PlanFacetValue::Keyword("electronics".into()),
                count: 12,
            },
            FacetHit {
                value: PlanFacetValue::Keyword("clothing".into()),
                count: 5,
            },
        ]),
    );

    let executor = Executor::new(Box::new(client), Some(test_local_config()));
    let report = executor
        .execute("FACET category FROM test_coll LIMIT 10", OnError::Stop)
        .await
        .expect("facet should succeed");

    assert!(report.ok);
    let facet = report.results[0]
        .facet()
        .expect("facet data should be present");
    assert_eq!(facet.len(), 2);
    assert_eq!(
        facet[0],
        (PlanFacetValue::Keyword("electronics".into()), 12)
    );
}

#[tokio::test]
async fn get_points_bare_array_result_yields_hits() {
    let client = MockQdrantClient {
        info: Some(collection_with_vectors(&["dense"], &[])),
        ..Default::default()
    };
    client.point_map.lock().unwrap().insert(
        "docs".to_string(),
        ExecData::Hits(vec![
            SearchHit {
                id: PlanPointId::Number(1483),
                score: 0.0,
                payload: Some(std::collections::HashMap::from([(
                    "text".to_string(),
                    serde_json::json!("first"),
                )])),
                collection: None,
                vector: None,
            },
            SearchHit {
                id: PlanPointId::Number(1787),
                score: 0.0,
                payload: Some(std::collections::HashMap::from([(
                    "text".to_string(),
                    serde_json::json!("second"),
                )])),
                collection: None,
                vector: None,
            },
        ]),
    );
    let executor = Executor::new(Box::new(client), Some(test_config()));
    let report = executor
        .execute("QUERY POINTS (1483, 1787) FROM docs", OnError::Stop)
        .await
        .expect("point lookup should succeed");
    assert!(report.ok, "{report:?}");
    let hits = report.results[0].hits_ref().expect("hits present");
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].id, qql_plan::PlanPointId::Number(1483));
    assert_eq!(hits[1].id, qql_plan::PlanPointId::Number(1787));
}
