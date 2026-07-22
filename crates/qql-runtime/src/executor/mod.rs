use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json;

use qql_core::ast::{self, Stmt};
use qql_core::error::QqlError;
use qql_core::parser;

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
    /// same-collection QUERY statements are grouped into a single network call.
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

    pub async fn execute_node(&self, mut stmt: Stmt) -> Result<ExecResponse, QqlError> {
        if let Some(ref embedder) = self.embedder {
            self.resolve_embeddings(&mut stmt, embedder.as_ref())
                .await?;
        }
        match stmt {
            Stmt::ShowCollections => self.do_show_collections().await,
            Stmt::ShowCollection(collection) => self.do_show_collection(&collection).await,
            Stmt::CreateCollection(n) => self.do_create_collection(*n).await,
            Stmt::AlterCollection(n) => self.do_alter_collection(*n).await,
            Stmt::DropCollection(n) => self.do_drop_collection(&n.collection).await,
            Stmt::Upsert(n) => self.do_upsert(*n).await,
            Stmt::Scroll(n) => self.do_scroll(*n).await,
            Stmt::Query(n) => self.do_query(*n).await,
            Stmt::Delete(n) => self.do_delete(*n).await,
            Stmt::ClearPayload(n) => self.do_clear_payload(*n).await,
            Stmt::DeleteVector(n) => self.do_delete_vector(*n).await,
            Stmt::UpdateVector(n) => self.do_update_vector(*n).await,
            Stmt::UpdatePayload(n) => self.do_update_payload(*n).await,
            Stmt::CreateIndex(n) => self.do_create_index(*n).await,
            Stmt::CreateShardKey(n) => self.do_create_shard_key(*n).await,
            Stmt::DropShardKey(n) => self.do_drop_shard_key(*n).await,
            Stmt::ShowShardKeys(collection) => self.do_show_shard_keys(&collection).await,
            Stmt::DropIndex(n) => self.do_drop_index(*n).await,
            Stmt::Count(n) => self.do_count(*n).await,
        }
    }

    pub async fn execute_batch(
        &self,
        queries: &[&str],
        stop_on_error: bool,
    ) -> Result<Vec<ExecResponse>, QqlError> {
        let mut results = Vec::with_capacity(queries.len());
        for query in queries {
            match self.execute(query).await {
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
        }
        Ok(results)
    }

    /// Execute pre-parsed statements.  Same-collection QUERY statements are
    /// automatically grouped into one network call via Qdrant's batch endpoint.
    pub async fn execute_batch_nodes(
        &self,
        stmts: Vec<Stmt>,
        stop_on_error: bool,
    ) -> Result<Vec<ExecResponse>, QqlError> {
        let mut results: Vec<ExecResponse> = Vec::with_capacity(stmts.len());

        // ── Phase 1: separate queries from mutations ──────────────
        let mut query_groups: HashMap<String, Vec<ast::QueryStmt>> = HashMap::new();
        let mut non_queries: Vec<Stmt> = Vec::new();

        for stmt in stmts {
            match stmt {
                Stmt::Query(q) => {
                    if let ast::QueryCollection::Explicit(ref name) = q.collection {
                        query_groups.entry(name.clone()).or_default().push(*q);
                    } else {
                        non_queries.push(Stmt::Query(q));
                    }
                }
                other => non_queries.push(other),
            }
        }

        // ── Phase 2: execute batched queries ──────────────────────
        for (_collection, queries) in query_groups {
            let batches = qql_plan::routing::route_query_batch(&queries);
            for (coll, batch) in batches {
                match self.client.execute_query_batch(&coll, &batch).await {
                    Ok(responses) => {
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
                        results.push(ExecResponse {
                            ok: false,
                            operation: "QUERY".to_string(),
                            message: e.to_string(),
                            data: None,
                        });
                    }
                }
            }
            // Any queries that weren't batched (singletons, non-batchable variants)
            // get executed individually.
            let remaining: Vec<_> = queries
                .into_iter()
                .filter(|q| {
                    // Keep queries that didn't go through the batch
                    matches!(q.expression, ast::QueryExpr::Points { .. }) || q.group.is_some()
                })
                .collect();
            for q in remaining {
                results.push(self.single_query_exec(q).await);
            }
        }

        // ── Phase 3: execute non-query statements individually ───
        for stmt in non_queries {
            match self.execute_node(stmt).await {
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
        }

        Ok(results)
    }

    async fn single_query_exec(&self, stmt: ast::QueryStmt) -> ExecResponse {
        match self.do_query(stmt).await {
            Ok(r) => r,
            Err(e) => ExecResponse {
                ok: false,
                operation: "QUERY".to_string(),
                message: e.to_string(),
                data: None,
            },
        }
    }
}

pub(crate) mod ddl;
pub(crate) mod dml;
pub(crate) mod helpers;
