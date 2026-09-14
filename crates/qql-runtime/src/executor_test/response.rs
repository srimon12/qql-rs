use crate::executor::{ExecData, ExecResponse, ExecutionReport, FacetHit, SearchHit};
use qql_plan::{PlanFacetValue, PlanVectorStruct, PlanVectorValue};

#[test]
fn test_execution_report_counts_mixed_results() {
    let report = ExecutionReport::from_results(vec![
        ExecResponse {
            ok: true,
            operation: "QUERY".to_string(),
            message: "ok".to_string(),
            data: None,
            telemetry: None,
        },
        ExecResponse {
            ok: false,
            operation: "QUERY".to_string(),
            message: "failed".to_string(),
            data: None,
            telemetry: None,
        },
    ]);

    assert!(!report.ok);
    assert_eq!(report.succeeded, 1);
    assert_eq!(report.failed, 1);
}

#[test]
fn test_search_hit_carries_typed_vector() {
    let hit = SearchHit {
        id: qql_plan::PlanPointId::Number(1),
        score: 0.88,
        payload: Some(std::collections::HashMap::from([(
            "title".to_string(),
            serde_json::json!("hello"),
        )])),
        collection: None,
        vector: Some(PlanVectorStruct::Single(PlanVectorValue::Dense(vec![
            0.5, 0.25, 0.125,
        ]))),
    };
    assert_eq!(hit.id, qql_plan::PlanPointId::Number(1));
    assert_eq!(hit.score, 0.88);
    assert_eq!(
        serde_json::to_value(&hit).unwrap()["vector"],
        serde_json::json!([0.5, 0.25, 0.125])
    );
}

#[cfg(feature = "rest")]
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
        payload: None,
        collection: None,
        vector: None,
    };
    let query_resp = ExecResponse {
        ok: true,
        operation: "QUERY".into(),
        message: "Found 1 hits".into(),
        data: Some(ExecData::Hits(vec![hit])),
        telemetry: None,
    };

    assert_eq!(query_resp.ids(), vec![42]);
    let typed_hits = query_resp.hits().expect("should hold typed hits");
    assert_eq!(typed_hits.len(), 1);
    assert_eq!(typed_hits[0].score, 0.95);

    let count_resp = ExecResponse {
        ok: true,
        operation: "COUNT".into(),
        message: "Count: 15".into(),
        data: Some(ExecData::Count(15)),
        telemetry: None,
    };
    assert_eq!(count_resp.count(), Some(15));

    let facet_resp = ExecResponse {
        ok: true,
        operation: "FACET".into(),
        message: "Facet hits: 2".into(),
        data: Some(ExecData::Facet(vec![
            FacetHit {
                value: PlanFacetValue::Keyword("Mitte".into()),
                count: 10,
            },
            FacetHit {
                value: PlanFacetValue::Keyword("Pankow".into()),
                count: 5,
            },
        ])),
        telemetry: None,
    };
    let facet_pairs = facet_resp.facet().expect("should parse facet pairs");
    assert_eq!(facet_pairs.len(), 2);
    assert_eq!(facet_pairs[0].0, PlanFacetValue::Keyword("Mitte".into()));
    assert_eq!(facet_pairs[0].1, 10);

    let report = ExecutionReport::from_results(vec![query_resp, count_resp, facet_resp]);
    assert_eq!(report.hits(0).unwrap().len(), 1);
    assert_eq!(report.ids(0), vec![42]);
    assert_eq!(report.first_ids(), vec![42]);
    assert_eq!(report.first_hits().unwrap().len(), 1);
    assert_eq!(report.count(1), Some(15));
    assert_eq!(report.first_count(), None);
    assert_eq!(report.facet(2).unwrap().len(), 2);
}
