use crate::executor::{ExecResponse, ExecutionReport, SearchHit};

#[test]
fn test_execution_report_counts_mixed_results() {
    let report = ExecutionReport::from_results(vec![
        ExecResponse {
            ok: true,
            operation: "QUERY".to_string(),
            message: "ok".to_string(),
            data: None,
            telemetry: None,
            typed_hits: std::sync::OnceLock::new(),
        },
        ExecResponse {
            ok: false,
            operation: "QUERY".to_string(),
            message: "failed".to_string(),
            data: None,
            telemetry: None,
            typed_hits: std::sync::OnceLock::new(),
        },
    ]);

    assert!(!report.ok);
    assert_eq!(report.succeeded, 1);
    assert_eq!(report.failed, 1);
}

#[test]
fn test_search_hit_preserves_vector() {
    let raw = serde_json::json!({
        "result": [
            {
                "id": 1,
                "score": 0.88,
                "vector": [0.1, 0.2, 0.3],
                "payload": {"title": "hello"}
            }
        ]
    });
    let hits = crate::executor::dml::query::extract_search_hits(&raw);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, qql_plan::PlanPointId::Number(1));
    assert_eq!(hits[0].score, 0.88);
    assert_eq!(hits[0].vector, Some(serde_json::json!([0.1, 0.2, 0.3])));
}

#[test]
fn test_backend_error_classification() {
    assert_eq!(
        crate::rest::classify_backend_error_code(400, "Index does not exist for facet query"),
        "QQL-BACKEND-INDEX-NOT-READY"
    );
    assert_eq!(
        crate::rest::classify_backend_error_code(404, "Collection test not found"),
        "QQL-BACKEND-COLLECTION-NOT-FOUND"
    );
    assert_eq!(
        crate::rest::classify_backend_error_code(400, "Wrong vector dimensionality 128 vs 256"),
        "QQL-BACKEND-DIMENSION-MISMATCH"
    );
    assert_eq!(
        crate::rest::classify_backend_error_code(403, "Strict mode violation"),
        "QQL-BACKEND-STRICT-MODE"
    );
    assert_eq!(
        crate::rest::classify_backend_error_code(401, "Unauthorized"),
        "QQL-BACKEND-AUTH"
    );
}

#[test]
fn test_exec_response_and_report_helpers() {
    let hit = SearchHit {
        id: qql_plan::PlanPointId::Number(42),
        score: 0.95,
        text: Some("hello".into()),
        payload: None,
        collection: None,
        vector: None,
    };
    let hits_json = serde_json::to_value(vec![hit]).unwrap();
    let query_resp = ExecResponse {
        ok: true,
        operation: "QUERY".into(),
        message: "Found 1 hits".into(),
        data: Some(hits_json),
        telemetry: None,
        typed_hits: std::sync::OnceLock::new(),
    };

    assert_eq!(query_resp.ids(), vec![42]);
    let typed_hits = query_resp.hits().expect("should deserialize hits");
    assert_eq!(typed_hits.len(), 1);
    assert_eq!(typed_hits[0].score, 0.95);
    assert_eq!(query_resp.hits_json().unwrap().len(), 1);

    let count_resp = ExecResponse {
        ok: true,
        operation: "COUNT".into(),
        message: "Count: 15".into(),
        data: Some(serde_json::json!({
            "result": { "count": 15 }
        })),
        telemetry: None,
        typed_hits: std::sync::OnceLock::new(),
    };
    assert_eq!(count_resp.count(), Some(15));

    let facet_resp = ExecResponse {
        ok: true,
        operation: "FACET".into(),
        message: "Facet hits: 2".into(),
        data: Some(serde_json::json!([
            { "value": "Mitte", "count": 10 },
            { "value": "Pankow", "count": 5 }
        ])),
        telemetry: None,
        typed_hits: std::sync::OnceLock::new(),
    };
    let facet_pairs = facet_resp.facet().expect("should parse facet pairs");
    assert_eq!(facet_pairs.len(), 2);
    assert_eq!(facet_pairs[0].0, serde_json::json!("Mitte"));
    assert_eq!(facet_pairs[0].1, 10);

    let report = ExecutionReport::from_results(vec![query_resp, count_resp, facet_resp]);
    assert_eq!(report.hits(0).unwrap().len(), 1);
    assert_eq!(report.ids(0), vec![42]);
    assert_eq!(report.first_ids(), vec![42]);
    assert_eq!(report.first_hits_raw().len(), 1);
    assert_eq!(report.first_hits_json().unwrap().len(), 1);
    assert_eq!(report.count(1), Some(15));
    assert_eq!(report.first_count(), None);
    assert_eq!(report.facet(2).unwrap().len(), 2);
}
