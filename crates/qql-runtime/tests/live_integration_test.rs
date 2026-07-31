use std::sync::Arc;

use qql::embedder::HttpEmbedder;
use qql::executor::{Executor, OnError};

#[tokio::test]
#[ignore = "requires local Qdrant (6333/6334) and Ollama (11434) running"]
async fn test_live_rest_and_grpc_with_ollama_embeddings() {
    let rest_url = "http://localhost:6333";
    let grpc_url = "http://localhost:6334";
    let ollama_url = "http://localhost:11434/v1/embeddings";
    let model_name = "all-minilm:l6-v2";

    let embedder = Arc::new(
        HttpEmbedder::new(
            ollama_url.to_string(),
            "".to_string(),
            model_name.to_string(),
            384,
        )
        .expect("HttpEmbedder creation failed"),
    );

    let rest_ops = Box::new(qql::rest::RestQdrant::new(rest_url, None));
    let rest_exec = Executor::with_embedder(rest_ops, None, Some(embedder.clone()));

    let grpc_ops = Box::new(qql::grpc::GrpcQdrant::from_url(grpc_url, None).unwrap());
    let grpc_exec = Executor::with_embedder(grpc_ops, None, Some(embedder.clone()));

    let collection_name = "live_integration_docs";

    // Clean up
    if rest_exec
        .ops()
        .collection_exists(collection_name)
        .await
        .unwrap_or(false)
    {
        let _ = rest_exec
            .execute(
                &format!("DROP COLLECTION {collection_name};"),
                OnError::Stop,
            )
            .await;
    }

    // Create collection
    let create_res = rest_exec
        .execute(
            &format!("CREATE COLLECTION {collection_name} (dense VECTOR(384, COSINE));"),
            OnError::Stop,
        )
        .await
        .unwrap();
    assert!(create_res.ok, "CREATE COLLECTION failed: {:?}", create_res);

    let exists = grpc_exec
        .ops()
        .collection_exists(collection_name)
        .await
        .unwrap();
    assert!(exists, "Collection should exist over gRPC");

    // Upsert documents
    let upsert_res = rest_exec
        .execute(
            &format!(
                "UPSERT INTO {collection_name} VALUES \
                 {{id: 1, text: 'Qdrant is a high performance vector database'}}, \
                 {{id: 2, text: 'Ollama enables running AI models locally'}}, \
                 {{id: 3, text: 'Rust provides memory safety and zero-cost abstractions'}} \
                 USING DENSE MODEL '{model_name}';"
            ),
            OnError::Stop,
        )
        .await
        .unwrap();
    assert!(upsert_res.ok, "UPSERT failed: {:?}", upsert_res);

    // REST search
    let rest_search = rest_exec
        .execute(
            &format!("QUERY '{model_name}' FROM {collection_name} USING dense LIMIT 5;"),
            OnError::Stop,
        )
        .await
        .unwrap();
    assert!(rest_search.ok, "REST search failed: {:?}", rest_search);
    assert!(
        rest_search.results[0].data.is_some(),
        "Search data should be present"
    );

    let hits = rest_search.results[0]
        .data
        .as_ref()
        .unwrap()
        .as_array()
        .unwrap();
    assert!(!hits.is_empty(), "Should return search hits");
    println!("REST Search returned {} hits: {:?}", hits.len(), hits);

    // gRPC search
    let grpc_search = grpc_exec
        .execute(
            &format!("QUERY 'vector search engine' FROM {collection_name} USING dense LIMIT 5;"),
            OnError::Stop,
        )
        .await
        .unwrap();
    assert!(grpc_search.ok, "gRPC search failed: {:?}", grpc_search);
    assert!(
        grpc_search.results[0].data.is_some(),
        "gRPC Search data should be present"
    );

    let grpc_hits = grpc_search.results[0]
        .data
        .as_ref()
        .unwrap()
        .as_array()
        .unwrap();
    assert!(!grpc_hits.is_empty(), "gRPC should return search hits");
    println!(
        "gRPC Search returned {} hits: {:?}",
        grpc_hits.len(),
        grpc_hits
    );

    // Points lookup
    let points_res = grpc_exec
        .execute(
            &format!("QUERY POINTS (1, 2) FROM {collection_name} WITH PAYLOAD true;"),
            OnError::Stop,
        )
        .await
        .unwrap();
    assert!(points_res.ok, "POINTS lookup failed: {:?}", points_res);
    // B-3 regression: GetPoints must report the retrieved hits, not 0.
    assert_eq!(points_res.results[0].message, "Found 2 hits");
    let points_hits = points_res.results[0]
        .data
        .as_ref()
        .and_then(|d| d.as_array())
        .expect("POINTS data should be an array");
    assert_eq!(points_hits.len(), 2, "POINTS lookup should return 2 hits");

    // Scroll
    let scroll_res = rest_exec
        .execute(
            &format!("SCROLL FROM {collection_name} LIMIT 10;"),
            OnError::Stop,
        )
        .await
        .unwrap();
    assert!(scroll_res.ok, "SCROLL failed");

    // Update payload
    let update_res = rest_exec
        .execute(
            &format!("UPDATE {collection_name} SET PAYLOAD = {{status: 'active'}} WHERE id = 1;"),
            OnError::Stop,
        )
        .await
        .unwrap();
    assert!(update_res.ok, "UPDATE payload failed");

    // Delete
    let delete_res = grpc_exec
        .execute(
            &format!("DELETE FROM {collection_name} WHERE id = 3;"),
            OnError::Stop,
        )
        .await
        .unwrap();
    assert!(delete_res.ok, "DELETE failed");

    // Cleanup
    let drop_res = rest_exec
        .execute(
            &format!("DROP COLLECTION {collection_name};"),
            OnError::Stop,
        )
        .await
        .unwrap();
    assert!(drop_res.ok, "DROP COLLECTION failed");

    println!("Full E2E Live Integration Test Passed cleanly for REST, gRPC, and Ollama!");
}
