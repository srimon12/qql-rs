use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json;

use qql_core::ast::Stmt;
use qql_core::error::QqlError;
use qql_core::parser;

use crate::config::QqlConfig;
use crate::embedder::Embedder;

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

    pub fn parse_query(query: &str) -> Result<Stmt, QqlError> {
        parser::Parser::parse(query)
    }

    pub async fn execute(&self, query: &str) -> Result<ExecResponse, QqlError> {
        let stmt = Self::parse_query(query)?;
        self.execute_node(stmt).await
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

    pub async fn execute_batch_nodes(
        &self,
        stmts: Vec<Stmt>,
        stop_on_error: bool,
    ) -> Result<Vec<ExecResponse>, QqlError> {
        let mut results = Vec::with_capacity(stmts.len());
        for stmt in stmts {
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

    pub async fn query_batch(&self, queries: &[&str]) -> Result<Vec<ExecResponse>, QqlError> {
        let mut parsed_stmts = Vec::with_capacity(queries.len());
        for q in queries {
            let stmt = Self::parse_query(q)?;
            if let Stmt::Query(query_stmt) = stmt {
                parsed_stmts.push(*query_stmt);
            } else {
                return Err(QqlError::execution(
                    "QQL-EXECUTION",
                    "query_batch only supports QUERY statements, got non-query statement"
                        .to_string(),
                    None,
                ));
            }
        }
        self.query_batch_nodes(parsed_stmts).await
    }

    pub async fn query_batch_nodes(
        &self,
        stmts: Vec<qql_core::ast::QueryStmt>,
    ) -> Result<Vec<ExecResponse>, QqlError> {
        let mut results = Vec::with_capacity(stmts.len());
        for stmt in stmts {
            results.push(self.do_query(stmt).await?);
        }
        Ok(results)
    }
}

pub(crate) mod ddl;
pub(crate) mod dml;
pub(crate) mod helpers;
