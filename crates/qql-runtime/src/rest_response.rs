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
    BackendResponse, ExecData, FacetHit, GroupedSearchResult, SearchHit, score_f64,
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
    // Telemetry only (lenient); the data path below is strict.
    let telemetry = ServerTelemetry::from_envelope_opt(&envelope);
    let data = match op {
        PlannedOperation::Query { .. } | PlannedOperation::Scroll { .. } => {
            ExecData::Hits(parse_points(envelope)?)
        }
        PlannedOperation::GetPoints { .. } => ExecData::Hits(parse_bare_records(envelope)?),
        PlannedOperation::QueryGroups { .. } => ExecData::Groups(parse_groups(envelope)?),
        PlannedOperation::Count { .. } => ExecData::Count(parse_count(envelope)?),
        PlannedOperation::Facet { .. } => ExecData::Facet(parse_facet(envelope)?),
        PlannedOperation::ListCollections => {
            ExecData::Collections(parse_collection_names(envelope)?)
        }
        PlannedOperation::GetCollection { .. } => {
            ExecData::Collection(parse_collection_info(envelope)?)
        }
        PlannedOperation::ListShardKeys { .. } => ExecData::ShardKeys(parse_shard_keys(envelope)?),
        PlannedOperation::GetQuotas => ExecData::Quotas(parse_quotas(envelope)?),
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
        | PlannedOperation::OverwritePayload { .. }
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
        PlannedOperation::Batch { .. } => {
            return Err(QqlError::execution(
                "QQL-REST-BATCH",
                "BATCH is dispatched by the Executor through the batch RPCs, not as a single REST route",
                None,
            ));
        }
    };
    Ok(BackendResponse { data, telemetry })
}

/// Parse one `/points/query/batch` result array: each OpenAPI `QueryResponse`
/// item carries its points at the item's top level (`{"points": […]}`).
///
/// Takes the `result` array value itself (the transport envelope is validated
/// and unwrapped by the caller), consuming items without cloning.
pub(crate) fn parse_query_batch(result: Value) -> Result<Vec<BackendResponse>, QqlError> {
    let items = match result {
        Value::Array(items) => items,
        _ => {
            return Err(envelope_err(
                "query batch response is missing result as an array",
            ));
        }
    };
    items
        .into_iter()
        .map(|item| {
            ensure_batch_item_ok(&item)?;
            // Telemetry reads borrowed fields; the points below move out.
            let telemetry = ServerTelemetry::from_envelope_opt(&item);
            let mut map = match item {
                Value::Object(map) => map,
                _ => {
                    return Err(envelope_err(
                        "query batch item is missing points as an array",
                    ));
                }
            };
            let points = match map.remove("points") {
                Some(Value::Array(points)) => points,
                _ => {
                    return Err(envelope_err(
                        "query batch item is missing points as an array",
                    ));
                }
            };
            let hits = points
                .into_iter()
                .map(parse_hit)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(BackendResponse {
                data: ExecData::Hits(hits),
                // Telemetry only (lenient); the hits above are strict.
                telemetry,
            })
        })
        .collect()
}

/// Parse one `/points/batch` result array. Per-item `UpdateResult`s are
/// status-only: the executor derives upsert counts from the request.
pub(crate) fn parse_update_batch(result: Value) -> Result<Vec<BackendResponse>, QqlError> {
    let items = match result {
        Value::Array(items) => items,
        _ => {
            return Err(envelope_err(
                "update batch response is missing result as an array",
            ));
        }
    };
    items
        .into_iter()
        .map(|item| {
            ensure_batch_item_ok(&item)?;
            Ok(BackendResponse {
                data: ExecData::Mutation { affected: None },
                telemetry: None,
            })
        })
        .collect()
}

