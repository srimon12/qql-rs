//! Batch execution pipeline: scripts, preparation, batching, dispatch.

use qql_core::error::QqlError;
use qql_core::parser::Parser;
use wasm_bindgen::prelude::*;

use super::client::Client;
use super::functions::qql_err_to_js;
use super::params::WasmOnError;
use super::report::{
    WasmReport, exec_response, parsed_update_batch, shaped_query_batch, shaped_success_response,
    shaped_update_batch, shaped_update_response,
};

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
                return Err(qql_err_to_js(error));
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
        qql_plan::plan(&stmt).map_err(qql_err_to_js)
    }
    pub(crate) async fn execute_planned_inner(
        &self,
        operation: &qql_plan::PlannedOperation,
    ) -> Result<serde_json::Value, JsValue> {
        let route =
            qql_plan::to_rest_route(operation).map_err(|err| qql_err_to_js(err.to_qql_error()))?;
        let result = self
            .send_json(route.method.as_str(), &route.path, route.body_json())
            .await?;
        shaped_success_response(operation, &result).map_err(|error| {
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
        use qql_plan::{PlannedOperation, verify_batch_cardinality};

        let route =
            qql_plan::to_rest_route(&op).map_err(|err| qql_err_to_js(err.to_qql_error()))?;
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
        let response = match self.send_json(&route_method, &path, body).await {
            Ok(response) => response,
            Err(error) => {
                if on_error == WasmOnError::Stop {
                    return Err(error);
                }
                retry_members(self, operations, on_error, results).await?;
                return Ok(());
            }
        };
        match key {
            qql_plan::BatchKey::Query { .. } => {
                let retry = match shaped_query_batch(&response) {
                    Ok(items) => {
                        if let Err(err) =
                            verify_batch_cardinality("query", operations.len(), items.len())
                        {
                            if on_error == WasmOnError::Stop {
                                return Err(qql_err_to_js(err));
                            }
                            true
                        } else {
                            results.extend(items);
                            false
                        }
                    }
                    Err(err) => {
                        if on_error == WasmOnError::Stop {
                            return Err(qql_err_to_js(err));
                        }
                        true
                    }
                };
                if retry {
                    retry_members(self, operations, on_error, results).await?;
                }
            }
            qql_plan::BatchKey::Mutation { .. } => {
                let retry = match shaped_update_batch(&response, &operations) {
                    Ok(items) => {
                        if let Err(err) =
                            verify_batch_cardinality("update", operations.len(), items.len())
                        {
                            if on_error == WasmOnError::Stop {
                                return Err(qql_err_to_js(err));
                            }
                            true
                        } else {
                            results.extend(items);
                            false
                        }
                    }
                    Err(err) => {
                        if on_error == WasmOnError::Stop {
                            return Err(qql_err_to_js(err));
                        }
                        true
                    }
                };
                if retry {
                    retry_members(self, operations, on_error, results).await?;
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
            PlannedOperation, into_query_batch, into_update_batch, verify_batch_cardinality,
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
                // Owned build: each `QueryRequest` moves (zero clones on the
                // hot path). The group's read opts are uniform by construction
                // (they are part of the batch key) and ride the batch header.
                let (collection, opts, batch) =
                    into_query_batch(operations).map_err(qql_err_to_js)?;
                let expected = batch.searches.len();
                let mut read_opts = Vec::new();
                qql_plan::query::push_read_opts(
                    &mut read_opts,
                    opts.timeout,
                    opts.consistency.as_ref(),
                );
                let mut path = format!("/collections/{collection}/points/query/batch");
                if !read_opts.is_empty() {
                    path.push('?');
                    path.push_str(
                        &read_opts
                            .iter()
                            .map(|(k, v)| format!("{k}={v}"))
                            .collect::<Vec<_>>()
                            .join("&"),
                    );
                }
                let body = serde_json::to_value(&batch).map_err(|error| {
                    qql_err_to_js(QqlError::execution("QQL-JSON", error.to_string(), None))
                })?;
                let retry = match self.send_json("POST", &path, Some(body)).await {
                    Ok(response) => match shaped_query_batch(&response) {
                        Ok(items) => {
                            if let Err(err) =
                                verify_batch_cardinality("query", expected, items.len())
                            {
                                if on_error == WasmOnError::Stop {
                                    return Err(qql_err_to_js(err));
                                }
                                true
                            } else {
                                results.extend(items);
                                false
                            }
                        }
                        Err(err) => {
                            if on_error == WasmOnError::Stop {
                                return Err(qql_err_to_js(err));
                            }
                            true
                        }
                    },
                    Err(error) => {
                        if on_error == WasmOnError::Stop {
                            return Err(error);
                        }
                        true
                    }
                };
                if retry {
                    for request in batch.searches {
                        self.dispatch_or_collect(
                            PlannedOperation::Query {
                                collection: collection.clone(),
                                request,
                            },
                            on_error,
                            results,
                        )
                        .await?;
                    }
                }
            }
            _ => {
                // Owned build: each mutation request moves (zero clones on the
                // hot path). The group's effective `wait` is uniform by
                // construction (it is part of the batch key).
                let (collection, _labels, opts, batch) =
                    into_update_batch(operations).map_err(qql_err_to_js)?;
                let expected = batch.operations.len();
                let wait = opts.wait.unwrap_or(true);
                let path = format!("/collections/{collection}/points/batch?wait={wait}");
                let body = serde_json::to_value(&batch).map_err(|error| {
                    qql_err_to_js(QqlError::execution("QQL-JSON", error.to_string(), None))
                })?;
                let retry = match self.send_json("POST", &path, Some(body)).await {
                    Ok(response) => match parsed_update_batch(&response) {
                        Ok(items) => {
                            if let Err(err) =
                                verify_batch_cardinality("update", expected, items.len())
                            {
                                if on_error == WasmOnError::Stop {
                                    return Err(qql_err_to_js(err));
                                }
                                true
                            } else {
                                for (update, item) in batch.operations.iter().zip(items) {
                                    results.push(
                                        shaped_update_response(update, item)
                                            .map_err(qql_err_to_js)?,
                                    );
                                }
                                false
                            }
                        }
                        Err(err) => {
                            if on_error == WasmOnError::Stop {
                                return Err(qql_err_to_js(err));
                            }
                            true
                        }
                    },
                    Err(error) => {
                        if on_error == WasmOnError::Stop {
                            return Err(error);
                        }
                        true
                    }
                };
                if retry {
                    for op in batch.operations {
                        self.dispatch_or_collect(
                            qql_plan::mutation::update_operation_into_planned(
                                &collection,
                                op,
                                wait,
                            ),
                            on_error,
                            results,
                        )
                        .await?;
                    }
                }
            }
        }
        Ok(())
    }
}
