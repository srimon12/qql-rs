use qql_core::ast::Stmt;
use qql_core::error::QqlError;
use qql_core::parser;
use qql_plan::{BatchGrouper, plan};

use crate::executor::dml::query::extract_search_hits;
use crate::executor::response::serialize_hits;
use crate::executor::{ExecResponse, ExecutionReport, Executor, OnError};

impl Executor {
    /// Parse every list entry to AST and run the unified prepared batch path.
    /// Contiguous same-collection operations are smart-batched just as for
    /// multi-statement scripts (RUN-013).
    pub async fn execute_batch(
        &self,
        queries: &[&str],
        on_error: OnError,
    ) -> Result<ExecutionReport, QqlError> {
        self.ensure_open()?;
        let stop_on_error = matches!(on_error, OnError::Stop);
        let mut pending = Vec::with_capacity(queries.len());
        let mut results = Vec::with_capacity(queries.len());
        for query in queries {
            match parser::Parser::parse_all(query) {
                Ok(parsed) => pending.extend(parsed),
                Err(error) => {
                    if !pending.is_empty() {
                        results.extend(
                            self.execute_batch_nodes(core::mem::take(&mut pending), stop_on_error)
                                .await?,
                        );
                    }
                    if stop_on_error {
                        return Err(error);
                    }
                    results.push(ExecResponse {
                        ok: false,
                        operation: "PARSE".to_string(),
                        message: error.to_string(),
                        data: None,
                        telemetry: None,
                        typed_hits: std::sync::OnceLock::new(),
                    });
                }
            }
        }
        if !pending.is_empty() {
            results.extend(self.execute_batch_nodes(pending, stop_on_error).await?);
        }
        if results.is_empty() {
            return Err(QqlError::validation(
                "QQL-VALIDATION-EMPTY-SCRIPT",
                "no statements to execute; the query list is empty or every entry is empty",
                None,
            ));
        }
        Ok(ExecutionReport::from_results(results))
    }

    /// Execute already-parsed statements through the unified batch path,
    /// honoring the configured request timeout.
    pub async fn execute_batch_nodes(
        &self,
        stmts: Vec<Stmt>,
        stop_on_error: bool,
    ) -> Result<Vec<ExecResponse>, QqlError> {
        self.ensure_open()?;
        if let Some(secs) = self.request_timeout() {
            match tokio::time::timeout(
                std::time::Duration::from_secs(secs),
                self.execute_batch_nodes_inner(stmts, stop_on_error),
            )
            .await
            {
                Ok(res) => res,
                Err(_) => Err(QqlError::transport(
                    "QQL-TIMEOUT",
                    format!("batch execution timed out after {secs}s"),
                    None,
                )),
            }
        } else {
            self.execute_batch_nodes_inner(stmts, stop_on_error).await
        }
    }