/// Parse `GET /collections` (`result.collections[*].name`).
pub(crate) fn parse_collection_names(envelope: Value) -> Result<Vec<String>, QqlError> {
    let missing = || envelope_err("list collections response is missing result.collections");
    let mut envelope = match envelope {
        Value::Object(map) => map,
        _ => return Err(missing()),
    };
    let mut result = match envelope.remove("result") {
        Some(Value::Object(map)) => map,
        _ => return Err(missing()),
    };
    let collections = match result.remove("collections") {
        Some(Value::Array(items)) => items,
        _ => return Err(missing()),
    };
    collections
        .into_iter()
        .map(|entry| match entry {
            Value::Object(mut map) => match map.remove("name") {
                Some(Value::String(name)) => Ok(name),
                _ => Err(envelope_err("collection entry is missing a string name")),
            },
            _ => Err(envelope_err("collection entry is missing a string name")),
        })
        .collect()
}

/// Parse `GET /collections/{name}` (`result` → [`CollectionInfo`]).
pub(crate) fn parse_collection_info(envelope: Value) -> Result<CollectionInfo, QqlError> {
    let result = match envelope {
        Value::Object(mut map) => match map.remove("result") {
            Some(Value::Object(result)) => result,
            _ => {
                return Err(envelope_err(
                    "get collection response is missing a result object",
                ));
            }
        },
        _ => {
            return Err(envelope_err(
                "get collection response is missing a result object",
            ));
        }
    };
    let status = result
        .get("status")
        .and_then(Value::as_str)
        .ok_or_else(|| envelope_err("collection info is missing a string status"))?
        .to_string();
    let segments_count = result
        .get("segments_count")
        .and_then(Value::as_u64)
        .ok_or_else(|| envelope_err("collection info is missing an unsigned segments_count"))?;
    // Re-wrap for the shared schema reader (borrows; no field clones).
    let result = Value::Object(result);
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
        schema: crate::backend::schema_from_rest_result(&result),
    })
}

/// Parse a query / scroll response (`result.points` → scored hits).
fn parse_points(envelope: Value) -> Result<Vec<SearchHit>, QqlError> {
    let mut envelope = match envelope {
        Value::Object(map) => map,
        _ => {
            return Err(envelope_err(
                "query/scroll response is missing result.points",
            ));
        }
    };
    let mut result = match envelope.remove("result") {
        Some(Value::Object(map)) => map,
        _ => {
            return Err(envelope_err(
                "query/scroll response is missing result.points",
            ));
        }
    };
    match result.remove("points") {
        Some(Value::Array(points)) => points.into_iter().map(parse_hit).collect(),
        _ => Err(envelope_err(
            "query/scroll response is missing result.points",
        )),
    }
}

/// Parse a get-points response: `result` is a bare array of point records.
fn parse_bare_records(envelope: Value) -> Result<Vec<SearchHit>, QqlError> {
    let mut envelope = match envelope {
        Value::Object(map) => map,
        _ => {
            return Err(envelope_err(
                "get points response is missing result as an array",
            ));
        }
    };
    match envelope.remove("result") {
        Some(Value::Array(records)) => records.into_iter().map(parse_hit).collect(),
        _ => Err(envelope_err(
            "get points response is missing result as an array",
        )),
    }
}

/// Parse one OpenAPI `ScoredPoint` / `Record` into a [`SearchHit`]. `score`
/// defaults to `0.0` for unscored retrieve/scroll records, matching the typed
/// gRPC/edge paths. Takes the record by value: ids, payload maps, and vectors
/// move out instead of cloning per field.
fn parse_hit(value: Value) -> Result<SearchHit, QqlError> {
    let mut record = match value {
        Value::Object(map) => map,
        _ => return Err(envelope_err("point record is not an object")),
    };
    let id = parse_point_id(
        record
            .remove("id")
            .ok_or_else(|| envelope_err("point record is missing id"))?,
    )?;
    let score = match record.remove("score") {
        None | Some(Value::Null) => 0.0,
        Some(Value::Number(number)) => score_f64(
            number
                .as_f64()
                .ok_or_else(|| envelope_err("point score is not a finite number"))?
                as f32,
        ),
        Some(other) => {
            return Err(envelope_err(format!(
                "point score must be a number, got {other}"
            )));
        }
    };
    let payload = match record.remove("payload") {
        None | Some(Value::Null) => None,
        Some(Value::Object(map)) => Some(map.into_iter().collect::<HashMap<_, _>>()),
        Some(other) => {
            return Err(envelope_err(format!(
                "point payload must be an object or null, got {other}"
            )));
        }
    };
    let vector = match record.remove("vector") {
        None | Some(Value::Null) => None,
        Some(vector) => Some(serde_json::from_value::<PlanVectorStruct>(vector).map_err(
            |error| {
                envelope_err(format!(
                    "point vector does not match VectorStructOutput: {error}"
                ))
            },
        )?),
    };
    Ok(SearchHit {
        id,
        score,
        payload,
        collection: None,
        vector,
    })
}

