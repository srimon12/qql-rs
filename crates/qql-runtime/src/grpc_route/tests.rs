//! gRPC converter tests (moved from `grpc_route.rs` unchanged).

use super::ddl::{
    hnsw_config_from_plan, quantization_config_from_plan, sparse_vectors_config_diff,
    vector_params, vectors_config_diff,
};
use super::filter::to_match;
use super::query::{
    plan_vector_to_proto, to_facet_counts, to_query_groups, to_query_points, to_scroll_points,
    to_vector_input, to_vectors,
};
use super::typed::{
    collection_mutation_to_typed, facet_hit_to_typed, mutation_response_to_typed,
    point_group_to_typed, retrieved_point_to_hit, scored_point_to_hit, telemetry_from_proto,
    usage_to_json, usage_to_telemetry,
};
use crate::executor::{ExecData, ServerUsage};
use crate::qdrant_grpc::qdrant;
use qql_core::ast::{VectorDatatype, VectorDistance};
use qql_core::parser::Parser;
use qql_plan::types::{FilterExpression, MatchValue};
use qql_plan::{
    DenseVectorParams, PlanFacetValue, PlanGroupId, PlanPointVectors, PlanQueryInput,
    PlanVectorStruct, PlanVectorValue,
};

fn dense_params(datatype: Option<VectorDatatype>) -> DenseVectorParams {
    DenseVectorParams {
        size: 128,
        distance: VectorDistance::Cosine,
        hnsw_config: None,
        quantization_config: None,
        on_disk: None,
        memory: None,
        datatype,
        multivector_config: None,
    }
}

#[test]
fn dense_vector_params_propagates_datatype() {
    let params = vector_params(&dense_params(Some(VectorDatatype::Uint8)));
    assert_eq!(params.datatype, Some(qdrant::Datatype::Uint8 as i32));

    let f16 = vector_params(&dense_params(Some(VectorDatatype::Float16)));
    assert_eq!(f16.datatype, Some(qdrant::Datatype::Float16 as i32));

    let none = vector_params(&dense_params(None));
    assert_eq!(none.datatype, None);
}

/// `ALTER COLLECTION` per-vector diffs convert directly into the typed
/// `UpdateCollection.vectors_config` oneof without a JSON hop.
#[test]
fn grpc_update_collection_converts_named_vector_diffs() {
    let stmt = Parser::parse(
        "ALTER COLLECTION docs \
         WITH VECTOR dense (HNSW (m = 32, inline_storage = true), QUANTIZATION (type = 'scalar', quantile = 0.99), VECTOR (memory = 'cold')) \
         WITH VECTOR colbert (QUANTIZATION (disabled = true)) \
         WITH SPARSE bm25 (SPARSE (modifier = 'idf', full_scan_threshold = 5000, datatype = 'float16'));",
    )
    .unwrap();
    let qql_plan::PlannedOperation::UpdateCollection { request, .. } =
        qql_plan::plan(&stmt).unwrap()
    else {
        panic!("expected UpdateCollection");
    };

    let proto = vectors_config_diff(request.vectors.as_ref().expect("vector diffs"));
    let Some(qdrant::vectors_config_diff::Config::ParamsMap(map)) = proto.config else {
        panic!("named diffs must use the params_map variant");
    };
    let dense = map.map.get("dense").expect("dense entry");
    assert_eq!(dense.hnsw_config.as_ref().and_then(|h| h.m), Some(32));
    assert_eq!(
        dense.hnsw_config.as_ref().and_then(|h| h.inline_storage),
        Some(true),
        "inline_storage must survive the typed → proto conversion"
    );
    assert_eq!(dense.memory, Some(qdrant::Memory::Cold as i32));
    match dense
        .quantization_config
        .as_ref()
        .and_then(|q| q.quantization.as_ref())
    {
        Some(qdrant::quantization_config_diff::Quantization::Scalar(scalar)) => {
            assert_eq!(scalar.quantile, Some(0.99));
        }
        other => panic!("expected scalar quantization, got {other:?}"),
    }

    let colbert = map.map.get("colbert").expect("colbert entry");
    assert!(matches!(
        colbert
            .quantization_config
            .as_ref()
            .and_then(|q| q.quantization.as_ref()),
        Some(qdrant::quantization_config_diff::Quantization::Disabled(_))
    ));

    let sparse = sparse_vectors_config_diff(request.sparse_vectors.as_ref().expect("sparse diffs"));
    let bm25 = sparse.map.get("bm25").expect("bm25 entry");
    assert_eq!(bm25.modifier, Some(qdrant::Modifier::Idf as i32));
    let index = bm25.index.as_ref().expect("sparse index");
    assert_eq!(index.full_scan_threshold, Some(5000));
    assert_eq!(index.datatype, Some(qdrant::Datatype::Float16 as i32));
}

