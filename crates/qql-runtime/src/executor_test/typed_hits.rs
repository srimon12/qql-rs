//! Typed result IR: `ExecData` accessors, shape-detecting deserialization,
//! and the REST envelope → typed parser (`envelope::parse_backend_response`).
//!
//! The wire contract stays report-JSON-compatible: `ExecData` serializes to
//! the same report JSON shapes (hits array, `{"count": n}`, facet array,
//! `{"groups": […]}` for groups, raw envelope), so SDK consumers keep reading
//! the shapes they already read.

use serde_json::json;

use super::mock::{MockQdrantClient, collection_with_vectors, test_config};
use crate::executor::response::{BackendResponse, GroupedSearchResult};
use crate::executor::{ExecData, ExecResponse, Executor, FacetHit, OnError, SearchHit};

fn hit(id: u64, score: f32) -> SearchHit {
    SearchHit {
        id: qql_plan::PlanPointId::Number(id),
        score,
        text: None,
        payload: None,
        collection: None,
        vector: None,
    }
}

fn planned(sql: &str) -> qql_plan::PlannedOperation {
    qql_plan::plan(&qql_core::parser::Parser::parse(sql).expect("parse")).expect("plan")
}

#[test]
fn exec_data_serializes_legacy_report_shapes() {
    assert_eq!(
        serde_json::to_value(ExecData::Hits(vec![hit(1, 0.5)])).unwrap(),
        json!([{"id": 1, "score": 0.5, "text": null, "payload": null}])
    );
    assert_eq!(
        serde_json::to_value(ExecData::Count(3)).unwrap(),
        json!({"count": 3})
    );
    assert_eq!(
        serde_json::to_value(ExecData::Facet(vec![FacetHit {
            value: json!("books"),
            count: 15
        }]))
        .unwrap(),
        json!([{"value": "books", "count": 15}])
    );
    assert_eq!(
        serde_json::to_value(ExecData::Raw(json!({"result": true}))).unwrap(),
        json!({"result": true})
    );
    assert_eq!(
        serde_json::to_value(ExecData::Groups(vec![GroupedSearchResult {
            group_id: json!("a"),
            hits: vec![hit(1, 1.0)],
        }]))
        .unwrap(),
        json!({"groups": [{"id": "a", "hits": [
            {"id": 1, "score": 1.0, "text": null, "payload": null}
        ]}]})
    );
    assert_eq!(
        serde_json::to_value(ExecData::Mutation { affected: Some(3) }).unwrap(),
        json!({"count": 3})
    );
    assert_eq!(
        serde_json::to_value(ExecData::Mutation { affected: None }).unwrap(),
        serde_json::Value::Null
    );
}

#[test]
fn exec_data_deserializes_by_shape() {
    let hits: ExecData = serde_json::from_value(json!([{"id": 1, "score": 0.5}])).unwrap();
    assert_eq!(hits.hits().unwrap()[0].id, qql_plan::PlanPointId::Number(1));

    let facet: ExecData = serde_json::from_value(json!([{"value": "books", "count": 15}])).unwrap();
    assert_eq!(
        facet,
        ExecData::Facet(vec![FacetHit {
            value: json!("books"),
            count: 15
        }])
    );

    let count: ExecData = serde_json::from_value(json!({"count": 42})).unwrap();
    assert_eq!(count, ExecData::Count(42));

    let groups: ExecData =
        serde_json::from_value(json!({"groups": [{"id": "a", "hits": [{"id": 1}]}]})).unwrap();
    assert_eq!(groups.groups().unwrap()[0].group_id, json!("a"));
    assert_eq!(groups.groups().unwrap()[0].hits.len(), 1);

    // `result`-wrapped count objects are envelopes, not bare counts.
    let raw: ExecData = serde_json::from_value(json!({"result": {"count": 42}})).unwrap();
    assert!(raw.as_raw().is_some());

    // Empty arrays read as "no hits", matching the pre-typing accessor.
    let empty: ExecData = serde_json::from_value(json!([])).unwrap();
    assert_eq!(empty, ExecData::Hits(Vec::new()));
    assert!(empty.is_empty());
}