fn parse_point_id(value: Value) -> Result<PlanPointId, QqlError> {
    match value {
        Value::Number(number) => number
            .as_u64()
            .map(PlanPointId::Number)
            .ok_or_else(|| envelope_err(format!("point id {number} is not an unsigned integer"))),
        Value::String(id) => Ok(PlanPointId::String(id)),
        other => Err(envelope_err(format!(
            "point id must be a string or unsigned integer, got {other}"
        ))),
    }
}

/// Parse a grouped query response (`result.groups`).
fn parse_groups(envelope: Value) -> Result<Vec<GroupedSearchResult>, QqlError> {
    let mut envelope = match envelope {
        Value::Object(map) => map,
        _ => {
            return Err(envelope_err(
                "grouped query response is missing result.groups",
            ));
        }
    };
    let mut result = match envelope.remove("result") {
        Some(Value::Object(map)) => map,
        _ => {
            return Err(envelope_err(
                "grouped query response is missing result.groups",
            ));
        }
    };
    match result.remove("groups") {
        Some(Value::Array(groups)) => groups
            .into_iter()
            .map(|group| {
                let mut group = match group {
                    Value::Object(map) => map,
                    _ => return Err(envelope_err("group is missing id")),
                };
                let group_id = serde_json::from_value::<PlanGroupId>(
                    group
                        .remove("id")
                        .ok_or_else(|| envelope_err("group is missing id"))?,
                )
                .map_err(|error| {
                    envelope_err(format!("group id does not match GroupId: {error}"))
                })?;
                let hits = match group.remove("hits") {
                    Some(Value::Array(hits)) => hits,
                    _ => return Err(envelope_err("group is missing hits as an array")),
                };
                let hits = hits
                    .into_iter()
                    .map(parse_hit)
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(GroupedSearchResult { group_id, hits })
            })
            .collect(),
        _ => Err(envelope_err(
            "grouped query response is missing result.groups",
        )),
    }
}

/// Parse a count response (`result.count`).
fn parse_count(envelope: Value) -> Result<u64, QqlError> {
    envelope
        .get("result")
        .and_then(|result| result.get("count"))
        .and_then(Value::as_u64)
        .ok_or_else(|| envelope_err("count response is missing an unsigned result.count"))
}

/// Parse a facet response (`result.hits[*]` with strict `{value, count}`).
fn parse_facet(envelope: Value) -> Result<Vec<FacetHit>, QqlError> {
    let mut envelope = match envelope {
        Value::Object(map) => map,
        _ => return Err(envelope_err("facet response is missing result.hits")),
    };
    let mut result = match envelope.remove("result") {
        Some(Value::Object(map)) => map,
        _ => return Err(envelope_err("facet response is missing result.hits")),
    };
    match result.remove("hits") {
        Some(Value::Array(hits)) => hits
            .into_iter()
            .map(|hit| {
                let mut hit = match hit {
                    Value::Object(map) => map,
                    _ => return Err(envelope_err("facet hit is missing value")),
                };
                let value = serde_json::from_value::<PlanFacetValue>(
                    hit.remove("value")
                        .ok_or_else(|| envelope_err("facet hit is missing value"))?,
                )
                .map_err(|error| {
                    envelope_err(format!("facet value does not match FacetValue: {error}"))
                })?;
                let count = match hit.remove("count") {
                    Some(Value::Number(count)) => count
                        .as_u64()
                        .ok_or_else(|| envelope_err("facet hit is missing an unsigned count"))?,
                    _ => {
                        return Err(envelope_err("facet hit is missing an unsigned count"));
                    }
                };
                Ok(FacetHit { value, count })
            })
            .collect(),
        _ => Err(envelope_err("facet response is missing result.hits")),
    }
}

