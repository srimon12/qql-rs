use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json;

use qql_core::ast::{self, Stmt};
use qql_core::error::QqlError;
use qql_core::parser;
use qql_plan::plan;

use crate::config::QqlConfig;
use crate::embedder::Embedder;
use crate::executor::dml::query::extract_search_hits;

pub use qql_embed::resolve::{DENSE_VECTOR_NAME, SPARSE_VECTOR_NAME};
/// Collection vector name reserved for multivector (ColBERT) rerank vectors.
pub const RERANK_VECTOR_NAME: &str = "colbert";
/// Dense model used when a default embedder is required and none is configured.
pub const DENSE_MODEL_DEFAULT: &str = "sentence-transformers/all-minilm-l6-v2";
/// Sparse model id used when a default sparse embedder is required.
pub const SPARSE_MODEL_DEFAULT: &str = "qdrant/bm25";
/// Cross-encoder model used when `CROSS RERANK` runs without an explicit model.
pub const RERANK_MODEL_DEFAULT: &str = "answerdotai/answerai-colbert-small-v1";
/// Fallback dense vector dimension when no embedder is configured (all-minilm-l6-v2).
pub const DENSE_VECTOR_SIZE: u64 = 384;
/// Fallback per-token multivector dimension (ColBERT-small) when none is configured.
pub const RERANK_VECTOR_SIZE: u64 = 96;
/// Default inference mode (`local` fastembed; `remote` uses HTTP endpoints).
pub const INFERENCE_MODE_DEFAULT: &str = "local";

/// Single-statement execution outcome: status, operation label, message, and data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecResponse {
    /// Whether the statement succeeded.
    pub ok: bool,
    /// Operation label (e.g. `QUERY`, `UPSERT`, `PARSE`) for this result.
    pub operation: String,
    /// Human-readable summary or error text.
    pub message: String,
    /// JSON payload (search hits, raw result, or counts), when the operation returns data.
    pub data: Option<serde_json::Value>,
}

/// Canonical cross-SDK execution result.  Every `client.execute(…)` call
/// returns this shape regardless of input type (string / Stmt / array).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionReport {
    /// Whether every statement succeeded (`failed == 0`).
    pub ok: bool,
    /// One `ExecResponse` per statement, in execution order.
    pub results: Vec<ExecResponse>,
    /// Number of successful statements.
    pub succeeded: usize,
    /// Number of failed statements.
    pub failed: usize,
}

impl ExecutionReport {
    /// Create from a collection of `ExecResponse`s.  `ok` is `failed == 0`.
    pub fn from_results(results: Vec<ExecResponse>) -> Self {
        let succeeded = results.iter().filter(|r| r.ok).count();
        let failed = results.len() - succeeded;
        Self {
            ok: failed == 0,
            results,
            succeeded,
            failed,
        }
    }

    /// Convenience wrapper for a single `ExecResponse`.
    pub fn single(resp: ExecResponse) -> Self {
        let ok = resp.ok;
        Self {
            ok,
            results: vec![resp],
            succeeded: if ok { 1 } else { 0 },
            failed: if ok { 0 } else { 1 },
        }
    }

    /// A successful report with no results (empty script).
    pub fn empty() -> Self {
        Self {
            ok: true,
            results: Vec::new(),
            succeeded: 0,
            failed: 0,
        }
    }
}

/// Controls batch-execution behaviour when a statement fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum OnError {
    /// Halt immediately on the first error (default).
    #[default]
    Stop,
    /// Continue executing remaining statements, collecting error
    /// responses alongside successes.
    Continue,
}

use qql_plan::{BatchKey, statement_batch_key};

/// Normalized search hit returned inside `ExecResponse` data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    /// Point ID (integer or string/UUID).
    pub id: qql_plan::PlanPointId,
    /// Similarity or rerank score.
    pub score: f32,
    /// Payload text extracted for text-centric results, when present.
    pub text: Option<String>,
    /// Point payload when requested via `WITH PAYLOAD`.
    pub payload: Option<HashMap<String, serde_json::Value>>,
    /// Source collection. Populated by cross-collection operations (e.g.
    /// CROSS RERANK) so results are unambiguous when multiple collections
    /// share the same point id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collection: Option<String>,
}

/// Grouped query result: one group key with its ordered hits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupedSearchResult {
    /// Group key value as returned by Qdrant (JSON-typed).
    pub group_id: serde_json::Value,
    /// Search hits in this group, in backend order.
    pub hits: Vec<SearchHit>,
}

