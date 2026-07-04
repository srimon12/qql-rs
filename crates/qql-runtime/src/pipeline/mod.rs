pub mod embed_nodes;
pub mod formula_nodes;
pub mod helpers;
pub mod query_nodes;

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::embedder::Embedder;
use crate::filter_conv::QdrantFilter;
use qql_core::error::QqlError;

pub use embed_nodes::{DenseEmbedNode, RawVectorNode, SparseEmbedNode};
pub use formula_nodes::{build_expression, build_match_condition_expression};
pub use helpers::{
    build_search_params, build_vector_input, is_uuid, point_id_to_value, to_point_id,
};
pub use query_nodes::{
    ContextNode, ContextPairInput, DiscoverNode, FusionNode, OrderByNode, RecommendNode,
    RelevanceFeedbackNode, RerankNode, SampleNode,
};

#[async_trait]
pub trait ExecutionNode: Send + Sync {
    async fn execute(&self, state: &mut QueryState) -> Result<(), QqlError>;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SearchParams {
    pub hnsw_ef: Option<u64>,
    pub exact: Option<bool>,
    pub acorn: Option<bool>,
    pub indexed_only: Option<bool>,
    pub quantization: Option<QuantizationSearchParams>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct QuantizationSearchParams {
    pub ignore: Option<bool>,
    pub rescore: Option<bool>,
    pub oversampling: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FusionType {
    Rrf,
    Dbsf,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RrfConfig {
    pub k: Option<u32>,
    pub weights: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RecommendInput {
    pub positive: Vec<VectorInput>,
    pub negative: Vec<VectorInput>,
    pub strategy: Option<RecommendStrategyType>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ContextInput {
    pub pairs: Vec<ContextPair>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ContextPair {
    pub positive: Option<VectorInput>,
    pub negative: Option<VectorInput>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct DiscoverInput {
    pub target: VectorInput,
    pub context: ContextInput,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct OrderByInput {
    pub key: String,
    pub direction: OrderByDirection,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderByDirection {
    Asc,
    Desc,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RelevanceFeedbackInput {
    pub target: VectorInput,
    pub feedback: Vec<FeedbackItem>,
    pub strategy: Option<NaiveFeedbackStrategy>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct FeedbackItem {
    pub example: VectorInput,
    pub score: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct NaiveFeedbackStrategy {
    pub a: f32,
    pub b: f32,
    pub c: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct LookupLocation {
    pub collection_name: String,
    pub vector_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct WithPayload {
    pub enable: Option<bool>,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct WithVectors {
    pub enable: Option<bool>,
    pub vectors: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct WithLookup {
    pub collection: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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

impl Default for QueryPipeline {
    fn default() -> Self {
        Self::new()
    }
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
