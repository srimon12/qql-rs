//! `Client.explainAnalyze`: static plan plus measured execution.
//!
//! Single-statement only, matching `Executor::explain_analyze` and the
//! Node and Python SDKs: array batches fail closed with
//! `QQL-VALIDATION-ANALYZE-BATCH`, multi-statement scripts fail with
//! `QQL-VALIDATION-MULTI-STMT`. Timings use `js_sys::Date::now` (no new
//! dependencies); server time and usage come from the Qdrant REST envelope
//! via the shared telemetry helpers.

use qql_core::parser::Parser;
use wasm_bindgen::prelude::*;

use super::client::Client;
use super::functions::{js_err, qql_err_to_js, to_js_value};
use super::params::{bind_stmt_values, bind_value_params, extract_ast_stmt, options_params};
use super::report::shaped_success_response;
use qql_plan::surface::telemetry::ServerTelemetry;

fn now_ms() -> f64 {
    js_sys::Date::now()
}

fn analyze_batch_error() -> JsValue {
    js_err(
        "QQL-VALIDATION-ANALYZE-BATCH",
        "explainAnalyze accepts a single statement (string or Stmt), not a batch; analyze each entry separately",
    )
}

fn analyze_empty_script_error() -> JsValue {
    js_err(
        "QQL-VALIDATION-EMPTY-SCRIPT",
        "no statements to analyze; the query string is empty or contains only comments",
    )
}

fn analyze_multi_stmt_error() -> JsValue {
    js_err(
        "QQL-VALIDATION-MULTI-STMT",
        "explainAnalyze accepts a single statement; analyze each statement separately",
    )
}

/// Bind `params` onto a parsed single statement and re-parse the bound source
/// so text embeddings and vector roles resolve from concrete values.
fn bind_analyzed_statement(
    source: &str,
    params: &qql_core::ast::Value,
) -> Result<qql_core::ast::Stmt, JsValue> {
    let bound_str = bind_value_params(source, params, false)?;
    let mut bound_stmts = Parser::parse_all(&bound_str).map_err(qql_err_to_js)?;
    match bound_stmts.as_slice() {
        [] => Err(analyze_empty_script_error()),
        [_] => Ok(bound_stmts.swap_remove(0)),
        _ => Err(analyze_multi_stmt_error()),
    }
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
        if let Some((mut stmt, _bound)) = extract_ast_stmt(&query) {
            if let Some(ref p) = params {
                let plan = qql_core::params_json::plan_value_params(p, 1).map_err(qql_err_to_js)?;
                bind_stmt_values(&mut stmt, qql_core::params_json::param_value_for(&plan, 0))?;
            }
            return self.analyze_stmt(stmt, 0.0, total_start).await;
        }
        if let Some(s) = query.as_string() {
            let parse_start = now_ms();
            let mut stmts = Parser::parse_all(&s).map_err(qql_err_to_js)?;
            if stmts.is_empty() {
                return Err(analyze_empty_script_error());
            }
            if stmts.len() != 1 {
                return Err(analyze_multi_stmt_error());
            }
            let mut stmt = stmts.swap_remove(0);
            let parse_ms = now_ms() - parse_start;
            if let Some(ref p) = params {
                let plan = qql_core::params_json::plan_value_params(p, 1).map_err(qql_err_to_js)?;
                stmt =
                    bind_analyzed_statement(&s, qql_core::params_json::param_value_for(&plan, 0))?;
            }
            return self.analyze_stmt(stmt, parse_ms, total_start).await;
        }
        Err(js_err(
            "QQL-BIND-TYPE-MISMATCH",
            "query must be a string or Stmt",
        ))
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
        let planned = qql_plan::plan(&prepared).map_err(qql_err_to_js)?;
        let prepare_plan_ms = now_ms() - prepare_start;
        let dispatch_start = now_ms();
        let route =
            qql_plan::to_rest_route(&planned).map_err(|err| qql_err_to_js(err.to_qql_error()))?;
        let envelope = self
            .send_json(route.method.as_str(), &route.path, route.body_json())
            .await?;
        let dispatch_ms = now_ms() - dispatch_start;
        let decode_start = now_ms();
        let telemetry = ServerTelemetry::from_envelope_opt(&envelope);
        let mut resp = shaped_success_response(&planned, &envelope).map_err(qql_err_to_js)?;
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
            Some(tel) => (tel.time_s, tel.usage),
            None => (None, None),
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
