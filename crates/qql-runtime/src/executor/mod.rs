use std::collections::HashMap;
use std::sync::Arc;

use qql_core::ast::{self, Stmt, Value};
use qql_core::error::QqlError;
use qql_core::parser;

use crate::backend::CollectionInfo;
use crate::client::QdrantOps;
use crate::config::QqlConfig;
use crate::embedder::Embedder;

pub(crate) mod batch;
/// DDL preparation and collection schema caching.
pub mod ddl;
pub(crate) mod dispatch;
pub(crate) mod dml;
pub(crate) mod prepared;
pub(crate) mod response;

pub use prepared::PreparedStatement;
pub use qql_embed::resolve::{DENSE_VECTOR_NAME, SPARSE_VECTOR_NAME};
pub use response::{ExecResponse, ExecutionReport, GroupedSearchResult, OnError, SearchHit};

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

/// The QQL executor: prepare (schema `USING` resolution + embeddings) → `plan()`
/// → batch classification → dispatch over a `QdrantOps` backend.
pub struct Executor {
    pub(crate) client: Box<dyn QdrantOps>,
    pub(crate) config: Option<QqlConfig>,
    pub(crate) embedder: Option<Arc<dyn Embedder>>,
    /// Collection topology (dense / sparse / multivector names). Invalidated
    /// after DDL so `USING` routing does not `GET /collections/{name}` per query.
    /// Repeated SQL templates belong on [`Executor::prepare`], not here.
    pub(crate) schema_cache: std::sync::RwLock<
        HashMap<String, (CollectionInfo, std::sync::Arc<qql_embed::TopologyNames>)>,
    >,
    /// Set by [`Executor::close`]; every execution entry point fails after it.
    closed: std::sync::atomic::AtomicBool,
    close_lock: tokio::sync::Mutex<()>,
}

impl Executor {
    /// Create an executor connected to Qdrant over HTTP REST.
    #[cfg(feature = "rest")]
    pub fn rest(url: impl Into<String>, api_key: Option<String>) -> Result<Self, QqlError> {
        Ok(Self::new(
            Box::new(crate::rest::RestQdrant::new(url, api_key)),
            None,
        ))
    }

    /// Create an executor connected to Qdrant over gRPC.
    #[cfg(feature = "grpc")]
    pub fn grpc(url: &str, api_key: Option<String>) -> Result<Self, QqlError> {
        Ok(Self::new(
            Box::new(crate::grpc::GrpcQdrant::from_url(url, api_key)?),
            None,
        ))
    }

    /// Construct an executor with a boxed backend and optional runtime configuration.
    pub fn new(client: Box<dyn QdrantOps>, config: Option<QqlConfig>) -> Self {
        Self {
            client,
            config,
            embedder: None,
            schema_cache: std::sync::RwLock::new(HashMap::new()),
            closed: std::sync::atomic::AtomicBool::new(false),
            close_lock: tokio::sync::Mutex::new(()),
        }
    }

    /// Construct an executor with a backend, config, and embedder.
    pub fn with_embedder(
        client: Box<dyn QdrantOps>,
        config: Option<QqlConfig>,
        embedder: Option<Arc<dyn Embedder>>,
    ) -> Self {
        Self {
            client,
            config,
            embedder,
            schema_cache: std::sync::RwLock::new(HashMap::new()),
            closed: std::sync::atomic::AtomicBool::new(false),
            close_lock: tokio::sync::Mutex::new(()),
        }
    }

    /// Borrow the underlying backend ops (alias of `client`).
    pub fn ops(&self) -> &dyn QdrantOps {
        self.client.as_ref()
    }

    /// Format an ASCII plan tree for the query without executing it.
    pub fn explain(query: &str) -> Result<String, QqlError> {
        qql_core::explain::explain(query)
    }

    /// Format plan trees for all statements in a multi-statement script.
    pub fn explain_all(query: &str) -> Result<String, QqlError> {
        qql_core::explain::explain_all(query)
    }

    /// Format the plan tree for a pre-parsed statement node.
    pub fn explain_node(stmt: &Stmt) -> Result<String, QqlError> {
        Ok(qql_core::explain::explain_node(stmt))
    }

