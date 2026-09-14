use std::collections::HashMap;

use crate::executor::Executor;
use crate::executor::response::OnError;

use super::mock::{
    MockQdrantClient, collection_with_vectors, point_dict, prepared_upsert_executor, test_config,
};

#[tokio::test]
async fn test_prepared_query_caching_and_vector_substitution() {
    let client = MockQdrantClient {
        exists: true,
        collections: vec!["docs".to_string()],
        info: Some(collection_with_vectors(&["dense"], &[])),
        ..Default::default()
    };
    let last_planned = client.last_planned.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));

    let prepared = executor
        .prepare("QUERY VECTOR :v FROM docs USING dense LIMIT 5")
        .await
        .expect("prepare should succeed");
    assert!(
        prepared.is_planned(),
        "vector parameterized query must be pre-planned"
    );

    let mut params = HashMap::new();
    params.insert(
        "v".to_string(),
        qql_core::ast::Value::F32Array(vec![0.42, 0.99, 0.12]),
    );

    let report = executor
        .execute_prepared(&prepared, &params)
        .await
        .expect("execute_prepared should succeed");
    assert!(report.ok);

    let op = last_planned.lock().unwrap().take().expect("planned op");
    match op {
        qql_plan::PlannedOperation::Query { request, .. } => match &request.query {
            qql_plan::QueryVariant::Nearest(nearest) => match &nearest.nearest {
                qql_plan::semantic::PlanQueryInput::Vector(qql_plan::PlanVectorValue::Dense(
                    vec,
                )) => {
                    assert_eq!(*vec, vec![0.42, 0.99, 0.12]);
                }
                other => panic!("expected dense vector input, got {other:?}"),
            },
            other => panic!("expected nearest query, got {other:?}"),
        },
        other => panic!("expected query operation, got {other:?}"),
    }
}

#[tokio::test]
async fn test_prepared_query_guards_and_scalar_fallback() {
    let client = MockQdrantClient {
        exists: true,
        collections: vec!["docs".to_string()],
        info: Some(collection_with_vectors(&["dense"], &[])),
        ..Default::default()
    };
    let executor = Executor::new(Box::new(client), Some(test_config()));

    // 1. Static template: is_planned() == true, rejects unused params
    let static_prep = executor
        .prepare("QUERY [0.1, 0.2, 0.3] FROM docs USING dense LIMIT 5")
        .await
        .expect("static prepare should succeed");
    assert!(static_prep.is_planned());
    assert_eq!(static_prep.named_params().len(), 0);
    assert_eq!(static_prep.positional_count(), 0);

    let mut unused_params = HashMap::new();
    unused_params.insert("v".to_string(), qql_core::ast::Value::Int(1));
    let err = executor
        .execute_prepared(&static_prep, &unused_params)
        .await
        .unwrap_err();
    assert_eq!(err.code, "QQL-BIND-UNUSED-PARAMS");

    let err_pos = executor
        .execute_prepared_positional(&static_prep, &[qql_core::ast::Value::Int(1)])
        .await
        .unwrap_err();
    assert_eq!(err_pos.code, "QQL-BIND-UNUSED-PARAMS");

    // 2. Vector param template: is_planned() == true, rejects missing or invalid param
    let vec_prep = executor
        .prepare("QUERY VECTOR :v FROM docs USING dense LIMIT 5")
        .await
        .expect("vector prepare should succeed");
    assert!(vec_prep.is_planned());
    assert!(vec_prep.named_params().contains("v"));

    let err_missing = executor
        .execute_prepared(&vec_prep, &HashMap::new())
        .await
        .unwrap_err();
    assert_eq!(err_missing.code, "QQL-BIND-MISSING-PARAM");

    let mut bad_params = HashMap::new();
    bad_params.insert("v".to_string(), qql_core::ast::Value::Int(42));
    let err_invalid = executor
        .execute_prepared(&vec_prep, &bad_params)
        .await
        .unwrap_err();
    assert_eq!(err_invalid.code, "QQL-BIND-INVALID-PARAMS");

    let mut extra_named = HashMap::new();
    extra_named.insert(
        "v".to_string(),
        qql_core::ast::Value::F32Array(vec![0.1, 0.2, 0.3]),
    );
    extra_named.insert("extra".to_string(), qql_core::ast::Value::Int(99));
    let err_extra_named = executor
        .execute_prepared(&vec_prep, &extra_named)
        .await
        .unwrap_err();
    assert_eq!(err_extra_named.code, "QQL-BIND-UNUSED-PARAMS");

    // 3. Scalar param template: falls back to AST execution (is_planned() == false)
    let scalar_prep = executor
        .prepare("QUERY [0.1, 0.2] FROM docs WHERE score > :min_score LIMIT 5")
        .await
        .expect("scalar prepare should succeed");
    assert!(
        !scalar_prep.is_planned(),
        "query with scalar params must fall back to AST execution"
    );

    // 4. Positional vector param template: is_planned() == true
    let pos_prep = executor
        .prepare("QUERY VECTOR ? FROM docs USING dense LIMIT 5")
        .await
        .expect("positional vector prepare should succeed");
    assert!(pos_prep.is_planned());
    assert_eq!(pos_prep.positional_count(), 1);

    let err_missing_pos = executor
        .execute_prepared_positional(&pos_prep, &[])
        .await
        .unwrap_err();
    assert_eq!(err_missing_pos.code, "QQL-BIND-MISSING-PARAM");

    let err_extra_pos = executor
        .execute_prepared_positional(
            &pos_prep,
            &[
                qql_core::ast::Value::F32Array(vec![0.1, 0.2, 0.3]),
                qql_core::ast::Value::Int(42),
            ],
        )
        .await
        .unwrap_err();
    assert_eq!(err_extra_pos.code, "QQL-BIND-UNUSED-PARAMS");

    let res_pos = executor
        .execute_prepared_positional(
            &pos_prep,
            &[qql_core::ast::Value::F32Array(vec![0.1, 0.2, 0.3])],
        )
        .await
        .expect("positional vector prepared query execution should succeed");
    assert!(res_pos.ok);
}

