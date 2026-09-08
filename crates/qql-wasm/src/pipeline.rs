//! Batch execution pipeline: scripts, preparation, batching, dispatch.

use qql_core::error::QqlError;
use qql_core::parser::Parser;
use wasm_bindgen::prelude::*;

use super::client::Client;
use super::params::WasmOnError;
use super::report::{WasmReport, exec_response};
use super::response::{wasm_search_hits, wasm_success_response};

#[wasm_bindgen]
impl Client {
    /// Execute one or more statements with order-preserving smart batching.
    /// Returns a JSON value shaped like ExecutionReport.
    pub(crate) async fn execute_script(
        &self,
        query: &str,
        on_error: WasmOnError,
    ) -> Result<WasmReport, JsValue> {
        use qql_plan::BatchGrouper;

        let stmts = match Parser::parse_all(query) {
            Ok(stmts) => stmts,
            Err(error) if on_error == WasmOnError::Stop => {
                return Err(JsValue::from_str(&error.to_string()));
            }
            Err(error) => {
                return Ok(WasmReport::single(exec_response(
                    false,
                    "PARSE",
                    &error.to_string(),
                    None,
                )));
            }
        };
        if stmts.is_empty() {
            // Fail closed like the executor does for an empty string —
            // an empty array must not return a silently-empty ok report.
            return Err(JsValue::from_str(
                &serde_json::to_string(&QqlError::validation(
                    "QQL-VALIDATION-EMPTY-SCRIPT",
                    "no statements to execute; the query is empty or contains only whitespace",
                    None,
                ))
                .unwrap_or_else(|_| "no statements to execute".into()),
            ));
        }

        let mut results: Vec<serde_json::Value> = Vec::with_capacity(stmts.len());
        let mut grouper = BatchGrouper::new();

        for stmt in stmts {
            if let Some(ops) = grouper.check_statement_barrier(&stmt) {
                self.flush_planned_group(ops, on_error, &mut results)
                    .await?;
            }

            let planned = match self.prepare_operation(&stmt).await {
                Ok(planned) => planned,
                Err(error) => {
                    if let Some(ops) = grouper.flush_on_error() {
                        self.flush_planned_group(ops, on_error, &mut results)
                            .await?;
                    }
                    if on_error == WasmOnError::Stop {
                        return Err(error);
                    }
                    results.push(exec_response(
                        false,
                        "PREPARE",
                        &error.as_string().unwrap_or_default(),
                        None,
                    ));
                    continue;
                }
            };

            let (flush_ops, dispatch_single) = grouper.push_planned(planned);
            if let Some(ops) = flush_ops {
                self.flush_planned_group(ops, on_error, &mut results)
                    .await?;
            }
            if let Some(single) = dispatch_single {
                self.dispatch_or_collect(single, on_error, &mut results)
                    .await?;
            }
        }

        if let Some(ops) = grouper.finish() {
            self.flush_planned_group(ops, on_error, &mut results)
                .await?;
        }
        Ok(WasmReport::from_results(results))
    }
    pub(crate) async fn prepare_operation(
        &self,
        stmt: &qql_core::ast::Stmt,
    ) -> Result<qql_plan::PlannedOperation, JsValue> {
        let mut stmt = stmt.clone();
        // Schema-first: fill USING kinds from collection topology before
        // embedding so `USING sparse` embeds sparse, not dense-by-default.
        self.resolve_stmt_vector_kinds(&mut stmt).await?;
        self.resolve_stmt_embeddings(&mut stmt).await?;
        qql_plan::plan(&stmt).map_err(|error| JsValue::from_str(&error.to_string()))
    }
    pub(crate) async fn execute_planned_inner(
        &self,
        operation: &qql_plan::PlannedOperation,
    ) -> Result<serde_json::Value, JsValue> {
        let route = qql_plan::to_rest_route(operation).map_err(|err| match err {
            qql_plan::RestProjectionError::ClientSideOnly { stmt_type } => {
                JsValue::from_str(&format!("{stmt_type} has no single Qdrant REST route"))
            }
        })?;
        let result = self
            .send_json(route.method.as_str(), &route.path, route.body_json())
            .await?;
        Ok(wasm_success_response(operation, result))
    }
    pub(crate) async fn dispatch_or_collect(
        &self,
        operation: qql_plan::PlannedOperation,
        on_error: WasmOnError,
        results: &mut Vec<serde_json::Value>,
    ) -> Result<(), JsValue> {
        match self.execute_planned_inner(&operation).await {
            Ok(response) => results.push(response),
            Err(error) if on_error == WasmOnError::Stop => return Err(error),
            Err(error) => results.push(exec_response(
                false,
                operation.operation_label(),
                &error.as_string().unwrap_or_default(),
                None,
            )),
        }
        Ok(())
    }
    pub(crate) async fn flush_planned_group(
        &self,
        mut operations: Vec<qql_plan::PlannedOperation>,
        on_error: WasmOnError,
        results: &mut Vec<serde_json::Value>,
    ) -> Result<(), JsValue> {
        use qql_plan::{
            PlannedOperation, batch_item_error, build_query_batch, build_update_batch,
            verify_batch_cardinality,
        };

        if operations.is_empty() {
            return Ok(());
        }
        if operations.len() == 1 {
            let operation = operations.pop().expect("pending contains one operation");
            return self.dispatch_or_collect(operation, on_error, results).await;
        }
        match &operations[0] {
            PlannedOperation::Query { .. } => {
                let (collection, batch) = build_query_batch(&operations)
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;
                let expected = batch.searches.len();
                let path = format!("/collections/{collection}/points/query/batch");
                let body = serde_json::to_value(&batch)
                    .map_err(|error| JsValue::from_str(&error.to_string()))?;
                match self.send_json("POST", &path, Some(body)).await {
                    Ok(response) => {
                        let values = response
                            .get("result")
                            .and_then(serde_json::Value::as_array)
                            .cloned()
                            .unwrap_or_default();
                        if let Err(err) = verify_batch_cardinality("query", expected, values.len())
                        {
                            let error = JsValue::from_str(&err.to_string());
                            if on_error == WasmOnError::Stop {
                                return Err(error);
                            }
                            for operation in operations {
                                self.dispatch_or_collect(operation, on_error, results)
                                    .await?;
                            }
                        } else {
                            for value in values {
                                if let Some(msg) = batch_item_error(&value) {
                                    results.push(exec_response(false, "QUERY", &msg, None));
                                    continue;
                                }
                                let hits = wasm_search_hits(&value);
                                let count = hits.as_array().map_or(0, Vec::len);
                                results.push(exec_response(
                                    true,
                                    "QUERY",
                                    &format!("Found {count} hits"),
                                    Some(hits),
                                ));
                            }
                        }
                    }
                    Err(error) => {
                        if on_error == WasmOnError::Stop {
                            return Err(error);
                        }
                        for operation in operations {
                            self.dispatch_or_collect(operation, on_error, results)
                                .await?;
                        }
                    }
                }
            }
            _ => {
                let (collection, labels, batch) = build_update_batch(&operations)
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;
                let expected = batch.operations.len();
                let path = format!("/collections/{collection}/points/batch?wait=true");
                let body = serde_json::to_value(&batch)
                    .map_err(|error| JsValue::from_str(&error.to_string()))?;
                match self.send_json("POST", &path, Some(body)).await {
                    Ok(response) => {
                        let values = response
                            .get("result")
                            .and_then(serde_json::Value::as_array)
                            .cloned()
                            .unwrap_or_default();
                        if let Err(err) = verify_batch_cardinality("update", expected, values.len())
                        {
                            let error = JsValue::from_str(&err.to_string());
                            if on_error == WasmOnError::Stop {
                                return Err(error);
                            }
                            for operation in operations {
                                self.dispatch_or_collect(operation, on_error, results)
                                    .await?;
                            }
                        } else {
                            for (value, label) in values.into_iter().zip(labels.iter()) {
                                if let Some(msg) = batch_item_error(&value) {
                                    results.push(exec_response(false, label, &msg, None));
                                    continue;
                                }
                                results.push(exec_response(
                                    true,
                                    label,
                                    &format!("{label} ok (batched)"),
                                    Some(value),
                                ));
                            }
                        }
                    }
                    Err(error) => {
                        if on_error == WasmOnError::Stop {
                            return Err(error);
                        }
                        for operation in operations {
                            self.dispatch_or_collect(operation, on_error, results)
                                .await?;
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
