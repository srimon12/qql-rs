use qql_core::ast::Stmt;
use qql_core::error::QqlError;
use qql_core::parser;
use qql_plan::{BatchGrouper, PlannedOperation, plan};

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
                // Explicit BATCH blocks never merge with ambient groups: flush
                // whatever is pending, then run members as one forced group.
                if matches!(single, PlannedOperation::Batch { .. }) {
                    if let Some(ops) = grouper.finish() {
                        self.flush_planned_group(ops, stop_on_error, &mut results)
                            .await?;
                    }
                    self.execute_batch_op(&single, stop_on_error, &mut results)
                        .await?;
                } else {
                    self.dispatch_or_collect(single, stop_on_error, &mut results)
                        .await?;
                }
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
            // Ambient groups carry no header opts: per-search read opts stay
            // on the member requests (gRPC) as before.
            match self
                .client
                .execute_query_batch(&collection, &batch, None, None)
                .await
            {
                Ok(responses) if responses.len() == expected => {
                    for (response, op) in responses.into_iter().zip(operations.iter()) {
                        // Batch items normalize exactly like singly dispatched
                        // operations; REST parses each item at its boundary.
                        results.push(Self::normalize_planned(op, response)?);
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

        let (collection, _labels, batch) = qql_plan::build_update_batch(&operations)?;
        let expected = batch.operations.len();
        // Ambient groups always wait, preserving prior behavior.
        match self
            .client
            .execute_update_batch(&collection, &batch, true)
            .await
        {
            Ok(responses) if responses.len() == expected => {
                for (response, op) in responses.into_iter().zip(operations.iter()) {
                    // Same normalization as single dispatch: upserts report
                    // their request point count, other writes are status-only.
                    results.push(Self::normalize_planned(op, response)?);
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

    /// Run an explicit `BATCH` block as one forced group with header opts.
    ///
    /// Unlike ambient groups, members always take the batch RPC path (even a
    /// lone member, so header `WAIT` / `PARAMS` are never dropped) and never
    /// merge with neighboring statements. Per-member responses normalize
    /// exactly like grouped results.
    pub(crate) async fn execute_batch_op(
        &self,
        op: &qql_plan::PlannedOperation,
        stop_on_error: bool,
        results: &mut Vec<ExecResponse>,
    ) -> Result<(), QqlError> {
        use qql_plan::PlannedOperation;
        let PlannedOperation::Batch {
            key,
            operations,
            wait,
            timeout,
            consistency,
        } = op
        else {
            return Err(QqlError::execution(
                "QQL-BATCH-INVARIANT",
                "execute_batch_op requires a Batch operation",
                None,
            ));
        };
        match key {
            qql_plan::BatchKey::Query(collection) => {
                let (_, batch) = qql_plan::build_query_batch(operations)?;
                let expected = batch.searches.len();
                match self
                    .client
                    .execute_query_batch(collection, &batch, *timeout, consistency.clone())
                    .await
                {
                    Ok(responses) if responses.len() == expected => {
                        for (response, op) in responses.into_iter().zip(operations.iter()) {
                            results.push(Self::normalize_planned(op, response)?);
                        }
                    }
                    Ok(responses) => {
                        let error =
                            qql_plan::verify_batch_cardinality("query", expected, responses.len())
                                .unwrap_err();
                        if stop_on_error {
                            return Err(error);
                        }
                        self.retry_forced_members_individually(operations.clone(), results)
                            .await?;
                    }
                    Err(error) => {
                        if stop_on_error {
                            return Err(error);
                        }
                        self.retry_forced_members_individually(operations.clone(), results)
                            .await?;
                    }
                }
            }
            qql_plan::BatchKey::Mutation(collection) => {
                let (_, _, batch) = qql_plan::build_update_batch(operations)?;
                let expected = batch.operations.len();
                match self
                    .client
                    .execute_update_batch(collection, &batch, wait.unwrap_or(true))
                    .await
                {
                    Ok(responses) if responses.len() == expected => {
                        for (response, op) in responses.into_iter().zip(operations.iter()) {
                            results.push(Self::normalize_planned(op, response)?);
                        }
                    }
                    Ok(responses) => {
                        let error =
                            qql_plan::verify_batch_cardinality("update", expected, responses.len())
                                .unwrap_err();
                        if stop_on_error {
                            return Err(error);
                        }
                        self.retry_forced_members_individually(operations.clone(), results)
                            .await?;
                    }
                    Err(error) => {
                        if stop_on_error {
                            return Err(error);
                        }
                        self.retry_forced_members_individually(operations.clone(), results)
                            .await?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Retry forced batch members one request at a time.
    ///
    /// Members are never `Batch` or client-side ops (the planner rejects
    /// those), so this dispatches straight through [`Self::dispatch_raw`]
    /// instead of [`Self::dispatch_or_collect`] — keeping
    /// [`Self::dispatch_planned`] non-recursive.
    async fn retry_forced_members_individually(
        &self,
        operations: Vec<qql_plan::PlannedOperation>,
        results: &mut Vec<ExecResponse>,
    ) -> Result<(), QqlError> {
        for operation in operations {
            match self.dispatch_raw(&operation).await {
                Ok(response) => results.push(Self::normalize_planned(&operation, response)?),
                Err(error) => results.push(ExecResponse {
                    ok: false,
                    operation: operation.operation_label().to_string(),
                    message: error.to_string(),
                    data: None,
                    telemetry: None,
                }),
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
}
