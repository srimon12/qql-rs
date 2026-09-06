//! gRPC converter tests (moved from `grpc_route.rs` unchanged).

use super::ddl::{hnsw_config_from_plan, quantization_config_from_plan, vector_params};
use super::filter::to_match;
use super::query::{to_query_groups, to_query_points};
use super::responses::get_points_envelope;
use crate::qdrant_grpc::qdrant;
use qql_core::parser::Parser;
use qql_plan::types::{FilterExpression, MatchValue};

#[test]
fn dense_vector_params_propagates_datatype() {
    let params = vector_params(&serde_json::json!({
        "size": 128,
        "distance": "Cosine",
        "datatype": "uint8",
    }));
    assert_eq!(params.datatype, Some(qdrant::Datatype::Uint8 as i32));

    let f16 = vector_params(&serde_json::json!({
        "size": 64,
        "distance": "Dot",
        "datatype": "float16",
    }));
    assert_eq!(f16.datatype, Some(qdrant::Datatype::Float16 as i32));

    let none = vector_params(&serde_json::json!({
        "size": 32,
        "distance": "Cosine",
    }));
    assert_eq!(none.datatype, None);
}

#[test]
fn test_grpc_route_conversion_all_statements() {
    let statements = [
        "QUERY TEXT 'search' MODEL 'test-model' FROM docs USING dense LIMIT 10;",
        "QUERY POINTS (1, 2, 'uuid-str') FROM docs WITH PAYLOAD INCLUDE ('title');",
        "SCROLL FROM docs WHERE status = 'active' LIMIT 50;",
        "UPSERT INTO docs VALUES {id: 1, text: 'hello', category: 'tech'} USING DENSE MODEL 'm';",
        "DELETE FROM docs WHERE category = 'old';",
        "UPDATE docs SET VECTOR dense = [0.1, 0.2] WHERE id = 1;",
        "UPDATE docs SET PAYLOAD = {status: 'ok'} WHERE id = 1;",
        "CREATE COLLECTION docs (dense VECTOR(384, COSINE), sparse SPARSE);",
        "ALTER COLLECTION docs WITH HNSW (m = 16);",
        "DROP COLLECTION docs;",
        "CREATE INDEX ON COLLECTION docs FOR title TYPE text;",
        "SHOW COLLECTIONS;",
        "SHOW COLLECTION docs;",
    ];

    for stmt_str in statements {
        let stmt =
            Parser::parse(stmt_str).unwrap_or_else(|e| panic!("parse failed for {stmt_str}: {e}"));
        let op = qql_plan::plan(&stmt).unwrap();
        match &op {
            qql_plan::PlannedOperation::Query {
                collection,
                request,
            } => {
                let grpc_req = to_query_points(request, collection);
                assert!(
                    grpc_req.is_ok(),
                    "to_query_points failed for {stmt_str}: {:?}",
                    grpc_req.err()
                );
            }
            qql_plan::PlannedOperation::QueryGroups {
                collection,
                request,
            } => {
                let grpc_req = to_query_groups(request, collection);
                assert!(
                    grpc_req.is_ok(),
                    "to_query_groups failed for {stmt_str}: {:?}",
                    grpc_req.err()
                );
            }
            qql_plan::PlannedOperation::GetPoints { request, .. } => {
                assert_eq!(request.ids.len(), 3);
            }
            qql_plan::PlannedOperation::Scroll { request, .. } => {
                assert!(request.filter.is_some());
            }
            qql_plan::PlannedOperation::Upsert { request, .. } => {
                assert_eq!(request.points.len(), 1);
            }
            qql_plan::PlannedOperation::Delete { request, .. } => {
                assert!(request.filter.is_some());
            }
            qql_plan::PlannedOperation::UpdateVectors { .. } => {}
            qql_plan::PlannedOperation::UpdatePayload { .. } => {}
            qql_plan::PlannedOperation::CreateCollection { request, .. } => {
                assert!(request.vectors.is_some() || request.hnsw_config.is_some());
            }
            qql_plan::PlannedOperation::UpdateCollection { .. } => {}
            qql_plan::PlannedOperation::CreateIndex { request, .. } => {
                assert_eq!(request.field_name, "title");
            }
            qql_plan::PlannedOperation::ClearPayload { .. } => {}
            qql_plan::PlannedOperation::DeleteVectors { .. } => {}
            qql_plan::PlannedOperation::Count { .. } => {}
            qql_plan::PlannedOperation::CreateShardKey { .. } => {}
            qql_plan::PlannedOperation::DropShardKey { .. } => {}
            _ => {}
        }
    }
}

