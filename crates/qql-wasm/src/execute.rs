//! Client execution: `execute`, `executeStmt`, `upsertMany`, batching.

use qql_core::ast::Value;
use qql_core::error::QqlError;
use qql_core::parser::Parser;
use wasm_bindgen::prelude::*;

use super::client::Client;
use super::functions::{compile, to_js_value};
use super::params::{
    WasmOnError, batch_size_from, bind_stmt_values, bind_value_params, extract_ast_stmt,
    jsvalue_to_value, maybe_bind, options_params, parse_on_error,
};
use super::report::{WasmReport, exec_response};
use super::stmt::Stmt;

#[wasm_bindgen]
impl Client {
    /// Parse, compile, embed if needed, and POST to Qdrant's REST API.
    ///
    /// Accepts a string, a Stmt, or an array of either. Always returns a stable
    /// `ExecutionReport` object:
    /// `{ "ok": bool, "results": [...], "succeeded": N, "failed": M }`.
    #[wasm_bindgen(unchecked_return_type = "ExecutionReport")]
    pub async fn execute(
        &self,
        #[wasm_bindgen(unchecked_param_type = "string | Stmt | (string | Stmt)[]")] query: JsValue,
        #[wasm_bindgen(unchecked_optional_param_type = "ExecuteOptions")] options: Option<JsValue>,
    ) -> Result<JsValue, JsValue> {
        let on_error = parse_on_error(options.as_ref())?;
        let params = options_params(options.as_ref())?;
        if js_sys::Array::is_array(&query) {
            let arr = js_sys::Array::from(&query);
            let len = arr.length() as usize;
            let mut all_results: Vec<serde_json::Value> = Vec::new();
            let mut succeeded = 0usize;
            let mut failed = 0usize;

            let plan = match params.as_ref() {
                Some(p) => Some(
                    qql_core::params_json::plan_value_params(p, len)
                        .map_err(|e| JsValue::from_str(&e.to_string()))?,
                ),
                None => None,
            };

            for i in 0..len {
                let item = arr.get(i as u32);
                let item_params = plan
                    .as_ref()
                    .map(|plan| qql_core::params_json::param_value_for(plan, i));

                if let Some(s) = item.as_string() {
                    let s = maybe_bind(&s, item_params)?;
                    match self.execute_script(&s, on_error).await {
                        Ok(report) => {
                            succeeded += report.succeeded;
                            failed += report.failed;
                            all_results.extend(report.results);
                        }
                        Err(e) => {
                            if on_error == WasmOnError::Stop {
                                return Err(e);
                            }
                            failed += 1;
                            all_results.push(exec_response(
                                false,
                                "ERROR",
                                &e.as_string().unwrap_or_default(),
                                None,
                            ));
                        }
                    }
                } else if let Some(mut stmt) = extract_ast_stmt(&item) {
                    if let Some(p) = item_params {
                        bind_stmt_values(&mut stmt, p)?;
                    }
                    match self.execute_stmt_inner(&stmt).await {
                        Ok(val) => {
                            succeeded += 1;
                            all_results.push(val);
                        }
                        Err(e) => {
                            if on_error == WasmOnError::Stop {
                                return Err(e);
                            }
                            failed += 1;
                            all_results.push(exec_response(
                                false,
                                "ERROR",
                                &e.as_string().unwrap_or_default(),
                                None,
                            ));
                        }
                    }
                } else {
                    return Err(JsValue::from_str(&format!(
                        "array item at index {} must be a string or Stmt, got {:?}",
                        i,
                        item.js_typeof()
                    )));
                }
            }
            let report = WasmReport {
                ok: failed == 0,
                results: all_results,
                succeeded,
                failed,
            };
            return to_js_value(&report);
        }

        if let Some(mut stmt) = extract_ast_stmt(&query) {
            if let Some(ref p) = params {
                let plan = qql_core::params_json::plan_value_params(p, 1)
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;
                bind_stmt_values(&mut stmt, qql_core::params_json::param_value_for(&plan, 0))?;
            }
            let val = self.execute_stmt_inner(&stmt).await?;
            let report = WasmReport::single(val);
            return to_js_value(&report);
        }

        if let Some(s) = query.as_string() {
            // A params array of containers (objects/arrays) is a scoped
            // candidate for scripts: parse once to count statements, then let
            // the shared planner enforce the exact length contract.
            if let Some(p) = params.as_ref()
                && matches!(p, Value::List(items)
                    if !items.is_empty()
                        && items
                            .iter()
                            .all(|e| matches!(e, Value::Dict(_) | Value::List(_))))
            {
                let parsed_stmts =
                    Parser::parse_all(&s).map_err(|e| JsValue::from_str(&e.to_string()))?;
                let plan = qql_core::params_json::plan_value_params(p, parsed_stmts.len())
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;
                if let qql_core::params_json::ValueParamPlan::Scoped(_) = &plan {
                    let mut bound_stmts = Vec::with_capacity(parsed_stmts.len());
                    for (i, mut stmt) in parsed_stmts.into_iter().enumerate() {
                        bind_stmt_values(
                            &mut stmt,
                            qql_core::params_json::param_value_for(&plan, i),
                        )?;
                        bound_stmts.push(stmt);
                    }
                    let mut all_results: Vec<serde_json::Value> = Vec::new();
                    let mut succeeded = 0usize;
                    let mut failed = 0usize;
                    for stmt in bound_stmts {
                        match self.execute_stmt_inner(&stmt).await {
                            Ok(val) => {
                                succeeded += 1;
                                all_results.push(val);
                            }
                            Err(e) => {
                                if on_error == WasmOnError::Stop {
                                    return Err(e);
                                }
                                failed += 1;
                                all_results.push(exec_response(
                                    false,
                                    "ERROR",
                                    &e.as_string().unwrap_or_default(),
                                    None,
                                ));
                            }
                        }
                    }
                    let report = WasmReport {
                        ok: failed == 0,
                        results: all_results,
                        succeeded,
                        failed,
                    };
                    return to_js_value(&report);
                }
            }
            let bound = match params.as_ref() {
                Some(p) => {
                    let plan = qql_core::params_json::plan_value_params(p, 1)
                        .map_err(|e| JsValue::from_str(&e.to_string()))?;
                    bind_value_params(&s, qql_core::params_json::param_value_for(&plan, 0), false)?
                }
                None => s.clone(),
            };
            let report = self.execute_script(&bound, on_error).await?;
            return to_js_value(&report);
        }

        Err(JsValue::from_str("query must be a string, Stmt, or array"))
    }

