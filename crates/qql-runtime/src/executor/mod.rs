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

pub const DENSE_VECTOR_NAME: &str = "dense";
pub const SPARSE_VECTOR_NAME: &str = "sparse";
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
    pub async fn execute(&self, query: &str) -> Result<ExecResponse, QqlError> {
        match parser::Parser::parse_all(query) {
            Ok(mut statements) => match statements.len() {
                0 => Ok(ExecResponse {
                    ok: true,
                    operation: "EMPTY".to_string(),
                    message: "empty script".to_string(),
                    data: None,
                }),
                1 => self.execute_node(statements.remove(0)).await,
                _ => {
                    let results = self.execute_batch_nodes(statements, true).await?;
                    let succeeded = results.iter().filter(|r| r.ok).count();
                    Ok(ExecResponse {
                        ok: true,
                        operation: "SCRIPT".to_string(),
                        message: format!(
                            "Executed {} statement(s) ({} succeeded, {} failed)",
                            results.len(),
                            succeeded,
                            results.len() - succeeded,
                        ),
                        data: Some(serde_json::to_value(&results).unwrap_or_default()),
                    })
                }
            },
            Err(_) => {
                let stmt = parser::Parser::parse(query)?;
                self.execute_node(stmt).await
            }
        }
    }

    pub async fn execute_node(&self, stmt: Stmt) -> Result<ExecResponse, QqlError> {
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
        stop_on_error: bool,
    ) -> Result<Vec<ExecResponse>, QqlError> {
        let mut stmts = Vec::with_capacity(queries.len());
        for query in queries {
            // Prefer multi-statement parse so each list entry can itself be a script.
            match parser::Parser::parse_all(query) {
                Ok(parsed) => stmts.extend(parsed),
                Err(first) => match parser::Parser::parse(query) {
                    Ok(stmt) => stmts.push(stmt),
                    Err(_) => {
                        if stop_on_error {
                            return Err(first);
                        }
                        // Keep position: push a no-op marker via a failed single later.
                        // Surface parse failure as a failed result by storing a
                        // ShowCollections placeholder is wrong — re-parse fails hard.
                        return Err(first);
                    }
                },
            }
        }
        self.execute_batch_nodes(stmts, stop_on_error).await
    }

    /// Execute pre-parsed statements with order-preserving smart batching.
    ///
    /// Contiguous runs of batchable QUERY statements targeting the same
    /// collection are sent via `/points/query/batch`. Contiguous runs of
    /// mutations (UPSERT, DELETE, UPDATE PAYLOAD/VECTOR, CLEAR PAYLOAD,
    /// DELETE VECTOR) targeting the same collection are sent via
    /// `/points/batch`. All other statements execute individually.
    /// Statement order is preserved.
    ///
    /// Every statement is prepared (embeddings + schema checks) before
    /// batch classification so single- and multi-statement paths share
    /// the same preparation semantics.
    pub async fn execute_batch_nodes(
        &self,
        stmts: Vec<Stmt>,
        stop_on_error: bool,
    ) -> Result<Vec<ExecResponse>, QqlError> {
        use qql_plan::mutation::planned_to_update_operation;
        use qql_plan::plan::{plan, BatchFamily, PlannedOperation};
        use qql_plan::{QueryBatchRequest, UpdateBatchRequest};

        // prepare → plan exactly once for every statement (Phase 2).
        let mut planned: Vec<PlannedOperation> = Vec::with_capacity(stmts.len());
        for stmt in stmts {
            let prepared = self.prepare_statement(stmt).await?;
            planned.push(plan(&prepared)?);
        }

        let mut results: Vec<ExecResponse> = Vec::with_capacity(planned.len());
        let mut i = 0;

        while i < planned.len() {
            // ── Contiguous mutation batch (same collection, 2+) ──
            if planned[i].batch_family() == BatchFamily::Mutation {
                if let Some((coll, first_op)) = planned_to_update_operation(&planned[i]) {
                    let mut ops = vec![first_op];
                    let mut j = i + 1;
                    while j < planned.len() {
                        match planned_to_update_operation(&planned[j]) {
                            Some((c, op)) if c == coll => {
                                ops.push(op);
                                j += 1;
                            }
                            _ => break,
                        }
                    }
                    if ops.len() >= 2 {
                        let expected = ops.len();
                        let op_names: Vec<&'static str> =
                            ops.iter().map(|o| o.operation_name()).collect();
                        let batch = UpdateBatchRequest { operations: ops };
                        match self.client.execute_update_batch(&coll, &batch).await {
                            Ok(responses) => {
                                if responses.len() != expected {
                                    return Err(QqlError::transport(
                                        "QQL-BATCH-CARDINALITY",
                                        format!(
                                            "update batch returned {} results for {expected} operations",
                                            responses.len()
                                        ),
                                        None,
                                    ));
                                }
                                for (k, val) in responses.into_iter().enumerate() {
                                    let name = op_names.get(k).copied().unwrap_or("MUTATION");
                                    results.push(ExecResponse {
                                        ok: true,
                                        operation: name.to_string(),
                                        message: format!("{name} ok (batched)"),
                                        data: Some(val),
                                    });
                                }
                            }
                            Err(e) => {
                                if stop_on_error {
                                    return Err(e);
                                }
                                for name in &op_names {
                                    results.push(ExecResponse {
                                        ok: false,
                                        operation: (*name).to_string(),
                                        message: e.to_string(),
                                        data: None,
                                    });
                                }
                            }
                        }
                        i = j;
                        continue;
                    }
                }
            }

            // ── Contiguous query batch (same collection, 2+) ──
            if let PlannedOperation::Query {
                collection: coll,
                request: q0,
            } = &planned[i]
            {
                let coll = coll.clone();
                let mut searches = vec![q0.clone()];
                let mut j = i + 1;
                while j < planned.len() {
                    match &planned[j] {
                        PlannedOperation::Query {
                            collection: c,
                            request,
                        } if c == &coll => {
                            searches.push(request.clone());
                            j += 1;
                        }
                        _ => break,
                    }
                }
                if searches.len() >= 2 {
                    let expected = searches.len();
                    let batch = QueryBatchRequest { searches };
                    match self.client.execute_query_batch(&coll, &batch).await {
                        Ok(responses) => {
                            if responses.len() != expected {
                                return Err(QqlError::transport(
                                    "QQL-BATCH-CARDINALITY",
                                    format!(
                                        "query batch returned {} results for {expected} operations",
                                        responses.len()
                                    ),
                                    None,
                                ));
                            }
                            for val in responses {
                                let hits = extract_search_hits(&val);
                                results.push(ExecResponse {
                                    ok: true,
                                    operation: "QUERY".to_string(),
                                    message: format!("Found {} hits", hits.len()),
                                    data: Some(serde_json::to_value(hits).unwrap_or_default()),
                                });
                            }
                        }
                        Err(e) => {
                            if stop_on_error {
                                return Err(e);
                            }
                            for _ in 0..expected {
                                results.push(ExecResponse {
                                    ok: false,
                                    operation: "QUERY".to_string(),
                                    message: e.to_string(),
                                    data: None,
                                });
                            }
                        }
                    }
                    i = j;
                    continue;
                }
            }

            // ── Individual planned operation ──
            match self.dispatch_planned(&planned[i]).await {
                Ok(resp) => results.push(resp),
                Err(err) => {
                    if stop_on_error {
                        return Err(err);
                    }
                    results.push(ExecResponse {
                        ok: false,
                        operation: "ERROR".to_string(),
                        message: err.to_string(),
                        data: None,
                    });
                }
            }
            i += 1;
        }

        Ok(results)
    }

    /// Shared preparation: embeddings, named-vector validation, upsert collection prep.
    async fn prepare_statement(&self, mut stmt: Stmt) -> Result<Stmt, QqlError> {
        if let Some(ref embedder) = self.embedder {
            self.resolve_embeddings(&mut stmt, embedder.as_ref())
                .await?;
        }

        if let Stmt::CreateCollection(create) = &mut stmt {
            self.prepare_create_collection(create).await?;
        }

        match &stmt {
            Stmt::Query(q) => {
                if let ast::QueryCollection::Explicit(ref collection_name) = q.collection {
                    self.ensure_vector_name(collection_name, &q.expression)
                        .await?;
                }
            }
            Stmt::Upsert(u) => {
                if let Some(ref emb) = u.embedding {
                    let (model, is_hybrid, dense_vec, sparse_vec) = match emb {
                        ast::EmbeddingSpec::Dense { model, vector } => {
                            (model.as_deref(), false, vector.as_deref(), None)
                        }
                        ast::EmbeddingSpec::Hybrid {
                            dense_model,
                            dense_vector,
                            sparse_vector,
                            ..
                        } => (
                            dense_model.as_deref(),
                            true,
                            dense_vector.as_deref(),
                            sparse_vector.as_deref(),
                        ),
                    };
                    self.ensure_collection_for_upsert(
                        &u.collection,
                        model,
                        is_hybrid,
                        dense_vec,
                        sparse_vec,
                    )
                    .await?;
                }
            }
            _ => {}
        }

        Ok(stmt)
    }

    async fn prepare_create_collection(
        &self,
        create: &mut ast::CreateCollectionStmt,
    ) -> Result<(), QqlError> {
        if !create.vectors.is_empty() {
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
        });
        if let Some(sparse_name) = sparse_name {
            create.sparse_vectors.push(ast::SparseVectorDef {
                name: sparse_name.to_string(),
            });
        }

        Ok(())
    }

    /// Dispatch a planned DML/query operation via REST route projection.
    async fn dispatch_planned(
        &self,
        op: &qql_plan::PlannedOperation,
    ) -> Result<ExecResponse, QqlError> {
        use qql_plan::plan::to_rest_route;
        use qql_plan::PlannedOperation;

        let label = op.operation_label();
        let route = to_rest_route(op);
        let result = self.client.execute_route(route).await?;
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
