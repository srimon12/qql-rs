use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json;

use qql_core::ast::Stmt;
use qql_core::error::QqlError;
use qql_core::parser;

use crate::config::QqlConfig;
use crate::embedder::Embedder;
use crate::pipeline::QueryPointsRequest;

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
            Box::new(crate::rest::RestQdrant::new(url, api_key)?),
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
        let stmt = qql_core::parser::Parser::parse(query)?;
        let mut plan = String::new();
        Self::explain_stmt(&stmt, &mut plan);
        plan.push_str("Action: Explain-only mode (no Qdrant server)\n");
        Ok(plan)
    }

    fn explain_stmt(stmt: &Stmt, plan: &mut String) {
        match stmt {
            Stmt::ShowCollections => {
                plan.push_str("Statement: SHOW COLLECTIONS\n");
            }
            Stmt::ShowCollection(collection) => {
                plan.push_str(&format!("Statement: SHOW COLLECTION {}\n", collection));
            }
            Stmt::CreateCollection(s) => {
                plan.push_str(&format!("Statement: CREATE COLLECTION {}\n", s.collection));
                if let Some(model) = &s.model {
                    plan.push_str(&format!("Model: {}\n", model));
                }
                if s.rerank {
                    plan.push_str("Type: HYBRID + RERANK (dense + sparse + ColBERT multivector)\n");
                } else if s.hybrid {
                    plan.push_str("Type: HYBRID (dense + sparse)\n");
                } else {
                    plan.push_str("Type: DENSE\n");
                }
                for v in &s.vectors {
                    plan.push_str(&format!("Vector: {}, Size: {}\n", v.name, v.size));
                }
            }
            Stmt::AlterCollection(s) => {
                plan.push_str(&format!("Statement: ALTER COLLECTION {}\n", s.collection));
            }
            Stmt::DropCollection(s) => {
                plan.push_str(&format!("Statement: DROP COLLECTION {}\n", s.collection));
            }
            Stmt::Insert(s) => {
                plan.push_str(&format!("Statement: INSERT INTO {}\n", s.collection));
                if let Some(model) = &s.model {
                    plan.push_str(&format!("Model: {}\n", model));
                }
                plan.push_str(&format!("Rows: {}\n", s.values_list.len()));
            }
            Stmt::Select(s) => {
                plan.push_str(&format!(
                    "Statement: SELECT * FROM {} WHERE id = '{:?}'\n",
                    s.collection, s.point_id
                ));
            }
            Stmt::Scroll(s) => {
                plan.push_str(&format!(
                    "Statement: SCROLL FROM {} LIMIT {}\n",
                    s.collection, s.limit
                ));
            }
            Stmt::Query(q) => {
                let mode_str = match q.mode {
                    qql_core::ast::QueryMode::Nearest => "NEAREST",
                    qql_core::ast::QueryMode::Recommend => "RECOMMEND",
                    qql_core::ast::QueryMode::Context => "CONTEXT",
                    qql_core::ast::QueryMode::Discover => "DISCOVER",
                    qql_core::ast::QueryMode::OrderBy => "ORDER BY",
                    qql_core::ast::QueryMode::Sample => "SAMPLE",
                    qql_core::ast::QueryMode::RelevanceFeedback => "RELEVANCE FEEDBACK",
                };
                let coll: &str = q.collection.as_deref().unwrap_or("<none>");
                if !mode_str.is_empty() {
                    plan.push_str(&format!(
                        "Statement: QUERY {} FROM {} LIMIT {}\n",
                        mode_str, coll, q.limit
                    ));
                } else {
                    plan.push_str(&format!(
                        "Statement: QUERY FROM {} LIMIT {}\n",
                        coll, q.limit
                    ));
                }
                if let Some(text) = &q.query_text {
                    plan.push_str(&format!("Query: '{}'\n", text));
                }
                if !q.raw_vector.is_empty() {
                    plan.push_str(&format!("Raw Vector: {:?}\n", q.raw_vector));
                }
                match q.query_type {
                    qql_core::ast::QueryType::Hybrid => plan.push_str("Using: HYBRID\n"),
                    qql_core::ast::QueryType::Sparse => plan.push_str("Using: SPARSE\n"),
                    qql_core::ast::QueryType::Dense => {}
                }
                if let Some(u) = &q.using_ {
                    plan.push_str(&format!("Using: '{}'\n", u));
                }
                if let Some(m) = &q.model {
                    plan.push_str(&format!("Model: {}\n", m));
                }
                if q.offset > 0 {
                    plan.push_str(&format!("Offset: {}\n", q.offset));
                }
                if let Some(th) = &q.score_threshold {
                    plan.push_str(&format!("Score threshold: {}\n", th));
                }
                if let Some(gb) = &q.group_by {
                    plan.push_str(&format!("Group by: {}\n", gb));
                }
                if q.rerank {
                    plan.push_str("Rerank: enabled\n");
                }
                if !q.ctes.is_empty() {
                    plan.push_str(&format!("CTEs: {} defined\n", q.ctes.len()));
                }
                if !q.prefetch_refs.is_empty() {
                    plan.push_str(&format!("Prefetch refs: {}\n", q.prefetch_refs.len()));
                }
                if let Some(ft) = &q.fusion_type {
                    plan.push_str(&format!("Fusion: {}\n", ft));
                }
            }
            Stmt::Delete(s) => {
                if let Some(field) = &s.field {
                    plan.push_str(&format!(
                        "Statement: DELETE FROM {} WHERE {} = '{:?}'\n",
                        s.collection, field, s.value
                    ));
                } else {
                    plan.push_str(&format!(
                        "Statement: DELETE FROM {} WHERE id = '{:?}'\n",
                        s.collection, s.point_id
                    ));
                }
            }
            Stmt::UpdateVector(s) => {
                plan.push_str(&format!(
                    "Statement: UPDATE {} SET VECTOR = [...] WHERE id = '{:?}'\n",
                    s.collection, s.point_id
                ));
            }
            Stmt::UpdatePayload(s) => {
                plan.push_str(&format!(
                    "Statement: UPDATE {} SET PAYLOAD = {{...}} WHERE id = '{:?}'\n",
                    s.collection, s.point_id
                ));
            }
            Stmt::CreateIndex(s) => {
                plan.push_str(&format!(
                    "Statement: CREATE INDEX ON COLLECTION {} FOR {} TYPE {}\n",
                    s.collection, s.field, s.field_type
                ));
            }
        }
    }

    pub fn client(&self) -> &dyn QdrantOps {
        self.client.as_ref()
    }

    pub fn embedder(&self) -> Option<Arc<dyn Embedder>> {
        self.embedder.clone()
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

    pub async fn execute_node(&self, stmt: Stmt) -> Result<ExecResponse, QqlError> {
        match stmt {
            Stmt::ShowCollections => self.do_show_collections().await,
            Stmt::ShowCollection(collection) => self.do_show_collection(&collection).await,
            Stmt::CreateCollection(n) => self.do_create_collection(*n).await,
            Stmt::AlterCollection(n) => self.do_alter_collection(*n).await,
            Stmt::DropCollection(n) => self.do_drop_collection(&n.collection).await,
            Stmt::Insert(n) => self.do_insert(*n).await,
            Stmt::Select(n) => self.do_select(*n).await,
            Stmt::Scroll(n) => self.do_scroll(*n).await,
            Stmt::Query(n) => self.do_query(*n).await,
            Stmt::Delete(n) => self.do_delete(*n).await,
            Stmt::UpdateVector(n) => self.do_update_vector(*n).await,
            Stmt::UpdatePayload(n) => self.do_update_payload(*n).await,
            Stmt::CreateIndex(n) => self.do_create_index(*n).await,
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
                return Err(QqlError::runtime(
                    "query_batch only supports QUERY statements, got non-query statement"
                        .to_string(),
                ));
            }
        }
        self.query_batch_nodes(parsed_stmts).await
    }

    pub async fn query_batch_nodes(
        &self,
        stmts: Vec<qql_core::ast::QueryStmt>,
    ) -> Result<Vec<ExecResponse>, QqlError> {
        let num_statements = stmts.len();
        if num_statements == 0 {
            return Ok(Vec::new());
        }

        // 1. Build state and pipeline for each query, and run their pipelines
        let mut prepared = Vec::with_capacity(num_statements);
        for stmt in stmts {
            let (mut state, pipeline) = self.build_query_state_and_pipeline(&stmt).await?;
            pipeline.execute(&mut state).await?;
            prepared.push((state, pipeline));
        }

        // 2. Group flat queries by collection
        struct CollectionBatch {
            indices: Vec<usize>,
            requests: Vec<QueryPointsRequest>,
        }

        let mut batches: HashMap<String, CollectionBatch> = HashMap::new();
        let mut ordered_collections = Vec::new();
        let mut results = vec![
            ExecResponse {
                ok: false,
                operation: String::new(),
                message: String::new(),
                data: None,
            };
            num_statements
        ];

        for (i, (state, pipeline)) in prepared.iter().enumerate() {
            if !state.group_by.is_empty() {
                // Execute grouped query individually
                let resp = self.execute_grouped_query(pipeline, state).await?;
                results[i] = resp;
            } else {
                let coll = state.collection_name.clone();
                if !batches.contains_key(&coll) {
                    ordered_collections.push(coll.clone());
                    batches.insert(
                        coll.clone(),
                        CollectionBatch {
                            indices: Vec::new(),
                            requests: Vec::new(),
                        },
                    );
                }
                let b = batches.get_mut(&coll).unwrap();
                let mut req = pipeline.build_flat_request(state)?;
                if req.with_payload.is_none() {
                    req.with_payload = Some(crate::pipeline::WithPayload {
                        enable: Some(true),
                        include: Vec::new(),
                        exclude: Vec::new(),
                    });
                }
                b.indices.push(i);
                b.requests.push(req);
            }
        }

        // 3. Execute batched flat queries per collection
        for coll in ordered_collections {
            let batch = batches.remove(&coll).unwrap();
            let batch_results = self.client.query_batch(batch.requests).await?;
            for (j, pts) in batch_results.into_iter().enumerate() {
                let orig_idx = batch.indices[j];
                let formatted: Vec<SearchHit> = pts
                    .into_iter()
                    .map(|hit| {
                        let payload_map = hit.payload.clone();
                        SearchHit {
                            id: crate::executor::helpers::point_id_string(&hit.id),
                            score: hit.score,
                            text: payload_map.as_ref().and_then(|p| {
                                p.get("text")
                                    .and_then(|v| v.as_str().map(|s| s.to_string()))
                            }),
                            payload: payload_map,
                        }
                    })
                    .collect();

                results[orig_idx] = ExecResponse {
                    ok: true,
                    operation: "QUERY".to_string(),
                    message: format!("Found {} hits", formatted.len()),
                    data: Some(serde_json::to_value(formatted).unwrap_or(serde_json::Value::Null)),
                };
            }
        }

        Ok(results)
    }
}

pub(crate) mod ddl;
pub(crate) mod dml;
pub(crate) mod helpers;