#[tokio::test]
async fn test_prepared_multi_positional_params_zero_based() {
    let client = MockQdrantClient {
        exists: true,
        collections: vec!["docs".to_string()],
        info: Some(collection_with_vectors(&["dense"], &[])),
        ..Default::default()
    };
    let last_planned = client.last_planned.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));

    // 1. Planned multi-positional upsert: 0-based indexing binds params[0] to first ?, params[1] to second ?
    let upsert_prep = executor
        .prepare("UPSERT INTO docs VALUES { id: 10, vector: ? }, { id: 20, vector: ? }")
        .await
        .expect("multi-positional upsert prepare should succeed");
    assert!(upsert_prep.is_planned());
    assert_eq!(upsert_prep.positional_count(), 2);

    // Missing positional param (only 1 supplied, expects 2) -> QQL-BIND-MISSING-PARAM
    let err_missing = executor
        .execute_prepared_positional(
            &upsert_prep,
            &[qql_core::ast::Value::F32Array(vec![0.1, 0.2, 0.3])],
        )
        .await
        .unwrap_err();
    assert_eq!(err_missing.code, "QQL-BIND-MISSING-PARAM");

    // Extra positional param (3 supplied, expects 2) -> QQL-BIND-UNUSED-PARAMS
    let err_extra = executor
        .execute_prepared_positional(
            &upsert_prep,
            &[
                qql_core::ast::Value::F32Array(vec![0.1, 0.2, 0.3]),
                qql_core::ast::Value::F32Array(vec![0.4, 0.5, 0.6]),
                qql_core::ast::Value::Int(123),
            ],
        )
        .await
        .unwrap_err();
    assert_eq!(err_extra.code, "QQL-BIND-UNUSED-PARAMS");

    // Exact 2 params -> executes planned op and preserves 0-based positional assignment
    let report = executor
        .execute_prepared_positional(
            &upsert_prep,
            &[
                qql_core::ast::Value::F32Array(vec![0.1, 0.2, 0.3]),
                qql_core::ast::Value::F32Array(vec![0.4, 0.5, 0.6]),
            ],
        )
        .await
        .expect("execute_prepared_positional should succeed");
    assert!(report.ok);

    let op = last_planned.lock().unwrap().take().expect("planned op");
    match op {
        qql_plan::PlannedOperation::Upsert { request, .. } => {
            assert_eq!(request.points.len(), 2);
            match &request.points[0].vector {
                Some(qql_plan::PlanPointVectors::Unnamed(qql_plan::PlanVectorValue::Dense(v))) => {
                    assert_eq!(*v, vec![0.1, 0.2, 0.3]);
                }
                other => panic!("expected dense vector for point 0, got {other:?}"),
            }
            match &request.points[1].vector {
                Some(qql_plan::PlanPointVectors::Unnamed(qql_plan::PlanVectorValue::Dense(v))) => {
                    assert_eq!(*v, vec![0.4, 0.5, 0.6]);
                }
                other => panic!("expected dense vector for point 1, got {other:?}"),
            }
        }
        other => panic!("expected upsert op, got {other:?}"),
    }

    // 2. Scalar multi-positional query (fallback path): tests 0-based binding through AST
    let scalar_prep = executor
        .prepare("QUERY [0.1, 0.2, 0.3] FROM docs WHERE score > ? LIMIT ?")
        .await
        .expect("prepare scalar multi-positional query should succeed");
    assert!(!scalar_prep.is_planned());
    assert_eq!(scalar_prep.positional_count(), 2);

    // Missing positional param in fallback path -> QQL-BIND-MISSING-PARAM
    let err_scalar_missing = executor
        .execute_prepared_positional(&scalar_prep, &[qql_core::ast::Value::Float(0.75)])
        .await
        .unwrap_err();
    assert_eq!(err_scalar_missing.code, "QQL-BIND-MISSING-PARAM");

    // Extra positional param in fallback path -> QQL-BIND-UNUSED-PARAMS
    let err_scalar_extra = executor
        .execute_prepared_positional(
            &scalar_prep,
            &[
                qql_core::ast::Value::Float(0.75),
                qql_core::ast::Value::Int(10),
                qql_core::ast::Value::Str("extra".to_string()),
            ],
        )
        .await
        .unwrap_err();
    assert_eq!(err_scalar_extra.code, "QQL-BIND-UNUSED-PARAMS");

    // Exact 2 params in fallback path -> executes successfully
    let report_scalar = executor
        .execute_prepared_positional(
            &scalar_prep,
            &[
                qql_core::ast::Value::Float(0.75),
                qql_core::ast::Value::Int(10),
            ],
        )
        .await
        .expect("execute scalar prepared query should succeed");
    assert!(report_scalar.ok);
}

