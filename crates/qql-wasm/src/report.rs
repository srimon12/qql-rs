//! Internal execution report matching the qql-runtime contract.

use super::telemetry::aggregate_telemetry;

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
    pub(crate) telemetry: Option<serde_json::Value>,
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
        let telemetry = resp.get("telemetry").cloned();
        Self {
            ok,
            results: vec![resp],
            succeeded: if ok { 1 } else { 0 },
            failed: if ok { 0 } else { 1 },
            telemetry,
        }
    }
}

/// Build an ExecResponse-compatible JSON value.
pub(crate) fn exec_response(
    ok: bool,
    operation: &str,
    message: &str,
    data: Option<serde_json::Value>,
) -> serde_json::Value {
    exec_response_with_telemetry(ok, operation, message, data, None)
}

/// Build an ExecResponse-compatible JSON value with optional server telemetry.
///
/// `telemetry` uses the `{"time_s": ..., "usage": ...}` shape; `None` omits
/// the key so pre-telemetry payloads stay byte-identical.
pub(crate) fn exec_response_with_telemetry(
    ok: bool,
    operation: &str,
    message: &str,
    data: Option<serde_json::Value>,
    telemetry: Option<serde_json::Value>,
) -> serde_json::Value {
    let mut obj = serde_json::Map::new();
    obj.insert("ok".to_string(), serde_json::Value::from(ok));
    obj.insert("operation".to_string(), serde_json::Value::from(operation));
    obj.insert("message".to_string(), serde_json::Value::from(message));
    obj.insert("data".to_string(), data.unwrap_or(serde_json::Value::Null));
    if let Some(tel) = telemetry {
        obj.insert("telemetry".to_string(), tel);
    }
    serde_json::Value::Object(obj)
}