#[test]
fn parse_backend_response_types_query_hits_and_telemetry() {
    let op = planned("QUERY TEXT 'search' MODEL 'm' FROM docs USING dense LIMIT 2");
    let response = crate::envelope::parse_backend_response(
        &op,
        json!({
            "result": {"points": [{"id": 1, "score": 0.9, "payload": {"text": "a"}}]},
            "status": "ok",
            "time": 0.25
        }),
    )
    .unwrap();
    let hits = response.data.hits().expect("query envelope yields hits");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, qql_plan::PlanPointId::Number(1));
    assert_eq!(
        serde_json::to_value(hits).unwrap()[0]["id"],
        1,
        "hits stay serializable in place"
    );
    assert!((response.telemetry.unwrap().time_s.unwrap() - 0.25).abs() < f64::EPSILON);
}

#[test]
fn parse_backend_response_types_count_and_facet() {
    let op = planned("COUNT FROM docs");
    let response = crate::envelope::parse_backend_response(
        &op,
        json!({"result": {"count": 7}, "status": "ok", "time": 0.1}),
    )
    .unwrap();
    assert_eq!(response.data.count(), Some(7));

    // Missing count defaults to 0, as before.
    let response =
        crate::envelope::parse_backend_response(&op, json!({"result": {}, "status": "ok"}))
            .unwrap();
    assert_eq!(response.data.count(), Some(0));

    let op = planned("FACET category FROM docs LIMIT 10");
    let response = crate::envelope::parse_backend_response(
        &op,
        json!({"result": {"hits": [{"value": "electronics", "count": 12}]}}),
    )
    .unwrap();
    assert_eq!(
        response.data.facet(),
        Some(
            [FacetHit {
                value: json!("electronics"),
                count: 12
            }]
            .as_slice()
        )
    );
}

#[test]
fn parse_backend_response_types_groups_and_normalization_trims_offset() {
    let op = planned(
        "QUERY TEXT 'x' MODEL 'm' FROM docs USING dense AS DENSE \
         GROUP BY category LIMIT 2 OFFSET 1",
    );
    let response = crate::envelope::parse_backend_response(
        &op,
        json!({
            "result": {"groups": [
                {"id": "a", "hits": [{"id": 1}]},
                {"id": "b", "hits": [{"id": 2}]},
                {"id": "c", "hits": [{"id": 3}]}
            ]},
            "status": "ok"
        }),
    )
    .unwrap();
    let groups = response.data.groups().expect("groups are typed");
    assert_eq!(groups.len(), 3, "parsing must not trim group_offset");

    // Normalization applies the client-side offset exactly once.
    let normalized = Executor::normalize_planned(&op, response).unwrap();
    let groups = normalized
        .data
        .as_ref()
        .and_then(ExecData::groups)
        .expect("typed groups survive normalization");
    assert_eq!(groups.len(), 2, "offset must drop exactly one group");
    assert_eq!(groups[0].group_id, json!("b"));
    assert_eq!(groups[1].group_id, json!("c"));
    assert_eq!(normalized.message, "Found 2 group(s)");
}

#[test]
fn parse_backend_response_passes_unmodelled_ops_through_raw() {
    let op = planned("DELETE FROM docs WHERE id = 1");
    let envelope = json!({"result": {"status": "completed"}, "status": "ok"});
    let response = crate::envelope::parse_backend_response(&op, envelope.clone()).unwrap();
    assert_eq!(response.data.as_raw(), Some(&envelope));
    assert!(response.telemetry.is_none());
}