#[tokio::test]
async fn test_prepared_upsert_point_rows_fast_path() {
    let (executor, info_count, last_planned) = prepared_upsert_executor();
    let prepared = executor
        .prepare("UPSERT INTO docs VALUES :rows")
        .await
        .expect("point template must prepare");
    assert!(
        prepared.is_planned(),
        "point template must take the fast path"
    );
    // Schema fetched once at prepare, never per execution.
    assert_eq!(*info_count.lock().unwrap(), 1);

    for batch in [0, 100] {
        let mut params = HashMap::new();
        params.insert(
            "rows".to_string(),
            qql_core::ast::Value::List(
                (0..3)
                    .map(|i| point_dict(batch + i, vec![0.1, 0.2, 0.3], "t"))
                    .collect(),
            ),
        );
        let report = executor
            .execute_prepared(&prepared, &params)
            .await
            .expect("point splice execution must succeed");
        assert!(report.ok);
    }
    assert_eq!(
        *info_count.lock().unwrap(),
        1,
        "two executions must not re-fetch schema"
    );
    let op = last_planned.lock().unwrap().take().expect("planned op");
    match op {
        qql_plan::PlannedOperation::Upsert { request, .. } => {
            assert_eq!(request.points.len(), 3, "second batch wins");
            let ids: Vec<_> = request
                .points
                .iter()
                .map(|p| match &p.id {
                    qql_plan::PlanPointId::Number(n) => *n,
                    other => panic!("expected numeric id, got {other:?}"),
                })
                .collect();
            assert_eq!(ids, vec![100, 101, 102]);
            // Unnamed vectors resolved to the single dense target from cache.
            for point in &request.points {
                match point.vector.as_ref().expect("vector") {
                    qql_plan::PlanPointVectors::Named(entries) => {
                        assert_eq!(entries[0].0, "dense");
                    }
                    other => panic!("expected named dense vector, got {other:?}"),
                }
            }
        }
        other => panic!("expected upsert operation, got {other:?}"),
    }
}

