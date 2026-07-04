use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json;

use qql_core::ast::{self, Stmt, Value};
use qql_core::error::QqlError;
use qql_core::parser;

use crate::config::QqlConfig;
use crate::embedder::{Embedder, HttpEmbedder};
use crate::filter_conv::{FilterConverter, QdrantFilter};
use crate::pipeline::{
    self, build_search_params, ContextNode, ContextPairInput, DenseEmbedNode, DiscoverNode,
    FusionNode, OrderByNode, PointId, QueryPipeline, QueryPointsGroupsRequest, QueryPointsRequest,
    QueryState, RawVectorNode, RecommendNode, RelevanceFeedbackNode, RerankNode, SampleNode,
    SparseEmbedNode, WithPayload, WithVectors,
};

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredPoint {
    pub id: PointId,
    pub score: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PointGroup {
    pub group_id: String,
    pub hits: Vec<ScoredPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub id: String,
    pub score: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupedSearchResult {
    pub group_id: String,
    pub hits: Vec<SearchHit>,
}

#[derive(Debug, Clone)]
pub struct VectorTopology {
    pub dense_vector: Option<String>,
    pub sparse_vector: Option<String>,
    pub rerank_vector: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateCollectionReq {
    pub collection_name: String,
    pub vectors_config: Option<serde_json::Value>,
    pub sparse_vectors_config: Option<serde_json::Value>,
    pub hnsw_config: Option<serde_json::Value>,
    pub optimizers_config: Option<serde_json::Value>,
    pub quantization_config: Option<serde_json::Value>,
    pub params: Option<serde_json::Value>,
}

impl CreateCollectionReq {
    pub fn new(collection_name: String) -> Self {
        CreateCollectionReq {
            collection_name,
            vectors_config: None,
            sparse_vectors_config: None,
            hnsw_config: None,
            optimizers_config: None,
            quantization_config: None,
            params: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UpsertPointsReq {
    pub collection_name: String,
    pub points: Vec<PointStruct>,
}

#[derive(Debug, Clone)]
pub struct PointStruct {
    pub id: PointId,
    pub vector: Option<serde_json::Value>,
    pub payload: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone)]
pub struct DeletePointsReq {
    pub collection_name: String,
    pub filter: Option<QdrantFilter>,
    pub point_id: Option<PointId>,
}

#[derive(Debug, Clone)]
pub struct UpdateVectorsReq {
    pub collection_name: String,
    pub point_id: PointId,
    pub vector: Vec<f32>,
    pub vector_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SetPayloadReq {
    pub collection_name: String,
    pub point_id: Option<PointId>,
    pub filter: Option<QdrantFilter>,
    pub payload: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct CreateFieldIndexReq {
    pub collection_name: String,
    pub field: String,
    pub field_type: String,
    pub options: Vec<(String, Value<'static>)>,
}

#[derive(Debug, Clone)]
pub struct ScrollPointsReq {
    pub collection_name: String,
    pub limit: u64,
    pub filter: Option<QdrantFilter>,
    pub after: Option<PointId>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RetrievedPoint {
    pub id: PointId,
    pub payload: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct CountPointsReq {
    pub collection_name: String,
    pub filter: Option<QdrantFilter>,
}

#[derive(Debug, Clone)]
pub struct CollectionInfo {
    pub name: String,
    pub status: String,
    pub points_count: Option<u64>,
    pub indexed_vectors_count: Option<u64>,
    pub segments_count: u64,
    pub config: Option<CollectionConfig>,
    pub payload_schema: HashMap<String, PayloadSchemaInfo>,
}

#[derive(Debug, Clone)]
pub struct CollectionConfig {
    pub params: Option<CollectionParams>,
}

#[derive(Debug, Clone)]
pub struct CollectionParams {
    pub vectors_config: Option<VectorsConfigType>,
    pub sparse_vectors_config: Option<HashMap<String, SparseVectorConfig>>,
    pub shard_number: Option<u64>,
    pub replication_factor: Option<u64>,
    pub write_consistency_factor: Option<u64>,
    pub read_fan_out_factor: Option<u64>,
    pub read_fan_out_delay_ms: Option<u64>,
    pub on_disk_payload: Option<bool>,
}

#[derive(Debug, Clone)]
pub enum VectorsConfigType {
    Single(VectorParams),
    Multi(HashMap<String, VectorParams>),
}

#[derive(Debug, Clone)]
pub struct VectorParams {
    pub size: u64,
    pub distance: String,
    pub on_disk: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct SparseVectorConfig {
    pub modifier: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PayloadSchemaInfo {
    pub data_type: String,
    pub params: Option<serde_json::Value>,
}

#[async_trait]
pub trait QdrantOperations: Send + Sync {
    async fn list_collections(&self) -> Result<Vec<String>, QqlError>;
    async fn collection_exists(&self, name: &str) -> Result<bool, QqlError>;
    async fn get_collection_info(&self, name: &str) -> Result<CollectionInfo, QqlError>;
    async fn create_collection(&self, req: CreateCollectionReq) -> Result<(), QqlError>;
    async fn update_collection(&self, req: serde_json::Value) -> Result<(), QqlError>;
    async fn delete_collection(&self, name: &str) -> Result<(), QqlError>;
    async fn upsert(&self, req: UpsertPointsReq) -> Result<(), QqlError>;
    async fn query(&self, req: QueryPointsRequest) -> Result<Vec<ScoredPoint>, QqlError>;
    async fn query_groups(
        &self,
        req: QueryPointsGroupsRequest,
    ) -> Result<Vec<PointGroup>, QqlError>;
    async fn query_batch(
        &self,
        req: Vec<QueryPointsRequest>,
    ) -> Result<Vec<Vec<ScoredPoint>>, QqlError>;
    async fn delete(&self, req: DeletePointsReq) -> Result<(), QqlError>;
    async fn update_vectors(&self, req: UpdateVectorsReq) -> Result<(), QqlError>;
    async fn set_payload(&self, req: SetPayloadReq) -> Result<(), QqlError>;
    async fn create_field_index(&self, req: CreateFieldIndexReq) -> Result<(), QqlError>;
    async fn scroll(
        &self,
        req: ScrollPointsReq,
    ) -> Result<(Vec<RetrievedPoint>, Option<PointId>), QqlError>;
    async fn count(&self, req: CountPointsReq) -> Result<u64, QqlError>;
    async fn get(&self, req: GetPointsReq) -> Result<Vec<RetrievedPoint>, QqlError>;
}

#[derive(Debug, Clone)]
pub struct GetPointsReq {
    pub collection_name: String,
    pub point_id: Value<'static>,
}

pub struct Executor {
    client: Box<dyn QdrantOperations>,
    config: Option<QqlConfig>,
    embedder: Option<Arc<dyn Embedder>>,
}

impl Executor {
    pub fn new(client: Box<dyn QdrantOperations>, config: Option<QqlConfig>) -> Self {
        Executor {
            client,
            config,
            embedder: None,
        }
    }

    pub fn with_embedder(
        client: Box<dyn QdrantOperations>,
        config: Option<QqlConfig>,
        embedder: Option<Arc<dyn Embedder>>,
    ) -> Self {
        Executor {
            client,
            config,
            embedder,
        }
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

    pub fn parse_query(query: &str) -> Result<Stmt<'_>, QqlError> {
        parser::Parser::parse(query)
    }

    pub async fn execute(&self, query: &str) -> Result<ExecResponse, QqlError> {
        let stmt = Self::parse_query(query)?;
        self.execute_node(stmt).await
    }

    pub async fn execute_node(&self, stmt: Stmt<'_>) -> Result<ExecResponse, QqlError> {
        match stmt {
            Stmt::ShowCollections => self.do_show_collections().await,
            Stmt::ShowCollection(collection) => self.do_show_collection(collection).await,
            Stmt::CreateCollection(n) => self.do_create_collection(*n).await,
            Stmt::AlterCollection(n) => self.do_alter_collection(*n).await,
            Stmt::DropCollection(n) => self.do_drop_collection(n.collection).await,
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

    // ── Query execution ──

    async fn build_query_state_and_pipeline(
        &self,
        stmt: &ast::QueryStmt<'_>,
    ) -> Result<(QueryState, QueryPipeline), QqlError> {
        let dense_vector_name: String;
        let sparse_vector_name: String;

        if let Some(using) = stmt.using_ {
            dense_vector_name = using.to_string();
            sparse_vector_name = using.to_string();
        } else {
            let topo = self
                .resolve_vector_topology(stmt.collection.unwrap_or(""))
                .await?;
            if let Some(ref dv) = topo.dense_vector {
                if !dv.is_empty() {
                    dense_vector_name = dv.clone();
                    if let Some(ref sv) = topo.sparse_vector {
                        if !sv.is_empty() {
                            sparse_vector_name = sv.clone();
                        } else {
                            sparse_vector_name = dense_vector_name.clone();
                        }
                    } else {
                        sparse_vector_name = dense_vector_name.clone();
                    }
                } else {
                    dense_vector_name = String::new();
                    sparse_vector_name = String::new();
                }
            } else {
                dense_vector_name = String::new();
                sparse_vector_name = String::new();
            }
        }

        let dense_model = self.resolve_dense_model(stmt.model);
        let sparse_model = if stmt.query_type == ast::QueryType::Hybrid {
            Some(self.resolve_sparse_model(stmt.model))
        } else {
            None
        };

        let qdrant_filter = if let Some(ref filter) = stmt.query_filter {
            let converter = FilterConverter::new();
            converter.build_filter(filter)?
        } else {
            None
        };

        let mut state = QueryState {
            query_text: stmt.query_text.map(|s| s.to_string()).unwrap_or_default(),
            prefetches: Vec::new(),
            manual_prefetches: Vec::new(),
            target_query: None,
            params: stmt
                .with_clause
                .as_ref()
                .and_then(|wc| build_search_params(wc)),
            fusion_config: None,
            has_mmr: has_mmr(stmt.with_clause.as_ref().map(|wc| wc.as_ref())),
            mmr_candidates: 0,
            mmr_diversity: 0.0,
            local_embed: self.uses_local_embeddings(),
            embedder: self.embedder.clone(),
            cloud_model_options: self.cloud_model_options(),
            dense_model: dense_model.clone(),

            doc_options: None,
            request_timeout: self.request_timeout(),

            collection_name: stmt.collection.map(|s| s.to_string()).unwrap_or_default(),
            vector_name: String::new(),
            limit: stmt.limit as u64,
            offset: stmt.offset as u64,
            qdrant_filter,
            score_threshold: stmt.score_threshold.map(|v| v as f32),
            lookup_from: if !stmt.lookup_from.unwrap_or("").is_empty() {
                Some(pipeline::LookupLocation {
                    collection_name: stmt.lookup_from.unwrap().to_string(),
                    vector_name: stmt.lookup_vector.map(|s| s.to_string()),
                })
            } else {
                None
            },
            with_payload: build_with_payload(stmt.with_payload.as_ref().map(|p| p.as_ref())),
            with_vectors: build_with_vectors(stmt.with_vectors.as_ref().map(|v| v.as_ref())),
            group_by: stmt.group_by.map(|s| s.to_string()).unwrap_or_default(),
            group_size: stmt.group_size.unwrap_or(0) as u64,
            with_lookup: stmt.with_lookup_collection.map(|c| pipeline::WithLookup {
                collection: c.to_string(),
            }),
        };

        // Fusion config
        if let Some(ref wc) = stmt.with_clause {
            if wc.rrf_k.is_some() || !wc.rrf_weights.is_empty() {
                state.fusion_config = Some(pipeline::RrfConfig {
                    k: wc.rrf_k.map(|k| k as u32),
                    weights: wc.rrf_weights.clone(),
                });
            }
        }

        // MMR
        if state.has_mmr {
            if let Some(ref wc) = stmt.with_clause {
                if let Some(d) = wc.mmr_diversity {
                    state.mmr_diversity = d as f32;
                }
                if let Some(c) = wc.mmr_candidates {
                    state.mmr_candidates = c as u32;
                }
            }
        }

        let mut exec_pipeline = QueryPipeline::new();

        match stmt.mode {
            ast::QueryMode::OrderBy => {
                let asc = stmt.order_by_asc.unwrap_or(true);
                exec_pipeline.add(Box::new(OrderByNode {
                    field: stmt
                        .order_by_field
                        .map(|s| s.to_string())
                        .unwrap_or_default(),
                    asc,
                }));
            }
            ast::QueryMode::Sample => {
                exec_pipeline.add(Box::new(SampleNode));
            }
            ast::QueryMode::RelevanceFeedback => {
                let feedback: Vec<(Value<'static>, f64)> = stmt
                    .feedback_items
                    .iter()
                    .map(|item| {
                        let example = clone_value(&item.example);
                        (example, item.score)
                    })
                    .collect();
                let strategy = stmt.feedback_strategy.as_ref().map(|s| (s.a, s.b, s.c));
                let target = clone_value(stmt.feedback_target.as_ref().unwrap_or(&Value::Null));
                exec_pipeline.add(Box::new(RelevanceFeedbackNode {
                    target,
                    feedback,
                    strategy,
                }));
            }
            ast::QueryMode::Nearest => {
                if !stmt.raw_vector.is_empty() {
                    if stmt.query_type == ast::QueryType::Hybrid {
                        if stmt.query_text.is_none() {
                            return Err(QqlError::runtime(
                                "USING HYBRID with a raw dense vector requires a text query for the sparse vector",
                            ));
                        }
                        exec_pipeline.add(Box::new(RawVectorNode {
                            vector: stmt.raw_vector.clone(),
                            vector_name: dense_vector_name.clone(),
                            limit: stmt.limit as u64 * 10,
                            as_prefetch: true,
                        }));
                        exec_pipeline.add(Box::new(SparseEmbedNode {
                            model: sparse_model
                                .clone()
                                .unwrap_or_else(|| SPARSE_MODEL_DEFAULT.to_string()),
                            vector_name: sparse_vector_name.clone(),
                            limit: stmt.limit as u64 * 10,
                            as_prefetch: true,
                        }));
                        exec_pipeline.add(Box::new(FusionNode {
                            mode: stmt.fusion_type.unwrap_or("rrf").to_string(),
                        }));
                    } else {
                        exec_pipeline.add(Box::new(RawVectorNode {
                            vector: stmt.raw_vector.clone(),
                            vector_name: dense_vector_name.clone(),
                            limit: stmt.limit as u64,
                            as_prefetch: false,
                        }));
                        if let Some(fusion_type) = &stmt.fusion_type {
                            exec_pipeline.add(Box::new(FusionNode {
                                mode: fusion_type.to_string(),
                            }));
                            state.vector_name = String::new();
                        }
                    }
                } else if let Some(query_id) = &stmt.query_id {
                    let id = clone_value(query_id);
                    exec_pipeline.add(Box::new(RecommendNode {
                        positive_ids: vec![id],
                        negative_ids: Vec::new(),
                        strategy: stmt.strategy.map(|s| s.to_string()),
                    }));
                } else {
                    if let Some(text) = &stmt.query_text {
                        state.query_text = text.to_string();
                    }
                    match stmt.query_type {
                        ast::QueryType::Hybrid => {
                            exec_pipeline.add(Box::new(DenseEmbedNode {
                                model: dense_model.clone(),
                                vector_name: dense_vector_name.clone(),
                                limit: stmt.limit as u64 * 10,
                                as_prefetch: true,
                            }));
                            exec_pipeline.add(Box::new(SparseEmbedNode {
                                model: sparse_model
                                    .clone()
                                    .unwrap_or_else(|| SPARSE_MODEL_DEFAULT.to_string()),
                                vector_name: sparse_vector_name.clone(),
                                limit: stmt.limit as u64 * 10,
                                as_prefetch: true,
                            }));
                            exec_pipeline.add(Box::new(FusionNode {
                                mode: stmt.fusion_type.unwrap_or("rrf").to_string(),
                            }));
                        }
                        ast::QueryType::Sparse => {
                            let sm = self.resolve_sparse_model(stmt.model);
                            exec_pipeline.add(Box::new(SparseEmbedNode {
                                model: sm.clone(),
                                vector_name: sparse_vector_name.clone(),
                                limit: stmt.limit as u64,
                                as_prefetch: false,
                            }));
                            state.vector_name = sparse_vector_name.clone();
                        }
                        ast::QueryType::Dense => {
                            if stmt.query_text.is_some() {
                                exec_pipeline.add(Box::new(DenseEmbedNode {
                                    model: dense_model.clone(),
                                    vector_name: dense_vector_name.clone(),
                                    limit: stmt.limit as u64,
                                    as_prefetch: false,
                                }));
                                state.vector_name = dense_vector_name.clone();
                            }
                        }
                    }
                    if let Some(fusion_type) = &stmt.fusion_type {
                        if stmt.query_type != ast::QueryType::Hybrid {
                            exec_pipeline.add(Box::new(FusionNode {
                                mode: fusion_type.to_string(),
                            }));
                            state.vector_name = String::new();
                        }
                    }
                }
            }
            ast::QueryMode::Recommend => {
                let pos: Vec<Value<'static>> = stmt.positive_ids.iter().map(clone_value).collect();
                let neg: Vec<Value<'static>> = stmt.negative_ids.iter().map(clone_value).collect();
                exec_pipeline.add(Box::new(RecommendNode {
                    positive_ids: pos,
                    negative_ids: neg,
                    strategy: stmt.strategy.map(|s| s.to_string()),
                }));
                state.vector_name = dense_vector_name.clone();
            }
            ast::QueryMode::Context => {
                let pairs: Vec<ContextPairInput> = stmt
                    .context_pairs
                    .iter()
                    .map(|p| ContextPairInput {
                        positive: Some(clone_value(&p.positive)),
                        negative: Some(clone_value(&p.negative)),
                    })
                    .collect();
                exec_pipeline.add(Box::new(ContextNode { pairs }));
                state.vector_name = dense_vector_name.clone();
            }
            ast::QueryMode::Discover => {
                let pairs: Vec<ContextPairInput> = stmt
                    .context_pairs
                    .iter()
                    .map(|p| ContextPairInput {
                        positive: Some(clone_value(&p.positive)),
                        negative: Some(clone_value(&p.negative)),
                    })
                    .collect();
                exec_pipeline.add(Box::new(DiscoverNode {
                    target: stmt.target.as_ref().map(clone_value),
                    pairs,
                }));
                state.vector_name = dense_vector_name.clone();
            }
        }

        if stmt.rerank {
            let rerank_model = stmt.rerank_model.unwrap_or("default-reranker");
            exec_pipeline.add(Box::new(RerankNode {
                model: rerank_model.to_string(),
            }));
        }

        Ok((state, exec_pipeline))
    }

    async fn do_query(&self, stmt: ast::QueryStmt<'_>) -> Result<ExecResponse, QqlError> {
        let (mut state, exec_pipeline) = self.build_query_state_and_pipeline(&stmt).await?;

        exec_pipeline.execute(&mut state).await?;

        if !state.group_by.is_empty() {
            self.execute_grouped_query(&exec_pipeline, &state).await
        } else {
            self.execute_flat_query(&exec_pipeline, &state).await
        }
    }

    async fn execute_flat_query(
        &self,
        pipeline: &QueryPipeline,
        state: &QueryState,
    ) -> Result<ExecResponse, QqlError> {
        let mut req = pipeline.build_flat_request(state);
        if req.with_payload.is_none() {
            req.with_payload = Some(WithPayload {
                enable: Some(true),
                include: Vec::new(),
                exclude: Vec::new(),
            });
        }
        let results = self.client.query(req).await?;

        let formatted: Vec<SearchHit> = results
            .into_iter()
            .map(|hit| SearchHit {
                id: point_id_string(&hit.id),
                score: hit.score,
                text: hit.payload.as_ref().and_then(|p| {
                    p.get("text")
                        .and_then(|v| v.as_str().map(|s| s.to_string()))
                }),
                payload: hit.payload,
            })
            .collect();

        Ok(ExecResponse {
            ok: true,
            operation: "QUERY".to_string(),
            message: format!("Found {} hits", formatted.len()),
            data: Some(serde_json::to_value(formatted).unwrap_or(serde_json::Value::Null)),
        })
    }

    async fn execute_grouped_query(
        &self,
        pipeline: &QueryPipeline,
        state: &QueryState,
    ) -> Result<ExecResponse, QqlError> {
        let mut req = pipeline.build_grouped_request(state);
        if req.with_payload.is_none() {
            req.with_payload = Some(WithPayload {
                enable: Some(true),
                include: Vec::new(),
                exclude: Vec::new(),
            });
        }
        let groups = self.client.query_groups(req).await?;

        let formatted: Vec<GroupedSearchResult> = groups
            .into_iter()
            .map(|g| {
                let hits: Vec<SearchHit> = g
                    .hits
                    .into_iter()
                    .map(|hit| SearchHit {
                        id: point_id_string(&hit.id),
                        score: hit.score,
                        text: hit.payload.as_ref().and_then(|p| {
                            p.get("text")
                                .and_then(|v| v.as_str().map(|s| s.to_string()))
                        }),
                        payload: hit.payload,
                    })
                    .collect();
                GroupedSearchResult {
                    group_id: g.group_id,
                    hits,
                }
            })
            .collect();

        Ok(ExecResponse {
            ok: true,
            operation: "QUERY_GROUPS".to_string(),
            message: format!("Found {} groups", formatted.len()),
            data: Some(serde_json::to_value(formatted).unwrap_or(serde_json::Value::Null)),
        })
    }

    // ── Helper methods ──

    fn resolve_dense_model(&self, override_model: Option<&str>) -> String {
        if let Some(m) = override_model {
            if !m.is_empty() {
                return m.to_string();
            }
        }
        if let Some(ref cfg) = self.config {
            if !cfg.embedding_model.as_deref().unwrap_or("").is_empty() {
                return cfg.embedding_model.as_ref().unwrap().clone();
            }
            if !cfg.inference_model.as_deref().unwrap_or("").is_empty() {
                return cfg.inference_model.as_ref().unwrap().clone();
            }
        }
        DENSE_MODEL_DEFAULT.to_string()
    }

    fn resolve_sparse_model(&self, override_model: Option<&str>) -> String {
        if let Some(m) = override_model {
            if !m.is_empty() {
                return m.to_string();
            }
        }
        if let Some(ref cfg) = self.config {
            if let Some(ref sm) = cfg.sparse_inference_model {
                if !sm.is_empty() {
                    return sm.clone();
                }
            }
        }
        SPARSE_MODEL_DEFAULT.to_string()
    }

    fn inference_mode(&self) -> String {
        if let Some(ref cfg) = self.config {
            let mode = cfg.inference_mode.trim();
            if !mode.is_empty() {
                return mode.to_lowercase();
            }
        }
        INFERENCE_MODE_DEFAULT.to_string()
    }

    fn uses_local_embeddings(&self) -> bool {
        let mode = self.inference_mode();
        mode == "local" || mode == "external"
    }

    fn cloud_model_options(&self) -> HashMap<String, String> {
        if self.uses_local_embeddings() {
            return HashMap::new();
        }
        self.config
            .as_ref()
            .map(|c| c.cloud_model_options.clone())
            .unwrap_or_default()
    }

    async fn resolve_vector_topology(&self, collection: &str) -> Result<VectorTopology, QqlError> {
        let info = self.client.get_collection_info(collection).await?;
        let mut topo = VectorTopology {
            dense_vector: None,
            sparse_vector: None,
            rerank_vector: None,
        };

        if let Some(ref config) = info.config {
            if let Some(ref params) = config.params {
                if let Some(ref vc) = params.vectors_config {
                    match vc {
                        VectorsConfigType::Multi(map) => {
                            for vname in map.keys() {
                                if vname == DENSE_VECTOR_NAME {
                                    topo.dense_vector = Some(DENSE_VECTOR_NAME.to_string());
                                } else if vname == RERANK_VECTOR_NAME {
                                    topo.rerank_vector = Some(RERANK_VECTOR_NAME.to_string());
                                } else if topo.dense_vector.is_none()
                                    || topo
                                        .dense_vector
                                        .as_ref()
                                        .map(|s| s.is_empty())
                                        .unwrap_or(true)
                                {
                                    topo.dense_vector = Some(vname.clone());
                                }
                            }
                        }
                        VectorsConfigType::Single(_) => {
                            topo.dense_vector = Some(String::new());
                        }
                    }
                }

                if let Some(ref svc) = params.sparse_vectors_config {
                    for vname in svc.keys() {
                        if vname == SPARSE_VECTOR_NAME {
                            topo.sparse_vector = Some(SPARSE_VECTOR_NAME.to_string());
                        } else if topo.sparse_vector.is_none()
                            || topo
                                .sparse_vector
                                .as_ref()
                                .map(|s| s.is_empty())
                                .unwrap_or(true)
                        {
                            topo.sparse_vector = Some(vname.clone());
                        }
                    }
                }
            }
        }

        Ok(topo)
    }

    async fn ensure_collection_for_insert(
        &self,
        collection: &str,
        model: Option<&str>,
        requested_hybrid: bool,
        explicit_dense: Option<&str>,
        explicit_sparse: Option<&str>,
    ) -> Result<bool, QqlError> {
        let exists = self.client.collection_exists(collection).await?;
        if exists {
            return Ok(false);
        }

        let dense_size = self.resolve_dense_vector_size(model).await?;
        let dense_name = explicit_dense.unwrap_or(DENSE_VECTOR_NAME);

        let mut vectors_map = HashMap::new();
        vectors_map.insert(
            dense_name.to_string(),
            VectorParams {
                size: dense_size as u64,
                distance: "Cosine".to_string(),
                on_disk: None,
            },
        );

        let mut create_req = CreateCollectionReq::new(collection.to_string());
        create_req.vectors_config = Some(serde_json::json!({
            dense_name: {
                "size": dense_size,
                "distance": "Cosine"
            }
        }));

        if requested_hybrid {
            let sparse_name = explicit_sparse.unwrap_or(SPARSE_VECTOR_NAME);
            create_req.sparse_vectors_config = Some(serde_json::json!({
                sparse_name: {
                    "modifier": "idf"
                }
            }));
        }

        self.client.create_collection(create_req).await?;
        // Wait for collection to be ready
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        Ok(true)
    }

    async fn resolve_dense_vector_size(&self, model: Option<&str>) -> Result<usize, QqlError> {
        if self.uses_local_embeddings() {
            if let Some(ref cfg) = self.config {
                if cfg.embedding_dimension > 0 {
                    return Ok(cfg.embedding_dimension);
                }
            }
            return match self.config.as_ref() {
                Some(cfg)
                    if !cfg.embedding_endpoint.as_deref().unwrap_or("").is_empty()
                        && !cfg.embedding_model.as_deref().unwrap_or("").is_empty() =>
                {
                    let embedder = HttpEmbedder::new(
                        cfg.embedding_endpoint.clone().unwrap_or_default(),
                        cfg.embedding_api_key.clone().unwrap_or_default(),
                        cfg.embedding_model.clone().unwrap_or_default(),
                        1,
                    )?;
                    let dim = embedder.probe_dimension("probe").await?;
                    Ok(dim)
                }
                _ => Err(QqlError::runtime(
                    "embedding_dimension must be configured for local inference mode",
                )),
            };
        }

        if let Some(ref cfg) = self.config {
            if cfg.embedding_dimension > 0 {
                return Ok(cfg.embedding_dimension);
            }
        }

        if model.is_some()
            && model.unwrap() != ""
            && self
                .config
                .as_ref()
                .map(|c| c.embedding_dimension == 0)
                .unwrap_or(true)
        {
            return Err(QqlError::runtime(
                "embedding_dimension must be configured when creating collections with USING MODEL",
            ));
        }

        Ok(DENSE_VECTOR_SIZE as usize)
    }

    #[allow(dead_code)]
    async fn collection_has_sparse_vector(&self, collection: &str) -> Result<bool, QqlError> {
        let exists = self.client.collection_exists(collection).await?;
        if !exists {
            return Ok(false);
        }
        let info = self.client.get_collection_info(collection).await?;
        if let Some(ref config) = info.config {
            if let Some(ref params) = config.params {
                if let Some(ref svc) = params.sparse_vectors_config {
                    return Ok(svc.contains_key(SPARSE_VECTOR_NAME));
                }
            }
        }
        Ok(false)
    }

    // ── Statement implementations ──

    async fn do_show_collections(&self) -> Result<ExecResponse, QqlError> {
        let collections = self.client.list_collections().await?;
        Ok(ExecResponse {
            ok: true,
            operation: "show_collections".to_string(),
            message: format!("Found {} collections", collections.len()),
            data: Some(serde_json::json!({"collections": collections})),
        })
    }

    async fn do_show_collection(&self, collection: &str) -> Result<ExecResponse, QqlError> {
        let exists = self.client.collection_exists(collection).await?;
        if !exists {
            return Err(QqlError::runtime(format!(
                "collection '{}' does not exist",
                collection
            )));
        }

        let info = self.client.get_collection_info(collection).await?;
        Ok(ExecResponse {
            ok: true,
            operation: "show_collection".to_string(),
            message: format!("Collection: {}", collection),
            data: Some(serde_json::json!({
                "name": collection,
                "status": info.status,
                "points_count": info.points_count,
                "segments_count": info.segments_count,
            })),
        })
    }

    async fn do_create_collection(
        &self,
        stmt: ast::CreateCollectionStmt<'_>,
    ) -> Result<ExecResponse, QqlError> {
        let exists = self.client.collection_exists(stmt.collection).await?;
        if exists {
            return Ok(ExecResponse {
                ok: true,
                operation: "create_collection".to_string(),
                message: format!("Collection '{}' already exists", stmt.collection),
                data: Some(serde_json::json!({
                    "collection": stmt.collection,
                    "exists": true,
                    "hybrid": stmt.hybrid,
                    "rerank": stmt.rerank,
                })),
            });
        }

        let mut create_req = CreateCollectionReq::new(stmt.collection.to_string());

        if !stmt.vectors.is_empty() {
            let mut params_map = serde_json::Map::new();
            for v in &stmt.vectors {
                let distance_str = match v.distance {
                    ast::VectorDistance::Cosine => "Cosine",
                    ast::VectorDistance::Dot => "Dot",
                    ast::VectorDistance::Euclid => "Euclid",
                    ast::VectorDistance::Manhattan => "Manhattan",
                };
                let mut vp = serde_json::json!({
                    "size": v.size,
                    "distance": distance_str,
                });
                let vp_obj = vp.as_object_mut().unwrap();
                if v.multivector.is_some() {
                    vp_obj.insert(
                        "multivector_config".to_string(),
                        serde_json::json!({"comparator": "max_sim"}),
                    );
                }
                if let Some(ref hnsw) = v.hnsw {
                    let mut hnsw_map = serde_json::Map::new();
                    if let Some(m) = hnsw.m {
                        hnsw_map.insert("m".to_string(), serde_json::json!(m));
                    }
                    if let Some(ef) = hnsw.ef_construct {
                        hnsw_map.insert("ef_construct".to_string(), serde_json::json!(ef));
                    }
                    if let Some(fs) = hnsw.full_scan_threshold {
                        hnsw_map.insert("full_scan_threshold".to_string(), serde_json::json!(fs));
                    }
                    if let Some(mi) = hnsw.max_indexing_threads {
                        hnsw_map.insert("max_indexing_threads".to_string(), serde_json::json!(mi));
                    }
                    if let Some(od) = hnsw.on_disk {
                        hnsw_map.insert("on_disk".to_string(), serde_json::json!(od));
                    }
                    if let Some(pm) = hnsw.payload_m {
                        hnsw_map.insert("payload_m".to_string(), serde_json::json!(pm));
                    }
                    vp_obj.insert(
                        "hnsw_config".to_string(),
                        serde_json::Value::Object(hnsw_map),
                    );
                }
                if let Some(ref quant) = v.quantization {
                    let q_val = build_quantization_config(quant)?;
                    vp_obj.insert("quantization_config".to_string(), q_val);
                }
                params_map.insert(v.name.to_string(), vp);
            }
            create_req.vectors_config = Some(serde_json::Value::Object(params_map));
        } else {
            let dense_size = self.resolve_dense_vector_size(stmt.model).await? as u64;
            let dense_name = stmt.dense_vector.unwrap_or(DENSE_VECTOR_NAME);
            create_req.vectors_config = Some(serde_json::json!({
                dense_name: {
                    "size": dense_size,
                    "distance": "Cosine"
                }
            }));
        }

        if !stmt.sparse_vectors.is_empty() {
            let mut sparse_map = serde_json::Map::new();
            for sv in &stmt.sparse_vectors {
                sparse_map.insert(sv.name.to_string(), serde_json::json!({"modifier": "idf"}));
            }
            create_req.sparse_vectors_config = Some(serde_json::Value::Object(sparse_map));
        } else if stmt.hybrid || stmt.rerank {
            let sparse_name = stmt.sparse_vector.unwrap_or(SPARSE_VECTOR_NAME);
            create_req.sparse_vectors_config = Some(serde_json::json!({
                sparse_name: {"modifier": "idf"}
            }));
        }

        if let Some(ref config) = stmt.config {
            if let Some(ref hnsw) = config.hnsw {
                let mut hnsw_map = serde_json::Map::new();
                if let Some(m) = hnsw.m {
                    hnsw_map.insert("m".to_string(), serde_json::json!(m));
                }
                if let Some(ef) = hnsw.ef_construct {
                    hnsw_map.insert("ef_construct".to_string(), serde_json::json!(ef));
                }
                if let Some(fs) = hnsw.full_scan_threshold {
                    hnsw_map.insert("full_scan_threshold".to_string(), serde_json::json!(fs));
                }
                if let Some(mi) = hnsw.max_indexing_threads {
                    hnsw_map.insert("max_indexing_threads".to_string(), serde_json::json!(mi));
                }
                if let Some(od) = hnsw.on_disk {
                    hnsw_map.insert("on_disk".to_string(), serde_json::json!(od));
                }
                if let Some(pm) = hnsw.payload_m {
                    hnsw_map.insert("payload_m".to_string(), serde_json::json!(pm));
                }
                create_req.hnsw_config = Some(serde_json::Value::Object(hnsw_map));
            }
            if let Some(ref opt) = config.optimizers {
                let mut opt_map = serde_json::Map::new();
                if let Some(dt) = opt.deleted_threshold {
                    opt_map.insert("deleted_threshold".to_string(), serde_json::json!(dt));
                }
                if let Some(vm) = opt.vacuum_min_vector_number {
                    opt_map.insert(
                        "vacuum_min_vector_number".to_string(),
                        serde_json::json!(vm),
                    );
                }
                if let Some(ds) = opt.default_segment_number {
                    opt_map.insert("default_segment_number".to_string(), serde_json::json!(ds));
                }
                if let Some(ms) = opt.max_segment_size {
                    opt_map.insert("max_segment_size".to_string(), serde_json::json!(ms));
                }
                if let Some(mt) = opt.memmap_threshold {
                    opt_map.insert("memmap_threshold".to_string(), serde_json::json!(mt));
                }
                if let Some(it) = opt.indexing_threshold {
                    opt_map.insert("indexing_threshold".to_string(), serde_json::json!(it));
                }
                if let Some(fi) = opt.flush_interval_sec {
                    opt_map.insert("flush_interval_sec".to_string(), serde_json::json!(fi));
                }
                if let Some(pu) = opt.prevent_unoptimized {
                    opt_map.insert("prevent_unoptimized".to_string(), serde_json::json!(pu));
                }
                if let Some(ref t) = opt.max_optimization_threads {
                    if t.auto_ {
                        opt_map.insert(
                            "max_optimization_threads".to_string(),
                            serde_json::json!("auto"),
                        );
                    } else {
                        opt_map.insert(
                            "max_optimization_threads".to_string(),
                            serde_json::json!(t.value),
                        );
                    }
                }
                create_req.optimizers_config = Some(serde_json::Value::Object(opt_map));
            }
            if let Some(ref params) = config.params {
                let mut params_map = serde_json::Map::new();
                if let Some(rf) = params.replication_factor {
                    params_map.insert("replication_factor".to_string(), serde_json::json!(rf));
                }
                if let Some(wc) = params.write_consistency_factor {
                    params_map.insert(
                        "write_consistency_factor".to_string(),
                        serde_json::json!(wc),
                    );
                }
                if let Some(od) = params.on_disk_payload {
                    params_map.insert("on_disk_payload".to_string(), serde_json::json!(od));
                }
                create_req.params = Some(serde_json::Value::Object(params_map));
            }
            if let Some(ref quant) = config.quantization {
                let q_val = build_quantization_config(quant)?;
                create_req.quantization_config = Some(q_val);
            }
            if let Some(ref vectors) = config.vectors {
                if let Some(on_disk) = vectors.on_disk {
                    if let Some(ref mut vec_val) = create_req.vectors_config {
                        if let Some(obj) = vec_val.as_object_mut() {
                            for (_, val) in obj.iter_mut() {
                                if let Some(param) = val.as_object_mut() {
                                    param.insert("on_disk".to_string(), serde_json::json!(on_disk));
                                }
                            }
                        }
                    }
                }
            }
        }

        self.client.create_collection(create_req).await?;
        // Wait for collection to be ready
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        let mut message = format!("Collection '{}' created", stmt.collection);
        if stmt.vectors.is_empty() {
            if stmt.rerank {
                message = format!(
                    "Collection '{}' created (hybrid: dense + sparse + ColBERT)",
                    stmt.collection
                );
            } else if stmt.hybrid {
                message = format!(
                    "Collection '{}' created (hybrid: dense + sparse)",
                    stmt.collection
                );
            } else {
                message.push_str(" (dense)");
            }
        } else {
            message.push_str(" (multi-vector schema)");
        }

        Ok(ExecResponse {
            ok: true,
            operation: "create_collection".to_string(),
            message,
            data: Some(serde_json::json!({
                "collection": stmt.collection,
                "exists": false,
                "hybrid": stmt.hybrid,
                "rerank": stmt.rerank,
            })),
        })
    }

    async fn do_alter_collection(
        &self,
        stmt: ast::AlterCollectionStmt<'_>,
    ) -> Result<ExecResponse, QqlError> {
        let exists = self.client.collection_exists(stmt.collection).await?;
        if !exists {
            return Err(QqlError::runtime(format!(
                "collection '{}' does not exist",
                stmt.collection
            )));
        }

        let mut req_map = serde_json::Map::new();
        req_map.insert(
            "collection_name".to_string(),
            serde_json::json!(stmt.collection),
        );

        if let Some(ref config) = stmt.config {
            if let Some(ref hnsw) = config.hnsw {
                let mut hnsw_map = serde_json::Map::new();
                if let Some(m) = hnsw.m {
                    hnsw_map.insert("m".to_string(), serde_json::json!(m));
                }
                if let Some(ef) = hnsw.ef_construct {
                    hnsw_map.insert("ef_construct".to_string(), serde_json::json!(ef));
                }
                if let Some(fs) = hnsw.full_scan_threshold {
                    hnsw_map.insert("full_scan_threshold".to_string(), serde_json::json!(fs));
                }
                if let Some(mi) = hnsw.max_indexing_threads {
                    hnsw_map.insert("max_indexing_threads".to_string(), serde_json::json!(mi));
                }
                if let Some(od) = hnsw.on_disk {
                    hnsw_map.insert("on_disk".to_string(), serde_json::json!(od));
                }
                if let Some(pm) = hnsw.payload_m {
                    hnsw_map.insert("payload_m".to_string(), serde_json::json!(pm));
                }
                req_map.insert(
                    "hnsw_config".to_string(),
                    serde_json::Value::Object(hnsw_map),
                );
            }
            if let Some(ref opt) = config.optimizers {
                let mut opt_map = serde_json::Map::new();
                if let Some(dt) = opt.deleted_threshold {
                    opt_map.insert("deleted_threshold".to_string(), serde_json::json!(dt));
                }
                if let Some(vm) = opt.vacuum_min_vector_number {
                    opt_map.insert(
                        "vacuum_min_vector_number".to_string(),
                        serde_json::json!(vm),
                    );
                }
                if let Some(ds) = opt.default_segment_number {
                    opt_map.insert("default_segment_number".to_string(), serde_json::json!(ds));
                }
                if let Some(ms) = opt.max_segment_size {
                    opt_map.insert("max_segment_size".to_string(), serde_json::json!(ms));
                }
                if let Some(mt) = opt.memmap_threshold {
                    opt_map.insert("memmap_threshold".to_string(), serde_json::json!(mt));
                }
                if let Some(it) = opt.indexing_threshold {
                    opt_map.insert("indexing_threshold".to_string(), serde_json::json!(it));
                }
                if let Some(fi) = opt.flush_interval_sec {
                    opt_map.insert("flush_interval_sec".to_string(), serde_json::json!(fi));
                }
                if let Some(pu) = opt.prevent_unoptimized {
                    opt_map.insert("prevent_unoptimized".to_string(), serde_json::json!(pu));
                }
                if let Some(ref t) = opt.max_optimization_threads {
                    if t.auto_ {
                        opt_map.insert(
                            "max_optimization_threads".to_string(),
                            serde_json::json!("auto"),
                        );
                    } else {
                        opt_map.insert(
                            "max_optimization_threads".to_string(),
                            serde_json::json!(t.value),
                        );
                    }
                }
                req_map.insert(
                    "optimizers_config".to_string(),
                    serde_json::Value::Object(opt_map),
                );
            }
            if let Some(ref params) = config.params {
                let mut params_map = serde_json::Map::new();
                if let Some(rf) = params.replication_factor {
                    params_map.insert("replication_factor".to_string(), serde_json::json!(rf));
                }
                if let Some(wc) = params.write_consistency_factor {
                    params_map.insert(
                        "write_consistency_factor".to_string(),
                        serde_json::json!(wc),
                    );
                }
                if let Some(od) = params.on_disk_payload {
                    params_map.insert("on_disk_payload".to_string(), serde_json::json!(od));
                }
                if let Some(rf_out) = params.read_fan_out_factor {
                    params_map.insert("read_fan_out_factor".to_string(), serde_json::json!(rf_out));
                }
                if let Some(rf_delay) = params.read_fan_out_delay_ms {
                    params_map.insert(
                        "read_fan_out_delay_ms".to_string(),
                        serde_json::json!(rf_delay),
                    );
                }
                req_map.insert("params".to_string(), serde_json::Value::Object(params_map));
            }
            if let Some(ref quant_update) = config.quantization_update {
                if quant_update.disabled {
                    req_map.insert(
                        "quantization_config".to_string(),
                        serde_json::json!({ "disabled": true }),
                    );
                } else if let Some(ref quant) = quant_update.config {
                    let q_val = build_quantization_config(quant)?;
                    req_map.insert("quantization_config".to_string(), q_val);
                }
            }
            if let Some(ref vectors) = config.vectors {
                if let Some(on_disk) = vectors.on_disk {
                    req_map.insert(
                        "vectors_config".to_string(),
                        serde_json::json!({ "on_disk": on_disk }),
                    );
                }
            }
        }

        self.client
            .update_collection(serde_json::Value::Object(req_map))
            .await?;

        Ok(ExecResponse {
            ok: true,
            operation: "alter_collection".to_string(),
            message: format!("Collection '{}' altered", stmt.collection),
            data: Some(serde_json::json!({"collection": stmt.collection})),
        })
    }

    async fn do_drop_collection(&self, collection: &str) -> Result<ExecResponse, QqlError> {
        let exists = self.client.collection_exists(collection).await?;
        if !exists {
            return Err(QqlError::runtime(format!(
                "collection '{}' does not exist",
                collection
            )));
        }

        self.client.delete_collection(collection).await?;
        Ok(ExecResponse {
            ok: true,
            operation: "drop_collection".to_string(),
            message: format!("Collection '{}' dropped", collection),
            data: Some(serde_json::json!({"collection": collection})),
        })
    }

    async fn do_insert(&self, stmt: ast::InsertStmt<'_>) -> Result<ExecResponse, QqlError> {
        let _created = self
            .ensure_collection_for_insert(
                stmt.collection,
                stmt.model,
                stmt.hybrid,
                stmt.dense_vector,
                stmt.sparse_vector,
            )
            .await?;

        let mut points = Vec::with_capacity(stmt.values_list.len());

        for row in &stmt.values_list {
            let payload: HashMap<String, serde_json::Value> = row
                .iter()
                .map(|(k, v)| (k.to_string(), value_to_json(v)))
                .collect();

            let mut point = PointStruct {
                id: PointId::Num(0),
                vector: None,
                payload: Some(payload),
            };

            // Extract ID from payload if present
            let id_val = row.iter().find(|(k, _)| *k == "id");
            if let Some((_, Value::Int(id))) = id_val {
                point.id = PointId::Num(*id as u64);
            } else if let Some((_, Value::Str(id))) = id_val {
                point.id = PointId::Uuid(id.to_string());
            }

            // Determine text field for embedding
            let text_field = row
                .iter()
                .find(|(k, _)| *k == "text" || *k == "description" || *k == "content")
                .map(|(_, v)| match v {
                    Value::Str(s) => s.to_string(),
                    _ => String::new(),
                })
                .unwrap_or_default();

            if !text_field.is_empty() && self.uses_local_embeddings() {
                let _dense_dim = self.resolve_dense_vector_size(stmt.model).await?;
                if let Some(ref embedder) = self.embedder {
                    let dense_vec = embedder
                        .embed_dense(&text_field, &self.resolve_dense_model(stmt.model))
                        .await?;
                    point.vector = Some(serde_json::json!({
                        stmt.dense_vector.unwrap_or(DENSE_VECTOR_NAME): dense_vec
                    }));
                }
            }

            points.push(point);
        }

        let req = UpsertPointsReq {
            collection_name: stmt.collection.to_string(),
            points,
        };

        self.client.upsert(req).await?;

        Ok(ExecResponse {
            ok: true,
            operation: "insert".to_string(),
            message: format!("Inserted {} point(s)", stmt.values_list.len()),
            data: Some(serde_json::json!({"count": stmt.values_list.len()})),
        })
    }

    async fn do_select(&self, stmt: ast::SelectStmt<'_>) -> Result<ExecResponse, QqlError> {
        let req = GetPointsReq {
            collection_name: stmt.collection.to_string(),
            point_id: clone_value(&stmt.point_id),
        };
        let results = self.client.get(req).await?;

        Ok(ExecResponse {
            ok: true,
            operation: "select".to_string(),
            message: format!("Found {} point(s)", results.len()),
            data: Some(serde_json::to_value(&results).unwrap_or(serde_json::Value::Null)),
        })
    }

    async fn do_scroll(&self, stmt: ast::ScrollStmt<'_>) -> Result<ExecResponse, QqlError> {
        let qdrant_filter = if let Some(ref filter) = stmt.query_filter {
            let converter = FilterConverter::new();
            converter.build_filter(filter)?
        } else {
            None
        };

        let after = stmt
            .after
            .as_ref()
            .map(|v| to_point_id_static(v))
            .transpose()?;

        let req = ScrollPointsReq {
            collection_name: stmt.collection.to_string(),
            limit: stmt.limit as u64,
            filter: qdrant_filter,
            after,
        };

        let (points, next_offset) = self.client.scroll(req).await?;

        Ok(ExecResponse {
            ok: true,
            operation: "scroll".to_string(),
            message: format!("Found {} point(s)", points.len()),
            data: Some(serde_json::json!({
                "points": points,
                "next_offset": next_offset.map(|p| point_id_string(&p)),
            })),
        })
    }

    async fn do_delete(&self, stmt: ast::DeleteStmt<'_>) -> Result<ExecResponse, QqlError> {
        let filter = if let Some(ref f) = stmt.query_filter {
            let converter = FilterConverter::new();
            converter.build_filter(f)?
        } else {
            None
        };

        let point_id = if let Some(ref id) = stmt.point_id {
            Some(to_point_id_static(id)?)
        } else {
            None
        };

        let req = DeletePointsReq {
            collection_name: stmt.collection.to_string(),
            filter,
            point_id,
        };

        self.client.delete(req).await?;

        Ok(ExecResponse {
            ok: true,
            operation: "delete".to_string(),
            message: "Points deleted".to_string(),
            data: None,
        })
    }

    async fn do_update_vector(
        &self,
        stmt: ast::UpdateVectorStmt<'_>,
    ) -> Result<ExecResponse, QqlError> {
        let point_id = to_point_id_static(&stmt.point_id)?;

        let req = UpdateVectorsReq {
            collection_name: stmt.collection.to_string(),
            point_id,
            vector: stmt.vector.clone(),
            vector_name: stmt.vector_name.map(|s| s.to_string()),
        };

        self.client.update_vectors(req).await?;

        Ok(ExecResponse {
            ok: true,
            operation: "update_vector".to_string(),
            message: "Vector updated".to_string(),
            data: None,
        })
    }

    async fn do_update_payload(
        &self,
        stmt: ast::UpdatePayloadStmt<'_>,
    ) -> Result<ExecResponse, QqlError> {
        let filter = if let Some(ref f) = stmt.query_filter {
            let converter = FilterConverter::new();
            converter.build_filter(f)?
        } else {
            None
        };

        let point_id = if let Some(ref id) = stmt.point_id {
            Some(to_point_id_static(id)?)
        } else {
            None
        };

        let payload: HashMap<String, serde_json::Value> = stmt
            .payload
            .iter()
            .map(|(k, v)| (k.to_string(), value_to_json(v)))
            .collect();

        let req = SetPayloadReq {
            collection_name: stmt.collection.to_string(),
            point_id,
            filter,
            payload,
        };

        self.client.set_payload(req).await?;

        Ok(ExecResponse {
            ok: true,
            operation: "update_payload".to_string(),
            message: "Payload updated".to_string(),
            data: None,
        })
    }

    async fn do_create_index(
        &self,
        stmt: ast::CreateIndexStmt<'_>,
    ) -> Result<ExecResponse, QqlError> {
        let req = CreateFieldIndexReq {
            collection_name: stmt.collection.to_string(),
            field: stmt.field.to_string(),
            field_type: stmt.field_type.to_string(),
            options: stmt
                .options
                .iter()
                .map(|(k, v)| (k.to_string(), clone_value(v)))
                .collect(),
        };

        self.client.create_field_index(req).await?;

        Ok(ExecResponse {
            ok: true,
            operation: "create_index".to_string(),
            message: format!("Index created on field '{}'", stmt.field),
            data: None,
        })
    }
}

// ── Helper functions ──

fn build_with_payload(sel: Option<&ast::PayloadSelector>) -> Option<WithPayload> {
    let sel = sel?;
    if let Some(enable) = sel.enable {
        return Some(WithPayload {
            enable: Some(enable),
            include: Vec::new(),
            exclude: Vec::new(),
        });
    }
    if !sel.include.is_empty() {
        return Some(WithPayload {
            enable: None,
            include: sel.include.iter().map(|s| s.to_string()).collect(),
            exclude: Vec::new(),
        });
    }
    if !sel.exclude.is_empty() {
        return Some(WithPayload {
            enable: None,
            include: Vec::new(),
            exclude: sel.exclude.iter().map(|s| s.to_string()).collect(),
        });
    }
    None
}

fn build_with_vectors(sel: Option<&ast::VectorsSelector>) -> Option<WithVectors> {
    let sel = sel?;
    if let Some(enable) = sel.enable {
        return Some(WithVectors {
            enable: Some(enable),
            vectors: Vec::new(),
        });
    }
    if !sel.vectors.is_empty() {
        return Some(WithVectors {
            enable: None,
            vectors: sel.vectors.iter().map(|s| s.to_string()).collect(),
        });
    }
    None
}

fn has_mmr(with_clause: Option<&ast::SearchWith>) -> bool {
    match with_clause {
        Some(wc) => wc.mmr_diversity.is_some() || wc.mmr_candidates.is_some(),
        None => false,
    }
}

fn point_id_string(id: &PointId) -> String {
    match id {
        PointId::Num(n) => n.to_string(),
        PointId::Uuid(s) => s.clone(),
    }
}

fn to_point_id_static(val: &ast::Value) -> Result<PointId, QqlError> {
    match val {
        Value::Str(s) => {
            if let Ok(num) = s.parse::<u64>() {
                Ok(PointId::Num(num))
            } else {
                Ok(PointId::Uuid(s.to_string()))
            }
        }
        Value::Int(i) => {
            if *i < 0 {
                return Err(QqlError::runtime("negative ID not supported"));
            }
            Ok(PointId::Num(*i as u64))
        }
        Value::Float(f) => {
            let v = *f;
            if v < 0.0 || v > (1u64 << 53) as f64 || v != (v as u64) as f64 {
                return Err(QqlError::runtime(
                    "unsupported point ID: non-integer or oversized float",
                ));
            }
            Ok(PointId::Num(v as u64))
        }
        _ => Err(QqlError::runtime(format!(
            "unsupported point ID type: {:?}",
            val
        ))),
    }
}

fn clone_value(val: &Value<'_>) -> Value<'static> {
    match val {
        Value::Str(s) => {
            let leaked: &'static str = Box::leak(s.to_string().into_boxed_str());
            Value::Str(leaked)
        }
        Value::Int(i) => Value::Int(*i),
        Value::Float(f) => Value::Float(*f),
        Value::Bool(b) => Value::Bool(*b),
        Value::Null => Value::Null,
        Value::Dict(items) => Value::Dict(
            items
                .iter()
                .map(|(k, v)| {
                    let s: &'static str = Box::leak(k.to_string().into_boxed_str());
                    (s, clone_value(v))
                })
                .collect(),
        ),
        Value::List(items) => Value::List(items.iter().map(clone_value).collect()),
    }
}

fn value_to_json(val: &Value) -> serde_json::Value {
    match val {
        Value::Str(s) => serde_json::Value::String(s.to_string()),
        Value::Int(i) => serde_json::Value::Number((*i).into()),
        Value::Float(f) => serde_json::Value::Number(
            serde_json::Number::from_f64(*f).unwrap_or(serde_json::Number::from_f64(0.0).unwrap()),
        ),
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Null => serde_json::Value::Null,
        Value::Dict(items) => {
            let mut map = serde_json::Map::new();
            for (k, v) in items {
                map.insert(k.to_string(), value_to_json(v));
            }
            serde_json::Value::Object(map)
        }
        Value::List(items) => serde_json::Value::Array(items.iter().map(value_to_json).collect()),
    }
}

fn build_quantization_config(
    quant: &ast::QuantizationConfig,
) -> Result<serde_json::Value, QqlError> {
    let mut config_map = serde_json::Map::new();
    config_map.insert(
        "always_ram".to_string(),
        serde_json::json!(quant.always_ram),
    );

    let key = match quant.qtype {
        ast::QuantizationType::Scalar => {
            config_map.insert("type".to_string(), serde_json::json!("int8"));
            if let Some(quantile) = quant.quantile {
                config_map.insert("quantile".to_string(), serde_json::json!(quantile));
            }
            "scalar"
        }
        ast::QuantizationType::Binary => "binary",
        ast::QuantizationType::Product => {
            config_map.insert("compression".to_string(), serde_json::json!("x4"));
            "product"
        }
        ast::QuantizationType::Turbo => {
            if let Some(bits) = quant.turbo_bits {
                let bit_str = if (bits - 1.0).abs() < f64::EPSILON {
                    "bits1"
                } else if (bits - 1.5).abs() < f64::EPSILON {
                    "bits1_5"
                } else if (bits - 2.0).abs() < f64::EPSILON {
                    "bits2"
                } else if (bits - 4.0).abs() < f64::EPSILON {
                    "bits4"
                } else {
                    return Err(QqlError::runtime(format!(
                        "unsupported TURBO bit depth {}; expected one of 1, 1.5, 2, or 4",
                        bits
                    )));
                };
                config_map.insert("bits".to_string(), serde_json::json!(bit_str));
            }
            "turbo"
        }
    };

    let mut wrapper = serde_json::Map::new();
    wrapper.insert(key.to_string(), serde_json::Value::Object(config_map));
    Ok(serde_json::Value::Object(wrapper))
}
