//! Batch execution pipeline: scripts, preparation, batching, dispatch.

use qql_core::error::QqlError;
use qql_core::parser::Parser;
use wasm_bindgen::prelude::*;

use super::client::Client;
use super::params::WasmOnError;
use super::report::{WasmReport, exec_response, exec_response_with_telemetry};
use super::response::{parse_query_batch, parse_update_batch, wasm_success_response};

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
                // Explicit BATCH blocks never merge with ambient groups: flush
                // whatever is pending, then run members as one forced group.
                if let qql_plan::PlannedOperation::Batch { .. } = &single {
                    if let Some(ops) = grouper.finish() {
                        self.flush_planned_group(ops, on_error, &mut results)
                            .await?;
                    }
                    self.execute_batch_op(single, on_error, &mut results)
                        .await?;
                } else {
                    self.dispatch_or_collect(single, on_error, &mut results)
                        .await?;
                }
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
            qql_plan::RestProjectionError::SerializeFailed { message } => {
                JsValue::from_str(&format!("plan IR serialization failed: {message}"))
            }
            qql_plan::RestProjectionError::OverwriteRequiresBatch => JsValue::from_str(
                "OVERWRITE has no single REST route: POST /points/payload is merge-only; use a BATCH block",
            ),
        })?;
        let result = self
            .send_json(route.method.as_str(), &route.path, route.body_json())
            .await?;
        wasm_success_response(operation, &result).map_err(|error| {
            JsValue::from_str(&serde_json::to_string(&error).unwrap_or_else(|_| error.to_string()))
        })
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
    /// Run an explicit `BATCH` block as one forced group with header opts.
    ///
    /// The planned op projects to a single batch route (method, path, query,
    /// body); the query string is appended to the path because `send_json`
    /// takes no separate query argument. Members always take the batch RPC
    /// path and never merge with neighboring statements.
    pub(crate) async fn execute_batch_op(
        &self,
        op: qql_plan::PlannedOperation,
        on_error: WasmOnError,
        results: &mut Vec<serde_json::Value>,
    ) -> Result<(), JsValue> {
        use qql_plan::{PlannedOperation, build_update_batch, verify_batch_cardinality};

        let route = qql_plan::to_rest_route(&op).map_err(|err| match err {
            qql_plan::RestProjectionError::ClientSideOnly { stmt_type } => {
                JsValue::from_str(&format!("{stmt_type} has no single Qdrant REST route"))
            }
            qql_plan::RestProjectionError::SerializeFailed { message } => {
                JsValue::from_str(&format!("plan IR serialization failed: {message}"))
            }
            qql_plan::RestProjectionError::OverwriteRequiresBatch => JsValue::from_str(
                "OVERWRITE has no single REST route: POST /points/payload is merge-only; use a BATCH block",
            ),
        })?;
        let PlannedOperation::Batch {
            key, operations, ..
        } = op
        else {
            return Err(JsValue::from_str(
                "execute_batch_op requires a Batch operation",
            ));
        };
        let route_method = route.method.as_str().to_string();
        let body = route.body_json();
        let mut path = route.path;
        if !route.query.is_empty() {
            path.push('?');
            path.push_str(
                &route
                    .query
                    .iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect::<Vec<_>>()
                    .join("&"),
            );
        }
        let response = match self.send_json(&route_method, &path, body).await {
            Ok(response) => response,
            Err(error) => {
                if on_error == WasmOnError::Stop {
                    return Err(error);
                }
                for operation in operations {
                    self.dispatch_or_collect(operation, on_error, results)
                        .await?;
                }
                return Ok(());
            }
        };
        async fn retry_members(
            client: &Client,
            operations: Vec<PlannedOperation>,
            on_error: WasmOnError,
            results: &mut Vec<serde_json::Value>,
        ) -> Result<(), JsValue> {
            for operation in operations {
                client
                    .dispatch_or_collect(operation, on_error, results)
                    .await?;
            }
            Ok(())
        }
        match key {
            qql_plan::BatchKey::Query(_) => match parse_query_batch(&response) {
                Ok(items) => {
                    if let Err(err) =
                        verify_batch_cardinality("query", operations.len(), items.len())
                    {
                        let error = JsValue::from_str(&err.to_string());
                        if on_error == WasmOnError::Stop {
                            return Err(error);
                        }
                        retry_members(self, operations, on_error, results).await?;
                    } else {
                        for (hits, telemetry) in items {
                            let count = hits.as_array().map_or(0, Vec::len);
                            results.push(exec_response_with_telemetry(
                                true,
                                "QUERY",
                                &format!("Found {count} hits"),
                                Some(hits),
                                telemetry,
                            ));
                        }
                    }
                }
                Err(err) => {
                    let error = JsValue::from_str(
                        &serde_json::to_string(&err).unwrap_or_else(|_| err.to_string()),
                    );
                    if on_error == WasmOnError::Stop {
                        return Err(error);
                    }
                    retry_members(self, operations, on_error, results).await?;
                }
            },
            qql_plan::BatchKey::Mutation(_) => {
                let (_, labels, _) = build_update_batch(&operations)
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;
                match parse_update_batch(&response) {
                    Ok(received) => {
                        if let Err(err) = verify_batch_cardinality("update", labels.len(), received)
                        {
                            let error = JsValue::from_str(&err.to_string());
                            if on_error == WasmOnError::Stop {
                                return Err(error);
                            }
                            retry_members(self, operations, on_error, results).await?;
                        } else {
                            for (label, operation) in labels.iter().zip(operations.iter()) {
                                let data = match operation {
                                    PlannedOperation::Upsert { request, .. } => {
                                        Some(serde_json::json!({"count": request.points.len()}))
                                    }
                                    _ => None,
                                };
                                let message = match operation {
                                    PlannedOperation::Upsert { request, .. } => {
                                        format!("Upserted {} point(s)", request.points.len())
                                    }
                                    _ => format!("{label} ok"),
                                };
                                results.push(exec_response(true, label, &message, data));
                            }
                        }
                    }
                    Err(err) => {
                        let error = JsValue::from_str(
                            &serde_json::to_string(&err).unwrap_or_else(|_| err.to_string()),
                        );
                        if on_error == WasmOnError::Stop {
                            return Err(error);
                        }
                        retry_members(self, operations, on_error, results).await?;
                    }
                }
            }
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
            PlannedOperation, build_query_batch, build_update_batch, verify_batch_cardinality,
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
                    Ok(response) => match parse_query_batch(&response) {
                        Ok(items) => {
                            if let Err(err) =
                                verify_batch_cardinality("query", expected, items.len())
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
                                for (hits, telemetry) in items {
                                    let count = hits.as_array().map_or(0, Vec::len);
                                    results.push(exec_response_with_telemetry(
                                        true,
                                        "QUERY",
                                        &format!("Found {count} hits"),
                                        Some(hits),
                                        telemetry,
                                    ));
                                }
                            }
                        }
                        Err(err) => {
                            let error = JsValue::from_str(
                                &serde_json::to_string(&err).unwrap_or_else(|_| err.to_string()),
                            );
                            if on_error == WasmOnError::Stop {
                                return Err(error);
                            }
                            for operation in operations {
                                self.dispatch_or_collect(operation, on_error, results)
                                    .await?;
                            }
                        }
                    },
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
                    Ok(response) => match parse_update_batch(&response) {
                        Ok(received) => {
                            if let Err(err) = verify_batch_cardinality("update", expected, received)
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
                                for (label, operation) in labels.iter().zip(operations.iter()) {
                                    // Same normalization as single dispatch:
                                    // upserts report their request point count,
                                    // other writes are status-only (`null`).
                                    let data = match operation {
                                        PlannedOperation::Upsert { request, .. } => {
                                            Some(serde_json::json!({"count": request.points.len()}))
                                        }
                                        _ => None,
                                    };
                                    let message = match operation {
                                        PlannedOperation::Upsert { request, .. } => {
                                            format!("Upserted {} point(s)", request.points.len())
                                        }
                                        _ => format!("{label} ok"),
                                    };
                                    results.push(exec_response(true, label, &message, data));
                                }
                            }
                        }
                        Err(err) => {
                            let error = JsValue::from_str(
                                &serde_json::to_string(&err).unwrap_or_else(|_| err.to_string()),
                            );
                            if on_error == WasmOnError::Stop {
                                return Err(error);
                            }
                            for operation in operations {
                                self.dispatch_or_collect(operation, on_error, results)
                                    .await?;
                            }
                        }
                    },
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