/// Parse `GET /collections/{name}/shards` (`result.shard_keys`, nullable when
/// the collection does not use custom sharding).
///
/// Lenient shape (unchanged): a missing/null `result` or `shard_keys` reads as
/// no custom sharding; only a present-but-mistyped value fails.
fn parse_shard_keys(envelope: Value) -> Result<Vec<PlanShardKey>, QqlError> {
    let mut envelope = match envelope {
        Value::Object(map) => map,
        _ => return Ok(Vec::new()),
    };
    let mut result = match envelope.remove("result") {
        Some(Value::Object(map)) => map,
        _ => return Ok(Vec::new()),
    };
    match result.remove("shard_keys") {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(entries)) => entries
            .into_iter()
            .map(|entry| {
                let mut entry = match entry {
                    Value::Object(map) => map,
                    _ => {
                        return Err(envelope_err("shard key entry is missing key"));
                    }
                };
                let key = entry
                    .remove("key")
                    .ok_or_else(|| envelope_err("shard key entry is missing key"))?;
                match key {
                    Value::String(keyword) => Ok(PlanShardKey::Keyword(keyword)),
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
fn parse_quotas(envelope: Value) -> Result<qql_plan::QuotaConfig, QqlError> {
    let mut envelope = match envelope {
        Value::Object(map) => map,
        _ => return Err(envelope_err("get quotas response is missing result.config")),
    };
    let mut result = match envelope.remove("result") {
        Some(Value::Object(map)) => map,
        _ => return Err(envelope_err("get quotas response is missing result.config")),
    };
    match result.remove("config") {
        Some(config) => serde_json::from_value(config)
            .map_err(|error| envelope_err(format!("quota config is invalid: {error}"))),
        None => Err(envelope_err("get quotas response is missing result.config")),
    }
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
        assert_eq!(hits[0].score, 0.75);
        assert_eq!(
            hits[0].vector,
            Some(PlanVectorStruct::Single(qql_plan::PlanVectorValue::Dense(
                vec![0.5, 0.25]
            )))
        );
        assert_eq!(response.telemetry.unwrap().time_s, Some(0.125));
    }

    #[test]
    fn parse_hit_rounds_score_once_to_shortest_f64() {
        // `0.95` arrives as a JSON decimal; ingestion stores the shortest
        // round-trip f64 exactly — no epsilon needed, and the serialized
        // report emits the same decimal.
        let response = parse_planned(
            &planned("SCROLL FROM docs LIMIT 1"),
            json!({"result": {"points": [{"id": 1, "score": 0.95}]}, "status": "ok"}),
        )
        .expect("strict parse");
        let hits = response.data.hits().expect("hits");
        assert_eq!(hits[0].score, 0.95f64);
        let value = serde_json::to_value(&hits[0]).expect("hit serializes");
        assert_eq!(value["score"], json!(0.95));
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

        let items = parse_query_batch(json!([
            {"points": [{"id": 1, "score": 0.9}, {"id": 2, "score": 0.8}]},
            {"points": [{"id": 3, "score": 0.7}]},
        ]))
        .unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].data.hits().unwrap().len(), 2);
        assert_eq!(items[1].data.hits().unwrap()[0].id, PlanPointId::Number(3));

        // Per-item failures surface as a batch error.
        let err = parse_query_batch(json!([{"status": "error", "error": "point 42 not found"}]))
            .unwrap_err();
        assert_eq!(err.code, "QQL-BACKEND-BATCH");
    }
}
