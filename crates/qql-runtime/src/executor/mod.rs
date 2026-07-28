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
pub const RERANK_VECTOR_NAME: &str = "colbert";
pub const DENSE_MODEL_DEFAULT: &str = "sentence-transformers/all-minilm-l6-v2";
pub const SPARSE_MODEL_DEFAULT: &str = "qdrant/bm25";
pub const RERANK_MODEL_DEFAULT: &str = "answerdotai/answerai-colbert-small-v1";
pub const DENSE_VECTOR_SIZE: u64 = 384;
pub const RERANK_VECTOR_SIZE: u64 = 96;
pub const INFERENCE_MODE_DEFAULT: &str = "local";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecResponse {
    pub ok: bool,
    pub operation: String,
    pub message: String,
    pub data: Option<serde_json::Value>,
}

/// Canonical cross-SDK execution result.  Every `client.execute(…)` call
/// returns this shape regardless of input type (string / Stmt / array).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionReport {
    pub ok: bool,
    pub results: Vec<ExecResponse>,
    pub succeeded: usize,
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum BatchKey {
    Query(String),
    Mutation(String),
}

fn statement_batch_key(stmt: &Stmt) -> Option<BatchKey> {
    match stmt {
        Stmt::Query(query)
            if query.group.is_none()
                && !matches!(query.expression, ast::QueryExpr::Points { .. }) =>
        {
            match &query.collection {
                ast::QueryCollection::Explicit(collection) => {
                    Some(BatchKey::Query(collection.clone()))
                }
                ast::QueryCollection::Inherited => None,
            }
        }
        Stmt::Upsert(stmt) => Some(BatchKey::Mutation(stmt.collection.clone())),
        Stmt::Delete(stmt) => Some(BatchKey::Mutation(stmt.collection.clone())),
        Stmt::UpdatePayload(stmt) => Some(BatchKey::Mutation(stmt.collection.clone())),
        Stmt::ClearPayload(stmt) => Some(BatchKey::Mutation(stmt.collection.clone())),
        Stmt::UpdateVector(stmt) => Some(BatchKey::Mutation(stmt.collection.clone())),
        Stmt::DeleteVector(stmt) => Some(BatchKey::Mutation(stmt.collection.clone())),
        _ => None,
    }
}