#[test]
fn converts_collection_quantization_and_vector_update() {
    let scalar = qql_plan::QuantizationConfig::Scalar {
        scalar: qql_plan::ScalarQuantization {
            qtype: "int8".into(),
            quantile: Some(0.95),
            always_ram: Some(true),
            memory: None,
        },
    };
    let q_proto = quantization_config_from_plan(&scalar);
    assert!(q_proto.is_some());

    let stmt = Parser::parse("UPDATE docs SET VECTOR dense = [0.1, 0.2] WHERE id = 1;").unwrap();
    let op = qql_plan::plan(&stmt).unwrap();
    if let qql_plan::PlannedOperation::UpdateVectors { request, .. } = op {
        assert_eq!(request.points.len(), 1);
    } else {
        panic!("expected UpdateVectors");
    }
}

/// Query → gRPC with limit, offset, using, score_threshold
#[test]
fn query_points_field_level_basics() {
    let stmt = Parser::parse(
        "QUERY TEXT 'search' MODEL 'test-model' FROM my_coll USING dense SCORE THRESHOLD 0.7 LIMIT 10 OFFSET 5;",
    )
    .unwrap();
    let op = qql_plan::plan(&stmt).unwrap();
    let (collection, req) = match &op {
        qql_plan::PlannedOperation::Query {
            collection,
            request,
        } => (collection, request),
        other => panic!("expected Query, got {:?}", other),
    };
    let qp = to_query_points(req, collection).unwrap();

    assert_eq!(qp.collection_name, "my_coll");
    assert_eq!(qp.limit, Some(10));
    assert_eq!(qp.offset, Some(5));
    assert_eq!(qp.using, Some("dense".into()));
    assert_eq!(qp.score_threshold, Some(0.7f32));
    // query variant should be Nearest with vector input
    let query = qp.query.expect("query should be set");
    assert!(matches!(
        query.variant,
        Some(qdrant::query::Variant::Nearest(_))
    ));
}

/// Query with WITH PAYLOAD INCLUDE + WITH VECTOR → gRPC selectors
#[test]
fn query_points_with_payload_and_vectors() {
    let stmt = Parser::parse(
        "QUERY TEXT 'x' MODEL 'test-model' FROM docs WITH PAYLOAD INCLUDE ('title', 'url') WITH VECTOR (dense) LIMIT 5;",
    )
    .unwrap();
    let op = qql_plan::plan(&stmt).unwrap();
    let (collection, req) = match &op {
        qql_plan::PlannedOperation::Query {
            collection,
            request,
        } => (collection, request),
        other => panic!("expected Query, got {:?}", other),
    };
    let qp = to_query_points(req, collection).unwrap();

    // with_payload → selector_options.Include
    let wp = qp.with_payload.expect("with_payload should be set");
    match wp.selector_options.expect("selector_options should be set") {
        qdrant::with_payload_selector::SelectorOptions::Include(inc) => {
            assert!(inc.fields.contains(&"title".to_string()));
            assert!(inc.fields.contains(&"url".to_string()));
        }
        other => panic!("expected Include, got {:?}", other),
    }
    // with_vectors → selector_options.Include
    let wv = qp.with_vectors.expect("with_vectors should be set");
    match wv.selector_options.expect("selector_options should be set") {
        qdrant::with_vectors_selector::SelectorOptions::Include(inc) => {
            assert_eq!(inc.names, vec!["dense"]);
        }
        other => panic!("expected Include, got {:?}", other),
    }
}

/// Query with SHARD KEY → gRPC shard_key_selector
#[test]
fn query_points_shard_key() {
    let stmt = Parser::parse(
        "QUERY TEXT 'x' MODEL 'test-model' FROM docs USING dense SHARD 'tenant-42' LIMIT 5;",
    )
    .unwrap();
    let op = qql_plan::plan(&stmt).unwrap();
    let (collection, req) = match &op {
        qql_plan::PlannedOperation::Query {
            collection,
            request,
        } => (collection, request),
        other => panic!("expected Query, got {:?}", other),
    };
    let qp = to_query_points(req, collection).unwrap();

    let sks = qp
        .shard_key_selector
        .expect("shard_key_selector should be set");
    assert_eq!(sks.shard_keys.len(), 1);
    match sks.shard_keys[0].key.as_ref().unwrap() {
        qdrant::shard_key::Key::Keyword(k) => assert_eq!(k, "tenant-42"),
        other => panic!("expected Keyword, got {:?}", other),
    }
}

