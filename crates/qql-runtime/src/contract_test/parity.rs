//! REST vs gRPC field parity for query, filter, hybrid, feedback.

use super::{openapi_or_skip, validate_ref};
use crate::grpc_route::test_api;
use crate::qdrant_grpc::qdrant;
use qql_core::parser::Parser;
use qql_plan::PlannedOperation;
use qql_plan::plan::{plan, to_rest_route};

#[test]
fn numeric_shard_key_stays_numeric_on_both_transports() {
    // The 1.7 mistargeting fix: SHARD 101 must reach the numeric
    // partition on REST (body number) and gRPC (Number selector) —
    // never the "101" keyword.
    let stmt = Parser::parse("DELETE FROM docs WHERE id = 1 SHARD 101;").unwrap();
    let op = plan(&stmt).unwrap();

    let route = to_rest_route(&op).expect("rest route");
    let body = route.body_json().unwrap();
    assert_eq!(body["shard_key"], serde_json::json!(101));
    assert!(
        route.query.iter().all(|(k, _)| k != "shard_key"),
        "routing rides the body field, not the query string: {:?}",
        route.query
    );

    let PlannedOperation::Delete {
        collection,
        request,
        ..
    } = &op
    else {
        panic!("expected Delete plan");
    };
    assert_eq!(collection, "docs");
    let selector = test_api::shard_key_selector(&request.shard_key);
    let keys = selector.unwrap().shard_keys;
    assert_eq!(keys.len(), 1);
    match keys[0].key.as_ref() {
        Some(qdrant::shard_key::Key::Number(n)) => assert_eq!(*n, 101),
        other => panic!("expected numeric shard, got {other:?}"),
    }
}

#[test]
fn rest_grpc_query_parity_timeout_consistency_shard_multi() {
    // MultiDense + timeout + consistency + shard_key
    let stmt = Parser::parse(
        "QUERY NEAREST VECTOR [[0.1, 0.2], [0.3, 0.4]] FROM docs \
             USING colbert SHARD 'acme' \
             PARAMS (timeout = 15, consistency = quorum) LIMIT 7;",
    )
    .unwrap();
    let op = plan(&stmt).unwrap();
    let PlannedOperation::Query {
        collection,
        request,
    } = &op
    else {
        panic!("expected Query plan");
    };
    assert_eq!(collection, "docs");

    // REST projection
    let route = to_rest_route(&op).expect("rest route");
    assert_eq!(route.path, "/collections/docs/points/query");
    assert!(route.query.iter().any(|(k, v)| k == "timeout" && v == "15"));
    assert!(
        route
            .query
            .iter()
            .any(|(k, v)| k == "consistency" && v == "quorum")
    );
    let body = route.body_json().unwrap();
    assert_eq!(body["limit"], 7);
    assert_eq!(body["using"], "colbert");
    assert_eq!(body["shard_key"], "acme");
    assert!(body["query"]["nearest"].is_array());
    assert!(body.get("timeout").is_none());
    assert!(body.get("consistency").is_none());

    // gRPC conversion (proto QueryPoints)
    let grpc = test_api::to_query_points(request, collection).expect("to_query_points");
    assert_eq!(grpc.collection_name, "docs");
    assert_eq!(grpc.using.as_deref(), Some("colbert"));
    assert_eq!(grpc.limit, Some(7));
    assert_eq!(grpc.timeout, Some(15));
    assert!(grpc.read_consistency.is_some());
    let rc = grpc.read_consistency.as_ref().unwrap();
    use qdrant::read_consistency::Value as RcVal;
    match rc.value.as_ref() {
        Some(RcVal::Type(t)) => {
            assert_eq!(*t, qdrant::ReadConsistencyType::Quorum as i32);
        }
        other => panic!("expected quorum type, got {other:?}"),
    }
    assert!(grpc.shard_key_selector.is_some());
    let sk = &grpc.shard_key_selector.as_ref().unwrap().shard_keys;
    assert_eq!(sk.len(), 1);
    match sk[0].key.as_ref() {
        Some(qdrant::shard_key::Key::Keyword(k)) => assert_eq!(k, "acme"),
        other => panic!("expected keyword shard, got {other:?}"),
    }

    // MultiDense variant on nearest
    let q = grpc.query.as_ref().expect("query");
    use qdrant::query::Variant as Qv;
    use qdrant::vector_input::Variant as Vi;
    match q.variant.as_ref() {
        Some(Qv::Nearest(vi)) => match vi.variant.as_ref() {
            Some(Vi::MultiDense(md)) => {
                assert_eq!(md.vectors.len(), 2);
                assert_eq!(md.vectors[0].data, vec![0.1, 0.2]);
                assert_eq!(md.vectors[1].data, vec![0.3, 0.4]);
            }
            other => panic!("expected MultiDense, got {other:?}"),
        },
        other => panic!("expected Nearest, got {other:?}"),
    }
}

