use std::sync::Arc;

use qql::embedder::HttpEmbedder;
use qql::executor::Executor;

#[tokio::test]
async fn test_live_rest_and_grpc_with_ollama_embeddings() {
    let rest_url = "http://localhost:6333";
    let grpc_url = "http://localhost:6334";
    let ollama_url = "http://localhost:11434/v1/embeddings";
    let model_name = "all-minilm:l6-v2";

    // 1. Create HttpEmbedder backed by local Ollama
    let embedder = Arc::new(
        HttpEmbedder::new(
            ollama_url.to_string(),
            "".to_string(), // no API key for local Ollama
            model_name.to_string(),
            384, // all-minilm:l6-v2 dimension
        )
        .expect("HttpEmbedder creation failed"),
    );

    // 2. Create REST and gRPC executors with Ollama embedder attached
    let rest_ops = Box::new(qql::rest::RestQdrant::new(rest_url, None));
    let rest_exec = Executor::with_embedder(rest_ops, None, Some(embedder.clone()));

    let grpc_ops = Box::new(qql::grpc::GrpcQdrant::from_url(grpc_url, None).unwrap());
    let grpc_exec = Executor::with_embedder(grpc_ops, None, Some(embedder.clone()));

    let collection_name = "live_integration_docs";

    // 3. Clean up old collection if present
    if rest_exec.ops().collection_exists(collection_name).await.unwrap_or(false) {
        let _ = rest_exec
            .execute(&format!("DROP COLLECTION {collection_name};"))
            .await;
    }

    // 4. Create collection over REST
    let create_res = rest_exec
        .execute(&format!(
            "CREATE COLLECTION {collection_name} (dense VECTOR(384, COSINE));"
        ))
        .await
        .unwrap();
    assert!(create_res.ok, "CREATE COLLECTION failed: {:?}", create_res);

    // Verify collection exists via gRPC
    let exists = grpc_exec.ops().collection_exists(collection_name).await.unwrap();
    assert!(exists, "Collection should exist over gRPC");

    // 5. Upsert documents with text embedding resolution over REST
    let upsert_res = rest_exec
        .execute(&format!(
            "UPSERT INTO {collection_name} VALUES \
             {{id: 1, text: 'Qdrant is a high performance vector database'}}, \
             {{id: 2, text: 'Ollama enables running AI models locally'}}, \
             {{id: 3, text: 'Rust provides memory safety and zero-cost abstractions'}} \
             USING DENSE MODEL '{model_name}';"
        ))
        .await
        .unwrap();
    assert!(upsert_res.ok, "UPSERT failed: {:?}", upsert_res);

    // 6. Execute text search query over REST (resolves text -> vector via Ollama)
    let rest_search = rest_exec
        .execute(&format!(
            "QUERY '{model_name}' FROM {collection_name} USING dense LIMIT 5;"
        ))
        .await
        .unwrap();
    assert!(rest_search.ok, "REST search failed: {:?}", rest_search);
    assert!(rest_search.data.is_some(), "Search data should be present");

    let hits = rest_search.data.as_ref().unwrap().as_array().unwrap();
    assert!(!hits.is_empty(), "Should return search hits");
    println!("REST Search returned {} hits: {:?}", hits.len(), hits);

    // 7. Execute text search query over gRPC (resolves text -> vector via Ollama)
    let grpc_search = grpc_exec
        .execute(&format!(
            "QUERY 'vector search engine' FROM {collection_name} USING dense LIMIT 5;"
        ))
        .await
        .unwrap();
    assert!(grpc_search.ok, "gRPC search failed: {:?}", grpc_search);
    assert!(grpc_search.data.is_some(), "gRPC Search data should be present");

    let grpc_hits = grpc_search.data.as_ref().unwrap().as_array().unwrap();
    assert!(!grpc_hits.is_empty(), "gRPC should return search hits");
    println!("gRPC Search returned {} hits: {:?}", grpc_hits.len(), grpc_hits);

    // 8. Points lookup over gRPC
    let points_res = grpc_exec
        .execute(&format!(
            "QUERY POINTS (1, 2) FROM {collection_name} WITH PAYLOAD true;"
        ))
        .await
        .unwrap();
    assert!(points_res.ok, "POINTS lookup failed");

    // 9. Scroll points over REST
    let scroll_res = rest_exec
        .execute(&format!("SCROLL FROM {collection_name} LIMIT 10;"))
        .await
        .unwrap();
    assert!(scroll_res.ok, "SCROLL failed");

    // 10. Update payload over REST
    let update_res = rest_exec
        .execute(&format!(
            "UPDATE {collection_name} SET PAYLOAD = {{status: 'active'}} WHERE id = 1;"
        ))
        .await
        .unwrap();
    assert!(update_res.ok, "UPDATE payload failed");

    // 11. Delete point over gRPC
    let delete_res = grpc_exec
        .execute(&format!("DELETE FROM {collection_name} WHERE id = 3;"))
        .await
        .unwrap();
    assert!(delete_res.ok, "DELETE failed");

    // 12. Final cleanup
    let drop_res = rest_exec
        .execute(&format!("DROP COLLECTION {collection_name};"))
        .await
        .unwrap();
    assert!(drop_res.ok, "DROP COLLECTION failed");

    println!("Full E2E Live Integration Test Passed cleanly for REST, gRPC, and Ollama!");
}