/// Query with WHERE → gRPC filter present with must conditions
#[test]
fn query_points_filter_equality() {
    let stmt = Parser::parse(
        "QUERY TEXT 'x' MODEL 'test-model' FROM docs WHERE status = 'active' LIMIT 5;",
    )
    .unwrap();
    let op = qql_plan::plan(&stmt).unwrap();
    let (collection, req) = match &op {
        qql_plan::PlannedOperation::Query {
            collection,
            request,
        } => (collection, request),
        other => panic!("expected Query, got {:?}", other),
    };
    let qp = to_query_points(req, collection).unwrap();

    let filter = qp.filter.expect("filter should be set");
    // Single condition wraps into must
    let must = &filter.must;
    assert_eq!(must.len(), 1, "expected 1 condition in must");
    let cond = &must[0];
    // condition_one_of should be Field with key="status"
    match cond.condition_one_of.as_ref().unwrap() {
        qdrant::condition::ConditionOneOf::Field(fc) => {
            assert_eq!(fc.key, "status");
            // match → Keyword("active")
            let mv = fc.r#match.as_ref().expect("match should be set");
            match mv.match_value.as_ref().unwrap() {
                qdrant::r#match::MatchValue::Keyword(kw) => assert_eq!(kw, "active"),
                _ => panic!("expected Keyword match"),
            }
        }
        other => panic!("expected Field condition, got {:?}", other),
    }
}

/// Query with AND → gRPC filter with 2 must conditions
#[test]
fn query_points_filter_range_compound() {
    let stmt = Parser::parse(
        "QUERY TEXT 'x' MODEL 'test-model' FROM docs WHERE age >= 18 AND age < 65 LIMIT 5;",
    )
    .unwrap();
    let op = qql_plan::plan(&stmt).unwrap();
    let (collection, req) = match &op {
        qql_plan::PlannedOperation::Query {
            collection,
            request,
        } => (collection, request),
        other => panic!("expected Query, got {:?}", other),
    };
    let qp = to_query_points(req, collection).unwrap();
    let filter = qp.filter.expect("filter should be set");
    let must = &filter.must;
    assert_eq!(must.len(), 2, "expected 2 conditions for AND (>= and <)");
    // Both should be Field conditions on key "age" with Range
    for c in must {
        match c.condition_one_of.as_ref().unwrap() {
            qdrant::condition::ConditionOneOf::Field(fc) => {
                assert_eq!(fc.key, "age");
                assert!(fc.range.is_some(), "expected Range for age comparison");
            }
            _ => panic!("expected Field condition"),
        }
    }
}

/// Group-by query → gRPC QueryPointGroups with lookup
#[test]
fn query_points_group_by_with_lookup() {
    let stmt = Parser::parse(
        "QUERY TEXT 'x' MODEL 'test-model' FROM docs GROUP BY category SIZE 3 LOOKUP FROM categories LIMIT 10;",
    )
    .unwrap();
    let op = qql_plan::plan(&stmt).unwrap();
    let (collection, req) = match &op {
        qql_plan::PlannedOperation::QueryGroups {
            collection,
            request,
        } => (collection, request),
        other => panic!("expected QueryGroups, got {:?}", other),
    };
    let qg = to_query_groups(req, collection).unwrap();

    assert_eq!(qg.group_by, "category");
    assert_eq!(qg.group_size, Some(3));
    assert_eq!(qg.limit, Some(10));
    let lookup = qg.with_lookup.expect("with_lookup should be set");
    assert_eq!(lookup.collection, "categories");
}

