//! Tests for corpus-true `avg_len` estimation: real sampled lengths drive
//! the estimate, validation fails closed, and empty samples yield `None`.

use std::collections::HashMap;

use qql_plan::PlanPointId;

use super::mock::{MockQdrantClient, collection_with_vectors};
use crate::executor::{ExecData, Executor, SearchHit};

fn hit(id: u64, payload: &[(&str, &str)]) -> SearchHit {
    SearchHit {
        id: PlanPointId::Number(id),
        score: 0.0,
        payload: Some(
            payload
                .iter()
                .map(|(k, v)| (k.to_string(), serde_json::json!(v)))
                .collect(),
        ),
        collection: None,
        vector: None,
    }
}

fn executor_with_docs() -> Executor {
    let mut client = MockQdrantClient::default();
    client.exists = true;
    client.info = Some(collection_with_vectors(&["dense"], &["sparse"]));
    client.point_map.lock().unwrap().insert(
        "docs".to_string(),
        ExecData::Hits(vec![
            hit(1, &[("title", "alpha beta gamma delta")]),
            hit(2, &[("title", "epsilon zeta")]),
            hit(3, &[("other", "zzz")]),
        ]),
    );
    Executor::new(Box::new(client), None)
}

#[tokio::test]
async fn estimate_uses_real_sampled_lengths() {
    let executor = executor_with_docs();
    let estimate = executor
        .estimate_bm25_avg_len("docs", "title", 100)
        .await
        .expect("estimate")
        .expect("sample has texts");
    // Post-pipeline English tokens: 4 + 2 over 2 documents.
    assert_eq!(estimate.docs, 2);
    assert_eq!(estimate.mean, 3.0);
}

#[tokio::test]
async fn estimate_supports_dotted_paths_and_arrays() {
    let mut client = MockQdrantClient::default();
    client.exists = true;
    client.info = Some(collection_with_vectors(&["dense"], &["sparse"]));
    let payload: HashMap<String, serde_json::Value> = HashMap::from([
        (
            "meta".to_string(),
            serde_json::json!({"title": "alpha beta"}),
        ),
        (
            "tags".to_string(),
            serde_json::json!(["gamma delta", "epsilon"]),
        ),
        ("count".to_string(), serde_json::json!(7)),
    ]);
    client.point_map.lock().unwrap().insert(
        "docs".to_string(),
        ExecData::Hits(vec![SearchHit {
            id: PlanPointId::Number(1),
            score: 0.0,
            payload: Some(payload),
            collection: None,
            vector: None,
        }]),
    );
    let executor = Executor::new(Box::new(client), None);
    let dotted = executor
        .estimate_bm25_avg_len("docs", "meta.title", 10)
        .await
        .expect("estimate")
        .expect("dotted path resolves");
    assert_eq!(dotted.mean, 2.0);
    let array = executor
        .estimate_bm25_avg_len("docs", "tags", 10)
        .await
        .expect("estimate")
        .expect("array elements count");
    assert_eq!(array.docs, 2);
    assert_eq!(array.mean, 1.5);
    assert!(
        executor
            .estimate_bm25_avg_len("docs", "count", 10)
            .await
            .expect("estimate")
            .is_none(),
        "non-string fields contribute nothing"
    );
}

#[tokio::test]
async fn estimate_fails_closed_and_handles_empties() {
    let executor = executor_with_docs();
    let err = executor
        .estimate_bm25_avg_len("", "title", 10)
        .await
        .expect_err("empty collection rejects");
    assert_eq!(err.code, "QQL-VALIDATION-COLLECTION");
    let err = executor
        .estimate_bm25_avg_len("docs", "  ", 10)
        .await
        .expect_err("empty field rejects");
    assert_eq!(err.code, "QQL-VALIDATION-FIELD");
    assert!(
        executor
            .estimate_bm25_avg_len("docs", "title", 0)
            .await
            .expect("zero sample")
            .is_none()
    );
    assert!(
        executor
            .estimate_bm25_avg_len("docs", "missing", 10)
            .await
            .expect("missing field")
            .is_none(),
        "no usable texts → None, not avg_len = 0"
    );
    // Quoted collection names with hostile characters parse, find nothing.
    assert!(
        executor
            .estimate_bm25_avg_len("o'brien", "title", 10)
            .await
            .expect("quoted name")
            .is_none()
    );
}