#[tokio::test]
async fn test_prepared_upsert_point_missing_and_unused() {
    let (executor, _, _) = prepared_upsert_executor();
    let prepared = executor
        .prepare("UPSERT INTO docs VALUES :rows")
        .await
        .unwrap();
    // Missing rows.
    let err = executor
        .execute_prepared(&prepared, &HashMap::new())
        .await
        .unwrap_err();
    assert_eq!(err.code, "QQL-BIND-MISSING-PARAM");
    // Extra unknown key.
    let mut params = HashMap::new();
    params.insert(
        "rows".to_string(),
        qql_core::ast::Value::List(vec![point_dict(1, vec![0.1], "t")]),
    );
    params.insert("bogus".to_string(), qql_core::ast::Value::Int(1));
    let err = executor
        .execute_prepared(&prepared, &params)
        .await
        .unwrap_err();
    assert_eq!(err.code, "QQL-BIND-UNUSED-PARAMS");
}

#[tokio::test]
async fn test_prepared_upsert_point_missing_schema_falls_back() {
    // Collection absent at prepare: no cached schema, every execution takes
    // the checking slow path (and re-checks, so a later-created collection
    // still resolves).
    let client = MockQdrantClient {
        exists: false,
        collections: vec![],
        info: None,
        ..Default::default()
    };
    let info_count = client.info_call_count.clone();
    let exists_count = client.exists_call_count.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));
    let prepared = executor
        .prepare("UPSERT INTO docs VALUES :rows")
        .await
        .expect("prepare must succeed without schema");
    let mut params = HashMap::new();
    params.insert(
        "rows".to_string(),
        qql_core::ast::Value::List(vec![point_dict(1, vec![0.1], "t")]),
    );
    let report = executor.execute_prepared(&prepared, &params).await.unwrap();
    assert!(report.ok, "fallback execution must succeed: {report:?}");
    // prepare() checked existence once; the fallback slow path re-checks per
    // call instead of trusting absence, so a later-created collection still
    // resolves. No schema was ever fetched (nothing to cache).
    assert_eq!(*exists_count.lock().unwrap(), 2);
    assert_eq!(*info_count.lock().unwrap(), 0);
}

#[tokio::test]
async fn test_prepared_upsert_point_positional() {
    let (executor, _, last_planned) = prepared_upsert_executor();
    let prepared = executor
        .prepare("UPSERT INTO docs VALUES ?, ?")
        .await
        .expect("positional point template must prepare");
    assert!(prepared.is_planned());
    assert_eq!(prepared.positional_count(), 2);
    let params = vec![
        qql_core::ast::Value::Dict(vec![("id".into(), qql_core::ast::Value::Int(1))]),
        qql_core::ast::Value::Dict(vec![("id".into(), qql_core::ast::Value::Int(2))]),
    ];
    let report = executor
        .execute_prepared_positional(&prepared, &params)
        .await
        .expect("positional splice must succeed");
    assert!(report.ok);
    let op = last_planned.lock().unwrap().take().expect("planned op");
    match op {
        qql_plan::PlannedOperation::Upsert { request, .. } => {
            assert_eq!(request.points.len(), 2);
        }
        other => panic!("expected upsert operation, got {other:?}"),
    }
    // Too few values fail closed.
    let err = executor
        .execute_prepared_positional(&prepared, &params[..1])
        .await
        .unwrap_err();
    assert_eq!(err.code, "QQL-BIND-MISSING-PARAM");
}

#[tokio::test]
async fn test_upsert_many_batches_with_single_schema_fetch() {
    let (executor, info_count, _) = prepared_upsert_executor();
    let rows: Vec<qql_core::ast::Value> = (0..5)
        .map(|i| point_dict(i, vec![0.1, 0.2, 0.3], "t"))
        .collect();
    let report = executor
        .upsert_many("docs", rows, 2, OnError::Stop)
        .await
        .expect("upsert_many must succeed");
    assert!(report.ok);
    assert_eq!(report.results.len(), 3, "5 rows / batch 2 → 3 chunks");
    assert_eq!(report.succeeded, 3);
    assert_eq!(report.failed, 0);
    assert_eq!(
        *info_count.lock().unwrap(),
        1,
        "schema fetched once at prepare, never per chunk"
    );
}

