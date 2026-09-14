//! Document/image embedding plan + OpenAPI shapes.

use super::{openapi_or_skip, validate_ref};
use qql_core::parser::Parser;
use qql_plan::plan::plan;
use qql_plan::routing::try_route;

#[test]
fn document_with_model_is_openapi_valid() {
    let Some(openapi) = openapi_or_skip() else {
        return;
    };
    let stmt = Parser::parse(
        "QUERY TEXT 'hello world' MODEL 'jinaai/jina-embeddings-v2-base-en' FROM docs LIMIT 5;",
    )
    .unwrap();
    let json = try_route(&stmt).unwrap().body_json().unwrap();
    let nearest = &json["query"]["nearest"];
    assert!(nearest.is_object(), "nearest must be an object: {nearest}");
    assert_eq!(nearest["text"], "hello world");
    assert_eq!(nearest["model"], "jinaai/jina-embeddings-v2-base-en");
    validate_ref(&openapi, "Document", nearest);
    validate_ref(&openapi, "Query", &json["query"]);
}

/// Image WITH model serializes as a valid OpenAPI Image object
/// `{"image": ..., "model": ...}` and passes the OpenAPI VectorInput schema.
#[test]
fn image_with_model_is_openapi_valid() {
    let Some(openapi) = openapi_or_skip() else {
        return;
    };
    let stmt = Parser::parse(
            "QUERY IMAGE 'https://example.com/photo.jpg' MODEL 'Qdrant/clip-ViT-B-32-vision' FROM docs USING image_vec LIMIT 5;",
        )
        .unwrap();
    let json = try_route(&stmt).unwrap().body_json().unwrap();
    let nearest = &json["query"]["nearest"];
    assert!(nearest.is_object(), "nearest must be an object: {nearest}");
    assert_eq!(nearest["image"], "https://example.com/photo.jpg");
    assert_eq!(nearest["model"], "Qdrant/clip-ViT-B-32-vision");
    // Validate against both Image and VectorInput schemas.
    validate_ref(&openapi, "Image", nearest);
    validate_ref(&openapi, "Query", &json["query"]);
}

/// Document WITHOUT model must fail planning with a clear validation error.
#[test]
fn document_without_model_plans_successfully() {
    // Plan layer is transport-agnostic — MODEL is filled by the executor.
    let result = plan(&Parser::parse("QUERY 'hello' FROM docs USING dense LIMIT 5;").unwrap());
    assert!(
        result.is_ok(),
        "plan should succeed without MODEL: {}",
        result.unwrap_err()
    );
}

/// Image WITHOUT model now plans successfully — MODEL resolution
/// happens at the executor layer, not in the plan IR.
#[test]
fn image_without_model_plans_successfully() {
    let result = plan(
        &Parser::parse(
            "QUERY IMAGE 'https://example.com/photo.jpg' FROM docs USING image_vec LIMIT 5;",
        )
        .unwrap(),
    );
    assert!(
        result.is_ok(),
        "plan should succeed without MODEL: {}",
        result.unwrap_err()
    );
}

/// Document with explicit MODEL '' (empty) plans successfully —
/// the plan IR preserves the value; the executor validates it.
#[test]
fn document_with_empty_model_plans_successfully() {
    let result =
        plan(&Parser::parse("QUERY TEXT 'hello' MODEL '' FROM docs USING dense LIMIT 5;").unwrap());
    assert!(result.is_ok());
}

/// HYBRID requires MODEL so both dense and sparse prefetches get a valid
/// Document object with the model field populated (no bare-string leakage).
#[test]
fn hybrid_with_model_propagates_to_both_prefetches() {
    let Some(openapi) = openapi_or_skip() else {
        return;
    };
    let stmt = Parser::parse(
            "QUERY HYBRID TEXT 'search' MODEL 'bge' DENSE dense SPARSE sparse FUSION RRF FROM docs LIMIT 10;",
        )
        .unwrap();
    let json = try_route(&stmt).unwrap().body_json().unwrap();
    let prefetch = json["prefetch"].as_array().unwrap();
    assert_eq!(prefetch.len(), 2);
    // Both prefetches must be Document objects (not bare strings).
    for (i, pf) in prefetch.iter().enumerate() {
        let nearest = &pf["query"]["nearest"];
        assert!(
            nearest.is_object(),
            "prefetch[{i}] nearest must be Document object, got {nearest}"
        );
        assert_eq!(nearest["model"], "bge");
        validate_ref(&openapi, "Document", nearest);
    }
    validate_ref(&openapi, "QueryRequest", &json);
}
