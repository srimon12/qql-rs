//! Internal execution report and shared response shaping for the WASM host.
//!
//! The report envelope matches the `qql-runtime` contract. Response parsing,
//! schema reading, telemetry extraction, and normalization all come from
//! [`qql_plan::surface`], so the browser host reports the same labels,
//! messages, and closed payloads as the native executor.

use qql_core::error::QqlError;
use qql_plan::surface::normalize::{
    normalize_planned, normalize_query_item, normalize_update_item,
};
use qql_plan::surface::rest::{parse_query_batch, parse_update_batch};
use qql_plan::surface::telemetry::ServerTelemetry;
use qql_plan::{PlannedOperation, UpdateOperation};

/// Internal execution report matching the qql-runtime contract.
/// Used so callers can access typed `succeeded`/`failed`/`results`
/// fields without chasing `serde_json::Value` keys.
#[derive(serde::Serialize)]
pub(crate) struct WasmReport {
    pub(crate) ok: bool,
    pub(crate) results: Vec<serde_json::Value>,
    pub(crate) succeeded: usize,
    pub(crate) failed: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) telemetry: Option<ServerTelemetry>,
}

impl WasmReport {
    pub(crate) fn from_results(results: Vec<serde_json::Value>) -> Self {
        let succeeded = results
            .iter()
            .filter(|r| r.get("ok").and_then(|v| v.as_bool()).unwrap_or(false))
            .count();
        let failed = results.len() - succeeded;
        let telemetry = aggregate_telemetry(&results);
        Self {
            ok: failed == 0,
            results,
            succeeded,
            failed,
            telemetry,
        }
    }

    pub(crate) fn single(resp: serde_json::Value) -> Self {
        let ok = resp.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
        let telemetry = resp
            .get("telemetry")
            .filter(|telemetry| !telemetry.is_null())
            .and_then(|telemetry| serde_json::from_value(telemetry.clone()).ok());
        Self {
            ok,
            results: vec![resp],
            succeeded: if ok { 1 } else { 0 },
            failed: if ok { 0 } else { 1 },
            telemetry,
        }
    }
}

/// Aggregate the per-response `telemetry` values of a report's results.
/// Entries without telemetry (or with `null`) are skipped.
fn aggregate_telemetry(results: &[serde_json::Value]) -> Option<ServerTelemetry> {
    let parsed: Vec<ServerTelemetry> = results
        .iter()
        .filter_map(|result| result.get("telemetry"))
        .filter(|telemetry| !telemetry.is_null())
        .filter_map(|telemetry| serde_json::from_value(telemetry.clone()).ok())
        .collect();
    ServerTelemetry::aggregate(parsed.iter())
}

/// Build an ExecResponse-compatible JSON value for failures and synthetic
/// results (the success path goes through [`shaped_success_response`]).
pub(crate) fn exec_response(
    ok: bool,
    operation: &str,
    message: &str,
    data: Option<serde_json::Value>,
) -> serde_json::Value {
    exec_response_with_telemetry(ok, operation, message, data, None)
}

/// [`exec_response`] with optional server telemetry; `None` omits the key so
/// pre-telemetry payloads stay byte-identical to the runtime's serialization.
pub(crate) fn exec_response_with_telemetry(
    ok: bool,
    operation: &str,
    message: &str,
    data: Option<serde_json::Value>,
    telemetry: Option<ServerTelemetry>,
) -> serde_json::Value {
    let mut obj = serde_json::Map::new();
    obj.insert("ok".to_string(), serde_json::Value::from(ok));
    obj.insert("operation".to_string(), serde_json::Value::from(operation));
    obj.insert("message".to_string(), serde_json::Value::from(message));
    obj.insert("data".to_string(), data.unwrap_or(serde_json::Value::Null));
    if let Some(tel) = telemetry
        && let Ok(value) = serde_json::to_value(tel)
    {
        obj.insert("telemetry".to_string(), value);
    }
    serde_json::Value::Object(obj)
}

/// Serialize a shaped value, mapping failures to the host JSON code.
fn to_json<T: serde::Serialize>(value: &T) -> Result<serde_json::Value, QqlError> {
    serde_json::to_value(value).map_err(|error| {
        QqlError::execution(
            "QQL-SERIALIZE",
            format!("response shaping failed: {error}"),
            None,
        )
    })
}

