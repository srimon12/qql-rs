//! Strict REST response parsing: one OpenAPI shape per operation.
//!
//! REST is the only transport that speaks JSON envelopes, so
//! `RestQdrant::execute_planned` is the canonical caller. Every function here
//! reads exactly the OpenAPI response shape of its operation; a missing or
//! mistyped field fails with `QQL-BACKEND-ENVELOPE`. There are no fallbacks,
//! no shape detection, and no raw passthrough.
//!
//! Server telemetry is the one lenient extraction
//! ([`ServerTelemetry::from_envelope_opt`]): absent or misshapen telemetry
//! degrades to `None` and never fails a successful response.
//!
//! [`ServerTelemetry::from_envelope_opt`]: crate::executor::telemetry::ServerTelemetry::from_envelope_opt

use std::collections::HashMap;

use serde_json::Value;

use qql_core::error::QqlError;
use qql_plan::{
    PlanFacetValue, PlanGroupId, PlanPointId, PlanShardKey, PlanVectorStruct, PlannedOperation,
};

use crate::backend::CollectionInfo;
use crate::executor::response::{
    BackendResponse, ExecData, FacetHit, GroupedSearchResult, SearchHit,
};
use crate::executor::telemetry::ServerTelemetry;

fn envelope_err(message: impl Into<String>) -> QqlError {
    QqlError::backend("QQL-BACKEND-ENVELOPE", message.into(), None)
}

/// Parse one REST envelope into typed data for `op`.
pub(crate) fn parse_planned(
    op: &PlannedOperation,
    envelope: Value,
) -> Result<BackendResponse, QqlError> {
    let telemetry = ServerTelemetry::from_envelope_opt(&envelope);
    let data = match op {
        PlannedOperation::Query { .. } | PlannedOperation::Scroll { .. } => {
            ExecData::Hits(parse_points(&envelope)?)
        }
        PlannedOperation::GetPoints { .. } => ExecData::Hits(parse_bare_records(&envelope)?),
        PlannedOperation::QueryGroups { .. } => ExecData::Groups(parse_groups(&envelope)?),
        PlannedOperation::Count { .. } => ExecData::Count(parse_count(&envelope)?),
        PlannedOperation::Facet { .. } => ExecData::Facet(parse_facet(&envelope)?),
        PlannedOperation::ListCollections => {
            ExecData::Collections(parse_collection_names(&envelope)?)
        }
        PlannedOperation::GetCollection { .. } => {
            ExecData::Collection(parse_collection_info(&envelope)?)
        }
        PlannedOperation::ListShardKeys { .. } => ExecData::ShardKeys(parse_shard_keys(&envelope)?),
        PlannedOperation::GetQuotas => ExecData::Quotas(parse_quotas(&envelope)?),
        PlannedOperation::SetQuotas { request } => {
            // `PUT /quotas` answers a boolean status; the typed result is the
            // replacement config the caller sent.
            match envelope.get("result") {
                Some(Value::Bool(_)) => ExecData::Quotas(request.config.clone()),
                _ => {
                    return Err(envelope_err(
                        "set quotas response is missing the boolean result field",
                    ));
                }
            }
        }
        // Writes carry no typed response body: the executor owns upsert counts
        // and error classification already happened at the HTTP status level.
        PlannedOperation::Upsert { .. }
        | PlannedOperation::Delete { .. }
        | PlannedOperation::UpdatePayload { .. }
        | PlannedOperation::ClearPayload { .. }
        | PlannedOperation::DeletePayload { .. }
        | PlannedOperation::UpdateVectors { .. }
        | PlannedOperation::DeleteVectors { .. }
        | PlannedOperation::CreateCollection { .. }
        | PlannedOperation::UpdateCollection { .. }
        | PlannedOperation::DropCollection { .. }
        | PlannedOperation::CreateIndex { .. }
        | PlannedOperation::DropIndex { .. }
        | PlannedOperation::CreateShardKey { .. }
        | PlannedOperation::DropShardKey { .. } => ExecData::Mutation { affected: None },
        PlannedOperation::CrossRerank { .. } => {
            return Err(QqlError::execution(
                "QQL-REST-CLIENT-SIDE",
                "CROSS RERANK is client-side and has no REST response",
                None,
            ));
        }
    };
    Ok(BackendResponse { data, telemetry })
}