pub use crate::client::*;

/// Serialize search hits into the `data` envelope of an `ExecResponse`.
///
/// Serialization cannot currently fail for [`SearchHit`]'s field types, but
/// failing loudly here beats silently emitting `null` data on a future type
/// change.
fn serialize_hits(hits: &[SearchHit]) -> Result<serde_json::Value, QqlError> {
    serde_json::to_value(hits).map_err(|error| {
        QqlError::execution(
            "QQL-RESPONSE-SERIALIZE",
            format!("failed to serialize search results: {error}"),
            None,
        )
    })
}

/// The QQL executor: prepare (schema `USING` resolution + embeddings) → `plan()`
/// → batch classification → dispatch over a `QdrantOps` backend.
pub struct Executor {
    pub(crate) client: Box<dyn QdrantOps>,
    pub(crate) config: Option<QqlConfig>,
    pub(crate) embedder: Option<Arc<dyn Embedder>>,
    /// Set by [`Executor::close`]; every execution entry point fails after it.
    closed: std::sync::atomic::AtomicBool,
}

impl Executor {
    /// Creates an executor backed by Qdrant's REST API.
    ///
    /// The backend owns a reusable HTTP client. Applications that need custom
    /// proxy, TLS, tracing, or pool settings can construct `RestQdrant` with
    /// their own `reqwest::Client` and pass it to [`Self::new`] instead.
    #[cfg(feature = "rest")]
    pub fn rest(url: impl Into<String>, api_key: Option<String>) -> Result<Self, QqlError> {
        Ok(Self::new(
            Box::new(crate::rest::RestQdrant::new(url, api_key)),
            None,
        ))
    }

    /// Creates an executor backed by Qdrant's gRPC API.
    #[cfg(feature = "grpc")]
    pub fn grpc(url: &str, api_key: Option<String>) -> Result<Self, QqlError> {
        Ok(Self::new(
            Box::new(crate::grpc::GrpcQdrant::from_url(url, api_key)?),
            None,
        ))
    }

