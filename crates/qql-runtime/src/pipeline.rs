use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use qql_core::ast;
use qql_core::error::QqlError;

use crate::embedder::Embedder;
use crate::filter_conv::{FilterConverter, QdrantFilter};

#[async_trait]
pub trait ExecutionNode: Send {
    async fn execute(&self, state: &mut QueryState) -> Result<(), QqlError>;
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchParams {
    pub hnsw_ef: Option<u64>,
    pub exact: Option<bool>,
    pub acorn: Option<bool>,
    pub indexed_only: Option<bool>,
    pub quantization: Option<QuantizationSearchParams>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct QuantizationSearchParams {
    pub ignore: Option<bool>,
    pub rescore: Option<bool>,
    pub oversampling: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FusionType {
    Rrf,
    Dbsf,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RrfConfig {
    pub k: Option<u32>,
    pub weights: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RecommendStrategyType {
    AverageVector,
    BestScore,
    SumScores,
}

impl RecommendStrategyType {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "average_vector" => Some(RecommendStrategyType::AverageVector),
            "best_score" => Some(RecommendStrategyType::BestScore),
            "sum_scores" => Some(RecommendStrategyType::SumScores),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum QueryVariant {
    Nearest(Vec<f32>),
    Sparse(Vec<u32>, Vec<f32>),
    Document {
        text: String,
        model: String,
        options: HashMap<String, String>,
    },
    Recommend(RecommendInput),
    Context(ContextInput),
    Discover(DiscoverInput),
    OrderBy(OrderByInput),
    Sample,
    Fusion(FusionType),
    Rrf(RrfConfig),
    RelevanceFeedback(RelevanceFeedbackInput),
    MMR {
        input: Box<QueryVariant>,
        diversity: f32,
        candidates: u32,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecommendInput {
    pub positive: Vec<VectorInput>,
    pub negative: Vec<VectorInput>,
    pub strategy: Option<RecommendStrategyType>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ContextInput {
    pub pairs: Vec<ContextPair>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ContextPair {
    pub positive: Option<VectorInput>,
    pub negative: Option<VectorInput>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DiscoverInput {
    pub target: VectorInput,
    pub context: ContextInput,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OrderByInput {
    pub key: String,
    pub direction: OrderByDirection,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OrderByDirection {
    Asc,
    Desc,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RelevanceFeedbackInput {
    pub target: VectorInput,
    pub feedback: Vec<FeedbackItem>,
    pub strategy: Option<NaiveFeedbackStrategy>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FeedbackItem {
    pub example: VectorInput,
    pub score: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NaiveFeedbackStrategy {
    pub a: f32,
    pub b: f32,
    pub c: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum VectorInput {
    Id(PointId),
    Dense(Vec<f32>),
    Document {
        text: String,
        model: String,
        options: HashMap<String, String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PointId {
    Num(u64),
    Uuid(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PrefetchQuery {
    pub prefetch: Vec<PrefetchQuery>,
    pub query: Option<QueryVariant>,
    pub using: Option<String>,
    pub filter: Option<QdrantFilter>,
    pub params: Option<SearchParams>,
    pub limit: Option<u64>,
    pub score_threshold: Option<f32>,
    pub lookup_from: Option<LookupLocation>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LookupLocation {
    pub collection_name: String,
    pub vector_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WithPayload {
    pub enable: Option<bool>,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WithVectors {
    pub enable: Option<bool>,
    pub vectors: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WithLookup {
    pub collection: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct QueryPointsRequest {
    pub collection_name: String,
    pub query: Option<QueryVariant>,
    pub prefetch: Vec<PrefetchQuery>,
    pub limit: u64,
    pub params: Option<SearchParams>,
    pub filter: Option<QdrantFilter>,
    pub with_payload: Option<WithPayload>,
    pub with_vectors: Option<WithVectors>,
    pub score_threshold: Option<f32>,
    pub offset: u64,
    pub lookup_from: Option<LookupLocation>,
    pub using: Option<String>,
    pub timeout: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct QueryPointsGroupsRequest {
    pub collection_name: String,
    pub query: Option<QueryVariant>,
    pub prefetch: Vec<PrefetchQuery>,
    pub limit: u64,
    pub group_by: String,
    pub group_size: u64,
    pub params: Option<SearchParams>,
    pub filter: Option<QdrantFilter>,
    pub with_payload: Option<WithPayload>,
    pub with_vectors: Option<WithVectors>,
    pub score_threshold: Option<f32>,
    pub lookup_from: Option<LookupLocation>,
    pub with_lookup: Option<WithLookup>,
    pub using: Option<String>,
}

#[derive(Default)]
pub struct QueryState {
    pub query_text: String,
    pub prefetches: Vec<PrefetchQuery>,
    pub manual_prefetches: Vec<PrefetchQuery>,
    pub target_query: Option<QueryVariant>,
    pub params: Option<SearchParams>,
    pub fusion_config: Option<RrfConfig>,

    pub has_mmr: bool,
    pub mmr_candidates: u32,
    pub mmr_diversity: f32,
    pub local_embed: bool,
    pub embedder: Option<Arc<dyn Embedder>>,
    pub cloud_model_options: HashMap<String, String>,
    pub dense_model: String,

    pub doc_options: Option<HashMap<String, String>>,
    pub request_timeout: Option<u64>,

    pub collection_name: String,
    pub vector_name: String,
    pub limit: u64,
    pub offset: u64,
    pub qdrant_filter: Option<QdrantFilter>,
    pub score_threshold: Option<f32>,
    pub lookup_from: Option<LookupLocation>,
    pub with_payload: Option<WithPayload>,
    pub with_vectors: Option<WithVectors>,

    pub group_by: String,
    pub group_size: u64,
    pub with_lookup: Option<WithLookup>,
}

impl QueryState {
    pub fn get_doc_options(&mut self) -> HashMap<String, String> {
        if self.doc_options.is_none() && !self.cloud_model_options.is_empty() {
            self.doc_options = Some(self.cloud_model_options.clone());
        }
        self.doc_options.clone().unwrap_or_default()
    }
}

pub struct QueryPipeline {
    nodes: Vec<Box<dyn ExecutionNode>>,
}

impl QueryPipeline {
    pub fn new() -> Self {
        QueryPipeline { nodes: Vec::new() }
    }

    pub fn add(&mut self, node: Box<dyn ExecutionNode>) {
        self.nodes.push(node);
    }

    pub async fn execute(&self, state: &mut QueryState) -> Result<(), QqlError> {
        for node in &self.nodes {
            node.execute(state).await?;
        }
        Ok(())
    }

    pub fn build_flat_request(&self, state: &QueryState) -> QueryPointsRequest {
        let mut prefetches = state.prefetches.clone();
        prefetches.extend(state.manual_prefetches.clone());

        let mut req = QueryPointsRequest {
            collection_name: state.collection_name.clone(),
            query: state.target_query.clone(),
            prefetch: prefetches,
            limit: state.limit,
            params: state.params.clone(),
            filter: state.qdrant_filter.clone(),
            with_payload: state.with_payload.clone(),
            with_vectors: state.with_vectors.clone(),
            score_threshold: state.score_threshold,
            offset: state.offset,
            lookup_from: state.lookup_from.clone(),
            using: None,
            timeout: state.request_timeout,
        };

        if !state.vector_name.is_empty() {
            req.using = Some(state.vector_name.clone());
        }

        req
    }

    pub fn build_grouped_request(&self, state: &QueryState) -> QueryPointsGroupsRequest {
        let flat = self.build_flat_request(state);
        QueryPointsGroupsRequest {
            collection_name: flat.collection_name,
            query: flat.query,
            prefetch: flat.prefetch,
            limit: flat.limit,
            group_by: state.group_by.clone(),
            group_size: state.group_size,
            params: flat.params,
            filter: flat.filter,
            with_payload: flat.with_payload,
            with_vectors: flat.with_vectors,
            score_threshold: flat.score_threshold,
            lookup_from: flat.lookup_from,
            with_lookup: state.with_lookup.clone(),
            using: flat.using,
        }
    }
}

pub struct DenseEmbedNode {
    pub model: String,
    pub vector_name: String,
    pub limit: u64,
    pub as_prefetch: bool,
}

#[async_trait]
impl ExecutionNode for DenseEmbedNode {
    async fn execute(&self, state: &mut QueryState) -> Result<(), QqlError> {
        let query: QueryVariant;
        let mmr_nearest: Option<VectorInput>;

        if state.local_embed {
            let embedder = state.embedder.as_ref().ok_or_else(|| {
                QqlError::runtime("local embedding requested but no Embedder provided")
            })?;
            let dense_vector = embedder
                .embed_dense(&state.query_text, &self.model)
                .await
                .map_err(|e| {
                    QqlError::runtime(format!("failed to embed dense search query: {}", e))
                })?;
            query = QueryVariant::Nearest(dense_vector.clone());
            mmr_nearest = if state.has_mmr {
                Some(VectorInput::Dense(dense_vector))
            } else {
                None
            };
        } else {
            let doc = QueryVariant::Document {
                text: state.query_text.clone(),
                model: self.model.clone(),
                options: state.get_doc_options(),
            };
            query = doc.clone();
            mmr_nearest = if state.has_mmr {
                Some(match &doc {
                    QueryVariant::Document {
                        text,
                        model,
                        options,
                    } => VectorInput::Document {
                        text: text.clone(),
                        model: model.clone(),
                        options: options.clone(),
                    },
                    _ => unreachable!(),
                })
            } else {
                None
            };
        }

        let final_query = if state.has_mmr {
            if let Some(input) = mmr_nearest {
                QueryVariant::MMR {
                    input: Box::new(QueryVariant::Nearest(match &input {
                        VectorInput::Dense(v) => v.clone(),
                        _ => return Err(QqlError::runtime("MMR requires dense vector input")),
                    })),
                    diversity: state.mmr_diversity,
                    candidates: state.mmr_candidates,
                }
            } else {
                query
            }
        } else {
            query
        };

        if self.as_prefetch {
            state.prefetches.push(PrefetchQuery {
                prefetch: Vec::new(),
                query: Some(final_query),
                using: Some(self.vector_name.clone()),
                limit: Some(self.limit),
                params: state.params.clone(),
                filter: None,
                score_threshold: None,
                lookup_from: None,
            });
        } else {
            state.target_query = Some(final_query);
        }

        Ok(())
    }
}

pub struct RawVectorNode {
    pub vector: Vec<f64>,
    pub vector_name: String,
    pub as_prefetch: bool,
    pub limit: u64,
}

#[async_trait]
impl ExecutionNode for RawVectorNode {
    async fn execute(&self, _state: &mut QueryState) -> Result<(), QqlError> {
        let raw: Vec<f32> = self.vector.iter().map(|v| *v as f32).collect();
        let query = QueryVariant::Nearest(raw);

        if self.as_prefetch {
            _state.prefetches.push(PrefetchQuery {
                prefetch: Vec::new(),
                query: Some(query),
                using: Some(self.vector_name.clone()),
                limit: Some(self.limit),
                params: _state.params.clone(),
                filter: None,
                score_threshold: None,
                lookup_from: None,
            });
        } else {
            _state.target_query = Some(query);
            if !self.vector_name.is_empty() {
                _state.vector_name = self.vector_name.clone();
            }
        }

        Ok(())
    }
}

pub struct SparseEmbedNode {
    pub model: String,
    pub vector_name: String,
    pub limit: u64,
    pub as_prefetch: bool,
}

#[async_trait]
impl ExecutionNode for SparseEmbedNode {
    async fn execute(&self, state: &mut QueryState) -> Result<(), QqlError> {
        if state.has_mmr && !self.as_prefetch {
            return Err(QqlError::runtime(
                "MMR is supported only for standard NEAREST queries, not sparse-only queries",
            ));
        }

        let query: QueryVariant;

        if state.local_embed {
            let embedder = state.embedder.as_ref().ok_or_else(|| {
                QqlError::runtime("local embedding requested but no Embedder provided")
            })?;
            let sv = embedder
                .embed_sparse(&state.query_text)
                .await
                .map_err(|e| {
                    QqlError::runtime(format!("failed to embed sparse search query: {}", e))
                })?;
            query = QueryVariant::Sparse(sv.indices, sv.values);
        } else {
            query = QueryVariant::Document {
                text: state.query_text.clone(),
                model: self.model.clone(),
                options: state.get_doc_options(),
            };
        }

        if self.as_prefetch {
            state.prefetches.push(PrefetchQuery {
                prefetch: Vec::new(),
                query: Some(query),
                using: Some(self.vector_name.clone()),
                limit: Some(self.limit),
                params: state.params.clone(),
                filter: None,
                score_threshold: None,
                lookup_from: None,
            });
        } else {
            state.target_query = Some(query);
        }

        Ok(())
    }
}

pub struct FusionNode {
    pub mode: String,
}

#[async_trait]
impl ExecutionNode for FusionNode {
    async fn execute(&self, state: &mut QueryState) -> Result<(), QqlError> {
        if self.mode == "rrf" && state.fusion_config.is_some() {
            let config = state
                .fusion_config
                .as_ref()
                .unwrap_or_else(|| unreachable!());
            state.target_query = Some(QueryVariant::Rrf(config.clone()));
            return Ok(());
        }

        match self.mode.as_str() {
            "rrf" => {
                state.target_query = Some(QueryVariant::Fusion(FusionType::Rrf));
            }
            "dbsf" => {
                state.target_query = Some(QueryVariant::Fusion(FusionType::Dbsf));
            }
            _ => {
                return Err(QqlError::runtime(format!(
                    "unknown fusion mode '{}'; expected 'rrf' or 'dbsf'",
                    self.mode
                )));
            }
        }

        Ok(())
    }
}

pub struct RerankNode {
    pub model: String,
}

#[async_trait]
impl ExecutionNode for RerankNode {
    async fn execute(&self, state: &mut QueryState) -> Result<(), QqlError> {
        if state.local_embed {
            return Err(QqlError::runtime(
                "RERANK is currently only available in cloud inference mode",
            ));
        }

        state.target_query = Some(QueryVariant::Document {
            text: state.query_text.clone(),
            model: self.model.clone(),
            options: state.get_doc_options(),
        });

        Ok(())
    }
}

pub struct RecommendNode {
    pub positive_ids: Vec<ast::Value<'static>>,
    pub negative_ids: Vec<ast::Value<'static>>,
    pub strategy: Option<String>,
}

#[async_trait]
impl ExecutionNode for RecommendNode {
    async fn execute(&self, state: &mut QueryState) -> Result<(), QqlError> {
        if state.has_mmr {
            return Err(QqlError::runtime(
                "MMR is supported only for standard NEAREST queries",
            ));
        }
        if self.positive_ids.is_empty() && self.negative_ids.is_empty() {
            return Err(QqlError::runtime(
                "RECOMMEND requires at least one POSITIVE or NEGATIVE ID",
            ));
        }

        let mut pos = Vec::new();
        for id in &self.positive_ids {
            let vi = build_vector_input(state, id).await?;
            pos.push(vi);
        }

        let mut neg = Vec::new();
        for id in &self.negative_ids {
            let vi = build_vector_input(state, id).await?;
            neg.push(vi);
        }

        let strategy = self
            .strategy
            .as_ref()
            .and_then(|s| RecommendStrategyType::parse(s));

        let rec = RecommendInput {
            positive: pos,
            negative: neg,
            strategy,
        };

        state.target_query = Some(QueryVariant::Recommend(rec));
        Ok(())
    }
}

pub struct ContextPairInput {
    pub positive: Option<ast::Value<'static>>,
    pub negative: Option<ast::Value<'static>>,
}

pub struct ContextNode {
    pub pairs: Vec<ContextPairInput>,
}

#[async_trait]
impl ExecutionNode for ContextNode {
    async fn execute(&self, state: &mut QueryState) -> Result<(), QqlError> {
        let pairs = build_context_pairs(state, &self.pairs).await?;
        state.target_query = Some(QueryVariant::Context(ContextInput { pairs }));
        Ok(())
    }
}

pub struct DiscoverNode {
    pub target: Option<ast::Value<'static>>,
    pub pairs: Vec<ContextPairInput>,
}

#[async_trait]
impl ExecutionNode for DiscoverNode {
    async fn execute(&self, state: &mut QueryState) -> Result<(), QqlError> {
        let target = match &self.target {
            Some(v) => build_vector_input(state, v).await?,
            None => return Err(QqlError::runtime("DISCOVER requires a target")),
        };
        let pairs = build_context_pairs(state, &self.pairs).await?;
        state.target_query = Some(QueryVariant::Discover(DiscoverInput {
            target,
            context: ContextInput { pairs },
        }));
        Ok(())
    }
}

async fn build_context_pairs(
    state: &QueryState,
    pairs: &[ContextPairInput],
) -> Result<Vec<ContextPair>, QqlError> {
    let mut result = Vec::with_capacity(pairs.len());
    for p in pairs {
        let positive = match &p.positive {
            Some(v) => Some(build_vector_input(state, v).await?),
            None => None,
        };
        let negative = match &p.negative {
            Some(v) => Some(build_vector_input(state, v).await?),
            None => None,
        };
        result.push(ContextPair { positive, negative });
    }
    Ok(result)
}

pub fn is_uuid(s: &str) -> bool {
    if s.len() != 36 {
        return false;
    }
    let bytes = s.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if i == 8 || i == 13 || i == 18 || i == 23 {
            if b != b'-' {
                return false;
            }
        } else {
            if !(b.is_ascii_digit() || (b'a'..=b'f').contains(&b) || (b'A'..=b'F').contains(&b)) {
                return false;
            }
        }
    }
    true
}

pub fn to_point_id(val: &ast::Value) -> Result<PointId, QqlError> {
    match val {
        ast::Value::Str(s) => {
            if let Ok(num) = s.parse::<u64>() {
                Ok(PointId::Num(num))
            } else {
                Ok(PointId::Uuid(s.to_string()))
            }
        }
        ast::Value::Int(i) => {
            if *i < 0 {
                return Err(QqlError::runtime(
                    "unsupported vector input type: negative integer",
                ));
            }
            Ok(PointId::Num(*i as u64))
        }
        ast::Value::Float(f) => {
            let v = *f;
            if v < 0.0 || v > (1u64 << 53) as f64 || v != (v as u64) as f64 {
                return Err(QqlError::runtime(
                    "unsupported vector input type: non-integer or oversized float",
                ));
            }
            Ok(PointId::Num(v as u64))
        }
        _ => Err(QqlError::runtime(format!(
            "unsupported vector input type: {:?}",
            val
        ))),
    }
}

#[allow(dead_code)]
fn point_id_to_value(pid: &PointId) -> ast::Value<'static> {
    match pid {
        PointId::Num(n) => ast::Value::Int(*n as i64),
        PointId::Uuid(s) => ast::Value::Str(Box::leak(s.clone().into_boxed_str())),
    }
}

async fn build_vector_input(
    state: &QueryState,
    val: &ast::Value<'_>,
) -> Result<VectorInput, QqlError> {
    match val {
        ast::Value::Str(s) => {
            if !is_uuid(s) && s.parse::<u64>().is_err() {
                if state.local_embed {
                    let embedder = state.embedder.as_ref().ok_or_else(|| {
                        QqlError::runtime("local embedding requested but no Embedder provided")
                    })?;
                    let dense_vector =
                        embedder
                            .embed_dense(s, &state.dense_model)
                            .await
                            .map_err(|e| {
                                QqlError::runtime(format!("failed to embed target query: {}", e))
                            })?;
                    return Ok(VectorInput::Dense(dense_vector));
                }
                return Ok(VectorInput::Document {
                    text: s.to_string(),
                    model: state.dense_model.clone(),
                    options: state.cloud_model_options.clone(),
                });
            }
            let pid = to_point_id(val)?;
            Ok(VectorInput::Id(pid))
        }
        ast::Value::Int(_) | ast::Value::Float(_) => {
            let pid = to_point_id(val)?;
            Ok(VectorInput::Id(pid))
        }
        _ => Err(QqlError::runtime(format!(
            "unsupported vector input type: {:?}",
            val
        ))),
    }
}

pub fn build_search_params(with_clause: &ast::SearchWith) -> Option<SearchParams> {
    let mut params = SearchParams {
        hnsw_ef: None,
        exact: None,
        acorn: None,
        indexed_only: None,
        quantization: None,
    };

    let mut has_any = false;

    if with_clause.hnsw_ef > 0 {
        params.hnsw_ef = Some(with_clause.hnsw_ef);
        has_any = true;
    }
    if with_clause.exact {
        params.exact = Some(true);
        has_any = true;
    }
    if with_clause.acorn {
        params.acorn = Some(true);
        has_any = true;
    }
    if with_clause.indexed_only {
        params.indexed_only = Some(true);
        has_any = true;
    }
    if let Some(ref q) = with_clause.quantization {
        params.quantization = Some(QuantizationSearchParams {
            ignore: q.ignore,
            rescore: q.rescore,
            oversampling: q.oversampling,
        });
        has_any = true;
    }

    if has_any {
        Some(params)
    } else {
        None
    }
}

pub struct OrderByNode {
    pub field: String,
    pub asc: bool,
}

#[async_trait]
impl ExecutionNode for OrderByNode {
    async fn execute(&self, state: &mut QueryState) -> Result<(), QqlError> {
        let direction = if self.asc {
            OrderByDirection::Asc
        } else {
            OrderByDirection::Desc
        };
        state.target_query = Some(QueryVariant::OrderBy(OrderByInput {
            key: self.field.clone(),
            direction,
        }));
        Ok(())
    }
}

pub struct SampleNode;

#[async_trait]
impl ExecutionNode for SampleNode {
    async fn execute(&self, state: &mut QueryState) -> Result<(), QqlError> {
        state.target_query = Some(QueryVariant::Sample);
        Ok(())
    }
}

pub struct RelevanceFeedbackNode {
    pub target: ast::Value<'static>,
    pub feedback: Vec<(ast::Value<'static>, f64)>,
    pub strategy: Option<(f64, f64, f64)>,
}

#[async_trait]
impl ExecutionNode for RelevanceFeedbackNode {
    async fn execute(&self, state: &mut QueryState) -> Result<(), QqlError> {
        fn build_vector_input_from_value(val: &ast::Value) -> Result<VectorInput, QqlError> {
            match val {
                ast::Value::List(items) => {
                    let vec: Vec<f32> = items
                        .iter()
                        .map(|v| match v {
                            ast::Value::Float(f) => Ok(*f as f32),
                            ast::Value::Int(i) => Ok(*i as f32),
                            _ => Err(QqlError::runtime("vector element is not a number")),
                        })
                        .collect::<Result<Vec<f32>, QqlError>>()?;
                    Ok(VectorInput::Dense(vec))
                }
                _ => {
                    let pid = to_point_id(val)?;
                    Ok(VectorInput::Id(pid))
                }
            }
        }

        let target_input = build_vector_input_from_value(&self.target)
            .map_err(|e| QqlError::runtime(format!("relevance feedback target: {}", e)))?;

        let mut feedback_items = Vec::with_capacity(self.feedback.len());
        for (i, (example, score)) in self.feedback.iter().enumerate() {
            let example_input = build_vector_input_from_value(example).map_err(|e| {
                QqlError::runtime(format!("relevance feedback example {}: {}", i, e))
            })?;
            feedback_items.push(FeedbackItem {
                example: example_input,
                score: *score as f32,
            });
        }

        let strategy = self.strategy.map(|(a, b, c)| NaiveFeedbackStrategy {
            a: a as f32,
            b: b as f32,
            c: c as f32,
        });

        state.target_query = Some(QueryVariant::RelevanceFeedback(RelevanceFeedbackInput {
            target: target_input,
            feedback: feedback_items,
            strategy,
        }));

        Ok(())
    }
}

pub struct FormulaNode {
    pub expr: ast::FormulaExpr<'static>,
    pub defaults: Vec<(String, f64)>,
}

pub(crate) fn build_expression(expr: &ast::FormulaExpr) -> Result<serde_json::Value, QqlError> {
    match expr {
        ast::FormulaExpr::Constant { value } => Ok(serde_json::json!({"constant": value})),
        ast::FormulaExpr::Variable { name } => Ok(serde_json::json!({"variable": name})),
        ast::FormulaExpr::Datetime { value } => Ok(serde_json::json!({"datetime": value})),
        ast::FormulaExpr::DatetimeKey { key } => Ok(serde_json::json!({"datetime_key": key})),
        ast::FormulaExpr::Sum { left, right } => {
            let l = build_expression(left)?;
            let r = build_expression(right)?;
            Ok(serde_json::json!({"sum": [l, r]}))
        }
        ast::FormulaExpr::Sub { left, right } => {
            let l = build_expression(left)?;
            let r = build_expression(right)?;
            let neg_r = serde_json::json!({"neg": r});
            Ok(serde_json::json!({"sum": [l, neg_r]}))
        }
        ast::FormulaExpr::Mul { left, right } => {
            let l = build_expression(left)?;
            let r = build_expression(right)?;
            Ok(serde_json::json!({"mult": [l, r]}))
        }
        ast::FormulaExpr::Div {
            left,
            right,
            by_zero_default,
        } => {
            let l = build_expression(left)?;
            let r = build_expression(right)?;
            let mut div = serde_json::json!({"left": l, "right": r});
            if let Some(default) = by_zero_default {
                div.as_object_mut()
                    .unwrap()
                    .insert("by_zero_default".to_string(), serde_json::json!(default));
            }
            Ok(serde_json::json!({"div": div}))
        }
        ast::FormulaExpr::Neg { operand } => {
            let op = build_expression(operand)?;
            Ok(serde_json::json!({"neg": op}))
        }
        ast::FormulaExpr::Abs { x } => {
            let inner = build_expression(x)?;
            Ok(serde_json::json!({"abs": inner}))
        }
        ast::FormulaExpr::Sqrt { x } => {
            let inner = build_expression(x)?;
            Ok(serde_json::json!({"sqrt": inner}))
        }
        ast::FormulaExpr::Log { x } => {
            let inner = build_expression(x)?;
            Ok(serde_json::json!({"log10": inner}))
        }
        ast::FormulaExpr::Ln { x } => {
            let inner = build_expression(x)?;
            Ok(serde_json::json!({"ln": inner}))
        }
        ast::FormulaExpr::Exp { x } => {
            let inner = build_expression(x)?;
            Ok(serde_json::json!({"exp": inner}))
        }
        ast::FormulaExpr::Pow { base, exponent } => {
            let b = build_expression(base)?;
            let e = build_expression(exponent)?;
            Ok(serde_json::json!({"pow": {"base": b, "exponent": e}}))
        }
        ast::FormulaExpr::GeoDistance { lat, lon, field } => Ok(
            serde_json::json!({"geo_distance": {"origin": {"lat": lat, "lon": lon}, "to": field}}),
        ),
        ast::FormulaExpr::Decay {
            kind,
            x,
            target,
            scale,
            midpoint,
        } => {
            let inner_x = build_expression(x)?;
            let mut decay = serde_json::json!({"x": inner_x});
            if let Some(t) = target {
                let target_expr = build_expression(t)?;
                decay
                    .as_object_mut()
                    .unwrap()
                    .insert("target".to_string(), target_expr);
            }
            if let Some(s) = scale {
                decay
                    .as_object_mut()
                    .unwrap()
                    .insert("scale".to_string(), serde_json::json!(s));
            }
            if let Some(m) = midpoint {
                decay
                    .as_object_mut()
                    .unwrap()
                    .insert("midpoint".to_string(), serde_json::json!(m));
            }
            let decay_key = match *kind {
                "exp_decay" => "exp_decay",
                "gauss_decay" => "gauss_decay",
                "lin_decay" => "lin_decay",
                _ => return Err(QqlError::runtime(format!("unknown decay kind: {}", kind))),
            };
            Ok(serde_json::json!({decay_key: decay}))
        }
        ast::FormulaExpr::Case { cond, then_, else_ } => {
            let filter_converter = FilterConverter::new();
            let qdrant_filter = filter_converter
                .build_filter(cond)?
                .ok_or_else(|| QqlError::runtime("empty condition in CASE expression"))?;
            let cond_json = serde_json::to_value(&qdrant_filter)
                .map_err(|e| QqlError::runtime(format!("failed to serialize filter: {}", e)))?;
            let cond_expr = serde_json::json!({"condition": cond_json});
            let not_cond_filter = serde_json::json!({
                "must_not": [{"filter": cond_json}]
            });
            let not_cond_expr = serde_json::json!({"condition": not_cond_filter});
            let then_expr = build_expression(then_)?;
            let else_expr = build_expression(else_)?;
            let then_part = serde_json::json!({"mult": [cond_expr, then_expr]});
            let else_part = serde_json::json!({"mult": [not_cond_expr, else_expr]});
            Ok(serde_json::json!({"sum": [then_part, else_part]}))
        }
        ast::FormulaExpr::MatchCondition { field, values } => {
            build_match_condition_expression(field, values)
        }
    }
}

pub(crate) fn build_match_condition_expression(
    field: &str,
    values: &[ast::Value],
) -> Result<serde_json::Value, QqlError> {
    if values.is_empty() {
        return Err(QqlError::runtime("MATCH requires at least one value"));
    }

    if values.len() == 1 {
        let condition = match &values[0] {
            ast::Value::Str(s) => {
                serde_json::json!({"match": {"key": field, "value": {"str": s}}})
            }
            ast::Value::Int(i) => {
                serde_json::json!({"match": {"key": field, "value": {"int": *i}}})
            }
            ast::Value::Float(f) => {
                serde_json::json!({"range": {"key": field, "gte": f, "lte": f}})
            }
            _ => {
                return Err(QqlError::runtime("MATCH value must be a string or number"));
            }
        };
        Ok(serde_json::json!({"condition": condition}))
    } else {
        let first = &values[0];
        match first {
            ast::Value::Str(_) => {
                let keywords: Vec<&str> = values
                    .iter()
                    .map(|v| match v {
                        ast::Value::Str(s) => *s,
                        _ => panic!("all values must be strings"),
                    })
                    .collect();
                let condition = serde_json::json!({
                    "match": {"key": field, "values": keywords.iter().map(|s| serde_json::json!({"str": s})).collect::<Vec<_>>()}
                });
                Ok(serde_json::json!({"condition": condition}))
            }
            ast::Value::Int(_) | ast::Value::Float(_) => {
                let ints: Vec<i64> = values
                    .iter()
                    .map(|v| match v {
                        ast::Value::Int(i) => *i,
                        ast::Value::Float(f) => *f as i64,
                        _ => panic!("all values must be numbers"),
                    })
                    .collect();
                let condition = serde_json::json!({
                    "match": {"key": field, "values": ints.iter().map(|i| serde_json::json!({"int": *i})).collect::<Vec<_>>()}
                });
                Ok(serde_json::json!({"condition": condition}))
            }
            _ => Err(QqlError::runtime("MATCH values must be strings or numbers")),
        }
    }
}

#[async_trait]
impl ExecutionNode for FormulaNode {
    async fn execute(&self, state: &mut QueryState) -> Result<(), QqlError> {
        let _expr = build_expression(&self.expr)?;

        let mut defs: HashMap<String, f64> = HashMap::new();
        for (k, v) in &self.defaults {
            defs.insert(k.clone(), *v);
        }

        if let Some(target) = &state.target_query {
            let pq = PrefetchQuery {
                prefetch: Vec::new(),
                query: Some(target.clone()),
                using: if state.vector_name.is_empty() {
                    None
                } else {
                    Some(state.vector_name.clone())
                },
                limit: None,
                params: None,
                filter: None,
                score_threshold: None,
                lookup_from: None,
            };
            state.prefetches.push(pq);
        }

        Ok(())
    }
}

impl Default for QueryPipeline {
    fn default() -> Self {
        Self::new()
    }
}