/// Parse one `/points/query/batch` result array: each OpenAPI `QueryResponse`
/// item carries its points at the item's top level (`{"points": […]}`).
pub(crate) fn parse_query_batch(envelope: &Value) -> Result<Vec<BackendResponse>, QqlError> {
    let items = envelope
        .get("result")
        .and_then(Value::as_array)
        .ok_or_else(|| envelope_err("query batch response is missing result as an array"))?;
    items
        .iter()
        .map(|item| {
            ensure_batch_item_ok(item)?;
            let points = item
                .get("points")
                .and_then(Value::as_array)
                .ok_or_else(|| envelope_err("query batch item is missing points as an array"))?;
            let hits = points
                .iter()
                .map(parse_hit)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(BackendResponse {
                data: ExecData::Hits(hits),
                telemetry: ServerTelemetry::from_envelope_opt(item),
            })
        })
        .collect()
}

/// Parse one `/points/batch` result array. Per-item `UpdateResult`s are
/// status-only: the executor derives upsert counts from the request.
pub(crate) fn parse_update_batch(envelope: &Value) -> Result<Vec<BackendResponse>, QqlError> {
    let items = envelope
        .get("result")
        .and_then(Value::as_array)
        .ok_or_else(|| envelope_err("update batch response is missing result as an array"))?;
    items
        .iter()
        .map(|item| {
            ensure_batch_item_ok(item)?;
            Ok(BackendResponse {
                data: ExecData::Mutation { affected: None },
                telemetry: None,
            })
        })
        .collect()
}

/// Parse `GET /collections` (`result.collections[*].name`).
pub(crate) fn parse_collection_names(envelope: &Value) -> Result<Vec<String>, QqlError> {
    let collections = envelope
        .get("result")
        .and_then(|result| result.get("collections"))
        .and_then(Value::as_array)
        .ok_or_else(|| envelope_err("list collections response is missing result.collections"))?;
    collections
        .iter()
        .map(|entry| {
            entry
                .get("name")
                .and_then(Value::as_str)
                .map(String::from)
                .ok_or_else(|| envelope_err("collection entry is missing a string name"))
        })
        .collect()
}

/// Parse `GET /collections/{name}` (`result` → [`CollectionInfo`]).
pub(crate) fn parse_collection_info(envelope: &Value) -> Result<CollectionInfo, QqlError> {
    let result = envelope
        .get("result")
        .filter(|value| value.is_object())
        .ok_or_else(|| envelope_err("get collection response is missing a result object"))?;
    let status = result
        .get("status")
        .and_then(Value::as_str)
        .ok_or_else(|| envelope_err("collection info is missing a string status"))?
        .to_string();
    let segments_count = result
        .get("segments_count")
        .and_then(Value::as_u64)
        .ok_or_else(|| envelope_err("collection info is missing an unsigned segments_count"))?;
    Ok(CollectionInfo {
        status,
        // `points_count` is nullable in the OpenAPI schema; an unreported
        // count reads as zero.
        points_count: result
            .get("points_count")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        // `indexed_vectors_count` is nullable in the OpenAPI schema; absent or
        // null both read as "not reported".
        indexed_vectors_count: result.get("indexed_vectors_count").and_then(Value::as_u64),
        segments_count,
        schema: crate::backend::schema_from_rest_result(result),
    })
}

/// Parse a query / scroll response (`result.points` → scored hits).
fn parse_points(envelope: &Value) -> Result<Vec<SearchHit>, QqlError> {
    let points = envelope
        .get("result")
        .and_then(|result| result.get("points"))
        .and_then(Value::as_array)
        .ok_or_else(|| envelope_err("query/scroll response is missing result.points"))?;
    points.iter().map(parse_hit).collect()
}

/// Parse a get-points response: `result` is a bare array of point records.
fn parse_bare_records(envelope: &Value) -> Result<Vec<SearchHit>, QqlError> {
    let records = envelope
        .get("result")
        .and_then(Value::as_array)
        .ok_or_else(|| envelope_err("get points response is missing result as an array"))?;
    records.iter().map(parse_hit).collect()
}

