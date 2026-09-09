//! Phase-1 telemetry tests: extraction, None-where-absent, serialization
//! back-compat, aggregation, and `explain_analyze` end-to-end shape.

use super::mock::{MockQdrantClient, collection_with_vectors, test_config};
use crate::executor::telemetry::{ServerTelemetry, ServerUsage};
use crate::executor::{AnalyzeReport, ExecResponse, ExecutionReport, Executor, OnError};

fn query_executor() -> Executor {
    let mut client = MockQdrantClient::default();
    client.exists = true;
    client.info = Some(collection_with_vectors(&["dense"], &[]));
    Executor::new(Box::new(client), Some(test_config()))
}

#[tokio::test]
async fn telemetry_populates_from_mock_envelope() {
    let executor = query_executor();
    let report = executor
        .execute(
            "QUERY NEAREST VECTOR [0.1, 0.2, 0.3] FROM docs USING dense LIMIT 5;",
            OnError::Stop,
        )
        .await
        .expect("execute ok");
    let tel = report.results[0]
        .telemetry
        .as_ref()
        .expect("mock envelope carries telemetry");
    assert!((tel.time_s.unwrap() - 0.0025).abs() < f64::EPSILON);
    let usage = tel.usage.as_ref().expect("mock usage present");
    let hw = usage.hardware.as_ref().expect("mock hardware present");
    assert_eq!(hw.cpu, 10);
    assert_eq!(hw.vector_io_write, 6);
    let tokens = usage.inference.as_ref().unwrap().models["mock-model"].tokens;
    assert_eq!(tokens, 7);
    // Report-level totals mirror the single response.
    let total = report.telemetry.as_ref().expect("report totals present");
    assert!((total.time_s.unwrap() - 0.0025).abs() < f64::EPSILON);
}

#[tokio::test]
async fn telemetry_none_where_backend_sends_none() {
    // `point_map` values are bare `{"result": …}` shapes with no
    // time/usage — extraction must yield None, not an error.
    let mut client = MockQdrantClient::default();
    client.exists = true;
    client.info = Some(collection_with_vectors(&["dense"], &[]));
    client.point_map.lock().unwrap().insert(
        "docs".to_string(),
        serde_json::json!({"result": {"points": [
            {"id": 1, "score": 0.9, "payload": {}}
        ]}}),
    );
    let executor = Executor::new(Box::new(client), Some(test_config()));
    let report = executor
        .execute(
            "QUERY NEAREST VECTOR [0.1, 0.2, 0.3] FROM docs USING dense LIMIT 5;",
            OnError::Stop,
        )
        .await
        .expect("execute ok");
    assert!(report.results[0].telemetry.is_none());
    assert!(report.telemetry.is_none());
    assert_eq!(report.results[0].hits_json().unwrap().len(), 1);
}

#[tokio::test]
async fn telemetry_none_for_batch_items() {
    // Batch items are bare per-item values with no envelope timing.
    let executor = query_executor();
    let stmts = qql_core::parser::Parser::parse_all(
        "QUERY NEAREST VECTOR [0.1, 0.2, 0.3] FROM docs USING dense LIMIT 5;
         QUERY NEAREST VECTOR [0.4, 0.5, 0.6] FROM docs USING dense LIMIT 5;",
    )
    .unwrap();
    let results = executor.execute_batch_nodes(stmts, true).await.unwrap();
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|r| r.telemetry.is_none()));
}

#[test]
fn telemetry_extraction_is_lenient_never_failing() {
    // Misshapen halves degrade to None independently.
    let v = serde_json::json!({"result": [], "time": "fast", "usage": [1, 2]});
    assert!(ServerTelemetry::from_envelope_opt(&v).is_none());
    let v = serde_json::json!({"result": [], "time": 0.5, "usage": {"hardware": "lots"}});
    let tel = ServerTelemetry::from_envelope_opt(&v).expect("time survives bad usage");
    assert!((tel.time_s.unwrap() - 0.5).abs() < f64::EPSILON);
    assert!(tel.usage.is_none());
    // Partial usage keeps the good section.
    let v = serde_json::json!({"usage": {"hardware": {"cpu": 3}, "inference": {"models": {}}}});
    let usage = ServerUsage::from_json(&v["usage"]).expect("partial usage parses");
    assert_eq!(usage.hardware.as_ref().unwrap().cpu, 3);
    assert!(usage.inference.as_ref().unwrap().models.is_empty());
}

