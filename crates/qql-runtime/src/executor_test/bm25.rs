use std::sync::Arc;

use qql_core::error::QqlError;

use super::mock::{MockQdrantClient, collection_with_vectors, test_local_config};
use crate::embedder::{Embedder, HttpEmbedder, HttpEmbedderOptions};
use crate::executor::Executor;
use crate::executor::response::OnError;

fn http_embedder(k1: Option<f64>, b: Option<f64>, avg_len: Option<f64>) -> Arc<dyn Embedder> {
    let embedder = HttpEmbedder::try_with_options(HttpEmbedderOptions {
        endpoint: "http://127.0.0.1:9/v1/embeddings".to_string(),
        api_key: String::new(),
        model: "unused-dense".to_string(),
        dimension: 3,
        bm25_k1: k1,
        bm25_b: b,
        bm25_avg_len: avg_len,
        ..Default::default()
    })
    .expect("dummy-endpoint HttpEmbedder must construct without network");
    Arc::new(embedder)
}

/// Executor-level: BM25 params configured on the local embedder must be the
/// weights the executor puts on the wire for a TEXT upsert (`docs` is a
/// sparse-only mock collection, so this exercises the document path only).
#[tokio::test]
async fn configured_bm25_params_land_in_planned_sparse_vector() {
    let mut client = MockQdrantClient::default();
    client.exists = true;
    client.info = Some(collection_with_vectors(&[], &["sparse"]));
    let last_planned = client.last_planned.clone();

    let params = qql_embed::Bm25Params::new(2.0, 0.5, 4.0).expect("valid params");
    let executor = Executor::with_embedder(
        Box::new(client),
        Some(test_local_config()),
        Some(http_embedder(
            Some(params.k1()),
            Some(params.b()),
            Some(params.avg_len()),
        )),
    );

    let text = "cat sat mat cat";
    let report = executor
        .execute(
            &format!("UPSERT INTO docs VALUES {{id: 1, text: '{text}'}}"),
            OnError::Stop,
        )
        .await
        .expect("upsert");
    assert!(report.ok, "upsert failed: {report:?}");

    let op = last_planned.lock().unwrap().take().expect("planned op");
    let route = qql_plan::plan::to_rest_route(&op).expect("rest route");
    let vector = &route.body_json().unwrap()["points"][0]["vector"]["sparse"];
    let indices: Vec<u32> =
        serde_json::from_value(vector["indices"].clone()).expect("sparse indices");
    let values: Vec<f32> = serde_json::from_value(vector["values"].clone()).expect("sparse values");

    let expected = qql_embed::sparse::embed_document_with_params(text, &params);
    assert_eq!(indices, expected.indices, "stored indices must match");
    assert_eq!(values.len(), expected.values.len());
    for (got, want) in values.iter().zip(&expected.values) {
        assert!(
            (got - want).abs() < 1e-6,
            "stored BM25 weight {got} != configured {want}"
        );
    }

    // Configured k1/b/avg_len genuinely differ from the Qdrant defaults.
    let default = qql_embed::sparse::embed_document(text);
    assert_ne!(values, default.values);
}

/// Unset parameters keep the Qdrant `qdrant/bm25` defaults byte-for-byte.
#[tokio::test]
async fn unset_bm25_params_stay_byte_identical_to_defaults() {
    let mut client = MockQdrantClient::default();
    client.exists = true;
    client.info = Some(collection_with_vectors(&[], &["sparse"]));
    let last_planned = client.last_planned.clone();

    let executor = Executor::with_embedder(
        Box::new(client),
        Some(test_local_config()),
        Some(http_embedder(None, None, None)),
    );

    let text = "Recipe for baking chocolate chip cookies";
    let report = executor
        .execute(
            &format!("UPSERT INTO docs VALUES {{id: 1, text: '{text}'}}"),
            OnError::Stop,
        )
        .await
        .expect("upsert");
    assert!(report.ok, "upsert failed: {report:?}");

    let op = last_planned.lock().unwrap().take().expect("planned op");
    let route = qql_plan::plan::to_rest_route(&op).expect("rest route");
    let vector = &route.body_json().unwrap()["points"][0]["vector"]["sparse"];
    let indices: Vec<u32> =
        serde_json::from_value(vector["indices"].clone()).expect("sparse indices");
    let values: Vec<f32> = serde_json::from_value(vector["values"].clone()).expect("sparse values");

    let expected = qql_embed::sparse::embed_document(text);
    assert_eq!(indices, expected.indices);
    assert_eq!(values, expected.values);
}

#[test]
fn http_embedder_rejects_invalid_bm25_params_fail_closed() {
    let cases = [
        (Some(0.0), None, None),
        (Some(f64::NAN), None, None),
        (None, Some(-0.5), None),
        (None, Some(1.5), None),
        (None, None, Some(0.0)),
        (None, None, Some(f64::INFINITY)),
    ];
    for (k1, b, avg_len) in cases {
        let err = http_embedder_err(k1, b, avg_len);
        assert_eq!(
            err.code, "QQL-VALIDATION-CONFIG",
            "({k1:?}, {b:?}, {avg_len:?}) must fail closed"
        );
    }
}

fn http_embedder_err(k1: Option<f64>, b: Option<f64>, avg_len: Option<f64>) -> QqlError {
    HttpEmbedder::try_with_options(HttpEmbedderOptions {
        endpoint: "http://127.0.0.1:9/v1/embeddings".to_string(),
        api_key: String::new(),
        model: "unused-dense".to_string(),
        dimension: 3,
        bm25_k1: k1,
        bm25_b: b,
        bm25_avg_len: avg_len,
        ..Default::default()
    })
    .err()
    .expect("invalid BM25 params must be rejected at construction")
}