/// The unnamed/default-vector diff uses the bare `params` oneof variant, the
/// same addressing create uses for a single unnamed vector.
#[test]
fn grpc_update_collection_default_vector_uses_params_variant() {
    let stmt = Parser::parse("ALTER COLLECTION docs WITH VECTOR (on_disk = true);").unwrap();
    let qql_plan::PlannedOperation::UpdateCollection { request, .. } =
        qql_plan::plan(&stmt).unwrap()
    else {
        panic!("expected UpdateCollection");
    };
    let proto = vectors_config_diff(request.vectors.as_ref().expect("vector diffs"));
    let Some(qdrant::vectors_config_diff::Config::Params(params)) = proto.config else {
        panic!("default vector diff must use the bare params variant");
    };
    assert_eq!(params.on_disk, Some(true));
    assert!(params.hnsw_config.is_none());
    assert!(params.quantization_config.is_none());
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
        "UPDATE docs SET VECTOR VALUES {id: 1, vector: [0.1]}, {id: 2, vector: {dense: [0.2]}};",
        "UPDATE docs SET PAYLOAD = {status: 'ok'} WHERE id = 1;",
        "CREATE COLLECTION docs (dense VECTOR(384, COSINE), sparse SPARSE);",
        "ALTER COLLECTION docs WITH HNSW (m = 16);",
        "DROP COLLECTION docs;",
        "CREATE INDEX ON COLLECTION docs FOR title TYPE text;",
        "SHOW COLLECTIONS;",
        "SHOW COLLECTION docs;",
        "FACET category FROM docs WHERE status = 'active' LIMIT 10 EXACT true SHARD 'tenant_1';",
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
            qql_plan::PlannedOperation::Facet {
                collection,
                request,
            } => {
                let grpc_req = to_facet_counts(request, collection);
                assert!(
                    grpc_req.is_ok(),
                    "to_facet_counts failed for {stmt_str}: {:?}",
                    grpc_req.err()
                );
            }
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

    let batch = Parser::parse(
        "UPDATE docs SET VECTOR VALUES {id: 1, vector: [0.1]}, {id: 2, vector: {dense: [0.2]}};",
    )
    .unwrap();
    let op = qql_plan::plan(&batch).unwrap();
    let qql_plan::PlannedOperation::UpdateVectors { request, .. } = op else {
        panic!("expected UpdateVectors");
    };
    assert_eq!(request.points.len(), 2);
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
    let qql_plan::DenseVectorsConfig::Named(map) = vectors else {
        panic!("expected named vectors, got {vectors:?}");
    };
    let dense = map.get("dense").expect("dense vector config missing");
    assert_eq!(dense.size, 384);
    assert_eq!(dense.distance, VectorDistance::Cosine);

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

/// Float equality filters lower to `range` (gte == lte) in the plan, so the
/// gRPC path converts them exactly like REST: no `QQL-GRPC-FLOAT-MATCH`, no
/// integer coercion (`2.0` stays a float bound).
#[test]
fn grpc_float_equality_filter_is_range() {
    for (sql, expected) in [
        (
            "QUERY TEXT 'x' MODEL 'test-model' FROM docs WHERE rating = 1.5 LIMIT 5;",
            1.5,
        ),
        (
            "QUERY TEXT 'x' MODEL 'test-model' FROM docs WHERE rating = 2.0 LIMIT 5;",
            2.0,
        ),
    ] {
        let stmt = Parser::parse(sql).unwrap();
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
        let range = filter.must[0]
            .condition_one_of
            .as_ref()
            .and_then(|c| match c {
                qdrant::condition::ConditionOneOf::Field(f) => f.range.as_ref(),
                _ => None,
            })
            .expect("field range");
        assert_eq!(range.gte, Some(expected));
        assert_eq!(range.lte, Some(expected));
        assert_eq!(range.gt, None);
        assert_eq!(range.lt, None);
    }
}

/// GetPoints must keep every hit (B-3 regression): the retired JSON path
/// silently dropped them when the envelope shape was wrong, and the typed path
/// maps every `RetrievedPoint` straight into a hit.
#[test]
fn typed_get_points_keeps_every_hit() {
    let point = |id: u64, title: &str| qdrant::RetrievedPoint {
        id: Some(qdrant::PointId {
            point_id_options: Some(qdrant::point_id::PointIdOptions::Num(id)),
        }),
        payload: [(
            "title".to_string(),
            proto_value(qdrant::value::Kind::StringValue(title.to_string())),
        )]
        .into_iter()
        .collect(),
        vectors: None,
        shard_key: None,
        order_value: None,
    };

    let hits: Vec<_> = vec![point(1, "a"), point(2, "b")]
        .into_iter()
        .map(|point| retrieved_point_to_hit(point).expect("retrieved point converts"))
        .collect();
    assert_eq!(hits.len(), 2, "GetPoints hits must survive conversion");
    assert_eq!(hits[0].id, qql_plan::PlanPointId::Number(1));
    assert_eq!(hits[1].id, qql_plan::PlanPointId::Number(2));
    assert_eq!(
        hits[1].payload.as_ref().expect("payload")["title"],
        serde_json::json!("b")
    );
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

#[test]
fn scroll_limit_overflow_is_rejected_in_grpc() {
    let stmt = Parser::parse("SCROLL FROM docs LIMIT 18446744073709551615;").unwrap();
    let op = qql_plan::plan(&stmt).unwrap();
    let (collection, req) = match &op {
        qql_plan::PlannedOperation::Scroll {
            collection,
            request,
        } => (collection, request),
        other => panic!("expected Scroll, got {other:?}"),
    };
    let err = to_scroll_points(req, collection).unwrap_err();
    assert_eq!(err.code, "QQL-GRPC-SCROLL-LIMIT");
    assert!(err.message.contains("scroll limit"));
}

#[test]
fn grpc_rerank_with_cte_prefetch_preserves_structure() {
    use qdrant::query::Variant as Qv;
    use qdrant::vector_input::Variant as Vi;

    let stmt = Parser::parse(
        "WITH a AS (QUERY TEXT 'candidate text' MODEL 'e5' FROM docs USING dense LIMIT 20) \
         QUERY RERANK TEXT 'query text' MODEL 'colbert' FROM docs USING colbert PREFETCH (a) LIMIT 10;",
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

    // Verify top-level query is Nearest with colbert model and document input
    assert_eq!(qp.collection_name, "docs");
    assert_eq!(qp.using, Some("colbert".to_string()));
    assert_eq!(qp.limit, Some(10));
    match qp.query.and_then(|q| q.variant) {
        Some(Qv::Nearest(near)) => match near.variant {
            Some(Vi::Document(doc)) => {
                assert_eq!(doc.text, "query text");
                assert_eq!(doc.model, "colbert");
            }
            other => panic!("expected Document variant, got {other:?}"),
        },
        other => panic!("expected Nearest query, got {other:?}"),
    }

    // Verify prefetch preserved CTE candidate query structure
    assert_eq!(qp.prefetch.len(), 1);
    let pf = &qp.prefetch[0];
    assert_eq!(pf.limit, Some(20));
    assert_eq!(pf.using, Some("dense".to_string()));
    match pf.query.as_ref().and_then(|q| q.variant.as_ref()) {
        Some(Qv::Nearest(near)) => match near.variant.as_ref() {
            Some(Vi::Document(doc)) => {
                assert_eq!(doc.text, "candidate text");
                assert_eq!(doc.model, "e5");
            }
            other => panic!("expected Document variant in prefetch, got {other:?}"),
        },
        other => panic!("expected Nearest query in prefetch, got {other:?}"),
    }
}

#[test]
fn grpc_fusion_rrf_standalone_with_params() {
    use qdrant::query::Variant as Qv;

    // Direct fusion with rrf_k and rrf_weights
    let stmt = Parser::parse(
        "QUERY FUSION RRF FROM docs \
         PREFETCH (QUERY TEXT 'a' MODEL 'm' FROM docs USING dense LIMIT 10, \
                   QUERY TEXT 'b' MODEL 'm' FROM docs USING dense LIMIT 10) \
         PARAMS (rrf_k = 10, rrf_weights = [0.75, 0.25]) LIMIT 5;",
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
    match qp.query.and_then(|q| q.variant) {
        Some(Qv::Rrf(rrf)) => {
            assert_eq!(rrf.k, Some(10));
            assert_eq!(rrf.weights, vec![0.75, 0.25]);
        }
        other => panic!("expected Rrf variant, got {other:?}"),
    }

    // Direct fusion with default params maps to Fusion::Rrf enum
    let stmt_default = Parser::parse(
        "QUERY FUSION RRF FROM docs \
         PREFETCH (QUERY TEXT 'a' MODEL 'm' FROM docs USING dense LIMIT 10, \
                   QUERY TEXT 'b' MODEL 'm' FROM docs USING dense LIMIT 10) LIMIT 5;",
    )
    .unwrap();
    let op_default = qql_plan::plan(&stmt_default).unwrap();
    let (collection, req_default) = match &op_default {
        qql_plan::PlannedOperation::Query {
            collection,
            request,
        } => (collection, request),
        other => panic!("expected Query, got {other:?}"),
    };
    let qp_default = to_query_points(req_default, collection).unwrap();
    match qp_default.query.and_then(|q| q.variant) {
        Some(Qv::Fusion(f)) => {
            assert_eq!(f, qdrant::Fusion::Rrf as i32);
        }
        other => panic!("expected Fusion(Rrf), got {other:?}"),
    }
}

#[test]
fn facet_counts_conversion_matches_plan() {
    let stmt = Parser::parse(
        "FACET room_type FROM stays WHERE price < 150 LIMIT 5 EXACT true SHARD 'tenant_1';",
    )
    .unwrap();
    let op = qql_plan::plan(&stmt).unwrap();
    let (collection, req) = match &op {
        qql_plan::PlannedOperation::Facet {
            collection,
            request,
        } => (collection, request),
        other => panic!("expected Facet, got {other:?}"),
    };

    let fc = to_facet_counts(req, collection).unwrap();
    assert_eq!(fc.collection_name, "stays");
    assert_eq!(fc.key, "room_type");
    assert_eq!(fc.limit, Some(5));
    assert_eq!(fc.exact, Some(true));
    assert!(fc.filter.is_some());
    match &fc.shard_key_selector {
        Some(sks) => {
            assert_eq!(sks.shard_keys.len(), 1);
            assert_eq!(
                sks.shard_keys[0].key.as_ref(),
                Some(&qdrant::shard_key::Key::Keyword("tenant_1".to_string()))
            );
        }
        None => panic!("expected shard_key_selector to be set"),
    }
}

#[test]
fn facet_hit_to_typed_handles_all_variants() {
    use qdrant::facet_value::Variant;
    use qdrant::{FacetHit, FacetValue};

    let cases = [
        (
            FacetHit {
                value: Some(FacetValue {
                    variant: Some(Variant::StringValue("hotel".to_string())),
                }),
                count: 42,
            },
            PlanFacetValue::Keyword("hotel".to_string()),
            42,
        ),
        (
            FacetHit {
                value: Some(FacetValue {
                    variant: Some(Variant::IntegerValue(101)),
                }),
                count: 7,
            },
            PlanFacetValue::Integer(101),
            7,
        ),
        (
            FacetHit {
                value: Some(FacetValue {
                    variant: Some(Variant::BoolValue(true)),
                }),
                count: 99,
            },
            PlanFacetValue::Bool(true),
            99,
        ),
    ];
    for (proto, value, count) in cases {
        let typed = facet_hit_to_typed(proto).expect("facet converts");
        assert_eq!(typed.value, value);
        assert_eq!(typed.count, count);
    }

    // A facet hit without a value is a backend contract violation.
    let error = facet_hit_to_typed(FacetHit {
        value: None,
        count: 0,
    })
    .unwrap_err();
    assert_eq!(error.code, "QQL-BACKEND-ENVELOPE");
}

/// RT-01: unbound vector params fail closed on the gRPC path (an error, never
/// a panic). `ensure_no_unbound_params` / `validate_no_unbound_scalar_params`
/// gate the normal paths; these pins cover a hand-built plan bypassing them.
#[test]
fn grpc_to_vector_input_rejects_unbound_params() {
    let named = PlanQueryInput::Vector(PlanVectorValue::Param("q".to_string()));
    let err = to_vector_input(&named).unwrap_err();
    assert_eq!(err.code, "QQL-BIND-UNBOUND-PARAM");

    let positional = PlanQueryInput::Vector(PlanVectorValue::PositionalParam(0));
    let err = to_vector_input(&positional).unwrap_err();
    assert_eq!(err.code, "QQL-BIND-MISSING-POSITIONAL");
}

/// RT-01: `plan_vector_to_proto` returns `QQL-BIND-*`, never panics.
#[test]
fn grpc_plan_vector_to_proto_rejects_unbound_params() {
    let err = plan_vector_to_proto(&PlanVectorValue::Param("v".to_string())).unwrap_err();
    assert_eq!(err.code, "QQL-BIND-UNBOUND-PARAM");

    let err = plan_vector_to_proto(&PlanVectorValue::PositionalParam(2)).unwrap_err();
    assert_eq!(err.code, "QQL-BIND-MISSING-POSITIONAL");
}

/// RT-01: `to_vectors` returns `QQL-BIND-*`, never panics.
#[test]
fn grpc_to_vectors_rejects_unbound_params() {
    let err = to_vectors(&PlanPointVectors::Param("vecs".to_string())).unwrap_err();
    assert_eq!(err.code, "QQL-BIND-UNBOUND-PARAM");

    let err = to_vectors(&PlanPointVectors::PositionalParam(1)).unwrap_err();
    assert_eq!(err.code, "QQL-BIND-MISSING-POSITIONAL");
}

// ── Typed read path: proto → BackendResponse, no JSON in between ──

fn proto_value(kind: qdrant::value::Kind) -> qdrant::Value {
    qdrant::Value { kind: Some(kind) }
}

fn sample_hardware_usage() -> qdrant::HardwareUsage {
    qdrant::HardwareUsage {
        cpu: 11,
        payload_io_read: 22,
        payload_io_write: 33,
        payload_index_io_read: 44,
        payload_index_io_write: 55,
        vector_io_read: 66,
        vector_io_write: 77,
    }
}

fn sample_usage() -> qdrant::Usage {
    qdrant::Usage {
        hardware: Some(sample_hardware_usage()),
        inference: Some(qdrant::InferenceUsage {
            models: [("bm25".to_string(), qdrant::ModelUsage { tokens: 512 })]
                .into_iter()
                .collect(),
        }),
    }
}

/// `scored_point_to_hit` maps point id, score, payload, and typed vectors
/// straight from the proto; `telemetry_from_proto` mirrors the same fields.
#[test]
fn typed_query_hit_maps_proto_directly() {
    let usage = sample_usage();
    let point = qdrant::ScoredPoint {
        id: Some(qdrant::PointId {
            point_id_options: Some(qdrant::point_id::PointIdOptions::Num(7)),
        }),
        payload: [
            (
                "title".to_string(),
                proto_value(qdrant::value::Kind::StringValue("a".to_string())),
            ),
            (
                "stars".to_string(),
                proto_value(qdrant::value::Kind::IntegerValue(5)),
            ),
        ]
        .into_iter()
        .collect(),
        score: 0.75,
        version: 3,
        vectors: Some(qdrant::VectorsOutput {
            vectors_options: Some(qdrant::vectors_output::VectorsOptions::Vector(
                qdrant::VectorOutput {
                    vector: Some(qdrant::vector_output::Vector::Dense(qdrant::DenseVector {
                        data: vec![0.5, 0.25],
                    })),
                    ..Default::default()
                },
            )),
        }),
        shard_key: None,
        order_value: None,
    };

    let hit = scored_point_to_hit(point).expect("scored point converts");
    assert_eq!(hit.id, qql_plan::PlanPointId::Number(7));
    assert_eq!(hit.score, 0.75);
    assert_eq!(
        hit.payload.as_ref().expect("payload")["title"],
        serde_json::json!("a")
    );
    assert_eq!(
        hit.vector,
        Some(PlanVectorStruct::Single(PlanVectorValue::Dense(vec![
            0.5, 0.25
        ]))),
        "dense vectors convert without a JSON hop"
    );

    let telemetry = telemetry_from_proto(0.125, Some(&usage));
    assert_eq!(
        telemetry,
        Some(crate::executor::ServerTelemetry {
            time_s: Some(0.125),
            usage: usage_to_telemetry(Some(&usage)),
        })
    );
}

/// Named proto vectors convert to [`PlanVectorStruct::Named`] with every
/// value typed (dense and sparse entries).
#[test]
fn typed_named_vectors_convert_to_plan_struct() {
    let vectors = qdrant::VectorsOutput {
        vectors_options: Some(qdrant::vectors_output::VectorsOptions::Vectors(
            qdrant::NamedVectorsOutput {
                vectors: [
                    (
                        "dense".to_string(),
                        qdrant::VectorOutput {
                            vector: Some(qdrant::vector_output::Vector::Dense(
                                qdrant::DenseVector {
                                    data: vec![0.5, 0.25],
                                },
                            )),
                            ..Default::default()
                        },
                    ),
                    (
                        "sparse".to_string(),
                        qdrant::VectorOutput {
                            vector: Some(qdrant::vector_output::Vector::Sparse(
                                qdrant::SparseVector {
                                    indices: vec![2, 9],
                                    values: vec![1.5, 0.5],
                                },
                            )),
                            ..Default::default()
                        },
                    ),
                ]
                .into_iter()
                .collect(),
            },
        )),
    };
    let converted = super::typed::vectors_output_to_typed(&vectors).expect("named vectors convert");
    let PlanVectorStruct::Named(entries) = converted else {
        panic!("expected named vector struct");
    };
    assert_eq!(entries["dense"], PlanVectorValue::Dense(vec![0.5, 0.25]));
    assert_eq!(
        entries["sparse"],
        PlanVectorValue::Sparse {
            indices: vec![2, 9],
            values: vec![1.5, 0.5],
        }
    );
}

/// `retrieved_point_to_hit` (get / scroll) keeps UUID ids and the unscored
/// `0.0` default.
#[test]
fn typed_retrieved_hit_maps_proto_directly() {
    let point = qdrant::RetrievedPoint {
        id: Some(qdrant::PointId {
            point_id_options: Some(qdrant::point_id::PointIdOptions::Uuid(
                "11111111-2222-3333-4444-555555555555".to_string(),
            )),
        }),
        payload: Default::default(),
        vectors: None,
        shard_key: None,
        order_value: None,
    };
    let hit = retrieved_point_to_hit(point).expect("retrieved point converts");
    assert_eq!(
        hit.id,
        qql_plan::PlanPointId::String("11111111-2222-3333-4444-555555555555".to_string())
    );
    assert_eq!(hit.score, 0.0);
    assert_eq!(hit.payload, None);
    assert_eq!(hit.vector, None);
}

/// `point_group_to_typed` maps every `GroupId` variant, keeps ordered hits,
/// and drops `lookup` points.
#[test]
fn typed_groups_map_proto_directly() {
    use qdrant::group_id::Kind;

    let point = |id: u64, score: f32| qdrant::ScoredPoint {
        id: Some(qdrant::PointId {
            point_id_options: Some(qdrant::point_id::PointIdOptions::Num(id)),
        }),
        payload: Default::default(),
        score,
        version: 0,
        vectors: None,
        shard_key: None,
        order_value: None,
    };

    let groups: Vec<_> = [
        qdrant::PointGroup {
            id: Some(qdrant::GroupId {
                kind: Some(Kind::UnsignedValue(7)),
            }),
            hits: vec![point(1, 0.75)],
            lookup: Some(qdrant::RetrievedPoint {
                id: Some(qdrant::PointId {
                    point_id_options: Some(qdrant::point_id::PointIdOptions::Num(99)),
                }),
                ..Default::default()
            }),
        },
        qdrant::PointGroup {
            id: Some(qdrant::GroupId {
                kind: Some(Kind::IntegerValue(-3)),
            }),
            hits: vec![point(2, 0.5)],
            lookup: None,
        },
        qdrant::PointGroup {
            id: Some(qdrant::GroupId {
                kind: Some(Kind::StringValue("a".to_string())),
            }),
            hits: Vec::new(),
            lookup: None,
        },
    ]
    .into_iter()
    .map(|group| point_group_to_typed(group).expect("group converts"))
    .collect();

    assert_eq!(groups[0].group_id, PlanGroupId::Unsigned(7));
    assert_eq!(groups[1].group_id, PlanGroupId::Signed(-3));
    assert_eq!(groups[2].group_id, PlanGroupId::Keyword("a".to_string()));
    assert_eq!(groups[0].hits.len(), 1, "lookup points expand into no hits");
}

/// Missing proto ids / group ids / vector values fail closed instead of
/// fabricating placeholder values.
#[test]
fn typed_converters_reject_missing_proto_values() {
    let err = scored_point_to_hit(qdrant::ScoredPoint::default()).unwrap_err();
    assert_eq!(err.code, "QQL-BACKEND-ENVELOPE");

    let err = point_group_to_typed(qdrant::PointGroup::default()).unwrap_err();
    assert_eq!(err.code, "QQL-BACKEND-ENVELOPE");

    let err = super::typed::vectors_output_to_typed(&qdrant::VectorsOutput {
        vectors_options: None,
    })
    .unwrap_err();
    assert_eq!(err.code, "QQL-BACKEND-ENVELOPE");
}

/// Write responses (upsert/delete/payload/vector ops) are status-only typed
/// mutations with telemetry straight from the proto: the proto `UpdateResult`
/// has no affected count (the executor derives upserts from the request).
#[test]
fn typed_mutation_response_is_status_only_with_telemetry() {
    let usage = sample_usage();
    let resp = qdrant::PointsOperationResponse {
        result: Some(qdrant::UpdateResult {
            operation_id: Some(42),
            status: qdrant::UpdateStatus::Completed as i32,
        }),
        time: 0.25,
        usage: Some(usage.clone()),
    };

    let typed = mutation_response_to_typed(resp);
    assert_eq!(typed.data, ExecData::Mutation { affected: None });
    assert_eq!(typed.telemetry, telemetry_from_proto(0.25, Some(&usage)));
    assert_eq!(
        serde_json::to_value(&typed.data).expect("serializes"),
        serde_json::Value::Null,
        "status-only mutations serialize as the report's null data"
    );
}

/// Collection / shard-key mutations (proto bool status + time) are status-only
/// typed mutations — no Raw envelope remains for status responses.
#[test]
fn typed_collection_mutation_is_status_only() {
    let typed = collection_mutation_to_typed(0.125);
    assert_eq!(typed.data, ExecData::Mutation { affected: None });
    assert_eq!(typed.telemetry, telemetry_from_proto(0.125, None));
}

/// `usage_to_telemetry` must mirror `usage_to_json` + `ServerUsage::from_json`
/// for every section combination (absent, empty, hardware-only,
/// inference-only, both).
#[test]
fn usage_to_telemetry_matches_json_pipeline() {
    let usage = sample_usage();
    assert_eq!(
        usage_to_telemetry(Some(&usage)),
        ServerUsage::from_json(&usage_to_json(Some(&usage))),
        "hardware + inference"
    );

    let hardware_only = qdrant::Usage {
        hardware: usage.hardware,
        inference: None,
    };
    assert_eq!(
        usage_to_telemetry(Some(&hardware_only)),
        ServerUsage::from_json(&usage_to_json(Some(&hardware_only))),
        "hardware only"
    );

    let inference_only = qdrant::Usage {
        hardware: None,
        inference: usage.inference.clone(),
    };
    assert_eq!(
        usage_to_telemetry(Some(&inference_only)),
        ServerUsage::from_json(&usage_to_json(Some(&inference_only))),
        "inference only"
    );

    let empty = qdrant::Usage {
        hardware: None,
        inference: None,
    };
    assert_eq!(usage_to_telemetry(Some(&empty)), None);
    assert_eq!(ServerUsage::from_json(&usage_to_json(Some(&empty))), None);
    assert_eq!(usage_to_telemetry(None), None);

    let parsed = usage_to_telemetry(Some(&usage)).expect("usage present");
    assert_eq!(
        parsed.hardware,
        Some(crate::executor::HardwareUsage {
            cpu: 11,
            payload_io_read: 22,
            payload_io_write: 33,
            payload_index_io_read: 44,
            payload_index_io_write: 55,
            vector_io_read: 66,
            vector_io_write: 77,
        })
    );
    assert_eq!(
        parsed.inference.expect("inference present").models["bm25"].tokens,
        512
    );
}