#[test]
fn telemetry_serialization_stays_back_compatible() {
    // Old payloads (no `telemetry` key) deserialize; `None` serializes
    // without the key, so the pre-telemetry shape is byte-identical.
    let old: ExecResponse = serde_json::from_value(serde_json::json!({
        "ok": true, "operation": "QUERY", "message": "Found 0 hits", "data": []
    }))
    .unwrap();
    assert!(old.telemetry.is_none());
    let val = serde_json::to_value(&old).unwrap();
    assert!(!val.as_object().unwrap().contains_key("telemetry"));

    let old_report: ExecutionReport = serde_json::from_value(serde_json::json!({
        "ok": true, "results": [], "succeeded": 0, "failed": 0
    }))
    .unwrap();
    assert!(old_report.telemetry.is_none());
    let val = serde_json::to_value(&old_report).unwrap();
    assert!(!val.as_object().unwrap().contains_key("telemetry"));

    // Populated telemetry round-trips.
    let tel = ServerTelemetry::from_envelope(&serde_json::json!({
        "time": 0.1, "usage": {"hardware": {"cpu": 1}}
    }));
    let resp = ExecResponse {
        ok: true,
        operation: "QUERY".into(),
        message: "ok".into(),
        data: None,
        telemetry: Some(tel),
        typed_hits: std::sync::OnceLock::new(),
    };
    let back: ExecResponse = serde_json::from_value(serde_json::to_value(&resp).unwrap()).unwrap();
    assert_eq!(back.telemetry.unwrap().time_s, Some(0.1));
}

#[test]
fn report_telemetry_aggregates_totals() {
    let mk = |time: Option<f64>, cpu: Option<u64>, model_tokens: Option<u64>| ExecResponse {
        ok: true,
        operation: "QUERY".into(),
        message: "ok".into(),
        data: None,
        telemetry: Some(ServerTelemetry {
            time_s: time,
            usage: match (cpu, model_tokens) {
                (None, None) => None,
                _ => Some(ServerUsage {
                    hardware: cpu.map(|cpu| crate::executor::HardwareUsage {
                        cpu,
                        ..Default::default()
                    }),
                    inference: model_tokens.map(|tokens| crate::executor::InferenceUsage {
                        models: std::collections::HashMap::from([(
                            "m".to_string(),
                            crate::executor::ModelUsage { tokens },
                        )]),
                    }),
                }),
            },
        }),
        typed_hits: std::sync::OnceLock::new(),
    };
    let report = ExecutionReport::from_results(vec![
        mk(Some(0.1), Some(5), Some(10)),
        mk(Some(0.2), Some(7), Some(4)),
        mk(None, None, None),
    ]);
    let total = report.telemetry.as_ref().expect("totals present");
    assert!((total.time_s.unwrap() - 0.3).abs() < 1e-9);
    assert_eq!(
        total.usage.as_ref().unwrap().hardware.as_ref().unwrap().cpu,
        12
    );
    assert_eq!(
        total
            .usage
            .as_ref()
            .unwrap()
            .inference
            .as_ref()
            .unwrap()
            .models["m"]
            .tokens,
        14
    );
    // All-absent aggregates to None, not a zeroed struct.
    let empty = ExecutionReport::from_results(vec![mk(None, None, None)]);
    assert!(empty.telemetry.is_none());
}

#[tokio::test]
async fn explain_analyze_end_to_end_shape() {
    let executor = query_executor();
    let report: AnalyzeReport = executor
        .explain_analyze(
            "QUERY NEAREST VECTOR [0.1, 0.2, 0.3] FROM docs USING dense LIMIT 5;",
            OnError::Stop,
        )
        .await
        .expect("analyze ok");
    assert!(report.ok);
    assert!(report.plan.contains("Statement: QUERY"));
    assert!(report.plan.contains("docs"));
    // Phases are populated and the total covers the parts timed inside it
    // (`parse_ms` is measured before the total stopwatch starts).
    let parts = report.phases.prepare_plan_ms + report.phases.dispatch_ms + report.phases.decode_ms;
    assert!(report.phases.total_ms + 1e-6 >= parts);
    assert_eq!(report.results.len(), 1);
    // Server halves mirror the per-response telemetry.
    let tel = report.results[0]
        .telemetry
        .as_ref()
        .expect("mock telemetry");
    assert_eq!(report.server_time_s, tel.time_s);
    assert_eq!(report.usage, tel.usage);
    assert!((report.server_time_s.unwrap() - 0.0025).abs() < f64::EPSILON);
    // AnalyzeReport itself is JSON-serializable for the bindings.
    let val = serde_json::to_value(&report).unwrap();
    assert_eq!(val["server_time_s"], serde_json::json!(0.0025));
    assert!(val["phases"]["total_ms"].as_f64().unwrap() >= 0.0);
}