#[test]
fn exec_response_serves_typed_data() {
    let resp = ExecResponse {
        ok: true,
        operation: "QUERY".into(),
        message: "Found 2 hits".into(),
        data: Some(ExecData::Hits(vec![hit(1, 0.9), hit(2, 0.5)])),
        telemetry: None,
    };
    assert_eq!(resp.hits_ref().unwrap().len(), 2);
    assert_eq!(resp.hits().unwrap().len(), 2);
    assert_eq!(resp.ids(), vec![1, 2]);
    assert!(resp.count().is_none());
    assert!(resp.facet().is_none());

    let count = ExecResponse {
        ok: true,
        operation: "COUNT".into(),
        message: "Count: 3".into(),
        data: Some(ExecData::Count(3)),
        telemetry: None,
    };
    assert_eq!(count.count(), Some(3));
    assert!(count.hits().is_none());
    assert!(count.facet().is_none());

    let facet = ExecResponse {
        ok: true,
        operation: "FACET".into(),
        message: "Found 1 facet hit(s)".into(),
        data: Some(ExecData::Facet(vec![FacetHit {
            value: json!("books"),
            count: 15,
        }])),
        telemetry: None,
    };
    assert_eq!(facet.facet(), Some(vec![(json!("books"), 15)]));
    assert!(facet.hits().is_none());
    assert!(facet.count().is_none());
}

#[test]
fn mutation_serializes_as_count_and_reads_back_as_count() {
    let resp = ExecResponse {
        ok: true,
        operation: "UPSERT".into(),
        message: "Upserted 4 point(s)".into(),
        data: Some(ExecData::Mutation { affected: Some(4) }),
        telemetry: None,
    };
    let value = serde_json::to_value(&resp).unwrap();
    assert_eq!(value["data"], json!({"count": 4}));
    assert_eq!(resp.count(), Some(4));

    let status_only = ExecResponse {
        ok: true,
        operation: "DELETE".into(),
        message: "DELETE ok".into(),
        data: Some(ExecData::Mutation { affected: None }),
        telemetry: None,
    };
    assert_eq!(
        serde_json::to_value(&status_only).unwrap()["data"],
        serde_json::Value::Null
    );
    assert_eq!(status_only.count(), None);
}

#[test]
fn response_round_trips_through_json() {
    let resp = ExecResponse {
        ok: true,
        operation: "QUERY".into(),
        message: "Found 1 hits".into(),
        data: Some(ExecData::Hits(vec![hit(1, 0.9)])),
        telemetry: None,
    };
    let value = serde_json::to_value(&resp).unwrap();
    assert_eq!(value["data"][0]["id"], 1, "legacy hits array shape");
    let back: ExecResponse = serde_json::from_value(value).unwrap();
    assert_eq!(back.hits().unwrap().len(), 1);
    assert_eq!(back.ids(), vec![1]);
}

#[tokio::test]
async fn dispatch_uses_typed_backend_response() {
    let mut client = MockQdrantClient::default();
    client.info = Some(collection_with_vectors(&["dense"], &[]));
    let planned_calls = client.execute_planned_call_count.clone();
    *client.typed_response.lock().unwrap() = Some(BackendResponse {
        data: ExecData::Hits(vec![hit(7, 1.0)]),
        telemetry: None,
    });
    let executor = Executor::new(Box::new(client), Some(test_config()));

    let report = executor
        .execute(
            "QUERY VECTOR [0.1, 0.2, 0.3] FROM docs USING dense LIMIT 5;",
            OnError::Stop,
        )
        .await
        .expect("typed response executes");
    assert_eq!(report.results[0].message, "Found 1 hits");
    assert_eq!(report.ids(0), vec![7]);
    assert_eq!(
        *planned_calls.lock().unwrap(),
        1,
        "the single typed entry point is used"
    );
}

#[tokio::test]
async fn dispatch_parses_backend_envelope() {
    let mut client = MockQdrantClient::default();
    client.info = Some(collection_with_vectors(&["dense"], &[]));
    client.point_map.lock().unwrap().insert(
        "docs".to_string(),
        json!({"result": {"points": [{"id": 1, "score": 0.9}]}}),
    );
    let planned_calls = client.execute_planned_call_count.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));

    let report = executor
        .execute(
            "QUERY VECTOR [0.1, 0.2, 0.3] FROM docs USING dense LIMIT 5;",
            OnError::Stop,
        )
        .await
        .expect("envelope response executes");
    assert_eq!(report.ids(0), vec![1]);
    assert_eq!(*planned_calls.lock().unwrap(), 1);
}