#[tokio::test]
async fn test_upsert_many_empty_is_noop() {
    let (executor, _, _) = prepared_upsert_executor();
    let report = executor
        .upsert_many("docs", Vec::new(), 100, OnError::Stop)
        .await
        .expect("empty rows must succeed without I/O");
    assert!(report.ok);
    assert!(report.results.is_empty());
}

#[tokio::test]
async fn test_upsert_many_batch_size_zero_fails_closed() {
    let (executor, _, _) = prepared_upsert_executor();
    let rows = vec![point_dict(1, vec![0.1, 0.2, 0.3], "t")];
    let err = executor
        .upsert_many("docs", rows, 0, OnError::Stop)
        .await
        .unwrap_err();
    assert_eq!(err.code, "QQL-VALIDATION-UPSERT-BATCH");
}

#[tokio::test]
async fn test_upsert_many_continue_collects_chunk_errors() {
    let client = MockQdrantClient {
        exists: true,
        collections: vec!["docs".to_string()],
        info: Some(collection_with_vectors(&["dense"], &[])),
        ..Default::default()
    };
    let fail_call = client.fail_execute_planned_call.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));
    let rows: Vec<qql_core::ast::Value> = (0..5)
        .map(|i| point_dict(i, vec![0.1, 0.2, 0.3], "t"))
        .collect();

    // Fail the 2nd chunk under Stop → the whole call errors.
    *fail_call.lock().unwrap() = 2;
    let err = executor
        .upsert_many("docs", rows.clone(), 2, OnError::Stop)
        .await
        .unwrap_err();
    assert_eq!(err.code, "QQL-EXECUTION");

    // Same failure under Continue → collected per-chunk, other chunks land.
    // (Call counts are global to the mock: the Stop run consumed calls 1–2,
    // so the Continue run's 2nd chunk is global call 4.)
    *fail_call.lock().unwrap() = 4;
    let report = executor
        .upsert_many("docs", rows, 2, OnError::Continue)
        .await
        .expect("continue must collect, not abort");
    assert!(!report.ok);
    assert_eq!(report.results.len(), 3);
    assert_eq!(report.succeeded, 2);
    assert_eq!(report.failed, 1);
}

#[tokio::test]
async fn test_execute_with_named_params() {
    let mut client = MockQdrantClient::default();
    client.info = Some(collection_with_vectors(&["dense"], &["bm25"]));
    let executor = Executor::new(Box::new(client), Some(test_config()));

    let param_sql = "QUERY :v FROM docs USING dense LIMIT 5";
    let rep_param = executor
        .execute_with_named_params(
            param_sql,
            &[("v", qql_core::ast::Value::F32Array(vec![0.1, 0.2, 0.3]))],
            OnError::Stop,
        )
        .await
        .unwrap();
    assert!(rep_param.ok);
}

#[tokio::test]
async fn test_prepared_clear_payload_with_filter_and_shard_params() {
    // Regression: Clear/DeletePayload/DeleteVector selector params were never
    // collected, so prepared execution rejected them as unused. Shard params
    // ride the same arms.
    let client = MockQdrantClient::default();
    let executor = Executor::new(Box::new(client), Some(test_config()));

    let prepared = executor
        .prepare("CLEAR PAYLOAD FROM docs WHERE x = :v SHARD :t")
        .await
        .expect("prepare should succeed");
    assert!(prepared.named_params().contains("v"));
    assert!(prepared.named_params().contains("t"));

    let mut params = HashMap::new();
    params.insert("v".to_string(), qql_core::ast::Value::Str("a".into()));
    params.insert("t".to_string(), qql_core::ast::Value::Str("acme".into()));
    let report = executor
        .execute_prepared(&prepared, &params)
        .await
        .expect("execute_prepared should succeed");
    assert!(report.ok, "prepared clear payload must succeed: {report:?}");
}