    /// Creates an executor around a custom `QdrantOps` backend with optional config.
    pub fn new(client: Box<dyn QdrantOps>, config: Option<QqlConfig>) -> Self {
        Executor {
            client,
            config,
            embedder: None,
            closed: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Like `new`, but with a pre-built embedder (e.g. `FastEmbedder` or `HttpEmbedder`).
    pub fn with_embedder(
        client: Box<dyn QdrantOps>,
        config: Option<QqlConfig>,
        embedder: Option<Arc<dyn Embedder>>,
    ) -> Self {
        Executor {
            client,
            config,
            embedder,
            closed: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Borrow the underlying backend ops (alias of `client`).
    pub fn ops(&self) -> &dyn QdrantOps {
        self.client.as_ref()
    }

    /// Explain a single QQL query string without executing it.
    pub fn explain(query: &str) -> Result<String, QqlError> {
        qql_core::explain::explain(query)
    }

    /// Explain every statement in a multi-statement script.
    pub fn explain_all(query: &str) -> Result<String, QqlError> {
        qql_core::explain::explain_all(query)
    }

    /// Explain an already-parsed statement.
    pub fn explain_node(stmt: &Stmt) -> Result<String, QqlError> {
        Ok(qql_core::explain::explain_node(stmt))
    }

    // --- explain_stmt removed --- moved to qql_core::explain

    /// Borrow the underlying backend `QdrantOps` implementation.
    pub fn client(&self) -> &dyn QdrantOps {
        self.client.as_ref()
    }

    /// Flush and release backend-owned resources. This is especially important
    /// for embedded backends before deleting their data directory.
    ///
    /// After `close`, every execution entry point fails with
    /// `QQL-CLIENT-CLOSED` — a closed client cannot run more statements.
    /// Calling `close` again is a no-op.
    pub async fn close(&self) -> Result<(), QqlError> {
        self.closed
            .store(true, std::sync::atomic::Ordering::Release);
        self.client.close().await
    }

    /// Whether [`Executor::close`] has been called.
    pub fn is_closed(&self) -> bool {
        self.closed.load(std::sync::atomic::Ordering::Acquire)
    }

    fn ensure_open(&self) -> Result<(), QqlError> {
        if self.is_closed() {
            return Err(QqlError::execution(
                "QQL-CLIENT-CLOSED",
                "client is closed; create a new Client to run more statements",
                None,
            ));
        }
        Ok(())
    }

    /// Borrow the configured embedder, if any.
    pub fn embedder(&self) -> Option<&Arc<dyn Embedder>> {
        self.embedder.as_ref()
    }

    /// Borrow the executor config, if any.
    pub fn config(&self) -> Option<&QqlConfig> {
        self.config.as_ref()
    }

    /// Configured request timeout in seconds, or `None` when disabled (`0`).
    pub fn request_timeout(&self) -> Option<u64> {
        self.config.as_ref().and_then(|c| {
            if c.request_timeout > 0 {
                Some(c.request_timeout)
            } else {
                None
            }
        })
    }

    /// Execute a QQL query string.  Semicolon-delimited multi-statement
    /// scripts are automatically detected, parsed, and executed in batch —
    /// contiguous same-collection QUERY statements use `/points/query/batch`,
    /// and contiguous same-collection mutations use `/points/batch`.
    ///
    /// Always returns a stable [`ExecutionReport`] for a single statement or
    /// semicolon-delimited script.
    pub async fn execute(
        &self,
        query: &str,
        on_error: OnError,
    ) -> Result<ExecutionReport, QqlError> {
        self.ensure_open()?;
        let stop_on_error = matches!(on_error, OnError::Stop);
        let statements = match parser::Parser::parse_all(query) {
            Ok(statements) => statements,
            Err(error) if stop_on_error => return Err(error),
            Err(error) => {
                return Ok(ExecutionReport::single(ExecResponse {
                    ok: false,
                    operation: "PARSE".to_string(),
                    message: error.to_string(),
                    data: None,
                }));
            }
        };
        if statements.is_empty() {
            // An empty script is a caller bug, not a statement failure — fail
            // closed instead of returning a silently-empty `ok: true` report
            // (parity with `";;"`, which is a parse error).
            return Err(QqlError::validation(
                "QQL-VALIDATION-EMPTY-SCRIPT",
                "no statements to execute; the query is empty or contains only whitespace",
                None,
            ));
        }
        let results = self.execute_batch_nodes(statements, stop_on_error).await?;
        Ok(ExecutionReport::from_results(results))
    }

    /// Execute a parameterized query with named parameters (`:name`).
    pub async fn execute_with_params(
        &self,
        query: &str,
        params: &HashMap<String, qql_core::ast::Value>,
        on_error: OnError,
    ) -> Result<ExecutionReport, QqlError> {
        self.ensure_open()?;
        let bound = qql_core::params::bind_named(query, |k| params.get(k).cloned())?;
        self.execute(&bound, on_error).await
    }

    /// Execute a parameterized query with positional parameters (`?`).
    pub async fn execute_with_positional_params(
        &self,
        query: &str,
        params: &[qql_core::ast::Value],
        on_error: OnError,
    ) -> Result<ExecutionReport, QqlError> {
        self.ensure_open()?;
        let bound = qql_core::params::bind_positional(query, params)?;
        self.execute(&bound, on_error).await
    }

    /// Execute one parsed statement under the configured timeout, returning a
    /// single response.
    pub async fn execute_node(&self, stmt: Stmt) -> Result<ExecResponse, QqlError> {
        self.ensure_open()?;
        if let Some(secs) = self.request_timeout() {
            match tokio::time::timeout(
                std::time::Duration::from_secs(secs),
                self.execute_node_inner(stmt),
            )
            .await
            {
                Ok(res) => res,
                Err(_) => Err(QqlError::transport(
                    "QQL-TIMEOUT",
                    format!("operation timed out after {secs}s"),
                    None,
                )),
            }
        } else {
            self.execute_node_inner(stmt).await
        }
    }

    async fn execute_node_inner(&self, stmt: Stmt) -> Result<ExecResponse, QqlError> {
        let prepared = self.prepare_statement(stmt).await?;
        let planned = plan(&prepared)?;
        self.dispatch_planned(&planned).await
    }

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
                    });
                }
            }
        }
        if !pending.is_empty() {
            results.extend(self.execute_batch_nodes(pending, stop_on_error).await?);
        }
        if results.is_empty() {
            // Every input parsed to zero statements (e.g. `["", "  "]`) — same
            // empty-script contract as `execute`.
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
        let mut pending = Vec::new();
        let mut pending_key: Option<BatchKey> = None;

        for stmt in stmts {
            let statement_key = statement_batch_key(&stmt);

            // A statement outside the current batch family is an execution
            // barrier. Flush before preparing it because preparation may read
            // or mutate backend state (for example UPSERT auto-creation).
            if !pending.is_empty() && statement_key != pending_key {
                self.flush_planned_group(&mut pending, stop_on_error, &mut results)
                    .await?;
                pending_key = None;
            }

            let prepared = match self.prepare_statement(stmt).await {
                Ok(p) => p,
                Err(e) => {
                    self.flush_planned_group(&mut pending, stop_on_error, &mut results)
                        .await?;
                    pending_key = None;
                    if stop_on_error {
                        return Err(e);
                    }
                    results.push(ExecResponse {
                        ok: false,
                        operation: "PREPARE".to_string(),
                        message: e.to_string(),
                        data: None,
                    });
                    continue;
                }
            };

            let planned = match plan(&prepared) {
                Ok(planned) => planned,
                Err(e) => {
                    self.flush_planned_group(&mut pending, stop_on_error, &mut results)
                        .await?;
                    pending_key = None;
                    if stop_on_error {
                        return Err(e);
                    }
                    results.push(ExecResponse {
                        ok: false,
                        operation: "PLAN".to_string(),
                        message: e.to_string(),
                        data: None,
                    });
                    continue;
                }
            };

            let key = planned.batch_key();
            if key.is_none() {
                self.flush_planned_group(&mut pending, stop_on_error, &mut results)
                    .await?;
                pending_key = None;
                self.dispatch_or_collect(planned, stop_on_error, &mut results)
                    .await?;
                continue;
            }

            if !pending.is_empty() && key != pending_key {
                self.flush_planned_group(&mut pending, stop_on_error, &mut results)
                    .await?;
            }
            pending_key = key;
            pending.push(planned);
        }

        self.flush_planned_group(&mut pending, stop_on_error, &mut results)
            .await?;
        Ok(results)
    }

    async fn dispatch_or_collect(
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
            }),
        }
        Ok(())
    }

    async fn flush_planned_group(
        &self,
        pending: &mut Vec<qql_plan::PlannedOperation>,
        stop_on_error: bool,
        results: &mut Vec<ExecResponse>,
    ) -> Result<(), QqlError> {
        use qql_plan::{PlannedOperation, QueryBatchRequest};

        if pending.is_empty() {
            return Ok(());
        }
        if pending.len() == 1 {
            let planned = pending.pop().expect("pending contains one operation");
            return self
                .dispatch_or_collect(planned, stop_on_error, results)
                .await;
        }

        let operations = core::mem::take(pending);
        let is_query = matches!(&operations[0], PlannedOperation::Query { .. });
        if is_query {
            let collection = operations[0].collection().unwrap_or_default().to_string();
            let searches = operations
                .iter()
                .map(|operation| match operation {
                    PlannedOperation::Query { request, .. } => Ok(request.clone()),
                    _ => Err(QqlError::execution(
                        "QQL-BATCH-INVARIANT",
                        "query batch contained a non-query operation",
                        None,
                    )),
                })
                .collect::<Result<Vec<_>, _>>()?;
            let expected = searches.len();
            let batch = QueryBatchRequest { searches };
            match self.client.execute_query_batch(&collection, &batch).await {
                Ok(responses) if responses.len() == expected => {
                    for value in responses {
                        if let Some(message) = Self::batch_item_error(&value) {
                            // A 200 batch response can carry per-item
                            // failures; keep them aligned with the request
                            // order instead of reporting everything as ok.
                            results.push(ExecResponse {
                                ok: false,
                                operation: "QUERY".to_string(),
                                message,
                                data: None,
                            });
                            continue;
                        }
                        let hits = extract_search_hits(&value);
                        results.push(ExecResponse {
                            ok: true,
                            operation: "QUERY".to_string(),
                            message: format!("Found {} hits", hits.len()),
                            data: Some(serialize_hits(&hits)?),
                        });
                    }
                }
                Ok(responses) => {
                    let error = QqlError::transport(
                        "QQL-BATCH-CARDINALITY",
                        format!(
                            "query batch returned {} results for {expected} operations",
                            responses.len()
                        ),
                        None,
                    );
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
            // Mutations batch. Most mutations lower to an
            // `UpdateOperation` and batch through `execute_update_batch`.
            // `DELETE PAYLOAD` has no wire batch form
            // (`planned_to_update_operation` returns `None`), so it is
            // deliberately isolated: consecutive batchable mutations still
            // run as one batch, and each non-batchable statement is
            // dispatched through the working single-op path in statement
            // order. Every operation in the group succeeds instead of the
            // whole group aborting with `QQL-BATCH-INVARIANT`.
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
                    // Non-batchable mutation (DELETE PAYLOAD): flush the
                    // batchable run first, then dispatch this statement
                    // singly so statement order is preserved.
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

    /// Dispatch a run of consecutive batchable mutations (same collection).
    /// A single operation goes through the plain dispatch path; multiple
    /// operations go through `execute_update_batch`.
    async fn flush_update_run(
        &self,
        operations: Vec<qql_plan::PlannedOperation>,
        stop_on_error: bool,
        results: &mut Vec<ExecResponse>,
    ) -> Result<(), QqlError> {
        use qql_plan::UpdateBatchRequest;
        use qql_plan::mutation::planned_to_update_operation;

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

        let mut update_operations = Vec::with_capacity(operations.len());
        let mut labels = Vec::with_capacity(operations.len());
        for operation in &operations {
            let (_, update) = planned_to_update_operation(operation).ok_or_else(|| {
                QqlError::execution(
                    "QQL-BATCH-INVARIANT",
                    "mutation batch contained a non-mutation operation",
                    None,
                )
            })?;
            labels.push(update.operation_name());
            update_operations.push(update);
        }
        let collection = operations[0].collection().unwrap_or_default().to_string();
        let expected = update_operations.len();
        let batch = UpdateBatchRequest {
            operations: update_operations,
        };
        match self.client.execute_update_batch(&collection, &batch).await {
            Ok(responses) if responses.len() == expected => {
                for (value, label) in responses.into_iter().zip(labels.iter()) {
                    if let Some(message) = Self::batch_item_error(&value) {
                        results.push(ExecResponse {
                            ok: false,
                            operation: (*label).to_string(),
                            message,
                            data: None,
                        });
                        continue;
                    }
                    results.push(ExecResponse {
                        ok: true,
                        operation: (*label).to_string(),
                        message: format!("{label} ok (batched)"),
                        data: Some(value),
                    });
                }
            }
            Ok(responses) => {
                let error = QqlError::transport(
                    "QQL-BATCH-CARDINALITY",
                    format!(
                        "update batch returned {} results for {expected} operations",
                        responses.len()
                    ),
                    None,
                );
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

    /// A batched RPC failed (or returned the wrong cardinality). With
    /// `on_error = "continue"`, fall back to dispatching each operation
    /// individually so per-statement success/failure stays accurate —
    /// reporting the whole group as failed would lose statements that
    /// succeed on their own.
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

    /// Qdrant batch endpoints answer per item; a 200 response can still carry
    /// per-item failures (`status: "error"`). Detect them so successes and
    /// failures stay aligned with the request order.
    fn batch_item_error(item: &serde_json::Value) -> Option<String> {
        if item.get("status").and_then(serde_json::Value::as_str) == Some("error") {
            return Some(
                item.get("error")
                    .and_then(serde_json::Value::as_str)
                    .or_else(|| {
                        item.pointer("/status/error")
                            .and_then(serde_json::Value::as_str)
                    })
                    .unwrap_or("batch item failed")
                    .to_string(),
            );
        }
        None
    }

    /// Shared preparation: embeddings, named-vector validation, and UPSERT
    /// collection auto-creation. Callers must preserve statement order because
    /// preparation may read or mutate backend state.
    async fn prepare_statement(&self, mut stmt: Stmt) -> Result<Stmt, QqlError> {
        let upsert_schema = match &mut stmt {
            Stmt::Query(query) => {
                if let ast::QueryCollection::Explicit(collection) = &query.collection {
                    let collection = collection.clone();
                    self.configure_query_vectors(&collection, query).await?;
                }
                None
            }
            Stmt::Upsert(upsert) => self.configure_upsert_embeddings(upsert).await?,
            _ => None,
        };

        if let Some(ref embedder) = self.embedder {
            self.resolve_embeddings(&mut stmt, embedder.as_ref())
                .await?;
        }

        if let Stmt::CreateCollection(create) = &mut stmt {
            self.prepare_create_collection(create).await?;
        }

        if let Stmt::Upsert(u) = &stmt {
            if let Some(ref emb) = u.embedding {
                type SpecTuple<'a> = (
                    Option<&'a str>,
                    bool,
                    bool,
                    Option<&'a str>,
                    Option<&'a str>,
                );

                fn collect_specs(spec: &ast::EmbeddingSpec) -> Vec<SpecTuple<'_>> {
                    match spec {
                        ast::EmbeddingSpec::Dense { model, vector, .. } => {
                            vec![(model.as_deref(), true, false, vector.as_deref(), None)]
                        }
                        ast::EmbeddingSpec::Sparse { model, vector, .. } => {
                            vec![(model.as_deref(), false, true, None, vector.as_deref())]
                        }
                        // MultiVector / Image alone do not auto-create a collection here.
                        ast::EmbeddingSpec::MultiVector { .. }
                        | ast::EmbeddingSpec::Image { .. } => Vec::new(),
                        ast::EmbeddingSpec::Hybrid {
                            dense_model,
                            dense_vector,
                            sparse_vector,
                            ..
                        } => vec![(
                            dense_model.as_deref(),
                            true,
                            true,
                            dense_vector.as_deref(),
                            sparse_vector.as_deref(),
                        )],
                        ast::EmbeddingSpec::Multi(specs) => {
                            specs.iter().flat_map(collect_specs).collect()
                        }
                    }
                }

                let specs = collect_specs(emb);
                if !specs.is_empty() {
                    let mut aggregated_dense = false;
                    let mut aggregated_sparse = false;
                    let mut main_model = None;
                    let mut main_dense_vec = None;
                    let mut main_sparse_vec = None;

                    for (model, has_dense, has_sparse, dense_vec, sparse_vec) in specs {
                        if has_dense {
                            aggregated_dense = true;
                            if main_dense_vec.is_none() {
                                main_dense_vec = dense_vec;
                            }
                        }
                        if has_sparse {
                            aggregated_sparse = true;
                            if main_sparse_vec.is_none() {
                                main_sparse_vec = sparse_vec;
                            }
                        }
                        if main_model.is_none() && model.is_some() {
                            main_model = model;
                        }
                    }

                    self.ensure_collection_for_upsert(
                        &u.collection,
                        main_model,
                        aggregated_dense,
                        aggregated_sparse,
                        main_dense_vec,
                        main_sparse_vec,
                    )
                    .await?;
                }
            }
            if let Some(info) = upsert_schema.as_ref() {
                self.validate_embedded_upsert(u, info)?;
            }
        }

        Ok(stmt)
    }

    async fn prepare_create_collection(
        &self,
        create: &mut ast::CreateCollectionStmt,
    ) -> Result<(), QqlError> {
        if let ast::CollectionMode::Dense { model: Some(model) } = &create.mode
            && let Some(embedder) = self.embedder.as_deref()
            && !embedder.accepts_model(model)
        {
            return Err(QqlError::execution(
                "QQL-EMBEDDING-MODEL",
                format!("embedding model '{model}' is not available from the configured embedder"),
                None,
            ));
        }
        if !create.vectors.is_empty() {
            if let ast::CollectionMode::Dense { model: Some(model) } = &create.mode {
                let expected = self.resolve_dense_vector_size(Some(model)).await? as u64;
                if create.vectors.len() == 1 && create.vectors[0].size != expected {
                    return Err(QqlError::execution(
                        "QQL-EMBEDDING-DIM",
                        format!(
                            "collection vector dimension {} does not match embedding model '{model}' dimension {expected}",
                            create.vectors[0].size
                        ),
                        None,
                    ));
                }
            }
            return Ok(());
        }
        if !create.sparse_vectors.is_empty()
            && matches!(create.mode, ast::CollectionMode::Dense { model: None })
        {
            // An explicit sparse definition is a valid sparse-only collection;
            // do not silently add the default dense vector to it.
            return Ok(());
        }

        let (model, dense_name, sparse_name, with_colbert) = match &create.mode {
            ast::CollectionMode::Dense { model } => {
                (model.as_deref(), DENSE_VECTOR_NAME, None, false)
            }
            ast::CollectionMode::Hybrid {
                dense_vector,
                sparse_vector,
            } => (
                None,
                dense_vector.as_deref().unwrap_or(DENSE_VECTOR_NAME),
                Some(sparse_vector.as_deref().unwrap_or(SPARSE_VECTOR_NAME)),
                false,
            ),
            // Conventional dense + sparse + ColBERT multivector topology.
            ast::CollectionMode::Rerank => {
                (None, DENSE_VECTOR_NAME, Some(SPARSE_VECTOR_NAME), true)
            }
        };
        let dense_size = self.resolve_dense_vector_size(model).await? as u64;
        create.vectors.push(ast::VectorDef {
            name: dense_name.to_string(),
            size: dense_size,
            distance: ast::VectorDistance::Cosine,
            hnsw: None,
            quantization: None,
            multivector: None,
            vectors: None,
        });
        if let Some(sparse_name) = sparse_name {
            create.sparse_vectors.push(ast::SparseVectorDef {
                name: sparse_name.to_string(),
                index: None,
                modifier: None,
            });
        }
        if with_colbert {
            let multi_size = self
                .embedder
                .as_deref()
                .and_then(crate::embedder::Embedder::multi_dimension)
                .or_else(|| {
                    self.config.as_ref().and_then(|c| {
                        (c.multi_embedding_dimension > 0).then_some(c.multi_embedding_dimension)
                    })
                })
                .unwrap_or(RERANK_VECTOR_SIZE as usize) as u64;
            create.vectors.push(ast::VectorDef {
                name: RERANK_VECTOR_NAME.to_string(),
                size: multi_size,
                distance: ast::VectorDistance::Cosine,
                hnsw: None,
                quantization: None,
                multivector: Some(ast::MultivectorConfig {
                    comparator: ast::MultivectorComparator::MaxSim,
                }),
                vectors: None,
            });
        }

        Ok(())
    }

    /// Dispatch a planned operation — gRPC goes direct, REST goes through Route.
    async fn dispatch_planned(
        &self,
        op: &qql_plan::PlannedOperation,
    ) -> Result<ExecResponse, QqlError> {
        use qql_plan::PlannedOperation;

        // Client-side pair scorer: never a single Qdrant route.
        if let PlannedOperation::CrossRerank {
            collection: _,
            query,
            model,
            field,
            limit,
            offset,
            candidates,
        } = op
        {
            return self
                .execute_cross_rerank(query, model, field, *limit, *offset, candidates)
                .await;
        }

        let label = op.operation_label();
        let mut result = self.client.execute_planned(op).await?;
        let (message, data) = match op {
            PlannedOperation::Query { .. }
            | PlannedOperation::Scroll { .. }
            | PlannedOperation::GetPoints { .. } => {
                let hits = extract_search_hits(&result);
                (
                    format!("Found {} hits", hits.len()),
                    Some(serialize_hits(&hits)?),
                )
            }
            PlannedOperation::QueryGroups { request, .. } => {
                if let Some(offset) = request.group_offset {
                    let offset = offset as usize;
                    let groups_opt = if result.get("result").is_some() {
                        result.get_mut("result").and_then(|r| r.get_mut("groups"))
                    } else {
                        result.get_mut("groups")
                    };
                    if let Some(groups) = groups_opt.and_then(|g| g.as_array_mut()) {
                        if offset < groups.len() {
                            groups.drain(0..offset);
                        } else {
                            groups.clear();
                        }
                    }
                }
                let groups_count = result
                    .get("result")
                    .and_then(|r| r.get("groups"))
                    .or_else(|| result.get("groups"))
                    .and_then(|g| g.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0);
                (format!("Found {groups_count} group(s)"), Some(result))
            }
            PlannedOperation::Count { .. } => {
                let count = result
                    .get("result")
                    .and_then(|r| r.get("count"))
                    .and_then(|c| c.as_u64())
                    .or_else(|| result.get("count").and_then(|c| c.as_u64()))
                    .unwrap_or(0);
                (format!("Count: {count}"), Some(result))
            }
            PlannedOperation::Facet { .. } => {
                let facet_hits = result
                    .get("result")
                    .and_then(|r| r.get("hits"))
                    .cloned()
                    .or_else(|| result.get("hits").cloned())
                    .unwrap_or_else(|| serde_json::json!([]));
                let count = facet_hits.as_array().map(|a| a.len()).unwrap_or(0);
                (format!("Found {count} facet hit(s)"), Some(facet_hits))
            }
            PlannedOperation::ListCollections => {
                let count = result
                    .get("result")
                    .and_then(|value| value.get("collections"))
                    .or_else(|| result.get("collections"))
                    .and_then(serde_json::Value::as_array)
                    .map_or(0, Vec::len);
                (format!("Found {count} collection(s)"), Some(result))
            }
            PlannedOperation::GetCollection { .. } => (format!("{label} ok"), Some(result)),
            PlannedOperation::Upsert { request, .. } => {
                let n = request.points.len();
                (
                    format!("Upserted {n} point(s)"),
                    Some(serde_json::json!({"count": n})),
                )
            }
            PlannedOperation::ListShardKeys { .. } => ("Shard keys listed".into(), Some(result)),
            PlannedOperation::GetQuotas => ("Quota configuration shown".into(), Some(result)),
            PlannedOperation::SetQuotas { .. } => {
                ("Quota configuration updated".into(), Some(result))
            }
            PlannedOperation::CrossRerank { .. } => {
                // Defensive: early return above must handle this variant.
                return Err(QqlError::execution(
                    "QQL-CROSS-RERANK",
                    "CROSS RERANK must be executed client-side, not via a Qdrant route",
                    None,
                ));
            }
            _ => (format!("{label} ok"), None),
        };
        Ok(ExecResponse {
            ok: true,
            operation: label.into(),
            message,
            data,
        })
    }

    /// Run candidate ANN stages, score (query, doc_text) with a cross-encoder, reorder.
    async fn execute_cross_rerank(
        &self,
        query: &str,
        model: &str,
        field: &str,
        limit: u64,
        offset: u64,
        candidates: &[(String, qql_plan::QueryRequest)],
    ) -> Result<ExecResponse, QqlError> {
        use std::collections::HashMap;

        let embedder = self.embedder.as_ref().ok_or_else(|| {
            QqlError::execution(
                "QQL-RERANK-CROSS",
                "CROSS RERANK requires a configured embedder with pair scoring \
                 (rerank_endpoint / edge reranker_model)",
                None,
            )
        })?;

        let mut by_key: HashMap<(String, qql_plan::PlanPointId), SearchHit> = HashMap::new();
        for (collection, request) in candidates {
            let op = qql_plan::PlannedOperation::Query {
                collection: collection.clone(),
                request: request.clone(),
            };
            let raw = self.client.execute_planned(&op).await?;
            for mut hit in extract_search_hits(&raw) {
                hit.collection = Some(collection.clone());
                by_key
                    .entry((collection.clone(), hit.id.clone()))
                    .or_insert(hit);
            }
        }

        if by_key.is_empty() {
            return Ok(ExecResponse {
                ok: true,
                operation: "CROSS_RERANK".into(),
                message: "Found 0 hits".into(),
                data: Some(serde_json::json!([])),
            });
        }

        // Stable order for scoring (then re-sort by pair score).
        let mut hits: Vec<SearchHit> = by_key.into_values().collect();
        hits.sort_by(|a, b| {
            a.collection
                .as_deref()
                .unwrap_or("")
                .cmp(b.collection.as_deref().unwrap_or(""))
                .then_with(|| a.id.cmp(&b.id))
        });

        let mut docs = Vec::with_capacity(hits.len());
        let mut keep_idx = Vec::with_capacity(hits.len());
        for (i, hit) in hits.iter().enumerate() {
            // Only use `hit.text` when the requested field is the conventional
            // "text" payload key. Falling back for other fields (e.g. `body`)
            // silently reranks against the wrong content.
            let from_payload = hit
                .payload
                .as_ref()
                .and_then(|p| p.get(field))
                .and_then(|v| v.as_str());
            let text = match from_payload {
                Some(s) if !s.is_empty() => s,
                _ if field.eq_ignore_ascii_case("text") => hit.text.as_deref().unwrap_or(""),
                _ => "",
            };
            if text.is_empty() {
                continue;
            }
            docs.push(text.to_string());
            keep_idx.push(i);
        }
        if docs.is_empty() {
            return Err(QqlError::execution(
                "QQL-RERANK-CROSS-FIELD",
                format!(
                    "CROSS RERANK found candidates but none had non-empty payload field '{field}'. \
                     Ensure UPSERT stores text on that field and PREFETCH returns WITH PAYLOAD."
                ),
                None,
            ));
        }

        let scores = embedder.rerank_pairs(query, &docs, model).await?;
        if scores.len() != docs.len() {
            return Err(QqlError::execution(
                "QQL-RERANK-CROSS",
                format!(
                    "rerank_pairs returned {} scores for {} documents",
                    scores.len(),
                    docs.len()
                ),
                None,
            ));
        }

        let mut ranked: Vec<(f32, SearchHit)> = keep_idx
            .into_iter()
            .zip(scores)
            .map(|(i, score)| {
                let mut h = hits[i].clone();
                h.score = score;
                (score, h)
            })
            .collect();
        ranked.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        let skip = offset as usize;
        let take = limit as usize;
        let out: Vec<SearchHit> = ranked
            .into_iter()
            .skip(skip)
            .take(take)
            .map(|(_, h)| h)
            .collect();
        let n = out.len();
        Ok(ExecResponse {
            ok: true,
            operation: "CROSS_RERANK".into(),
            message: format!("Found {n} hits (cross-encoder)"),
            data: Some(serialize_hits(&out)?),
        })
    }
}

pub(crate) mod dml;