/// Parse + normalize one successful `PlannedOperation` REST envelope into the
/// canonical `ExecResponse` JSON, using the shared runtime parser.
pub(crate) fn shaped_success_response(
    operation: &PlannedOperation,
    envelope: &serde_json::Value,
) -> Result<serde_json::Value, QqlError> {
    let parsed = qql_plan::surface::rest::parse_planned(operation, envelope.clone())?;
    let normalized = normalize_planned(operation, parsed)?;
    to_json(&normalized)
}

/// The `result` member of a batch envelope, or `null` so the shared parser
/// reports the canonical missing-result error.
pub(crate) fn batch_result(envelope: &serde_json::Value) -> serde_json::Value {
    envelope
        .get("result")
        .cloned()
        .unwrap_or(serde_json::Value::Null)
}

/// Canonical per-item `ExecResponse` JSON for a `/points/query/batch` envelope.
pub(crate) fn shaped_query_batch(
    envelope: &serde_json::Value,
) -> Result<Vec<serde_json::Value>, QqlError> {
    parsed_query_batch(envelope)?
        .into_iter()
        .map(shaped_query_response)
        .collect()
}

/// Canonical `ExecResponse` JSON for one parsed query-batch member.
pub(crate) fn shaped_query_response(
    response: qql_plan::surface::response::BackendResponse,
) -> Result<serde_json::Value, QqlError> {
    normalize_query_item(response).and_then(|exec| to_json(&exec))
}

/// Parse a `/points/query/batch` envelope into typed responses.
pub(crate) fn parsed_query_batch(
    envelope: &serde_json::Value,
) -> Result<Vec<qql_plan::surface::response::BackendResponse>, QqlError> {
    parse_query_batch(batch_result(envelope))
}

/// Parse a `/points/batch` envelope into typed responses.
pub(crate) fn parsed_update_batch(
    envelope: &serde_json::Value,
) -> Result<Vec<qql_plan::surface::response::BackendResponse>, QqlError> {
    parse_update_batch(batch_result(envelope))
}

/// Canonical per-item `ExecResponse` JSON for a `/points/batch` envelope.
///
/// Items arrive in request order, so `operations` aligns with the parsed
/// responses one-to-one.
pub(crate) fn shaped_update_batch(
    envelope: &serde_json::Value,
    operations: &[PlannedOperation],
) -> Result<Vec<serde_json::Value>, QqlError> {
    operations
        .iter()
        .zip(parsed_update_batch(envelope)?)
        .map(|(operation, response)| {
            let (_, update) = planned_to_update_operation(operation)?;
            shaped_update_response(&update, response)
        })
        .collect()
}

/// Canonical `ExecResponse` JSON for one typed update-batch member (the flush
/// path already holds `UpdateOperation`s).
pub(crate) fn shaped_update_response(
    update: &UpdateOperation,
    response: qql_plan::surface::response::BackendResponse,
) -> Result<serde_json::Value, QqlError> {
    normalize_update_item(update, response).and_then(|exec| to_json(&exec))
}

