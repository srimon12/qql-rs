//! Strict REST response parsing and canonical shaping for the WASM JS boundary.
//!
//! WASM cannot depend on `qql-runtime`, so this module re-shapes Qdrant REST
//! results into the same JSON that the runtime's closed `ExecData` enum
//! serializes. Extraction mirrors `qql-runtime/src/rest_response.rs`: one
//! OpenAPI shape per operation, and a missing or mistyped field fails closed
//! with `QQL-BACKEND-ENVELOPE`. There are no envelope fallback chains, no
//! shape detection, and no synthesized fields (in particular, hits carry no
//! `text`). Server telemetry stays optional and lenient by contract.
//!
//! `qql-runtime/src/rest_response.rs` is the canonical parser: mirror every
//! shape change there here so the two never diverge.

use serde::Serialize;
use serde_json::{Map, Value};

use qql_core::error::QqlError;
use qql_plan::{
    PlanFacetValue, PlanGroupId, PlanPointId, PlanShardKey, PlanVectorStruct, PlannedOperation,
    QuotaConfig,
};

use super::report::exec_response_with_telemetry;
use super::schema::parse_collection_info;
use super::telemetry::{ServerTelemetry, telemetry_from_envelope};

/// Fail-closed envelope error, mirroring the runtime's `QQL-BACKEND-ENVELOPE`.
pub(crate) fn envelope_err(message: impl Into<String>) -> QqlError {
    QqlError::backend("QQL-BACKEND-ENVELOPE", message.into(), None)
}

/// One canonical hit: `{id, score, payload, vector?}` — byte-compatible with
/// the runtime's `SearchHit` serialization. IDs keep their JSON type (number
/// or string); `score` rounds through f32 and renders as the shortest
/// round-trip decimal (`0.95`, not `0.949999988079071`).
#[derive(Debug, Serialize)]
struct WasmHit {
    id: PlanPointId,
    #[serde(serialize_with = "serialize_score_f32")]
    score: f32,
    payload: Option<Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    vector: Option<PlanVectorStruct>,
}

/// Serialize an f32 with its shortest round-trip decimal, matching the
/// runtime's `SearchHit`.
fn serialize_score_f32<S: serde::Serializer>(
    score: &f32,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match score.to_string().parse::<f64>() {
        Ok(value) => serializer.serialize_f64(value),
        Err(_) => serializer.serialize_f32(*score),
    }
}

/// Grouped query result, serialized as Qdrant's `{"id": …, "hits": […]}`.
#[derive(Debug, Serialize)]
struct WasmGroup {
    id: PlanGroupId,
    hits: Vec<WasmHit>,
}

/// One facet entry, serialized as `{"value": …, "count": n}`.
#[derive(Debug, Serialize)]
struct WasmFacetHit {
    value: PlanFacetValue,
    count: u64,
}

fn to_json<T: Serialize>(value: &T) -> Result<Value, QqlError> {
    serde_json::to_value(value).map_err(|error| {
        QqlError::execution(
            "QQL-SERIALIZE",
            format!("response shaping failed: {error}"),
            None,
        )
    })
}

/// Parse one point record into a canonical hit. `score` defaults to `0.0` for
/// unscored retrieve / scroll records, matching the typed gRPC/edge paths.
fn parse_hit(value: &Value) -> Result<WasmHit, QqlError> {
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
        Some(Value::Object(map)) => Some(map.clone()),
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
    Ok(WasmHit {
        id,
        score,
        payload,
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

/// `result.points` — the `/points/query` and `/points/scroll` envelope.
fn parse_points(envelope: &Value) -> Result<Vec<WasmHit>, QqlError> {
    let points = envelope
        .get("result")
        .and_then(|result| result.get("points"))
        .and_then(Value::as_array)
        .ok_or_else(|| envelope_err("query/scroll response is missing result.points"))?;
    points.iter().map(parse_hit).collect()
}

/// `result` — the bare array envelope of `POST /points` (retrieve).
fn parse_bare_records(envelope: &Value) -> Result<Vec<WasmHit>, QqlError> {
    let records = envelope
        .get("result")
        .and_then(Value::as_array)
        .ok_or_else(|| envelope_err("get points response is missing result as an array"))?;
    records.iter().map(parse_hit).collect()
}

/// `result.groups` — the grouped query envelope.
fn parse_groups(envelope: &Value) -> Result<Vec<WasmGroup>, QqlError> {
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
            Ok(WasmGroup { id: group_id, hits })
        })
        .collect()
}

/// `result.count` — the count envelope.
fn parse_count(envelope: &Value) -> Result<u64, QqlError> {
    envelope
        .get("result")
        .and_then(|result| result.get("count"))
        .and_then(Value::as_u64)
        .ok_or_else(|| envelope_err("count response is missing an unsigned result.count"))
}

/// `result.hits[*]` — the facet envelope, strict `{value, count}`.
fn parse_facet(envelope: &Value) -> Result<Vec<WasmFacetHit>, QqlError> {
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
            Ok(WasmFacetHit { value, count })
        })
        .collect()
}