    /// Reference to the underlying backend ops (`RestQdrant`, `GrpcQdrant`, or test mock).
    pub fn client(&self) -> &dyn QdrantOps {
        self.client.as_ref()
    }

    /// Close the executor, aborting in-flight work and preventing future executions.
    pub async fn close(&self) -> Result<(), QqlError> {
        let _guard = self.close_lock.lock().await;
        if self.closed.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return Ok(());
        }
        if let Ok(mut cache) = self.schema_cache.write() {
            cache.clear();
        }
        Ok(())
    }

    /// Check whether this executor has been closed.
    pub fn is_closed(&self) -> bool {
        self.closed.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub(crate) fn ensure_open(&self) -> Result<(), QqlError> {
        if self.is_closed() {
            Err(QqlError::execution(
                "QQL-CLIENT-CLOSED",
                "cannot execute query on a closed executor",
                None,
            ))
        } else {
            Ok(())
        }
    }

    /// Borrow the embedder reference if configured.
    pub fn embedder(&self) -> Option<&Arc<dyn Embedder>> {
        self.embedder.as_ref()
    }

    /// Borrow the runtime configuration if set.
    pub fn config(&self) -> Option<&QqlConfig> {
        self.config.as_ref()
    }

    /// Request timeout in seconds, if configured.
    pub fn request_timeout(&self) -> Option<u64> {
        self.config.as_ref().and_then(|c| {
            if c.request_timeout > 0 {
                Some(c.request_timeout)
            } else {
                None
            }
        })
    }

    /// Execute a multi-statement QQL script string under the configured timeout.
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
                return Ok(ExecutionReport::from_results(vec![ExecResponse {
                    ok: false,
                    operation: "PARSE".to_string(),
                    message: error.to_string(),
                    data: None,
                }]));
            }
        };
        if statements.is_empty() {
            return Err(QqlError::validation(
                "QQL-VALIDATION-EMPTY-SCRIPT",
                "no statements to execute; the query string is empty or contains only comments",
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
        params: &HashMap<String, Value>,
        on_error: OnError,
    ) -> Result<ExecutionReport, QqlError> {
        self.ensure_open()?;
        let mut statements = parser::Parser::parse_all(query)?;
        for stmt in &mut statements {
            qql_core::params::bind_stmt(stmt, |k| params.get(k).cloned(), &[])?;
        }
        let stop_on_error = matches!(on_error, OnError::Stop);
        let results = self.execute_batch_nodes(statements, stop_on_error).await?;
        Ok(ExecutionReport::from_results(results))
    }

    /// Execute a parameterized query with named parameters given as a slice of `(name, value)` pairs.
    pub async fn execute_with_named_params<K: AsRef<str>>(
        &self,
        query: &str,
        params: &[(K, Value)],
        on_error: OnError,
    ) -> Result<ExecutionReport, QqlError> {
        self.ensure_open()?;
        let mut statements = parser::Parser::parse_all(query)?;
        for stmt in &mut statements {
            qql_core::params::bind_stmt(
                stmt,
                |k| {
                    params
                        .iter()
                        .find(|(name, _)| name.as_ref() == k)
                        .map(|(_, v)| v.clone())
                },
                &[],
            )?;
        }
        let stop_on_error = matches!(on_error, OnError::Stop);
        let results = self.execute_batch_nodes(statements, stop_on_error).await?;
        Ok(ExecutionReport::from_results(results))
    }

    /// Execute a parameterized query with positional parameters (`?`).
    pub async fn execute_with_positional_params(
        &self,
        query: &str,
        params: &[Value],
        on_error: OnError,
    ) -> Result<ExecutionReport, QqlError> {
        self.ensure_open()?;
        let mut statements = parser::Parser::parse_all(query)?;
        for stmt in &mut statements {
            qql_core::params::bind_stmt(stmt, |_| None, params)?;
        }
        let stop_on_error = matches!(on_error, OnError::Stop);
        let results = self.execute_batch_nodes(statements, stop_on_error).await?;
        Ok(ExecutionReport::from_results(results))
    }

    /// Shared preparation: embeddings, named-vector validation, and UPSERT
    /// collection auto-creation. Callers must preserve statement order because
    /// preparation may read or mutate backend state.
    pub(crate) async fn prepare_statement(&self, mut stmt: Stmt) -> Result<Stmt, QqlError> {
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
}