/// CreateCollection → gRPC vectors config + HNSW validation
#[test]
fn create_collection_vectors_and_hnsw() {
    let stmt = Parser::parse(
        "CREATE COLLECTION docs (dense VECTOR(384, COSINE)) WITH HNSW (m = 32, ef_construct = 100);",
    )
    .unwrap();
    let op = qql_plan::plan(&stmt).unwrap();
    let req = match &op {
        qql_plan::PlannedOperation::CreateCollection { request, .. } => request,
        other => panic!("expected CreateCollection, got {:?}", other),
    };

    // vectors should contain dense → size 384, distance Cosine
    let vectors = req.vectors.as_ref().expect("vectors should be set");
    let dense = vectors.get("dense").expect("dense vector config missing");
    assert_eq!(dense["size"], 384);
    assert_eq!(dense["distance"], "Cosine");

    // HNSW → gRPC conversion with m + ef_construct
    let hnsw_json = req.hnsw_config.as_ref().expect("hnsw_config should be set");
    let hnsw = hnsw_config_from_plan(hnsw_json);
    assert_eq!(hnsw.m, Some(32));
    assert_eq!(hnsw.ef_construct, Some(100));
}

/// Upsert → gRPC point count + shard key
#[test]
fn upsert_points_field_level() {
    let stmt =
        Parser::parse("UPSERT INTO docs VALUES {id: 1, text: 'hello'}, {id: 2, text: 'world'};")
            .unwrap();
    let op = qql_plan::plan(&stmt).unwrap();
    let req = match &op {
        qql_plan::PlannedOperation::Upsert { request, .. } => request,
        other => panic!("expected Upsert, got {:?}", other),
    };

    assert_eq!(req.points.len(), 2);
    assert!(req.shard_key.is_none());
}

/// DELETE with compound filter → gRPC must conditions present
#[test]
fn delete_with_compound_filter() {
    let stmt =
        Parser::parse("DELETE FROM docs WHERE category = 'archived' AND priority < 3;").unwrap();
    let op = qql_plan::plan(&stmt).unwrap();
    let req = match &op {
        qql_plan::PlannedOperation::Delete { request, .. } => request,
        other => panic!("expected Delete, got {:?}", other),
    };

    let filter = req.filter.as_ref().expect("delete filter should be set");
    match filter {
        FilterExpression::Compound(fc) => {
            assert_eq!(
                fc.must.len(),
                2,
                "expected 2 conditions in compound AND filter"
            );
        }
        other => panic!(
            "expected Compound filter, got {:?}",
            std::mem::discriminant(other)
        ),
    }
}

/// Order-by query → gRPC OrderBy variant + filter
#[test]
fn query_order_by_direction() {
    let stmt =
        Parser::parse("QUERY ORDER BY created_at DESC FROM docs WHERE status = 'active' LIMIT 20;")
            .unwrap();
    let op = qql_plan::plan(&stmt).unwrap();
    let (collection, req) = match &op {
        qql_plan::PlannedOperation::Query {
            collection,
            request,
        } => (collection, request),
        other => panic!("expected Query, got {:?}", other),
    };
    let qp = to_query_points(req, collection).unwrap();

    let query = qp.query.expect("query should be set");
    match query.variant.expect("variant should be set") {
        qdrant::query::Variant::OrderBy(ob) => {
            assert_eq!(ob.key, "created_at");
            assert_eq!(ob.direction, Some(qdrant::Direction::Desc as i32));
        }
        other => panic!("expected OrderBy variant, got {:?}", other),
    }
    // Filter should be present
    assert!(qp.filter.is_some(), "filter should be set for WHERE clause");
}

/// RRF `k` + `weights` survive the gRPC conversion (regression: they were
/// silently dropped into a bare `Fusion(1)`).
#[test]
fn grpc_rrf_params_are_transmitted() {
    let stmt = Parser::parse(
        "QUERY HYBRID TEXT 'search' MODEL 'bge' DENSE dense SPARSE sparse FUSION RRF \
         FROM docs PARAMS (rrf_k = 5, rrf_weights = [1.0, 0.5]) LIMIT 10;",
    )
    .unwrap();
    let op = qql_plan::plan(&stmt).unwrap();
    let (collection, req) = match &op {
        qql_plan::PlannedOperation::Query {
            collection,
            request,
        } => (collection, request),
        other => panic!("expected Query, got {:?}", other),
    };
    let qp = to_query_points(req, collection).unwrap();
    use qdrant::query::Variant as Qv;
    match qp.query.as_ref().and_then(|q| q.variant.as_ref()) {
        Some(Qv::Rrf(rrf)) => {
            assert_eq!(rrf.k, Some(5), "rrf k must be transmitted");
            assert_eq!(rrf.weights, vec![1.0, 0.5]);
        }
        other => panic!("expected Rrf variant, got {other:?}"),
    }
}

