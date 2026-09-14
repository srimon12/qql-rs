//! Multi-dense, cross-rerank, facet, and batch-error shapes.

use super::{openapi_or_skip, validate_ref};
use crate::grpc_route::test_api;
use crate::qdrant_grpc::qdrant;
use qql_core::parser::Parser;
use qql_plan::PlannedOperation;
use qql_plan::plan::plan;
use qql_plan::routing::try_route;
use qql_plan::types::{PlanQueryInput, PlanVectorValue};

#[test]
fn multi_dense_plan_vector_matches_rest_and_grpc_shape() {
    let multi = PlanVectorValue::MultiDense(vec![vec![1.0, 2.0], vec![3.0, 4.0]]);
    let rest = serde_json::to_value(&multi).unwrap();
    assert_eq!(
        rest,
        serde_json::json!([[1.0, 2.0], [3.0, 4.0]]),
        "REST multi is array-of-arrays"
    );

    let input = PlanQueryInput::Vector(multi);
    let vi = test_api::to_vector_input(&input).expect("dense multi converts");
    use qdrant::vector_input::Variant as Vi;
    match vi.variant {
        Some(Vi::MultiDense(md)) => {
            assert_eq!(md.vectors.len(), 2);
            assert_eq!(md.vectors[0].data, vec![1.0, 2.0]);
        }
        other => panic!("gRPC multi must be MultiDense, got {other:?}"),
    }
}

#[test]
fn cross_rerank_is_not_a_qdrant_query_variant() {
    let stmt = Parser::parse(
        "WITH c AS (QUERY VECTOR [0.1, 0.2] FROM docs USING dense LIMIT 20) \
             QUERY CROSS RERANK TEXT 'q' MODEL 'm' FROM docs PREFETCH (c) LIMIT 5;",
    )
    .unwrap();
    let op = plan(&stmt).unwrap();
    assert!(matches!(op, PlannedOperation::CrossRerank { .. }));
    if let PlannedOperation::Query { request, .. } = &op {
        panic!("must not be Query: {:?}", request.query);
    }
}

#[test]
fn facet_contract_matches_openapi_and_grpc() {
    let Some(openapi) = openapi_or_skip() else {
        return;
    };

    let stmt = Parser::parse(
        "FACET room_type FROM stays WHERE price < 150 LIMIT 5 EXACT true SHARD 'tenant_1';",
    )
    .unwrap();

    // 1. REST route body matches OpenAPI FacetRequest schema
    let route = try_route(&stmt).unwrap();
    assert_eq!(route.path, "/collections/stays/facet");
    let body = route.body_json().unwrap();
    validate_ref(&openapi, "FacetRequest", &body);

    assert_eq!(body["key"], "room_type");
    assert_eq!(body["limit"], 5);
    assert_eq!(body["exact"], true);
    assert_eq!(body["shard_key"], "tenant_1");
    assert!(body["filter"].is_object());

    // 2. gRPC conversion matches the planned operation
    let op = plan(&stmt).unwrap();
    let (collection, req) = match &op {
        PlannedOperation::Facet {
            collection,
            request,
        } => (collection, request),
        other => panic!("expected Facet operation, got {other:?}"),
    };

    let fc = test_api::to_facet_counts(req, collection).unwrap();
    assert_eq!(fc.collection_name, "stays");
    assert_eq!(fc.key, "room_type");
    assert_eq!(fc.limit, Some(5));
    assert_eq!(fc.exact, Some(true));
    assert!(fc.filter.is_some());
    assert!(fc.shard_key_selector.is_some());

    // 3. Response conversion: gRPC FacetHit normalizes to the typed value,
    // which serializes as the OpenAPI `FacetValueHit` shape.
    let hit_str = qdrant::FacetHit {
        value: Some(qdrant::FacetValue {
            variant: Some(qdrant::facet_value::Variant::StringValue(
                "entire_home".into(),
            )),
        }),
        count: 42,
    };
    let normalized =
        serde_json::to_value(test_api::facet_hit_to_typed(hit_str).expect("facet converts"))
            .expect("facet serializes");
    validate_ref(&openapi, "FacetValueHit", &normalized);
    assert_eq!(normalized["value"], "entire_home");
    assert_eq!(normalized["count"], 42);
}

#[test]
fn test_batch_item_error_shape() {
    use qql_plan::batch_item_error;
    use serde_json::json;

    // 1. Root error property with status "error"
    let err_item = json!({
        "status": "error",
        "error": "point 42 not found"
    });
    assert_eq!(
        batch_item_error(&err_item),
        Some("point 42 not found".to_string())
    );

    // 2. Fallback when error field is missing but status is "error"
    let fallback_err = json!({
        "status": "error"
    });
    assert_eq!(
        batch_item_error(&fallback_err),
        Some("batch item failed".to_string())
    );

    // 3. Successful or completed items return None
    let completed = json!({
        "status": "completed",
        "result": { "operation_id": 1 }
    });
    assert_eq!(batch_item_error(&completed), None);

    let acknowledged = json!({
        "status": "acknowledged",
        "operation_id": 10
    });
    assert_eq!(batch_item_error(&acknowledged), None);

    let normal_hit = json!({
        "id": 1,
        "score": 0.95
    });
    assert_eq!(batch_item_error(&normal_hit), None);
}