#[test]
fn rest_grpc_relevance_feedback_parity() {
    let stmt = Parser::parse(
            "QUERY RELEVANCE FEEDBACK TARGET POINT 42 FEEDBACK ((POINT 43, 0.5), (POINT 44, -0.2)) STRATEGY NAIVE (a = 1.0, b = 0.5, c = 0.5) FROM docs USING dense LIMIT 10;",
        )
        .unwrap();
    let op = plan(&stmt).unwrap();
    let PlannedOperation::Query {
        collection,
        request,
    } = &op
    else {
        panic!("expected Query");
    };

    // REST projection carries the typed feedback shape.
    let body = to_rest_route(&op).expect("rest route").body_json().unwrap();
    let rf = &body["query"]["relevance_feedback"];
    assert!(
        rf.is_object(),
        "REST relevance_feedback missing: {}",
        body["query"]
    );
    assert_eq!(rf["feedback"].as_array().map(Vec::len), Some(2));
    assert_eq!(rf["strategy"]["naive"]["a"], 1.0);

    // gRPC conversion carries target + both items + strategy.
    let grpc = test_api::to_query_points(request, collection).unwrap();
    use qdrant::query::Variant as Qv;
    let Some(Qv::RelevanceFeedback(fb)) = grpc.query.as_ref().and_then(|q| q.variant.as_ref())
    else {
        panic!("expected RelevanceFeedback, got {:?}", grpc.query);
    };
    assert!(fb.target.is_some(), "feedback target must convert");
    assert_eq!(fb.feedback.len(), 2, "both feedback items must convert");
    assert!(fb.strategy.is_some(), "naive strategy must convert");
}

#[test]
fn match_except_filter_contract_matches_openapi() {
    // `MatchValue::Except` has no QQL surface syntax (there is no
    // `MATCH EXCEPT` keyword in `qql-core`), so it is covered at the
    // typed level: serialize the plan type and validate the REST shape.
    // The gRPC half is covered by
    // `grpc_exact_list_match_is_homogeneous_and_fallible`.
    let Some(openapi) = openapi_or_skip() else {
        return;
    };
    use qql_plan::{FieldCondition, FilterClause, MatchValue};
    let clause = FilterClause::Field(Box::new(FieldCondition {
        key: "tag".into(),
        r#match: Some(MatchValue::Except {
            except: vec![serde_json::json!("a"), serde_json::json!("b")],
        }),
        ..Default::default()
    }));
    let filter = serde_json::json!({
        "must": [serde_json::to_value(&clause).expect("except clause serializes")],
    });
    validate_ref(&openapi, "Filter", &filter);
}

#[test]
fn rest_grpc_hybrid_prefetch_parity() {
    let stmt = Parser::parse(
            "QUERY HYBRID TEXT 'search' MODEL 'bge' DENSE dense SPARSE sparse FUSION RRF FROM docs LIMIT 10;",
        )
        .unwrap();
    let op = plan(&stmt).unwrap();
    let PlannedOperation::Query {
        collection,
        request,
    } = &op
    else {
        panic!("expected Query");
    };

    let body = to_rest_route(&op).expect("rest route").body_json().unwrap();
    assert_eq!(body["query"]["fusion"], "rrf");
    assert_eq!(body["prefetch"].as_array().unwrap().len(), 2);
    assert_eq!(body["prefetch"][0]["using"], "dense");
    assert_eq!(body["prefetch"][1]["using"], "sparse");

    let grpc = test_api::to_query_points(request, collection).unwrap();
    assert_eq!(grpc.prefetch.len(), 2);
    assert_eq!(grpc.prefetch[0].using.as_deref(), Some("dense"));
    assert_eq!(grpc.prefetch[1].using.as_deref(), Some("sparse"));
    use qdrant::query::Variant as Qv;
    match grpc.query.as_ref().and_then(|q| q.variant.as_ref()) {
        Some(Qv::Fusion(f)) => {
            let _ = f;
        }
        other => panic!("expected Fusion query variant, got {other:?}"),
    }
}