    /// Bulk ingest: `rows` is an array of point objects
    /// (`{id, vector, …payload}`) spliced through the `:rows` point-splice
    /// path in `batchSize` chunks (default 100). Row vectors accept plain
    /// arrays, `Float32Array` / `Float64Array` (packed, one copy), integer
    /// typed arrays (sparse `indices`), and the flat `{data, dim}`
    /// multivector form — the same inputs as `bind`.
    #[wasm_bindgen(js_name = upsertMany, unchecked_return_type = "ExecutionReport")]
    pub async fn upsert_many(
        &self,
        collection: String,
        rows: JsValue,
        options: Option<JsValue>,
    ) -> Result<JsValue, JsValue> {
        let on_error = parse_on_error(options.as_ref())?;
        let batch_size =
            batch_size_from(options.as_ref()).map_err(|e| JsValue::from_str(&e.to_string()))?;
        let rows = jsvalue_to_value(&rows).map_err(|e| JsValue::from_str(&e.to_string()))?;
        let rows = match rows {
            Value::List(rows) => rows,
            _ => {
                return Err(JsValue::from_str(
                    &serde_json::to_string(&QqlError::validation(
                        "QQL-BIND-TYPE-MISMATCH",
                        "upsertMany rows must be an array of point objects ({id, vector, …payload})",
                        None,
                    ))
                    .unwrap_or_else(|_| "upsertMany rows must be an array".into()),
                ));
            }
        };
        if rows.is_empty() {
            return to_js_value(&WasmReport::from_results(Vec::new()));
        }
        // Template built as AST (never interpolated SQL), so the collection
        // name stays data. Re-cloned pristine per chunk because binding
        // splices placeholders into inline points.
        let template = qql_core::ast::Stmt::Upsert(Box::new(qql_core::ast::UpsertStmt {
            collection,
            points: vec![qql_core::ast::PointEntry::Param("rows".to_string(), None)],
            embedding: None,
            embed: Vec::new(),
            shard_key: None,
            wait: None,
        }));
        let stop = on_error == WasmOnError::Stop;
        let mut results = Vec::new();
        // Move (never clone) each chunk out of `rows`.
        let mut rows = rows.into_iter();
        loop {
            let chunk: Vec<Value> = rows.by_ref().take(batch_size).collect();
            if chunk.is_empty() {
                break;
            }
            let mut stmt = template.clone();
            qql_core::params_json::bind_stmt_with_values(
                &mut stmt,
                &Value::Dict(vec![("rows".to_string(), Value::List(chunk))]),
            )
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
            match self.execute_stmt_inner(&stmt).await {
                Ok(val) => results.push(val),
                Err(e) if !stop => results.push(exec_response(
                    false,
                    "UPSERT",
                    &e.as_string().unwrap_or_default(),
                    None,
                )),
                Err(e) => return Err(e),
            }
        }
        to_js_value(&WasmReport::from_results(results))
    }

