//! `Client.explainAnalyze`: static plan plus measured execution.
//!
//! Single-statement only, matching `Executor::explain_analyze` and the
//! Node and Python SDKs: array batches fail closed with
//! `QQL-VALIDATION-ANALYZE-BATCH`, multi-statement scripts fail with
//! `QQL-VALIDATION-MULTI-STMT`. Timings use `js_sys::Date::now` (no new
//! dependencies); server time and usage come from the Qdrant REST envelope
//! via the shared telemetry helpers.

use qql_core::error::QqlError;
use qql_core::parser::Parser;
use wasm_bindgen::prelude::*;

use super::client::Client;
use super::functions::to_js_value;
use super::params::{bind_stmt_values, bind_value_params, extract_ast_stmt, options_params};
use super::response::wasm_success_response;
use super::telemetry::telemetry_from_envelope;

fn now_ms() -> f64 {
    js_sys::Date::now()
}

fn analyze_batch_error() -> JsValue {
    let err = QqlError::validation(
        "QQL-VALIDATION-ANALYZE-BATCH",
        "explainAnalyze accepts a single statement (string or Stmt), not a batch; analyze each entry separately",
        None,
    );
    JsValue::from_str(&serde_json::to_string(&err).unwrap_or_else(|_| err.to_string()))
}

#[wasm_bindgen]
impl Client {
    /// Analyze a single query string or `Stmt`: static plan plus measured
    /// execution (per-phase client timings, server time, hardware and
    /// inference usage). Returns an `AnalyzeReport` object:
    /// `{ ok, plan, phases, server_time_s, usage, results }`.
    /// Batches fail closed. `options.params` binds before analysis.
    #[wasm_bindgen(js_name = explainAnalyze, unchecked_return_type = "AnalyzeReport")]
    pub async fn explain_analyze(
        &self,
        #[wasm_bindgen(unchecked_param_type = "string | Stmt")] query: JsValue,
        #[wasm_bindgen(unchecked_optional_param_type = "ExecuteOptions")] options: Option<JsValue>,
    ) -> Result<JsValue, JsValue> {
        let total_start = now_ms();
        let params = options_params(options.as_ref())?;
        if js_sys::Array::is_array(&query) {
            return Err(analyze_batch_error());
        }
        if let Some(mut stmt) = extract_ast_stmt(&query) {
            if let Some(ref p) = params {
                let plan = qql_core::params_json::plan_value_params(p, 1)
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;
                bind_stmt_values(&mut stmt, qql_core::params_json::param_value_for(&plan, 0))?;
            }
            return self.analyze_stmt(stmt, 0.0, total_start).await;
        }
        if let Some(s) = query.as_string() {
            let parse_start = now_ms();
            let mut stmts = Parser::parse_all(&s).map_err(|e| JsValue::from_str(&e.to_string()))?;
            if stmts.is_empty() {
                let err = QqlError::validation(
                    "QQL-VALIDATION-EMPTY-SCRIPT",
                    "no statements to analyze; the query string is empty or contains only comments",
                    None,
                );
                return Err(JsValue::from_str(
                    &serde_json::to_string(&err).unwrap_or_else(|_| err.to_string()),
                ));
            }
            if stmts.len() != 1 {
                let err = QqlError::validation(
                    "QQL-VALIDATION-MULTI-STMT",
                    "explainAnalyze accepts a single statement; analyze each statement separately",
                    None,
                );
                return Err(JsValue::from_str(
                    &serde_json::to_string(&err).unwrap_or_else(|_| err.to_string()),
                ));
            }
            let mut stmt = stmts.pop().expect("single stmt");
            let parse_ms = now_ms() - parse_start;
            if let Some(ref p) = params {
                let plan = qql_core::params_json::plan_value_params(p, 1)
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;
                let bound_str =
                    bind_value_params(&s, qql_core::params_json::param_value_for(&plan, 0), false)?;
                // Re-parse the bound source so text embeddings and vector roles
                // resolve from concrete values, matching `execute`.
                let mut bound_stmts =
                    Parser::parse_all(&bound_str).map_err(|e| JsValue::from_str(&e.to_string()))?;
                stmt = bound_stmts.pop().expect("single bound stmt");
            }
            return self.analyze_stmt(stmt, parse_ms, total_start).await;
        }
        Err(JsValue::from_str("query must be a string or Stmt"))
    }

    async fn analyze_stmt(
        &self,
        stmt: qql_core::ast::Stmt,
        parse_ms: f64,
        total_start: f64,
    ) -> Result<JsValue, JsValue> {
        let plan_text = qql_core::explain::explain_node(&stmt);
        let prepare_start = now_ms();
        let mut prepared = stmt.clone();
        self.resolve_stmt_vector_kinds(&mut prepared).await?;
        self.resolve_stmt_embeddings(&mut prepared).await?;
        let planned = qql_plan::plan(&prepared).map_err(|e| JsValue::from_str(&e.to_string()))?;
        let prepare_plan_ms = now_ms() - prepare_start;
        let dispatch_start = now_ms();
        let route = qql_plan::to_rest_route(&planned).map_err(|err| match err {
            qql_plan::RestProjectionError::ClientSideOnly { stmt_type } => {
                JsValue::from_str(&format!("{stmt_type} has no single Qdrant REST route"))
            }
            qql_plan::RestProjectionError::SerializeFailed { message } => {
                JsValue::from_str(&format!("plan IR serialization failed: {message}"))
            }
        })?;
        let envelope = self
            .send_json(route.method.as_str(), &route.path, route.body_json())
            .await?;
        let dispatch_ms = now_ms() - dispatch_start;
        let decode_start = now_ms();
        let telemetry = telemetry_from_envelope(&envelope);
        let mut resp = wasm_success_response(&planned, envelope);
        // Ensure the key exists (null when absent) so JS readers never branch
        // on key presence.
        if resp.get("telemetry").is_none()
            && let Some(obj) = resp.as_object_mut()
        {
            obj.insert("telemetry".to_string(), serde_json::Value::Null);
        }
        let decode_ms = now_ms() - decode_start;
        let total_ms = now_ms() - total_start;
        let (server_time_s, usage) = match telemetry {
            Some(tel) => (
                tel.get("time_s")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null),
                tel.get("usage").cloned().unwrap_or(serde_json::Value::Null),
            ),
            None => (serde_json::Value::Null, serde_json::Value::Null),
        };
        let report = serde_json::json!({
            "ok": true,
            "plan": plan_text,
            "phases": {
                "parse_ms": parse_ms,
                "prepare_plan_ms": prepare_plan_ms,
                "dispatch_ms": dispatch_ms,
                "decode_ms": decode_ms,
                "total_ms": total_ms,
            },
            "server_time_s": server_time_s,
            "usage": usage,
            "results": [resp],
        });
        to_js_value(&report)
    }
}
