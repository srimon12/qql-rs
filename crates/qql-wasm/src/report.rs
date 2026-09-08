//! Internal execution report matching the qql-runtime contract.

/// Internal execution report matching the qql-runtime contract.
/// Used so callers can access typed `succeeded`/`failed`/`results`
/// fields without chasing `serde_json::Value` keys.
#[derive(serde::Serialize)]
pub(crate) struct WasmReport {
    pub(crate) ok: bool,
    pub(crate) results: Vec<serde_json::Value>,
    pub(crate) succeeded: usize,
    pub(crate) failed: usize,
}

impl WasmReport {
    pub(crate) fn from_results(results: Vec<serde_json::Value>) -> Self {
        let succeeded = results
            .iter()
            .filter(|r| r.get("ok").and_then(|v| v.as_bool()).unwrap_or(false))
            .count();
        let failed = results.len() - succeeded;
        Self {
            ok: failed == 0,
            results,
            succeeded,
            failed,
        }
    }

    pub(crate) fn single(resp: serde_json::Value) -> Self {
        let ok = resp.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
        Self {
            ok,
            results: vec![resp],
            succeeded: if ok { 1 } else { 0 },
            failed: if ok { 0 } else { 1 },
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
    serde_json::json!({
        "ok": ok,
        "operation": operation,
        "message": message,
        "data": data.unwrap_or(serde_json::Value::Null),
    })
}