/// Parse one OpenAPI `ScoredPoint` / `Record` into a [`SearchHit`]. `score`
/// defaults to `0.0` for unscored retrieve/scroll records, matching the typed
/// gRPC/edge paths.
fn parse_hit(value: &Value) -> Result<SearchHit, QqlError> {
    let record = value
        .as_object()
        .ok_or_else(|| envelope_err("point record is not an object"))?;
    let id = parse_point_id(
        record
            .get("id")
            .ok_or_else(|| envelope_err("point record is missing id"))?,
    )?;
    let score = match record.get("score") {
        None | Some(Value::Null) => 0.0,
        Some(Value::Number(number)) => number
            .as_f64()
            .ok_or_else(|| envelope_err("point score is not a finite number"))?
            as f32,
        Some(other) => {
            return Err(envelope_err(format!(
                "point score must be a number, got {other}"
            )));
        }
    };
    let payload = match record.get("payload") {
        None | Some(Value::Null) => None,
        Some(Value::Object(map)) => Some(
            map.iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect::<HashMap<_, _>>(),
        ),
        Some(other) => {
            return Err(envelope_err(format!(
                "point payload must be an object or null, got {other}"
            )));
        }
    };
    let vector = match record.get("vector") {
        None | Some(Value::Null) => None,
        Some(vector) => Some(
            serde_json::from_value::<PlanVectorStruct>(vector.clone()).map_err(|error| {
                envelope_err(format!(
                    "point vector does not match VectorStructOutput: {error}"
                ))
            })?,
        ),
    };
    Ok(SearchHit {
        id,
        score,
        payload,
        collection: None,
        vector,
    })
}

fn parse_point_id(value: &Value) -> Result<PlanPointId, QqlError> {
    match value {
        Value::Number(number) => number
            .as_u64()
            .map(PlanPointId::Number)
            .ok_or_else(|| envelope_err(format!("point id {number} is not an unsigned integer"))),
        Value::String(id) => Ok(PlanPointId::String(id.clone())),
        other => Err(envelope_err(format!(
            "point id must be a string or unsigned integer, got {other}"
        ))),
    }
}

/// Parse a grouped query response (`result.groups`).
fn parse_groups(envelope: &Value) -> Result<Vec<GroupedSearchResult>, QqlError> {
    let groups = envelope
        .get("result")
        .and_then(|result| result.get("groups"))
        .and_then(Value::as_array)
        .ok_or_else(|| envelope_err("grouped query response is missing result.groups"))?;
    groups
        .iter()
        .map(|group| {
            let group_id = serde_json::from_value::<PlanGroupId>(
                group
                    .get("id")
                    .cloned()
                    .ok_or_else(|| envelope_err("group is missing id"))?,
            )
            .map_err(|error| envelope_err(format!("group id does not match GroupId: {error}")))?;
            let hits = group
                .get("hits")
                .and_then(Value::as_array)
                .ok_or_else(|| envelope_err("group is missing hits as an array"))?
                .iter()
                .map(parse_hit)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(GroupedSearchResult { group_id, hits })
        })
        .collect()
}

/// Parse a count response (`result.count`).
fn parse_count(envelope: &Value) -> Result<u64, QqlError> {
    envelope
        .get("result")
        .and_then(|result| result.get("count"))
        .and_then(Value::as_u64)
        .ok_or_else(|| envelope_err("count response is missing an unsigned result.count"))
}

/// Parse a facet response (`result.hits[*]` with strict `{value, count}`).
fn parse_facet(envelope: &Value) -> Result<Vec<FacetHit>, QqlError> {
    let hits = envelope
        .get("result")
        .and_then(|result| result.get("hits"))
        .and_then(Value::as_array)
        .ok_or_else(|| envelope_err("facet response is missing result.hits"))?;
    hits.iter()
        .map(|hit| {
            let value = serde_json::from_value::<PlanFacetValue>(
                hit.get("value")
                    .cloned()
                    .ok_or_else(|| envelope_err("facet hit is missing value"))?,
            )
            .map_err(|error| {
                envelope_err(format!("facet value does not match FacetValue: {error}"))
            })?;
            let count = hit
                .get("count")
                .and_then(Value::as_u64)
                .ok_or_else(|| envelope_err("facet hit is missing an unsigned count"))?;
            Ok(FacetHit { value, count })
        })
        .collect()
}

/// Parse `GET /collections/{name}/shards` (`result.shard_keys`, nullable when
/// the collection does not use custom sharding).
fn parse_shard_keys(envelope: &Value) -> Result<Vec<PlanShardKey>, QqlError> {
    match envelope
        .get("result")
        .and_then(|result| result.get("shard_keys"))
    {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(entries)) => entries
            .iter()
            .map(|entry| {
                let key = entry
                    .get("key")
                    .ok_or_else(|| envelope_err("shard key entry is missing key"))?;
                match key {
                    Value::String(keyword) => Ok(PlanShardKey::Keyword(keyword.clone())),
                    Value::Number(number) => {
                        number.as_u64().map(PlanShardKey::Number).ok_or_else(|| {
                            envelope_err(format!("shard key {number} is not an unsigned integer"))
                        })
                    }
                    other => Err(envelope_err(format!(
                        "shard key must be a string or unsigned integer, got {other}"
                    ))),
                }
            })
            .collect(),
        Some(other) => Err(envelope_err(format!(
            "shard_keys must be an array or null, got {other}"
        ))),
    }
}