/// RRF values that the pinned proto cannot represent exactly must produce
/// structured errors, never silent loss.
#[test]
fn grpc_rrf_unrepresentable_values_error() {
    // k beyond uint32 range.
    let stmt = Parser::parse(
        "QUERY HYBRID TEXT 'search' MODEL 'bge' DENSE dense SPARSE sparse FUSION RRF \
         FROM docs PARAMS (rrf_k = 4294967296) LIMIT 10;",
    )
    .unwrap();
    let op = qql_plan::plan(&stmt).unwrap();
    let (collection, req) = match &op {
        qql_plan::PlannedOperation::Query {
            collection,
            request,
        } => (collection, request),
        other => panic!("expected Query, got {:?}", other),
    };
    let err = to_query_points(req, collection).unwrap_err();
    assert_eq!(err.kind, qql_core::error::ErrorKind::Validation);
    assert_eq!(err.code, "QQL-GRPC-RRF-K");
    assert!(err.message.contains("rrf_k"));

    // Weight that does not round-trip f64 → f32 exactly (0.1 in f32 is
    // 0.10000000149011612…).
    let stmt = Parser::parse(
        "QUERY HYBRID TEXT 'search' MODEL 'bge' DENSE dense SPARSE sparse FUSION RRF \
         FROM docs PARAMS (rrf_weights = [1.0, 0.1]) LIMIT 10;",
    )
    .unwrap();
    let op = qql_plan::plan(&stmt).unwrap();
    let (collection, req) = match &op {
        qql_plan::PlannedOperation::Query {
            collection,
            request,
        } => (collection, request),
        other => panic!("expected Query, got {:?}", other),
    };
    let err = to_query_points(req, collection).unwrap_err();
    assert_eq!(err.code, "QQL-GRPC-RRF-WEIGHT");
    assert!(err.message.contains("0.1"));
}

/// Fusion methods must map to the correct proto enum values (RRF=0,
/// DBSF=1); previously "rrf" was mapped to 1 (DBSF) and "dbsf" to an
/// undefined 2.
#[test]
fn grpc_fusion_method_maps_to_correct_enum() {
    use qdrant::query::Variant as Qv;

    let stmt = Parser::parse(
        "QUERY HYBRID TEXT 'search' MODEL 'bge' DENSE dense SPARSE sparse FUSION RRF FROM docs LIMIT 10;",
    )
    .unwrap();
    let op = qql_plan::plan(&stmt).unwrap();
    let (collection, req) = match &op {
        qql_plan::PlannedOperation::Query {
            collection,
            request,
        } => (collection, request),
        other => panic!("expected Query, got {:?}", other),
    };
    let qp = to_query_points(req, collection).unwrap();
    match qp.query.as_ref().and_then(|q| q.variant.as_ref()) {
        Some(Qv::Fusion(f)) => assert_eq!(*f, qdrant::Fusion::Rrf as i32),
        other => panic!("expected Fusion(Rrf), got {other:?}"),
    }

    let stmt = Parser::parse(
        "QUERY HYBRID TEXT 'search' MODEL 'bge' DENSE dense SPARSE sparse FUSION DBSF FROM docs LIMIT 10;",
    )
    .unwrap();
    let op = qql_plan::plan(&stmt).unwrap();
    let (collection, req) = match &op {
        qql_plan::PlannedOperation::Query {
            collection,
            request,
        } => (collection, request),
        other => panic!("expected Query, got {:?}", other),
    };
    let qp = to_query_points(req, collection).unwrap();
    match qp.query.as_ref().and_then(|q| q.variant.as_ref()) {
        Some(Qv::Fusion(f)) => assert_eq!(*f, qdrant::Fusion::Dbsf as i32),
        other => panic!("expected Fusion(Dbsf), got {other:?}"),
    }
}