#[tokio::test]
async fn explain_analyze_covers_write_routes_and_rejects_scripts() {
    let executor = query_executor();
    // Write route: UPSERT normalizes into a fresh `{"count": n}` body, but
    // telemetry is extracted from the raw envelope first.
    let report = executor
        .explain_analyze(
            "UPSERT INTO docs VALUES {id: 1, vector: [0.1, 0.2, 0.3]};",
            OnError::Stop,
        )
        .await
        .expect("upsert analyze ok");
    assert!(report.plan.contains("Statement: UPSERT"));
    assert!(report.server_time_s.is_some());
    assert!(report.usage.is_some());

    // Multi-statement scripts fail closed (analyze is single-statement).
    let err = executor
        .explain_analyze(
            "QUERY NEAREST VECTOR [0.1] FROM docs USING dense LIMIT 1;
             QUERY NEAREST VECTOR [0.2] FROM docs USING dense LIMIT 1;",
            OnError::Stop,
        )
        .await
        .unwrap_err();
    assert_eq!(err.code, "QQL-VALIDATION-MULTI-STMT");

    // Params variant mirrors execute_with_named_params conventions.
    let report = executor
        .explain_analyze_with_named_params(
            "QUERY NEAREST VECTOR :v FROM docs USING dense LIMIT 5;",
            &[("v", qql_core::ast::Value::F32Array(vec![0.1, 0.2, 0.3]))],
            OnError::Stop,
        )
        .await
        .expect("params analyze ok");
    assert!(report.ok);
    assert!(report.server_time_s.is_some());
}

#[test]
fn normalize_planned_keeps_telemetry_out_of_the_error_path() {
    // Garbage telemetry must not fail normalization.
    let op = qql_plan::PlannedOperation::Count {
        collection: "docs".to_string(),
        request: qql_plan::CountRequest {
            filter: None,
            exact: None,
            shard_key: None,
        },
    };
    let resp = Executor::normalize_planned(
        &op,
        serde_json::json!({"result": {"count": 3}, "status": "ok", "time": "soon", "usage": 42}),
    )
    .expect("garbage telemetry never fails");
    assert!(resp.telemetry.is_none());
    assert_eq!(resp.count(), Some(3));

    // Well-formed telemetry attaches without disturbing the payload.
    let resp = Executor::normalize_planned(
        &op,
        serde_json::json!({"result": {"count": 3}, "status": "ok", "time": 0.01,
            "usage": {"inference": {"models": {"e5": {"tokens": 9}}}}}),
    )
    .unwrap();
    let tel = resp.telemetry.as_ref().unwrap();
    assert!((tel.time_s.unwrap() - 0.01).abs() < f64::EPSILON);
    assert_eq!(
        tel.usage
            .as_ref()
            .unwrap()
            .inference
            .as_ref()
            .unwrap()
            .models["e5"]
            .tokens,
        9
    );
}

#[cfg(feature = "grpc")]
#[test]
fn grpc_usage_json_matches_rest_shape() {
    use crate::grpc_route::test_api::usage_to_json;
    // `None` (collection/DDL routes carry no `usage` field) → JSON null,
    // which the executor reads as absent.
    let null = usage_to_json(None);
    assert!(null.is_null());
    assert!(ServerUsage::from_json(&null).is_none());
    // A populated proto Usage converts to the REST shape the executor parses.
    let usage = crate::qdrant_grpc::qdrant::Usage {
        hardware: Some(crate::qdrant_grpc::qdrant::HardwareUsage {
            cpu: 11,
            vector_io_read: 2,
            ..Default::default()
        }),
        inference: Some(crate::qdrant_grpc::qdrant::InferenceUsage {
            models: std::collections::HashMap::from([(
                "e5".to_string(),
                crate::qdrant_grpc::qdrant::ModelUsage { tokens: 5 },
            )]),
        }),
    };
    let parsed =
        ServerUsage::from_json(&usage_to_json(Some(&usage))).expect("grpc usage parses as REST");
    assert_eq!(parsed.hardware.as_ref().unwrap().cpu, 11);
    assert_eq!(parsed.hardware.as_ref().unwrap().vector_io_read, 2);
    assert_eq!(parsed.inference.as_ref().unwrap().models["e5"].tokens, 5);
}