fn planned_batch_key(operation: &qql_plan::PlannedOperation) -> Option<BatchKey> {
    use qql_plan::{BatchFamily, PlannedOperation};

    match operation.batch_family() {
        BatchFamily::Query => match operation {
            PlannedOperation::Query { collection, .. } => Some(BatchKey::Query(collection.clone())),
            _ => None,
        },
        BatchFamily::Mutation => operation
            .collection()
            .map(|collection| BatchKey::Mutation(collection.to_owned())),
        BatchFamily::Single => None,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub id: String,
    pub score: f32,
    pub text: Option<String>,
    pub payload: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupedSearchResult {
    pub group_id: serde_json::Value,
    pub hits: Vec<SearchHit>,
}

pub use crate::client::*;

pub struct Executor {
    pub(crate) client: Box<dyn QdrantOps>,
    pub(crate) config: Option<QqlConfig>,
    pub(crate) embedder: Option<Arc<dyn Embedder>>,
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

    #[cfg(feature = "grpc")]
    pub fn grpc(url: &str, api_key: Option<String>) -> Result<Self, QqlError> {
        Ok(Self::new(
            Box::new(crate::grpc::GrpcQdrant::from_url(url, api_key)?),
            None,
        ))
    }

    pub fn new(client: Box<dyn QdrantOps>, config: Option<QqlConfig>) -> Self {
        Executor {
            client,
            config,
            embedder: None,
        }
    }

    pub fn with_embedder(
        client: Box<dyn QdrantOps>,
        config: Option<QqlConfig>,
        embedder: Option<Arc<dyn Embedder>>,
    ) -> Self {
        Executor {
            client,
            config,
            embedder,
        }
    }

    pub fn ops(&self) -> &dyn QdrantOps {
        self.client.as_ref()
    }

    pub fn explain(query: &str) -> Result<String, QqlError> {
        qql_core::explain::explain(query)
    }

    /// Explain every statement in a multi-statement script.
    pub fn explain_all(query: &str) -> Result<String, QqlError> {
        qql_core::explain::explain_all(query)
    }

    pub fn explain_node(stmt: &Stmt) -> Result<String, QqlError> {
        Ok(qql_core::explain::explain_node(stmt))
    }

    // --- explain_stmt removed --- moved to qql_core::explain

    pub fn client(&self) -> &dyn QdrantOps {
        self.client.as_ref()
    }

    /// Flush and release backend-owned resources. This is especially important
    /// for embedded backends before deleting their data directory.
    pub async fn close(&self) -> Result<(), QqlError> {
        self.client.close().await
    }

    pub fn embedder(&self) -> Option<&Arc<dyn Embedder>> {
        self.embedder.as_ref()
    }

    pub fn config(&self) -> Option<&QqlConfig> {
        self.config.as_ref()
    }

    pub fn default_context_timeout(&self) -> u64 {
        self.config
            .as_ref()
            .and_then(|c| {
                if c.request_timeout > 0 {
                    Some(c.request_timeout)
                } else {
                    None
                }
            })
            .unwrap_or(30)
    }

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
            return Ok(ExecutionReport::empty());
        }
        let results = self.execute_batch_nodes(statements, stop_on_error).await?;
        Ok(ExecutionReport::from_results(results))
    }

    pub async fn execute_node(&self, stmt: Stmt) -> Result<ExecResponse, QqlError> {
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
        Ok(ExecutionReport::from_results(results))
    }

    pub async fn execute_batch_nodes(
        &self,
        stmts: Vec<Stmt>,
        stop_on_error: bool,
    ) -> Result<Vec<ExecResponse>, QqlError> {
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

            let key = planned_batch_key(&planned);
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
        use qql_plan::mutation::planned_to_update_operation;
        use qql_plan::{PlannedOperation, QueryBatchRequest, UpdateBatchRequest};

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
        match &operations[0] {
            PlannedOperation::Query { collection, .. } => {
                let collection = collection.clone();
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
                            let hits = extract_search_hits(&value);
                            results.push(ExecResponse {
                                ok: true,
                                operation: "QUERY".to_string(),
                                message: format!("Found {} hits", hits.len()),
                                data: Some(serde_json::to_value(hits).unwrap_or_default()),
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
                        self.collect_batch_error(
                            error,
                            &vec!["QUERY"; expected],
                            stop_on_error,
                            results,
                        )?;
                    }
                    Err(error) => self.collect_batch_error(
                        error,
                        &vec!["QUERY"; expected],
                        stop_on_error,
                        results,
                    )?,
                }
            }
            _ => {
                let mut update_operations = Vec::with_capacity(operations.len());
                let mut labels = Vec::with_capacity(operations.len());
                let mut collection = None;
                for operation in &operations {
                    let Some((current_collection, update)) = planned_to_update_operation(operation)
                    else {
                        return Err(QqlError::execution(
                            "QQL-BATCH-INVARIANT",
                            "mutation batch contained a non-mutation operation",
                            None,
                        ));
                    };
                    if collection
                        .as_ref()
                        .is_some_and(|collection| collection != &current_collection)
                    {
                        return Err(QqlError::execution(
                            "QQL-BATCH-INVARIANT",
                            "mutation batch contained multiple collections",
                            None,
                        ));
                    }
                    collection.get_or_insert(current_collection);
                    labels.push(update.operation_name());
                    update_operations.push(update);
                }
                let collection = collection.unwrap_or_default();
                let expected = update_operations.len();
                let batch = UpdateBatchRequest {
                    operations: update_operations,
                };
                match self.client.execute_update_batch(&collection, &batch).await {
                    Ok(responses) if responses.len() == expected => {
                        for (value, label) in responses.into_iter().zip(labels.iter()) {
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
                        self.collect_batch_error(error, &labels, stop_on_error, results)?;
                    }
                    Err(error) => {
                        self.collect_batch_error(error, &labels, stop_on_error, results)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn collect_batch_error(
        &self,
        error: QqlError,
        labels: &[&str],
        stop_on_error: bool,
        results: &mut Vec<ExecResponse>,
    ) -> Result<(), QqlError> {
        if stop_on_error {
            return Err(error);
        }
        let message = error.to_string();
        results.extend(labels.iter().map(|label| ExecResponse {
            ok: false,
            operation: (*label).to_string(),
            message: message.clone(),
            data: None,
        }));
        Ok(())
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
            Stmt::Upsert(upsert) => {
                self.configure_upsert_embeddings(upsert).await?
            }
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
        if let ast::CollectionMode::Dense { model: Some(model) } = &create.mode {
            if let Some(embedder) = self.embedder.as_deref() {
                if !embedder.accepts_model(model) {
                    return Err(QqlError::execution(
                        "QQL-EMBEDDING-MODEL",
                        format!("embedding model '{model}' is not available from the configured embedder"),
                        None,
                    ));
                }
            }
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

        let (model, dense_name, sparse_name) = match &create.mode {
            ast::CollectionMode::Dense { model } => (model.as_deref(), DENSE_VECTOR_NAME, None),
            ast::CollectionMode::Hybrid {
                dense_vector,
                sparse_vector,
            } => (
                None,
                dense_vector.as_deref().unwrap_or(DENSE_VECTOR_NAME),
                Some(sparse_vector.as_deref().unwrap_or(SPARSE_VECTOR_NAME)),
            ),
            ast::CollectionMode::Rerank => (None, DENSE_VECTOR_NAME, Some(SPARSE_VECTOR_NAME)),
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

        Ok(())
    }

    /// Dispatch a planned operation — gRPC goes direct, REST goes through Route.
    async fn dispatch_planned(
        &self,
        op: &qql_plan::PlannedOperation,
    ) -> Result<ExecResponse, QqlError> {
        use qql_plan::PlannedOperation;

        let label = op.operation_label();
        let result = self.client.execute_planned(op).await?;
        let (message, data) = match op {
            PlannedOperation::Query { .. }
            | PlannedOperation::Scroll { .. }
            | PlannedOperation::GetPoints { .. } => {
                let hits = extract_search_hits(&result);
                (
                    format!("Found {} hits", hits.len()),
                    Some(serde_json::to_value(hits).unwrap_or_default()),
                )
            }
            PlannedOperation::QueryGroups { .. } => {
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
            _ => (format!("{label} ok"), None),
        };
        Ok(ExecResponse {
            ok: true,
            operation: label.into(),
            message,
            data,
        })
    }
}

pub(crate) mod dml;
#[cfg(feature = "rest")]
pub(crate) mod helpers;