/// Float equality filters must not silently lower to an empty `Match`:
/// integral floats map to an integer match, non-integral floats produce a
/// structured error (the pinned proto's Match has no double field).
#[test]
fn grpc_float_equality_filter_is_explicit() {
    // Non-integral float → structured error.
    let stmt =
        Parser::parse("QUERY TEXT 'x' MODEL 'test-model' FROM docs WHERE rating = 1.5 LIMIT 5;")
            .unwrap();
    let op = qql_plan::plan(&stmt).unwrap();
    let (collection, req) = match &op {
        qql_plan::PlannedOperation::Query {
            collection,
            request,
        } => (collection, request),
        other => panic!("expected Query, got {:?}", other),
    };
    let err = to_query_points(req, collection).unwrap_err();
    assert_eq!(err.kind, qql_core::error::ErrorKind::Validation);
    assert_eq!(err.code, "QQL-GRPC-FLOAT-MATCH");
    assert!(err.message.contains("1.5"));

    // Integral float → integer match (numerically equivalent in Qdrant).
    let stmt =
        Parser::parse("QUERY TEXT 'x' MODEL 'test-model' FROM docs WHERE rating = 2.0 LIMIT 5;")
            .unwrap();
    let op = qql_plan::plan(&stmt).unwrap();
    let (collection, req) = match &op {
        qql_plan::PlannedOperation::Query {
            collection,
            request,
        } => (collection, request),
        other => panic!("expected Query, got {:?}", other),
    };
    let qp = to_query_points(req, collection).unwrap();
    let filter = qp.filter.expect("filter should be set");
    let mv = filter.must[0]
        .condition_one_of
        .as_ref()
        .and_then(|c| match c {
            qdrant::condition::ConditionOneOf::Field(f) => f.r#match.as_ref(),
            _ => None,
        })
        .expect("field match");
    match mv.match_value.as_ref().unwrap() {
        qdrant::r#match::MatchValue::Integer(n) => assert_eq!(*n, 2),
        other => panic!("expected Integer(2), got {other:?}"),
    }
}

/// GetPoints must wrap hits in `result.points` so hit extraction sees
/// them (B-3 regression); a bare `result` array would report 0 hits.
#[test]
fn grpc_get_points_envelope_keeps_hits_extractable() {
    use crate::executor::dml::query::extract_search_hits;

    let envelope = get_points_envelope(
        vec![
            serde_json::json!({"id": 1, "payload": {"title": "a"}}),
            serde_json::json!({"id": 2, "payload": {"title": "b"}}),
        ],
        0.0,
    );
    assert_eq!(envelope["result"]["points"].as_array().unwrap().len(), 2);
    let hits = extract_search_hits(&envelope);
    assert_eq!(hits.len(), 2, "GetPoints hits must survive hit extraction");
    assert_eq!(hits[0].id, qql_plan::PlanPointId::Number(1));
    assert_eq!(hits[1].id, qql_plan::PlanPointId::Number(2));

    // The empty case still yields zero hits, not an error.
    let envelope = get_points_envelope(Vec::new(), 0.0);
    assert_eq!(extract_search_hits(&envelope).len(), 0);
}

/// IN/NOT IN lists must be homogeneous and int64-representable: valid
/// string/integer lists survive, while mixed lists, non-integral floats,
/// and `u64` values above `i64::MAX` produce structured errors instead of
/// being silently dropped or wrapped into negative integers.
#[test]
fn grpc_exact_list_match_is_homogeneous_and_fallible() {
    // Homogeneous string list → keywords.
    let mv = to_match(&MatchValue::Any {
        any: vec![serde_json::json!("a"), serde_json::json!("b")],
    })
    .unwrap();
    match mv.match_value.unwrap() {
        qdrant::r#match::MatchValue::Keywords(k) => {
            assert_eq!(k.strings, vec!["a", "b"]);
        }
        other => panic!("expected Keywords, got {other:?}"),
    }

    // Homogeneous string list via Except → except_keywords.
    let mv = to_match(&MatchValue::Except {
        except: vec![serde_json::json!("a"), serde_json::json!("b")],
    })
    .unwrap();
    match mv.match_value.unwrap() {
        qdrant::r#match::MatchValue::ExceptKeywords(k) => {
            assert_eq!(k.strings, vec!["a", "b"]);
        }
        other => panic!("expected ExceptKeywords, got {other:?}"),
    }

    // Homogeneous integer list (positive and negative) → integers.
    let mv = to_match(&MatchValue::Any {
        any: vec![
            serde_json::json!(1),
            serde_json::json!(-2),
            serde_json::json!(3),
        ],
    })
    .unwrap();
    match mv.match_value.unwrap() {
        qdrant::r#match::MatchValue::Integers(k) => {
            assert_eq!(k.integers, vec![1, -2, 3]);
        }
        other => panic!("expected Integers, got {other:?}"),
    }

    // Homogeneous integer list via Except → except_integers.
    let mv = to_match(&MatchValue::Except {
        except: vec![serde_json::json!(10), serde_json::json!(20)],
    })
    .unwrap();
    match mv.match_value.unwrap() {
        qdrant::r#match::MatchValue::ExceptIntegers(k) => {
            assert_eq!(k.integers, vec![10, 20]);
        }
        other => panic!("expected ExceptIntegers, got {other:?}"),
    }

    // Integral floats map to integer matches, mirroring single-value
    // `WHERE x = 2.0` → Integer(2).
    let mv = to_match(&MatchValue::Any {
        any: vec![serde_json::json!(2.0), serde_json::json!(3.0)],
    })
    .unwrap();
    match mv.match_value.unwrap() {
        qdrant::r#match::MatchValue::Integers(k) => {
            assert_eq!(k.integers, vec![2, 3]);
        }
        other => panic!("expected Integers, got {other:?}"),
    }
}