/// `result.collections[*].name` — canonical collection-name strings.
fn parse_collection_names(envelope: &Value) -> Result<Vec<String>, QqlError> {
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

/// `result.shard_keys` — canonical `PlanShardKey` values. Nullable when the
/// collection does not use custom sharding.
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

/// `result.config` — canonical `QuotaConfig`.
fn parse_quotas(envelope: &Value) -> Result<QuotaConfig, QqlError> {
    let config = envelope
        .get("result")
        .and_then(|result| result.get("config"))
        .ok_or_else(|| envelope_err("get quotas response is missing result.config"))?;
    serde_json::from_value(config.clone())
        .map_err(|error| envelope_err(format!("quota config is invalid: {error}")))
}

/// Parse one strict `/points/query/batch` item: each OpenAPI `QueryResponse`
/// item carries its points at the item's top level (`{"points": […]}`).
pub(crate) fn parse_query_batch(
    envelope: &Value,
) -> Result<Vec<(Value, Option<ServerTelemetry>)>, QqlError> {
    let items = envelope
        .get("result")
        .and_then(Value::as_array)
        .ok_or_else(|| envelope_err("query batch response is missing result as an array"))?;
    items
        .iter()
        .map(|item| {
            if let Some(message) = qql_plan::batch_item_error(item) {
                return Err(QqlError::backend("QQL-BACKEND-BATCH", message, None));
            }
            let points = item
                .get("points")
                .and_then(Value::as_array)
                .ok_or_else(|| envelope_err("query batch item is missing points as an array"))?;
            let hits = points
                .iter()
                .map(parse_hit)
                .collect::<Result<Vec<_>, _>>()?;
            Ok((to_json(&hits)?, telemetry_from_envelope(item)))
        })
        .collect()
}

/// Parse a strict `/points/batch` (update) envelope and return the item count.
/// Per-item `UpdateResult`s are status-only, so only `status: "error"` items
/// fail (`QQL-BACKEND-BATCH`), mirroring the runtime.
pub(crate) fn parse_update_batch(envelope: &Value) -> Result<usize, QqlError> {
    let items = envelope
        .get("result")
        .and_then(Value::as_array)
        .ok_or_else(|| envelope_err("update batch response is missing result as an array"))?;
    items
        .iter()
        .try_for_each(|item| match qql_plan::batch_item_error(item) {
            Some(message) => Err(QqlError::backend("QQL-BACKEND-BATCH", message, None)),
            None => Ok(()),
        })?;
    Ok(items.len())
}

/// Build the canonical `ExecResponse` JSON for one planned operation.
///
/// Every read operation parses exactly its OpenAPI response shape; a missing
/// or mistyped field returns `QQL-BACKEND-ENVELOPE`. Writes carry no typed
/// response body (the executor owns upsert counts), matching the runtime.
pub(crate) fn wasm_success_response(
    operation: &PlannedOperation,
    result: &Value,
) -> Result<Value, QqlError> {
    let telemetry = telemetry_from_envelope(result);
    let label = operation.operation_label();
    let (message, data) = match operation {
        PlannedOperation::Query { .. } | PlannedOperation::Scroll { .. } => {
            let hits = parse_points(result)?;
            let count = hits.len();
            (format!("Found {count} hits"), Some(to_json(&hits)?))
        }
        PlannedOperation::GetPoints { .. } => {
            let hits = parse_bare_records(result)?;
            let count = hits.len();
            (format!("Found {count} hits"), Some(to_json(&hits)?))
        }
        PlannedOperation::QueryGroups { request, .. } => {
            let mut groups = parse_groups(result)?;
            // `group_offset` has no wire representation; trim client-side,
            // exactly like the runtime's normalization.
            if let Some(offset) = request.group_offset {
                let offset = offset as usize;
                if offset < groups.len() {
                    groups.drain(0..offset);
                } else {
                    groups.clear();
                }
            }
            let count = groups.len();
            let mut payload = Map::new();
            payload.insert("groups".into(), to_json(&groups)?);
            (
                format!("Found {count} group(s)"),
                Some(Value::Object(payload)),
            )
        }
        PlannedOperation::Count { .. } => {
            let count = parse_count(result)?;
            let mut payload = Map::new();
            payload.insert("count".into(), Value::from(count));
            (format!("Count: {count}"), Some(Value::Object(payload)))
        }
        PlannedOperation::Facet { .. } => {
            let hits = parse_facet(result)?;
            let count = hits.len();
            (format!("Found {count} facet hit(s)"), Some(to_json(&hits)?))
        }
        PlannedOperation::Upsert { request, .. } => {
            let count = request.points.len();
            let mut payload = Map::new();
            payload.insert("count".into(), Value::from(count as u64));
            (
                format!("Upserted {count} point(s)"),
                Some(Value::Object(payload)),
            )
        }
        PlannedOperation::ListCollections => {
            let names = parse_collection_names(result)?;
            let count = names.len();
            let mut payload = Map::new();
            payload.insert("collections".into(), Value::from(names));
            (
                format!("Found {count} collection(s)"),
                Some(Value::Object(payload)),
            )
        }
        PlannedOperation::GetCollection { .. } => {
            let info = parse_collection_info(result)?;
            (format!("{label} ok"), Some(info))
        }
        PlannedOperation::ListShardKeys { .. } => {
            let keys = parse_shard_keys(result)?;
            let mut payload = Map::new();
            payload.insert("shard_keys".into(), to_json(&keys)?);
            (
                "Shard keys listed".to_string(),
                Some(Value::Object(payload)),
            )
        }
        PlannedOperation::GetQuotas => {
            let config = parse_quotas(result)?;
            (
                "Quota configuration shown".to_string(),
                Some(to_json(&config)?),
            )
        }
        PlannedOperation::SetQuotas { request } => {
            // `PUT /quotas` answers a boolean status; the typed result is the
            // replacement config the caller sent, mirroring the runtime.
            match result.get("result") {
                Some(Value::Bool(_)) => (
                    "Quota configuration updated".to_string(),
                    Some(to_json(&request.config)?),
                ),
                _ => {
                    return Err(envelope_err(
                        "set quotas response is missing the boolean result field",
                    ));
                }
            }
        }
        PlannedOperation::CrossRerank { .. } => {
            return Err(QqlError::execution(
                "QQL-REST-CLIENT-SIDE",
                "CROSS RERANK is client-side and has no REST response",
                None,
            ));
        }
        _ => (format!("{label} ok"), None),
    };
    Ok(exec_response_with_telemetry(
        true, label, &message, data, telemetry,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn planned(sql: &str) -> PlannedOperation {
        qql_plan::plan(&qql_core::parser::Parser::parse(sql).expect("parse")).expect("plan")
    }

    #[test]
    fn parses_scored_points_strictly_into_runtime_shape() {
        let response = wasm_success_response(
            &planned("SCROLL FROM docs LIMIT 1"),
            &json!({
                "result": {"points": [{
                    "id": 7,
                    "score": 0.949999988079071,
                    "payload": {"title": "a"},
                    "vector": [0.5, 0.25],
                }]},
                "status": "ok",
                "time": 0.125,
            }),
        )
        .expect("strict parse");
        assert_eq!(response["ok"], true);
        assert_eq!(response["operation"], "SCROLL");
        assert_eq!(response["message"], "Found 1 hits");
        assert_eq!(
            response["data"],
            json!([{
                "id": 7,
                "score": 0.95,
                "payload": {"title": "a"},
                "vector": [0.5, 0.25],
            }])
        );
        assert_eq!(response["telemetry"]["time_s"], 0.125);
    }

    #[test]
    fn unscored_records_default_zero_without_fabricated_fields() {
        let response = wasm_success_response(
            &planned("QUERY POINTS (1) FROM docs"),
            &json!({"result": [{"id": "uuid-1"}], "status": "ok"}),
        )
        .expect("strict parse");
        assert_eq!(response["operation"], "GET_POINTS");
        assert_eq!(
            response["data"],
            json!([{"id": "uuid-1", "score": 0.0, "payload": null}])
        );
        assert!(response["data"][0].get("text").is_none());
    }

    #[test]
    fn malformed_read_shapes_fail_closed() {
        let scroll = planned("SCROLL FROM docs LIMIT 1");
        for envelope in [
            json!({"result": {}, "status": "ok"}),
            json!({"result": {"points": [{"score": 1.0}]}, "status": "ok"}),
            json!({"result": {"points": [{"id": -1}]}, "status": "ok"}),
            json!({"result": {"points": [{"id": 1, "score": "high"}]}, "status": "ok"}),
            json!({"result": {"points": [{"id": 1, "payload": 4}]}, "status": "ok"}),
            json!({"result": {"points": [{"id": 1, "vector": {"text": "x"}}]}, "status": "ok"}),
        ] {
            let err = wasm_success_response(&scroll, &envelope).unwrap_err();
            assert_eq!(err.code, "QQL-BACKEND-ENVELOPE");
        }

        let err = wasm_success_response(
            &planned("COUNT FROM docs"),
            &json!({"result": {}, "status": "ok"}),
        )
        .unwrap_err();
        assert_eq!(err.code, "QQL-BACKEND-ENVELOPE");

        let err = wasm_success_response(
            &planned("FACET city FROM docs LIMIT 10"),
            &json!({"result": {"hits": [{"value": {"nested": 1}, "count": 2}]}, "status": "ok"}),
        )
        .unwrap_err();
        assert_eq!(err.code, "QQL-BACKEND-ENVELOPE");

        let err = wasm_success_response(
            &PlannedOperation::ListCollections,
            &json!({"result": {"collections": [{"name": 3}]}, "status": "ok"}),
        )
        .unwrap_err();
        assert_eq!(err.code, "QQL-BACKEND-ENVELOPE");

        let err = wasm_success_response(
            &PlannedOperation::GetQuotas,
            &json!({"result": {"config": {"enabled": "yes"}}, "status": "ok"}),
        )
        .unwrap_err();
        assert_eq!(err.code, "QQL-BACKEND-ENVELOPE");
    }

    #[test]
    fn parses_count_facet_and_groups_canonically() {
        let count = wasm_success_response(
            &planned("COUNT FROM docs"),
            &json!({"result": {"count": 7}, "status": "ok"}),
        )
        .unwrap();
        assert_eq!(count["data"], json!({"count": 7}));
        assert_eq!(count["message"], "Count: 7");

        let facet = wasm_success_response(
            &planned("FACET city FROM docs LIMIT 10"),
            &json!({"result": {"hits": [
                {"value": "NYC", "count": 2},
                {"value": 7, "count": 1},
            ]}, "status": "ok"}),
        )
        .unwrap();
        assert_eq!(
            facet["data"],
            json!([{"value": "NYC", "count": 2}, {"value": 7, "count": 1}])
        );

        let groups = wasm_success_response(
            &planned("QUERY TEXT 'x' MODEL 'm' FROM docs USING dense GROUP BY category LIMIT 3"),
            &json!({"result": {"groups": [
                {"id": -3, "hits": [{"id": 1, "score": 0.5}]},
                {"id": "b", "hits": []},
            ]}, "status": "ok"}),
        )
        .unwrap();
        assert_eq!(groups["operation"], "QUERY_GROUPS");
        assert_eq!(groups["message"], "Found 2 group(s)");
        assert_eq!(
            groups["data"],
            json!({"groups": [
                {"id": -3, "hits": [{"id": 1, "score": 0.5, "payload": null}]},
                {"id": "b", "hits": []},
            ]})
        );
        assert_eq!(groups["data"].as_object().unwrap().len(), 1);
    }

    #[test]
    fn group_offset_trims_like_the_runtime() {
        let groups = wasm_success_response(
            &planned(
                "QUERY TEXT 'x' MODEL 'm' FROM docs USING dense GROUP BY category LIMIT 3 OFFSET 1",
            ),
            &json!({"result": {"groups": [
                {"id": 1, "hits": []},
                {"id": 2, "hits": []},
            ]}, "status": "ok"}),
        )
        .unwrap();
        assert_eq!(groups["data"], json!({"groups": [{"id": 2, "hits": []}]}));
    }

    #[test]
    fn parses_collections_shard_keys_and_quotas() {
        let collections = wasm_success_response(
            &PlannedOperation::ListCollections,
            &json!({"result": {"collections": [{"name": "alpha"}, {"name": "beta"}]}, "status": "ok"}),
        )
        .unwrap();
        assert_eq!(
            collections["data"],
            json!({"collections": ["alpha", "beta"]})
        );

        let keys = wasm_success_response(
            &planned("SHOW SHARD KEYS ON COLLECTION docs"),
            &json!({"result": {"shard_keys": [{"key": "acme"}, {"key": 101}]}, "status": "ok"}),
        )
        .unwrap();
        assert_eq!(keys["data"], json!({"shard_keys": ["acme", 101]}));

        let none = wasm_success_response(
            &planned("SHOW SHARD KEYS ON COLLECTION docs"),
            &json!({"result": {"shard_keys": null}, "status": "ok"}),
        )
        .unwrap();
        assert_eq!(none["data"], json!({"shard_keys": []}));

        let quotas = wasm_success_response(
            &PlannedOperation::GetQuotas,
            &json!({
                "result": {"config": {"enabled": true, "max_resident_memory_percent": 80}},
                "status": "ok",
            }),
        )
        .unwrap();
        assert_eq!(
            quotas["data"],
            json!({"enabled": true, "max_resident_memory_percent": 80})
        );
    }

    #[test]
    fn set_quotas_requires_boolean_result() {
        let ok = wasm_success_response(
            &planned("SET QUOTA (enabled = true, max_resident_memory_percent = 80)"),
            &json!({"result": true, "status": "ok"}),
        )
        .unwrap();
        assert_eq!(
            ok["data"],
            json!({"enabled": true, "max_resident_memory_percent": 80})
        );

        let err = wasm_success_response(
            &planned("SET QUOTA (enabled = true)"),
            &json!({"result": {"ok": true}, "status": "ok"}),
        )
        .unwrap_err();
        assert_eq!(err.code, "QQL-BACKEND-ENVELOPE");
    }

    #[test]
    fn upsert_reports_request_point_count_and_writes_are_status_only() {
        let upsert = wasm_success_response(
            &planned("UPSERT INTO docs VALUES {id: 1, vector: [0.1, 0.2]}"),
            &json!({"result": {"status": "completed", "operation_id": 0}, "status": "ok"}),
        )
        .unwrap();
        assert_eq!(upsert["operation"], "UPSERT");
        assert_eq!(upsert["message"], "Upserted 1 point(s)");
        assert_eq!(upsert["data"], json!({"count": 1}));

        let delete = wasm_success_response(
            &planned("DELETE FROM docs WHERE id = 1"),
            &json!({"result": {"status": "completed", "operation_id": 1}, "status": "ok"}),
        )
        .unwrap();
        assert_eq!(delete["data"], Value::Null);
    }

    #[test]
    fn batch_items_are_strict_and_carry_telemetry() {
        let parsed = parse_query_batch(&json!({
            "result": [
                {"points": [{"id": 1, "score": 0.9}], "time": 0.5},
                {"points": []},
            ],
            "status": "ok",
        }))
        .unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(
            parsed[0].0,
            json!([{"id": 1, "score": 0.9, "payload": null}])
        );
        assert_eq!(parsed[0].1.as_ref().unwrap().time_s, Some(0.5));

        let err =
            parse_query_batch(&json!({"result": [{"points": 1}], "status": "ok"})).unwrap_err();
        assert_eq!(err.code, "QQL-BACKEND-ENVELOPE");

        let err = parse_query_batch(&json!({
            "result": [{"status": "error", "error": "point 42 not found"}],
            "status": "ok",
        }))
        .unwrap_err();
        assert_eq!(err.code, "QQL-BACKEND-BATCH");

        assert_eq!(
            parse_update_batch(&json!({"result": [{"status": "completed"}, {"status": "acknowledged"}], "status": "ok"}))
                .unwrap(),
            2
        );
        let err = parse_update_batch(&json!({"result": {"status": "completed"}, "status": "ok"}))
            .unwrap_err();
        assert_eq!(err.code, "QQL-BACKEND-ENVELOPE");
    }
}
