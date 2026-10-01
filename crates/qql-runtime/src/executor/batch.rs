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
        let mut pending = Vec::with_capacity(queries.len());
        let mut results = Vec::with_capacity(queries.len());
        for query in queries {
            match parser::Parser::parse_all(query) {
                Ok(parsed) => pending.extend(parsed),
                Err(error) => {
                    if !pending.is_empty() {
                        results.extend(
                            self.execute_batch_nodes(core::mem::take(&mut pending), on_error)
                                .await?,
                        );
                    }
                    if matches!(on_error, OnError::Stop) {
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
            results.extend(self.execute_batch_nodes(pending, on_error).await?);
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
        on_error: OnError,
    ) -> Result<Vec<ExecResponse>, QqlError> {
        self.ensure_open()?;
        self.with_timeout(self.execute_batch_nodes_inner(stmts, on_error))
            .await
    }

    async fn execute_batch_nodes_inner(
        &self,
        stmts: Vec<Stmt>,
        on_error: OnError,
    ) -> Result<Vec<ExecResponse>, QqlError> {
        let stop_on_error = matches!(on_error, OnError::Stop);
        let mut results = Vec::with_capacity(stmts.len());
        let mut grouper = BatchGrouper::new();

        for stmt in stmts {
            if let Err(e) = qql_plan::ensure_no_unbound_params(&stmt) {
                if let Some(ops) = grouper.flush_on_error() {
                    self.flush_planned_group(ops, on_error, &mut results)
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
                self.flush_planned_group(ops, on_error, &mut results)
                    .await?;
            }

            let prepared = match self.prepare_statement(stmt).await {
                Ok(p) => p,
                Err(e) => {
                    if let Some(ops) = grouper.flush_on_error() {
                        self.flush_planned_group(ops, on_error, &mut results)
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
                        self.flush_planned_group(ops, on_error, &mut results)
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
                self.flush_planned_group(ops, on_error, &mut results)
                    .await?;
            }
            if let Some(single) = dispatch_single {
                // Explicit BATCH blocks never merge with ambient groups: flush
                // whatever is pending, then run members as one forced group.
                if matches!(single, PlannedOperation::Batch { .. }) {
                    if let Some(ops) = grouper.finish() {
                        self.flush_planned_group(ops, on_error, &mut results)
                            .await?;
                    }
                    self.execute_batch_op(&single, on_error, &mut results)
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
        Ok(results)
    }

    pub(crate) async fn dispatch_or_collect(
        &self,
        planned: qql_plan::PlannedOperation,
        on_error: OnError,
        results: &mut Vec<ExecResponse>,
    ) -> Result<(), QqlError> {
        let stop_on_error = matches!(on_error, OnError::Stop);
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
        on_error: OnError,
        results: &mut Vec<ExecResponse>,
    ) -> Result<(), QqlError> {
        use qql_plan::PlannedOperation;
        let stop_on_error = matches!(on_error, OnError::Stop);

        if operations.is_empty() {
            return Ok(());
        }
        if operations.len() == 1 {
            let planned = operations.pop().expect("pending contains one operation");
            return self.dispatch_or_collect(planned, on_error, results).await;
        }
        let is_query = matches!(&operations[0], PlannedOperation::Query { .. });
        if is_query {
            // Owned build: each `QueryRequest` moves (zero clones on the hot
            // path). The group's read opts are uniform by construction (they
            // are part of the batch key) and ride the batch RPC header.
            let (collection, opts, batch) = qql_plan::into_query_batch(operations)?;
            let expected = batch.searches.len();
            let retry = match self
                .client
                .execute_query_batch(&collection, &batch, opts.timeout, opts.consistency)
                .await
            {
                Ok(responses) if responses.len() == expected => {
                    for response in responses {
                        results.push(super::normalize::normalize_query_item(response)?);
                    }
                    false
                }
                Ok(responses) => {
                    // The server answered with the wrong item count: it is a
                    // backend contract violation, not a transport failure, so
                    // the group is never retried. Report one failure per
                    // expected member.
                    if let Err(error) =
                        qql_plan::verify_batch_cardinality("query", expected, responses.len())
                    {
                        if stop_on_error {
                            return Err(error);
                        }
                        for _ in 0..expected {
                            results.push(ExecResponse {
                                ok: false,
                                operation: "QUERY".to_string(),
                                message: error.to_string(),
                                data: None,
                                telemetry: None,
                            });
                        }
                    }
                    false
                }
                Err(error) => {
                    if stop_on_error {
                        return Err(error);
                    }
                    true
                }
            };
            if retry {
                // Transport failure: the batch RPC may have applied some or
                // all members server-side, so individual retries are
                // at-least-once. Every QQL mutation is idempotent in effect
                // (upsert/delete/set/overwrite/clear/delete payload/vectors),
                // which is what makes this safe today.
                std::hint::cold_path();
                let operations = batch
                    .searches
                    .into_iter()
                    .map(|request| PlannedOperation::Query {
                        collection: collection.clone(),
                        request,
                    })
                    .collect();
                self.retry_batch_individually(operations, results).await?;
            }
        } else {
            let mut collection: Option<String> = None;
            let mut run: Vec<qql_plan::PlannedOperation> = Vec::new();
            for operation in operations {
                // Borrow the member key for comparison: only the first
                // member's key is owned (one alloc per group instead of one
                // per member); the rest compare as `&str`.
                let op_collection = operation.collection();
                if collection
                    .as_deref()
                    .is_some_and(|c| Some(c) != op_collection)
                {
                    return Err(QqlError::execution(
                        "QQL-BATCH-INVARIANT",
                        "mutation batch contained multiple collections",
                        None,
                    ));
                }
                if collection.is_none() {
                    collection = op_collection.map(str::to_owned);
                }
                if operation.batch_family() == qql_plan::BatchFamily::Mutation {
                    run.push(operation);
                } else {
                    self.flush_update_run(core::mem::take(&mut run), on_error, results)
                        .await?;
                    self.dispatch_or_collect(operation, on_error, results)
                        .await?;
                }
            }
            self.flush_update_run(run, on_error, results).await?;
        }
        Ok(())
    }

    async fn flush_update_run(
        &self,
        operations: Vec<qql_plan::PlannedOperation>,
        on_error: OnError,
        results: &mut Vec<ExecResponse>,
    ) -> Result<(), QqlError> {
        let stop_on_error = matches!(on_error, OnError::Stop);
        if operations.is_empty() {
            return Ok(());
        }
        if operations.len() == 1 {
            let planned = operations
                .into_iter()
                .next()
                .expect("run contains one operation");
            return self.dispatch_or_collect(planned, on_error, results).await;
        }

        // Owned build: each mutation request moves (zero clones on the hot
        // path). The group's effective `wait` is uniform by construction (it
        // is part of the batch key) and rides the batch RPC header.
        let (collection, _, opts, batch) = qql_plan::into_update_batch(operations)?;
        let expected = batch.operations.len();
        let wait = opts.wait.unwrap_or(true);
        let retry = match self
            .client
            .execute_update_batch(&collection, &batch, wait)
            .await
        {
            Ok(responses) if responses.len() == expected => {
                for (response, op) in responses.into_iter().zip(batch.operations.iter()) {
                    // Same normalization as single dispatch: upserts report
                    // their request point count, other writes are status-only.
                    results.push(super::normalize::normalize_update_item(op, response)?);
                }
                false
            }
            Ok(responses) => {
                // Server-answered cardinality mismatch: never retried (the
                // server may already have applied members). One failure per
                // expected member.
                if let Err(error) =
                    qql_plan::verify_batch_cardinality("update", expected, responses.len())
                {
                    if stop_on_error {
                        return Err(error);
                    }
                    for op in &batch.operations {
                        results.push(ExecResponse {
                            ok: false,
                            operation: op.operation_name().to_string(),
                            message: error.to_string(),
                            data: None,
                            telemetry: None,
                        });
                    }
                }
                false
            }
            Err(error) => {
                if stop_on_error {
                    return Err(error);
                }
                true
            }
        };
        if retry {
            // Transport failure: see the query arm for the at-least-once
            // rationale.
            std::hint::cold_path();
            let operations = batch
                .operations
                .into_iter()
                .map(|op| qql_plan::mutation::update_operation_into_planned(&collection, op, wait))
                .collect();
            self.retry_batch_individually(operations, results).await?;
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
        on_error: OnError,
        results: &mut Vec<ExecResponse>,
    ) -> Result<(), QqlError> {
        use qql_plan::PlannedOperation;
        let stop_on_error = matches!(on_error, OnError::Stop);
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
            qql_plan::BatchKey::Query { collection, .. } => {
                let (_, _, batch) = qql_plan::build_query_batch(operations)?;
                let expected = batch.searches.len();
                match self
                    .client
                    .execute_query_batch(collection, &batch, *timeout, consistency.clone())
                    .await
                {
                    Ok(responses) if responses.len() == expected => {
                        for response in responses {
                            results.push(super::normalize::normalize_query_item(response)?);
                        }
                    }
                    Ok(responses) => {
                        // Server-answered cardinality mismatch: no retry.
                        if let Err(error) = qql_plan::verify_batch_cardinality(
                            "query",
                            expected,
                            responses.len(),
                        ) {
                            if stop_on_error {
                                return Err(error);
                            }
                            for operation in operations {
                                results.push(ExecResponse {
                                    ok: false,
                                    operation: operation.operation_label().to_string(),
                                    message: error.to_string(),
                                    data: None,
                                    telemetry: None,
                                });
                            }
                        }
                    }
                    Err(error) => {
                        if stop_on_error {
                            return Err(error);
                        }
                        self.retry_forced_members_individually(operations, results)
                            .await?;
                    }
                }
            }
            qql_plan::BatchKey::Mutation { collection, .. } => {
                let (_, _, _, batch) = qql_plan::build_update_batch(operations)?;
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
                        // Server-answered cardinality mismatch: no retry.
                        if let Err(error) = qql_plan::verify_batch_cardinality(
                            "update",
                            expected,
                            responses.len(),
                        ) {
                            if stop_on_error {
                                return Err(error);
                            }
                            for operation in operations {
                                results.push(ExecResponse {
                                    ok: false,
                                    operation: operation.operation_label().to_string(),
                                    message: error.to_string(),
                                    data: None,
                                    telemetry: None,
                                });
                            }
                        }
                    }
                    Err(error) => {
                        if stop_on_error {
                            return Err(error);
                        }
                        self.retry_forced_members_individually(operations, results)
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
        operations: &[qql_plan::PlannedOperation],
        results: &mut Vec<ExecResponse>,
    ) -> Result<(), QqlError> {
        for operation in operations {
            match self.dispatch_raw(operation).await {
                Ok(response) => results.push(Self::normalize_planned(operation, response)?),
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
            self.dispatch_or_collect(operation, OnError::Continue, results)
                .await?;
        }
        Ok(())
    }
}