    async fn execute_batch_nodes_inner(
        &self,
        stmts: Vec<Stmt>,
        stop_on_error: bool,
    ) -> Result<Vec<ExecResponse>, QqlError> {
        let mut results = Vec::with_capacity(stmts.len());
        let mut grouper = BatchGrouper::new();

        for stmt in stmts {
            if let Err(e) = qql_plan::ensure_no_unbound_params(&stmt) {
                if let Some(ops) = grouper.flush_on_error() {
                    self.flush_planned_group(ops, stop_on_error, &mut results)
                        .await?;
                }
                if stop_on_error {
                    return Err(e);
                }
                results.push(ExecResponse {
                    ok: false,
                    operation: "BIND".to_string(),
                    message: e.to_string(),
                    data: None,
                    telemetry: None,
                    typed_hits: std::sync::OnceLock::new(),
                });
                continue;
            }

            if let Some(ops) = grouper.check_statement_barrier(&stmt) {
                self.flush_planned_group(ops, stop_on_error, &mut results)
                    .await?;
            }

            let prepared = match self.prepare_statement(stmt).await {
                Ok(p) => p,
                Err(e) => {
                    if let Some(ops) = grouper.flush_on_error() {
                        self.flush_planned_group(ops, stop_on_error, &mut results)
                            .await?;
                    }
                    if stop_on_error {
                        return Err(e);
                    }
                    results.push(ExecResponse {
                        ok: false,
                        operation: "PREPARE".to_string(),
                        message: e.to_string(),
                        data: None,
                        telemetry: None,
                        typed_hits: std::sync::OnceLock::new(),
                    });
                    continue;
                }
            };

            let planned = match plan(&prepared) {
                Ok(planned) => planned,
                Err(e) => {
                    if let Some(ops) = grouper.flush_on_error() {
                        self.flush_planned_group(ops, stop_on_error, &mut results)
                            .await?;
                    }
                    if stop_on_error {
                        return Err(e);
                    }
                    results.push(ExecResponse {
                        ok: false,
                        operation: "PLAN".to_string(),
                        message: e.to_string(),
                        data: None,
                        telemetry: None,
                        typed_hits: std::sync::OnceLock::new(),
                    });
                    continue;
                }
            };

            let (flush_ops, dispatch_single) = grouper.push_planned(planned);
            if let Some(ops) = flush_ops {
                self.flush_planned_group(ops, stop_on_error, &mut results)
                    .await?;
            }
            if let Some(single) = dispatch_single {
                self.dispatch_or_collect(single, stop_on_error, &mut results)
                    .await?;
            }
        }

        if let Some(ops) = grouper.finish() {
            self.flush_planned_group(ops, stop_on_error, &mut results)
                .await?;
        }
        Ok(results)
    }

    pub(crate) async fn dispatch_or_collect(
        &self,
        planned: qql_plan::PlannedOperation,
        stop_on_error: bool,
        results: &mut Vec<ExecResponse>,
    ) -> Result<(), QqlError> {
        match self.dispatch_planned(&planned).await {
            Ok(response) => results.push(response),
            Err(error) if stop_on_error => return Err(error),
            Err(error) => results.push(ExecResponse {
                ok: false,
                operation: planned.operation_label().to_string(),
                message: error.to_string(),
                data: None,
                telemetry: None,
                typed_hits: std::sync::OnceLock::new(),
            }),
        }
        Ok(())
    }

    async fn flush_planned_group(
        &self,
        mut operations: Vec<qql_plan::PlannedOperation>,
        stop_on_error: bool,
        results: &mut Vec<ExecResponse>,
    ) -> Result<(), QqlError> {
        use qql_plan::PlannedOperation;

        if operations.is_empty() {
            return Ok(());
        }
        if operations.len() == 1 {
            let planned = operations.pop().expect("pending contains one operation");
            return self
                .dispatch_or_collect(planned, stop_on_error, results)
                .await;
        }
        let is_query = matches!(&operations[0], PlannedOperation::Query { .. });
        if is_query {
            let (collection, batch) = qql_plan::build_query_batch(&operations)?;
            let expected = batch.searches.len();
            match self.client.execute_query_batch(&collection, &batch).await {
                Ok(responses) if responses.len() == expected => {
                    for mut value in responses {
                        if let Some(message) = Self::batch_item_error(&value) {
                            results.push(ExecResponse {
                                ok: false,
                                operation: "QUERY".to_string(),
                                message,
                                data: None,
                                telemetry: None,
                                typed_hits: std::sync::OnceLock::new(),
                            });
                            continue;
                        }
                        let (hits_val, count, typed) = {
                            let pts_opt = if let Some(serde_json::Value::Object(obj)) =
                                value.get_mut("result")
                            {
                                obj.remove("points")
                            } else if let Some(serde_json::Value::Array(_)) = value.get("result") {
                                value.as_object_mut().and_then(|o| o.remove("result"))
                            } else if let Some(obj) = value.as_object_mut() {
                                obj.remove("points")
                            } else {
                                None
                            };
                            if let Some(pts) = pts_opt {
                                let c = pts.as_array().map(|a| a.len()).unwrap_or(0);
                                (pts, c, None)
                            } else {
                                let hits = extract_search_hits(&value);
                                let c = hits.len();
                                let data = serialize_hits(&hits)?;
                                (data, c, Some(hits))
                            }
                        };
                        let mut response = ExecResponse {
                            ok: true,
                            operation: "QUERY".to_string(),
                            message: format!("Found {} hits", count),
                            data: Some(hits_val),
                            telemetry: None,
                            typed_hits: std::sync::OnceLock::new(),
                        };
                        if let Some(hits) = typed {
                            response = response.with_typed_hits(hits);
                        }
                        results.push(response);
                    }
                }
                Ok(responses) => {
                    let error =
                        qql_plan::verify_batch_cardinality("query", expected, responses.len())
                            .unwrap_err();
                    if stop_on_error {
                        return Err(error);
                    }
                    self.retry_batch_individually(operations, results).await?;
                }
                Err(error) => {
                    if stop_on_error {
                        return Err(error);
                    }
                    self.retry_batch_individually(operations, results).await?;
                }
            }
        } else {
            let mut collection: Option<String> = None;
            let mut run: Vec<qql_plan::PlannedOperation> = Vec::new();
            for operation in operations {
                let op_collection = operation.collection().map(str::to_owned);
                if collection
                    .as_ref()
                    .is_some_and(|c| Some(c.as_str()) != op_collection.as_deref())
                {
                    return Err(QqlError::execution(
                        "QQL-BATCH-INVARIANT",
                        "mutation batch contained multiple collections",
                        None,
                    ));
                }
                if collection.is_none() {
                    collection = op_collection;
                }
                if qql_plan::mutation::planned_to_update_operation(&operation).is_some() {
                    run.push(operation);
                } else {
                    self.flush_update_run(core::mem::take(&mut run), stop_on_error, results)
                        .await?;
                    self.dispatch_or_collect(operation, stop_on_error, results)
                        .await?;
                }
            }
            self.flush_update_run(run, stop_on_error, results).await?;
        }
        Ok(())
    }