/// Parse `GET /quotas` (`result.config` → [`QuotaConfig`](qql_plan::QuotaConfig)).
fn parse_quotas(envelope: &Value) -> Result<qql_plan::QuotaConfig, QqlError> {
    let config = envelope
        .get("result")
        .and_then(|result| result.get("config"))
        .ok_or_else(|| envelope_err("get quotas response is missing result.config"))?;
    serde_json::from_value(config.clone())
        .map_err(|error| envelope_err(format!("quota config is invalid: {error}")))
}

/// Qdrant batch endpoints answer per item; a 200 response can still carry
/// per-item failures (`status: "error"`). Surface the first one as a batch
/// error so the executor's continue path retries the group individually.
fn ensure_batch_item_ok(item: &Value) -> Result<(), QqlError> {
    match qql_plan::batch_item_error(item) {
        Some(message) => Err(QqlError::backend("QQL-BACKEND-BATCH", message, None)),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn planned(sql: &str) -> PlannedOperation {
        qql_plan::plan(&qql_core::parser::Parser::parse(sql).expect("parse")).expect("plan")
    }

    #[test]
    fn parses_scored_points_and_vectors_strictly() {
        let envelope = json!({
            "result": {"points": [{
                "id": 7,
                "score": 0.75,
                "payload": {"title": "a"},
                "vector": [0.5, 0.25],
            }]},
            "status": "ok",
            "time": 0.125,
        });
        let response =
            parse_planned(&planned("SCROLL FROM docs LIMIT 1"), envelope).expect("strict parse");
        let hits = response.data.hits().expect("hits");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, PlanPointId::Number(7));
        assert!((hits[0].score - 0.75).abs() < f32::EPSILON);
        assert_eq!(
            hits[0].vector,
            Some(PlanVectorStruct::Single(qql_plan::PlanVectorValue::Dense(
                vec![0.5, 0.25]
            )))
        );
        assert_eq!(response.telemetry.unwrap().time_s, Some(0.125));
    }

    #[test]
    fn get_points_parses_bare_record_array() {
        let op = planned("QUERY POINTS (1) FROM docs");
        let response = parse_planned(
            &op,
            json!({"result": [{"id": 1, "payload": {"text": "a"}}], "status": "ok"}),
        )
        .expect("strict parse");
        let hits = response.data.hits().expect("hits");
        assert_eq!(hits[0].id, PlanPointId::Number(1));
        assert_eq!(hits[0].score, 0.0);
        assert!(response.telemetry.is_none());
    }

    #[test]
    fn missing_or_mistyped_shapes_fail_closed() {
        let scroll = planned("SCROLL FROM docs LIMIT 1");
        // Missing result.points.
        let err = parse_planned(&scroll, json!({"result": {}, "status": "ok"})).unwrap_err();
        assert_eq!(err.code, "QQL-BACKEND-ENVELOPE");
        // Missing count (no zero fallback).
        let err = parse_planned(
            &planned("COUNT FROM docs"),
            json!({"result": {}, "status": "ok"}),
        )
        .unwrap_err();
        assert_eq!(err.code, "QQL-BACKEND-ENVELOPE");
        // Fabricated point id.
        let err = parse_planned(
            &scroll,
            json!({"result": {"points": [{"score": 1.0}]}, "status": "ok"}),
        )
        .unwrap_err();
        assert_eq!(err.code, "QQL-BACKEND-ENVELOPE");
        // Unmodelled vector shape.
        let err = parse_planned(
            &scroll,
            json!({"result": {"points": [{"id": 1, "vector": {"text": "x"}}]}, "status": "ok"}),
        )
        .unwrap_err();
        assert_eq!(err.code, "QQL-BACKEND-ENVELOPE");
    }

    #[test]
    fn parses_count_facet_and_groups() {
        let count = parse_planned(
            &planned("COUNT FROM docs"),
            json!({"result": {"count": 7}, "status": "ok"}),
        )
        .unwrap();
        assert_eq!(count.data.count(), Some(7));

        let facet = parse_planned(
            &planned("FACET city FROM docs LIMIT 10"),
            json!({"result": {"hits": [
                {"value": "NYC", "count": 2},
                {"value": 7, "count": 1},
                {"value": true, "count": 3},
            ]}, "status": "ok"}),
        )
        .unwrap();
        assert_eq!(
            facet.data.facet().unwrap(),
            [
                FacetHit {
                    value: PlanFacetValue::Keyword("NYC".into()),
                    count: 2
                },
                FacetHit {
                    value: PlanFacetValue::Integer(7),
                    count: 1
                },
                FacetHit {
                    value: PlanFacetValue::Bool(true),
                    count: 3
                },
            ]
        );

        let groups = parse_planned(
            &planned("QUERY TEXT 'x' MODEL 'm' FROM docs USING dense GROUP BY category LIMIT 3"),
            json!({"result": {"groups": [{"id": -3, "hits": [{"id": 1}]}]}, "status": "ok"}),
        )
        .unwrap();
        let groups = groups.data.groups().unwrap();
        assert_eq!(groups[0].group_id, PlanGroupId::Signed(-3));
        assert_eq!(groups[0].hits.len(), 1);
    }

    #[test]
    fn parses_collections_collection_shard_keys_and_quotas() {
        let collections = parse_planned(
            &PlannedOperation::ListCollections,
            json!({"result": {"collections": [{"name": "alpha"}, {"name": "beta"}]}, "status": "ok"}),
        )
        .unwrap();
        assert_eq!(
            collections.data.collections().unwrap(),
            ["alpha".to_string(), "beta".to_string()]
        );

        let info = parse_planned(
            &planned("SHOW COLLECTION docs"),
            json!({
                "result": {
                    "status": "green",
                    "points_count": 12,
                    "segments_count": 2,
                    "config": {"params": {"vectors": {"size": 3, "distance": "Cosine"}}},
                    "payload_schema": {},
                },
                "status": "ok",
            }),
        )
        .unwrap();
        let info = info.data.collection().expect("collection info");
        assert_eq!(info.status, "green");
        assert_eq!(info.points_count, 12);
        assert_eq!(info.segments_count, 2);
        assert_eq!(info.schema.vectors.len(), 1);

        let keys = parse_planned(
            &planned("SHOW SHARD KEYS ON COLLECTION docs"),
            json!({"result": {"shard_keys": [{"key": "acme"}, {"key": 101}]}, "status": "ok"}),
        )
        .unwrap();
        assert_eq!(
            keys.data.shard_keys().unwrap(),
            [
                PlanShardKey::Keyword("acme".into()),
                PlanShardKey::Number(101)
            ]
        );

        // Null shard_keys (non-custom sharding) is an empty list.
        let none = parse_planned(
            &planned("SHOW SHARD KEYS ON COLLECTION docs"),
            json!({"result": {"shard_keys": null}, "status": "ok"}),
        )
        .unwrap();
        assert_eq!(none.data.shard_keys().unwrap(), []);

        let quotas = parse_planned(
            &PlannedOperation::GetQuotas,
            json!({
                "result": {"config": {"enabled": true, "max_resident_memory_percent": 80}},
                "status": "ok",
            }),
        )
        .unwrap();
        let config = quotas.data.quotas().expect("quota config");
        assert_eq!(config.enabled, Some(true));
        assert_eq!(config.max_resident_memory_percent, Some(80));
    }

    #[test]
    fn parses_set_quotas_result_and_query_batch_items() {
        let op = planned("SET QUOTA (enabled = true)");
        let response = parse_planned(&op, json!({"result": true, "status": "ok"})).unwrap();
        assert_eq!(response.data.quotas().unwrap().enabled, Some(true));

        let err = parse_planned(&op, json!({"result": {"ok": true}, "status": "ok"})).unwrap_err();
        assert_eq!(err.code, "QQL-BACKEND-ENVELOPE");

        let items = parse_query_batch(&json!({
            "result": [
                {"points": [{"id": 1, "score": 0.9}, {"id": 2, "score": 0.8}]},
                {"points": [{"id": 3, "score": 0.7}]},
            ],
            "status": "ok",
        }))
        .unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].data.hits().unwrap().len(), 2);
        assert_eq!(items[1].data.hits().unwrap()[0].id, PlanPointId::Number(3));

        // Per-item failures surface as a batch error.
        let err = parse_query_batch(&json!({
            "result": [{"status": "error", "error": "point 42 not found"}],
            "status": "ok",
        }))
        .unwrap_err();
        assert_eq!(err.code, "QQL-BACKEND-BATCH");
    }
}