/// Typed update lowering for one planned mutation, failing like the executor
/// when a non-mutation slipped into an update batch.
fn planned_to_update_operation(
    operation: &PlannedOperation,
) -> Result<(String, UpdateOperation), QqlError> {
    qql_plan::mutation::planned_to_update_operation(operation).ok_or_else(|| {
        QqlError::validation(
            "QQL-VALIDATION-BATCH-MEMBER",
            format!(
                "BATCH member cannot run in a batch RPC: {}",
                operation.operation_label()
            ),
            None,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use qql_plan::PlannedOperation;
    use serde_json::json;

    fn planned(sql: &str) -> PlannedOperation {
        qql_plan::plan(&qql_core::parser::Parser::parse(sql).expect("parse")).expect("plan")
    }

    #[test]
    fn exec_response_builders_omit_none_telemetry() {
        let value = exec_response(true, "PARSE", "ok", None);
        assert_eq!(value["data"], serde_json::Value::Null);
        assert!(value.get("telemetry").is_none());

        let with = exec_response_with_telemetry(
            true,
            "QUERY",
            "Found 0 hits",
            Some(json!([])),
            Some(ServerTelemetry {
                time_s: Some(0.25),
                ..ServerTelemetry::default()
            }),
        );
        assert_eq!(with["telemetry"]["time_s"], 0.25);
    }

    #[test]
    fn report_aggregates_typed_telemetry_and_omits_none() {
        let with_telemetry = json!({
            "ok": true,
            "operation": "QUERY",
            "message": "Found 1 hits",
            "data": [],
            "telemetry": {"time_s": 0.5},
        });
        let report = WasmReport::from_results(vec![
            with_telemetry,
            json!({"ok": false, "operation": "QUERY", "message": "boom"}),
        ]);
        let value = serde_json::to_value(&report).unwrap();
        assert_eq!(value["ok"], false);
        assert_eq!(value["succeeded"], 1);
        assert_eq!(value["failed"], 1);
        assert_eq!(value["telemetry"]["time_s"], 0.5);

        let single = WasmReport::single(json!({"ok": true, "data": null}));
        let value = serde_json::to_value(&single).unwrap();
        assert_eq!(value["ok"], true);
        assert_eq!(value["results"][0]["data"], serde_json::Value::Null);
        assert!(
            value.get("telemetry").is_none(),
            "None telemetry is omitted"
        );
    }

    #[test]
    fn parses_scored_points_strictly_into_runtime_shape() {
        let response = shaped_success_response(
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
        let response = shaped_success_response(
            &planned("QUERY POINTS (1) FROM docs"),
            &json!({"result": [{"id": "550e8400-e29b-41d4-a716-446655440001"}], "status": "ok"}),
        )
        .expect("strict parse");
        assert_eq!(response["operation"], "GET_POINTS");
        assert_eq!(
            response["data"],
            json!([{
                "id": "550e8400-e29b-41d4-a716-446655440001",
                "score": 0.0,
                "payload": null,
            }])
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
            let err = shaped_success_response(&scroll, &envelope).unwrap_err();
            assert_eq!(err.code, "QQL-BACKEND-ENVELOPE");
        }

        let err = shaped_success_response(
            &planned("COUNT FROM docs"),
            &json!({"result": {}, "status": "ok"}),
        )
        .unwrap_err();
        assert_eq!(err.code, "QQL-BACKEND-ENVELOPE");

        let err = shaped_success_response(
            &planned("FACET city FROM docs LIMIT 10"),
            &json!({"result": {"hits": [{"value": {"nested": 1}, "count": 2}]}, "status": "ok"}),
        )
        .unwrap_err();
        assert_eq!(err.code, "QQL-BACKEND-ENVELOPE");
    }

    #[test]
    fn parses_count_facet_and_groups_canonically() {
        let count = shaped_success_response(
            &planned("COUNT FROM docs"),
            &json!({"result": {"count": 7}, "status": "ok"}),
        )
        .unwrap();
        assert_eq!(count["data"], json!({"count": 7}));
        assert_eq!(count["message"], "Count: 7");

        let facet = shaped_success_response(
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

        let groups = shaped_success_response(
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
    }

    #[test]
    fn group_offset_trims_like_the_runtime() {
        let groups = shaped_success_response(
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
        let collections = shaped_success_response(
            &PlannedOperation::ListCollections,
            &json!({"result": {"collections": [{"name": "alpha"}, {"name": "beta"}]}, "status": "ok"}),
        )
        .unwrap();
        assert_eq!(
            collections["data"],
            json!({"collections": ["alpha", "beta"]})
        );

        let keys = shaped_success_response(
            &planned("SHOW SHARD KEYS ON COLLECTION docs"),
            &json!({"result": {"shard_keys": [{"key": "acme"}, {"key": 101}]}, "status": "ok"}),
        )
        .unwrap();
        assert_eq!(keys["data"], json!({"shard_keys": ["acme", 101]}));

        let none = shaped_success_response(
            &planned("SHOW SHARD KEYS ON COLLECTION docs"),
            &json!({"result": {"shard_keys": null}, "status": "ok"}),
        )
        .unwrap();
        assert_eq!(none["data"], json!({"shard_keys": []}));

        let quotas = shaped_success_response(
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
        let ok = shaped_success_response(
            &planned("SET QUOTA (enabled = true, max_resident_memory_percent = 80)"),
            &json!({"result": true, "status": "ok"}),
        )
        .unwrap();
        assert_eq!(
            ok["data"],
            json!({"enabled": true, "max_resident_memory_percent": 80})
        );

        let err = shaped_success_response(
            &planned("SET QUOTA (enabled = true)"),
            &json!({"result": {"ok": true}, "status": "ok"}),
        )
        .unwrap_err();
        assert_eq!(err.code, "QQL-BACKEND-ENVELOPE");
    }

    #[test]
    fn upsert_reports_request_point_count_and_writes_are_status_only() {
        let upsert = shaped_success_response(
            &planned("UPSERT INTO docs VALUES {id: 1, vector: [0.1, 0.2]}"),
            &json!({"result": {"status": "completed", "operation_id": 0}, "status": "ok"}),
        )
        .unwrap();
        assert_eq!(upsert["operation"], "UPSERT");
        assert_eq!(upsert["message"], "Upserted 1 point(s)");
        assert_eq!(upsert["data"], json!({"count": 1}));

        let delete = shaped_success_response(
            &planned("DELETE FROM docs WHERE id = 1"),
            &json!({"result": {"status": "completed", "operation_id": 1}, "status": "ok"}),
        )
        .unwrap();
        assert_eq!(delete["data"], serde_json::Value::Null);
    }

    #[test]
    fn snowflake_u64_ids_survive_shaping_exactly() {
        // Geosmart point IDs are u64 snowflakes above MAX_SAFE_INTEGER
        // (real hit: 1479834607549681654 > 9007199254740991). The Rust shaping
        // layer must keep them exact — the JS-boundary BigInt mapping in
        // `to_js_value` relies on receiving the full u64 here, never a
        // rounded f64.
        const SNOWFLAKE: u64 = 1479834607549681654;
        let response = shaped_success_response(
            &planned("SCROLL FROM docs LIMIT 1"),
            &json!({
                "result": {"points": [{
                    "id": SNOWFLAKE,
                    "payload": {"name": "x"},
                }]},
                "status": "ok",
            }),
        )
        .expect("strict parse");
        assert_eq!(response["data"][0]["id"], json!(SNOWFLAKE));
        assert_eq!(response["data"][0]["id"].as_u64(), Some(SNOWFLAKE));
    }

    #[test]
    fn batch_items_are_strict_and_carry_telemetry() {
        let parsed = shaped_query_batch(&json!({
            "result": [
                {"points": [{"id": 1, "score": 0.9}], "time": 0.5},
                {"points": []},
            ],
            "status": "ok",
        }))
        .unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(
            parsed[0]["data"],
            json!([{"id": 1, "score": 0.9, "payload": null}])
        );
        assert_eq!(parsed[0]["telemetry"]["time_s"], 0.5);
        assert_eq!(parsed[1]["message"], "Found 0 hits");

        let err =
            shaped_query_batch(&json!({"result": [{"points": 1}], "status": "ok"})).unwrap_err();
        assert_eq!(err.code, "QQL-BACKEND-ENVELOPE");

        let err = shaped_query_batch(&json!({
            "result": [{"status": "error", "error": "point 42 not found"}],
            "status": "ok",
        }))
        .unwrap_err();
        // Canonical runtime behavior: an item without its `points` array is an
        // envelope violation, regardless of extra status-like keys.
        assert_eq!(err.code, "QQL-BACKEND-ENVELOPE");

        let operations = [planned("UPSERT INTO docs VALUES {id: 1, vector: [0.1]}")];
        let updates = shaped_update_batch(
            &json!({"result": [{"status": "completed"}], "status": "ok"}),
            &operations,
        )
        .unwrap();
        assert_eq!(updates[0]["data"], json!({"count": 1}));
        assert_eq!(updates[0]["message"], "Upserted 1 point(s)");

        let err = shaped_update_batch(
            &json!({"result": [{"status": "wait_timeout"}], "status": "ok"}),
            &operations,
        )
        .unwrap_err();
        assert_eq!(err.code, "QQL-BACKEND-BATCH");
    }
}