    async fn flush_update_run(
        &self,
        operations: Vec<qql_plan::PlannedOperation>,
        stop_on_error: bool,
        results: &mut Vec<ExecResponse>,
    ) -> Result<(), QqlError> {
        if operations.is_empty() {
            return Ok(());
        }
        if operations.len() == 1 {
            let planned = operations
                .into_iter()
                .next()
                .expect("run contains one operation");
            return self
                .dispatch_or_collect(planned, stop_on_error, results)
                .await;
        }

        let (collection, labels, batch) = qql_plan::build_update_batch(&operations)?;
        let expected = batch.operations.len();
        match self.client.execute_update_batch(&collection, &batch).await {
            Ok(responses) if responses.len() == expected => {
                for (value, label) in responses.into_iter().zip(labels.iter()) {
                    if let Some(message) = Self::batch_item_error(&value) {
                        results.push(ExecResponse {
                            ok: false,
                            operation: (*label).to_string(),
                            message,
                            data: None,
                            telemetry: None,
                            typed_hits: std::sync::OnceLock::new(),
                        });
                        continue;
                    }
                    results.push(ExecResponse {
                        ok: true,
                        operation: (*label).to_string(),
                        message: format!("{label} ok (batched)"),
                        data: Some(value),
                        telemetry: None,
                        typed_hits: std::sync::OnceLock::new(),
                    });
                }
            }
            Ok(responses) => {
                let error = qql_plan::verify_batch_cardinality("update", expected, responses.len())
                    .unwrap_err();
                if stop_on_error {
                    return Err(error);
                }
                self.retry_batch_individually(operations, results).await?;
            }
            Err(error) => {
                if stop_on_error {
                    return Err(error);
                }
                self.retry_batch_individually(operations, results).await?;
            }
        }
        Ok(())
    }

    async fn retry_batch_individually(
        &self,
        operations: Vec<qql_plan::PlannedOperation>,
        results: &mut Vec<ExecResponse>,
    ) -> Result<(), QqlError> {
        for operation in operations {
            self.dispatch_or_collect(operation, false, results).await?;
        }
        Ok(())
    }

    fn batch_item_error(item: &serde_json::Value) -> Option<String> {
        qql_plan::batch_item_error(item)
    }
}
