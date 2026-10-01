//! Batch execution pipeline: scripts, preparation, batching, dispatch.

use qql_core::error::QqlError;
use qql_core::parser::Parser;
use wasm_bindgen::prelude::*;

use super::client::Client;
use super::functions::{is_transport_error, js_err, qql_err_to_js, thrown_message};
use super::params::WasmOnError;
use super::report::{
    WasmReport, exec_response, parsed_update_batch, shaped_query_batch, shaped_success_response,
    shaped_update_batch, shaped_update_response,
};

/// One ERROR result per batch member, preserving each operation's label.
fn push_batch_failures(
    results: &mut Vec<serde_json::Value>,
    labels: &[&'static str],
    message: &str,
) {
    for label in labels {
        results.push(exec_response(false, label, message, None));
    }
}

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
            return Err(js_err(
                "QQL-VALIDATION-EMPTY-SCRIPT",
                "no statements to execute; the query is empty or contains only whitespace",
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
                        &thrown_message(&error),
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
        // `CREATE COLLECTION` with a deferred PATCH and/or custom shard keys
        // has no single REST route: the plan owns the step sequence (PUT body,
        // conditional PATCH, per-key shard PUTs) and this host sends the steps
        // through its fetch path, mirroring the native REST adapter.
        if let qql_plan::PlannedOperation::CreateCollection {
            collection,
            request,
        } = operation
            && qql_plan::ddl::create_collection_needs_multi_step(request)
        {
            let steps = qql_plan::ddl::create_collection_rest_steps(collection, request).map_err(
                |error| {
                    qql_err_to_js(QqlError::execution(
                        "QQL-PLAN-SERIALIZE",
                        format!("plan IR REST request body serialization failed: {error}"),
                        None,
                    ))
                },
            )?;
            let mut last_envelope = None;
            for step in steps {
                last_envelope = Some(
                    self.send_json(step.method.as_str(), &step.path, Some(step.body))
                        .await?,
                );
            }
            let envelope = last_envelope.ok_or_else(|| {
                qql_err_to_js(QqlError::execution(
                    "QQL-PLAN-SERIALIZE",
                    "create collection projected to no REST steps",
                    None,
                ))
            })?;
            return shaped_success_response(operation, &envelope).map_err(qql_err_to_js);
        }
        let route =
            qql_plan::to_rest_route(operation).map_err(|err| qql_err_to_js(err.to_qql_error()))?;
        let result = self
            .send_json(route.method.as_str(), &route.path, route.body_json())
            .await?;
        shaped_success_response(operation, &result).map_err(qql_err_to_js)
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
                &thrown_message(&error),
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
    ///
    /// **Retry policy (at-least-once):** only connection/timeout failures
    /// re-run members individually; a batch the server answered with a
    /// cardinality mismatch or an unparsable shape surfaces
    /// `QQL-BACKEND-BATCH` / `QQL-BACKEND-ENVELOPE` for the members instead
    /// of re-executing side effects that may already have landed.
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
            return Err(js_err(
                "QQL-BIND-TYPE-MISMATCH",
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
        let labels: Vec<&'static str> = operations
            .iter()
            .map(qql_plan::PlannedOperation::operation_label)
            .collect();
        let response = match self.send_json(&route_method, &path, body).await {
            Ok(response) => response,
            Err(error) => {
                if on_error == WasmOnError::Stop {
                    return Err(error);
                }
                if is_transport_error(&error) {
                    for operation in operations {
                        self.dispatch_or_collect(operation, on_error, results)
                            .await?;
                    }
                } else {
                    push_batch_failures(results, &labels, &thrown_message(&error));
                }
                return Ok(());
            }
        };
        let expected = operations.len();
        let batch_error = match key {
            qql_plan::BatchKey::Query { .. } => match shaped_query_batch(&response) {
                Ok(items) => match verify_batch_cardinality("query", expected, items.len()) {
                    Ok(()) => {
                        results.extend(items);
                        None
                    }
                    Err(err) => Some(err),
                },
                Err(err) => Some(err),
            },
            qql_plan::BatchKey::Mutation { .. } => {
                match shaped_update_batch(&response, &operations) {
                    Ok(items) => match verify_batch_cardinality("update", expected, items.len()) {
                        Ok(()) => {
                            results.extend(items);
                            None
                        }
                        Err(err) => Some(err),
                    },
                    Err(err) => Some(err),
                }
            }
        };
        if let Some(err) = batch_error {
            // The server answered; a member retry cannot fix a shape mismatch
            // and would repeat side effects (at-least-once is documented on
            // `Client.execute`).
            if on_error == WasmOnError::Stop {
                return Err(qql_err_to_js(err));
            }
            push_batch_failures(results, &labels, &err.to_string());
        }
        Ok(())
    }
    /// Flush an auto-grouped batch. Same at-least-once policy as
    /// [`Self::execute_batch_op`]: transport failures may re-run members
    /// individually; an answered shape mismatch surfaces the batch error.
    pub(crate) async fn flush_planned_group(
        &self,
        mut operations: Vec<qql_plan::PlannedOperation>,
        on_error: WasmOnError,
        results: &mut Vec<serde_json::Value>,
    ) -> Result<(), JsValue> {
        use qql_plan::{PlannedOperation, into_query_batch, into_update_batch};

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
                    qql_err_to_js(QqlError::execution(
                        "QQL-SERIALIZE",
                        error.to_string(),
                        None,
                    ))
                })?;
                let response = match self.send_json("POST", &path, Some(body)).await {
                    Ok(response) => response,
                    Err(error) => {
                        if on_error == WasmOnError::Stop {
                            return Err(error);
                        }
                        if is_transport_error(&error) {
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
                        } else {
                            for _ in 0..expected {
                                results.push(exec_response(
                                    false,
                                    "QUERY",
                                    &thrown_message(&error),
                                    None,
                                ));
                            }
                        }
                        return Ok(());
                    }
                };
                let batch_error = match shaped_query_batch(&response) {
                    Ok(items) => {
                        match qql_plan::verify_batch_cardinality("query", expected, items.len()) {
                            Ok(()) => {
                                results.extend(items);
                                None
                            }
                            Err(err) => Some(err),
                        }
                    }
                    Err(err) => Some(err),
                };
                if let Some(err) = batch_error {
                    if on_error == WasmOnError::Stop {
                        return Err(qql_err_to_js(err));
                    }
                    for _ in 0..expected {
                        results.push(exec_response(false, "QUERY", &err.to_string(), None));
                    }
                }
            }
            _ => {
                // Owned build: each mutation request moves (zero clones on the
                // hot path). The group's effective `wait` is uniform by
                // construction (it is part of the batch key).
                let (collection, labels, opts, batch) =
                    into_update_batch(operations).map_err(qql_err_to_js)?;
                let expected = batch.operations.len();
                let wait = opts.wait.unwrap_or(true);
                let path = format!("/collections/{collection}/points/batch?wait={wait}");
                let body = serde_json::to_value(&batch).map_err(|error| {
                    qql_err_to_js(QqlError::execution(
                        "QQL-SERIALIZE",
                        error.to_string(),
                        None,
                    ))
                })?;
                let response = match self.send_json("POST", &path, Some(body)).await {
                    Ok(response) => response,
                    Err(error) => {
                        if on_error == WasmOnError::Stop {
                            return Err(error);
                        }
                        if is_transport_error(&error) {
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
                        } else {
                            push_batch_failures(results, &labels, &thrown_message(&error));
                        }
                        return Ok(());
                    }
                };
                let batch_error = match parsed_update_batch(&response) {
                    Ok(items) => {
                        match qql_plan::verify_batch_cardinality("update", expected, items.len()) {
                            Ok(()) => {
                                for (update, item) in batch.operations.iter().zip(items) {
                                    results.push(
                                        shaped_update_response(update, item)
                                            .map_err(qql_err_to_js)?,
                                    );
                                }
                                None
                            }
                            Err(err) => Some(err),
                        }
                    }
                    Err(err) => Some(err),
                };
                if let Some(err) = batch_error {
                    if on_error == WasmOnError::Stop {
                        return Err(qql_err_to_js(err));
                    }
                    push_batch_failures(results, &labels, &err.to_string());
                }
            }
        }
        Ok(())
    }
}