    /// Execute a pre-parsed Stmt object. Injects embeddings for UPSERT
    /// if an embedder is configured. Optionally binds parameters.
    #[wasm_bindgen(js_name = executeStmt, unchecked_return_type = "ExecutionReport")]
    pub async fn execute_stmt(
        &self,
        stmt: &Stmt,
        #[wasm_bindgen(unchecked_optional_param_type = "ExecuteOptions")] options: Option<JsValue>,
    ) -> Result<JsValue, JsValue> {
        let params = options_params(options.as_ref())?;
        if params.is_some() && stmt.bound {
            return Err(JsValue::from_str(
                &serde_json::to_string(&QqlError::validation(
                    "QQL-BIND-ALREADY-BOUND",
                    "cannot bind parameters into a Stmt that has already been bound (params would be silently ignored)",
                    None,
                ))
                .unwrap_or_else(|_| "cannot bind parameters into an already bound Stmt".into()),
            ));
        }
        let mut inner = stmt.inner.clone();
        if let Some(ref p) = params {
            let plan = qql_core::params_json::plan_value_params(p, 1)
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            bind_stmt_values(&mut inner, qql_core::params_json::param_value_for(&plan, 0))?;
        }
        let val = self.execute_stmt_inner(&inner).await?;
        let report = WasmReport::single(val);
        to_js_value(&report)
    }

    pub(crate) async fn execute_stmt_inner(
        &self,
        stmt: &qql_core::ast::Stmt,
    ) -> Result<serde_json::Value, JsValue> {
        let operation = self.prepare_operation(stmt).await?;
        self.execute_planned_inner(&operation).await
    }

    /// Parse and compile one statement without executing it. Optional
    /// `params` bind before parsing (same shape as the module-level `bind`).
    #[wasm_bindgen(unchecked_return_type = "CompiledRoute")]
    pub fn compile(&self, query: &str, params: Option<JsValue>) -> Result<JsValue, JsValue> {
        compile(query, params)
    }

    /// Parse and compile one statement without executing it. Alias for `compile`.
    #[wasm_bindgen(js_name = compileQuery, unchecked_return_type = "CompiledRoute")]
    pub fn compile_query(&self, query: &str, params: Option<JsValue>) -> Result<JsValue, JsValue> {
        compile(query, params)
    }

    /// Parse and explain the query — no server needed.
    #[wasm_bindgen]
    pub fn explain(&self, query: &str) -> Result<String, JsValue> {
        qql_core::explain::explain(query).map_err(|e| JsValue::from_str(&e.to_string()))
    }
}