#[test]
fn grpc_exact_list_match_rejects_unrepresentable() {
    // Mixed string + integer list → structured error, not a silent drop
    // of the integer entry.
    let err = to_match(&MatchValue::Any {
        any: vec![serde_json::json!("a"), serde_json::json!(1)],
    })
    .unwrap_err();
    assert_eq!(err.kind, qql_core::error::ErrorKind::Validation);
    assert_eq!(err.code, "QQL-GRPC-LIST-TYPE");
    assert!(err.message.contains("mixes strings"), "{err}");

    // Non-integral floats have no list encoding → structured error.
    let err = to_match(&MatchValue::Any {
        any: vec![serde_json::json!(1.5), serde_json::json!(2.5)],
    })
    .unwrap_err();
    assert_eq!(err.kind, qql_core::error::ErrorKind::Validation);
    assert_eq!(err.code, "QQL-GRPC-LIST-TYPE");
    assert!(err.message.contains("1.5"), "{err}");

    // A float mixed into a string list is caught as heterogeneous.
    let err = to_match(&MatchValue::Except {
        except: vec![serde_json::json!("a"), serde_json::json!(1.5)],
    })
    .unwrap_err();
    assert_eq!(err.code, "QQL-GRPC-LIST-TYPE");
    assert!(err.message.contains("mixes strings"), "{err}");

    // Booleans are unrepresentable in either list form.
    let err = to_match(&MatchValue::Any {
        any: vec![serde_json::json!(true), serde_json::json!(false)],
    })
    .unwrap_err();
    assert_eq!(err.code, "QQL-GRPC-LIST-TYPE");
    assert!(err.message.contains("true"), "{err}");

    // u64 above i64::MAX must error, never wrap into a negative integer.
    let oversized = i64::MAX as u64 + 1;
    let err = to_match(&MatchValue::Any {
        any: vec![serde_json::json!(oversized)],
    })
    .unwrap_err();
    assert_eq!(err.kind, qql_core::error::ErrorKind::Validation);
    assert_eq!(err.code, "QQL-GRPC-LIST-INT");
    assert!(err.message.contains("9223372036854775808"), "{err}");
}

/// Errors from list conversion must propagate through the full filter
/// path (`to_filter` → `to_condition` → `to_match`).
#[test]
fn grpc_in_list_errors_propagate_through_filter() {
    let stmt = Parser::parse(
        "QUERY TEXT 'x' MODEL 'test-model' FROM docs WHERE status IN ('a', 1) LIMIT 5;",
    )
    .unwrap();
    let op = qql_plan::plan(&stmt).unwrap();
    let (collection, req) = match &op {
        qql_plan::PlannedOperation::Query {
            collection,
            request,
        } => (collection, request),
        other => panic!("expected Query, got {other:?}"),
    };
    let err = to_query_points(req, collection).unwrap_err();
    assert_eq!(err.kind, qql_core::error::ErrorKind::Validation);
    assert_eq!(err.code, "QQL-GRPC-LIST-TYPE");

    // Non-integral floats in IN also propagate.
    let stmt = Parser::parse(
        "QUERY TEXT 'x' MODEL 'test-model' FROM docs WHERE rating IN (1.5, 2.5) LIMIT 5;",
    )
    .unwrap();
    let op = qql_plan::plan(&stmt).unwrap();
    let (collection, req) = match &op {
        qql_plan::PlannedOperation::Query {
            collection,
            request,
        } => (collection, request),
        other => panic!("expected Query, got {other:?}"),
    };
    let err = to_query_points(req, collection).unwrap_err();
    assert_eq!(err.code, "QQL-GRPC-LIST-TYPE");
    assert!(err.message.contains("1.5"), "{err}");
}

