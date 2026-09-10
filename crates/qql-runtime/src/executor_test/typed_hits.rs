//! Typed result IR: `ExecData` payload shapes, accessors, and executor
//! normalization over directly-constructed [`BackendResponse`]s.
//!
//! The wire contract stays report-JSON-compatible: `ExecData` serializes to
//! the same report JSON shapes (hits array, `{"count": n}`, facet array,
//! `{"groups": […]}` for groups, `{"collections": […]}` for lists, the
//! collection info object, `{"shard_keys": […]}`), so SDK consumers keep
//! reading the shapes they already read.

use serde_json::json;

use super::mock::{MockQdrantClient, collection_with_vectors, test_config};
use crate::executor::response::{BackendResponse, GroupedSearchResult};
use crate::executor::{ExecData, ExecResponse, Executor, FacetHit, OnError, SearchHit};
use qql_plan::{PlanFacetValue, PlanGroupId, PlanVectorStruct, PlanVectorValue};

fn hit(id: u64, score: f32) -> SearchHit {
    SearchHit {
        id: qql_plan::PlanPointId::Number(id),
        score,
        payload: None,
        collection: None,
        vector: None,
    }
}

fn planned(sql: &str) -> qql_plan::PlannedOperation {
    qql_plan::plan(&qql_core::parser::Parser::parse(sql).expect("parse")).expect("plan")
}

#[test]
fn exec_data_serializes_report_shapes() {
    assert_eq!(
        serde_json::to_value(ExecData::Hits(vec![hit(1, 0.5)])).unwrap(),
        json!([{"id": 1, "score": 0.5, "payload": null}])
    );
    assert_eq!(
        serde_json::to_value(ExecData::Count(3)).unwrap(),
        json!({"count": 3})
    );
    assert_eq!(
        serde_json::to_value(ExecData::Facet(vec![FacetHit {
            value: PlanFacetValue::Keyword("books".into()),
            count: 15
        }]))
        .unwrap(),
        json!([{"value": "books", "count": 15}])
    );
    assert_eq!(
        serde_json::to_value(ExecData::Collections(vec!["a".into(), "b".into()])).unwrap(),
        json!({"collections": ["a", "b"]})
    );
    assert_eq!(
        serde_json::to_value(ExecData::ShardKeys(vec![
            qql_plan::PlanShardKey::Keyword("acme".into()),
            qql_plan::PlanShardKey::Number(101),
        ]))
        .unwrap(),
        json!({"shard_keys": ["acme", 101]})
    );
    assert_eq!(
        serde_json::to_value(ExecData::Quotas(qql_plan::QuotaConfig {
            enabled: Some(true),
            ..Default::default()
        }))
        .unwrap(),
        json!({"enabled": true})
    );

    let info = ExecData::Collection(crate::backend::CollectionInfo {
        status: "green".into(),
        points_count: 12,
        segments_count: 2,
        ..Default::default()
    });
    let value = serde_json::to_value(&info).unwrap();
    assert_eq!(value["status"], "green");
    assert_eq!(value["points_count"], 12);
    assert_eq!(value["segments_count"], 2);

    assert_eq!(
        serde_json::to_value(ExecData::Groups(vec![GroupedSearchResult {
            group_id: PlanGroupId::Keyword("a".into()),
            hits: vec![hit(1, 1.0)],
        }]))
        .unwrap(),
        json!({"groups": [{"id": "a", "hits": [
            {"id": 1, "score": 1.0, "payload": null}
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
fn exec_data_accessors_and_emptiness() {
    let hits = ExecData::Hits(vec![SearchHit {
        id: qql_plan::PlanPointId::Number(1),
        score: 0.5,
        payload: None,
        collection: None,
        vector: Some(PlanVectorStruct::Single(PlanVectorValue::Dense(vec![0.5]))),
    }]);
    assert_eq!(hits.hits().unwrap()[0].id, qql_plan::PlanPointId::Number(1));
    assert!(!hits.is_empty());
    assert!(ExecData::Hits(Vec::new()).is_empty());
    assert!(ExecData::Groups(Vec::new()).is_empty());
    assert!(ExecData::Facet(Vec::new()).is_empty());
    assert!(ExecData::Collections(Vec::new()).is_empty());
    assert!(ExecData::ShardKeys(Vec::new()).is_empty());
    assert!(!ExecData::Count(0).is_empty());
    assert!(!ExecData::Mutation { affected: None }.is_empty());
    assert!(!ExecData::Quotas(qql_plan::QuotaConfig::default()).is_empty());

    let collections = ExecData::Collections(vec!["docs".into()]);
    assert_eq!(
        collections.collections(),
        Some(["docs".to_string()].as_slice())
    );
    let shard_keys = ExecData::ShardKeys(vec![qql_plan::PlanShardKey::Number(7)]);
    assert_eq!(
        shard_keys.shard_keys(),
        Some([qql_plan::PlanShardKey::Number(7)].as_slice())
    );
    let quotas = ExecData::Quotas(qql_plan::QuotaConfig {
        enabled: Some(true),
        ..Default::default()
    });
    assert_eq!(quotas.quotas().unwrap().enabled, Some(true));
    let info = ExecData::Collection(crate::backend::CollectionInfo::default());
    assert!(info.collection().is_some());
}

#[test]
fn grouped_normalization_trims_offset_exactly_once() {
    let op = planned(
        "QUERY TEXT 'x' MODEL 'm' FROM docs USING dense AS DENSE \
         GROUP BY category LIMIT 2 OFFSET 1",
    );
    let response = BackendResponse {
        data: ExecData::Groups(
            ["a", "b", "c"]
                .into_iter()
                .enumerate()
                .map(|(i, id)| GroupedSearchResult {
                    group_id: PlanGroupId::Keyword(id.into()),
                    hits: vec![hit(i as u64 + 1, 1.0)],
                })
                .collect(),
        ),
        telemetry: None,
    };

    // Normalization applies the client-side offset exactly once.
    let normalized = Executor::normalize_planned(&op, response).unwrap();
    let groups = normalized
        .data
        .as_ref()
        .and_then(ExecData::groups)
        .expect("typed groups survive normalization");
    assert_eq!(groups.len(), 2, "offset must drop exactly one group");
    assert_eq!(groups[0].group_id, PlanGroupId::Keyword("b".into()));
    assert_eq!(groups[1].group_id, PlanGroupId::Keyword("c".into()));
    assert_eq!(normalized.message, "Found 2 group(s)");
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
            value: PlanFacetValue::Keyword("books".into()),
            count: 15,
        }])),
        telemetry: None,
    };
    assert_eq!(
        facet.facet(),
        Some(vec![(PlanFacetValue::Keyword("books".into()), 15)])
    );
    assert!(facet.hits().is_none());
    assert!(facet.count().is_none());
}

#[test]
fn mutation_serializes_as_count() {
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
async fn dispatch_serves_typed_point_map() {
    let mut client = MockQdrantClient::default();
    client.info = Some(collection_with_vectors(&["dense"], &[]));
    client
        .point_map
        .lock()
        .unwrap()
        .insert("docs".to_string(), ExecData::Hits(vec![hit(1, 0.9)]));
    let planned_calls = client.execute_planned_call_count.clone();
    let executor = Executor::new(Box::new(client), Some(test_config()));

    let report = executor
        .execute(
            "QUERY VECTOR [0.1, 0.2, 0.3] FROM docs USING dense LIMIT 5;",
            OnError::Stop,
        )
        .await
        .expect("typed response executes");
    assert_eq!(report.ids(0), vec![1]);
    assert_eq!(*planned_calls.lock().unwrap(), 1);
}