/// Valid homogeneous lists survive the full parser → plan → gRPC path.
#[test]
fn grpc_in_list_homogeneous_lists_survive_full_path() {
    // String list → keywords.
    let stmt = Parser::parse(
        "QUERY TEXT 'x' MODEL 'test-model' FROM docs WHERE status IN ('a', 'b') LIMIT 5;",
    )
    .unwrap();
    let op = qql_plan::plan(&stmt).unwrap();
    let (collection, req) = match &op {
        qql_plan::PlannedOperation::Query {
            collection,
            request,
        } => (collection, request),
        other => panic!("expected Query, got {other:?}"),
    };
    let qp = to_query_points(req, collection).unwrap();
    let filter = qp.filter.expect("filter should be set");
    let mv = filter.must[0]
        .condition_one_of
        .as_ref()
        .and_then(|c| match c {
            qdrant::condition::ConditionOneOf::Field(f) => f.r#match.as_ref(),
            _ => None,
        })
        .expect("field match");
    match mv.match_value.as_ref().unwrap() {
        qdrant::r#match::MatchValue::Keywords(k) => {
            assert_eq!(k.strings, vec!["a", "b"]);
        }
        other => panic!("expected Keywords, got {other:?}"),
    }

    // Integer list → integers.
    let stmt = Parser::parse(
        "QUERY TEXT 'x' MODEL 'test-model' FROM docs WHERE year IN (2024, 2025) LIMIT 5;",
    )
    .unwrap();
    let op = qql_plan::plan(&stmt).unwrap();
    let (collection, req) = match &op {
        qql_plan::PlannedOperation::Query {
            collection,
            request,
        } => (collection, request),
        other => panic!("expected Query, got {other:?}"),
    };
    let qp = to_query_points(req, collection).unwrap();
    let filter = qp.filter.expect("filter should be set");
    let mv = filter.must[0]
        .condition_one_of
        .as_ref()
        .and_then(|c| match c {
            qdrant::condition::ConditionOneOf::Field(f) => f.r#match.as_ref(),
            _ => None,
        })
        .expect("field match");
    match mv.match_value.as_ref().unwrap() {
        qdrant::r#match::MatchValue::Integers(k) => {
            assert_eq!(k.integers, vec![2024, 2025]);
        }
        other => panic!("expected Integers, got {other:?}"),
    }

    // NOT IN lowers to a `must_not` condition that still carries the
    // keyword list.
    let stmt = Parser::parse(
        "QUERY TEXT 'x' MODEL 'test-model' FROM docs WHERE status NOT IN ('deleted', 'archived') LIMIT 5;",
    )
    .unwrap();
    let op = qql_plan::plan(&stmt).unwrap();
    let (collection, req) = match &op {
        qql_plan::PlannedOperation::Query {
            collection,
            request,
        } => (collection, request),
        other => panic!("expected Query, got {other:?}"),
    };
    let qp = to_query_points(req, collection).unwrap();
    let filter = qp.filter.expect("filter should be set");
    assert_eq!(filter.must_not.len(), 1, "NOT IN should lower to must_not");
    let mv = filter.must_not[0]
        .condition_one_of
        .as_ref()
        .and_then(|c| match c {
            qdrant::condition::ConditionOneOf::Field(f) => f.r#match.as_ref(),
            _ => None,
        })
        .expect("field match");
    match mv.match_value.as_ref().unwrap() {
        qdrant::r#match::MatchValue::Keywords(k) => {
            assert_eq!(k.strings, vec!["deleted", "archived"]);
        }
        other => panic!("expected Keywords, got {other:?}"),
    }
}
