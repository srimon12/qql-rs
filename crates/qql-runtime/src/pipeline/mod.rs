pub mod embed_nodes;
pub mod formula_nodes;
pub mod helpers;
pub mod query_nodes;

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::backend::Filter;
pub use crate::backend::PointId;
use crate::embedder::Embedder;
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

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait ExecutionNode: crate::client::QdrantOpsBound {
    async fn execute(&self, state: &mut QueryState) -> Result<(), QqlError>;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AcornSearchParams {
    pub enable: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SearchParams {
    pub hnsw_ef: Option<u64>,
    pub exact: Option<bool>,
    pub acorn: Option<AcornSearchParams>,
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

#[derive(Debug, Clone, PartialEq, Deserialize)]
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
    Formula {
        expression: serde_json::Value,
        defaults: HashMap<String, f64>,
    },
    RelevanceFeedback(RelevanceFeedbackInput),
    MMR {
        input: Box<QueryVariant>,
        diversity: f32,
        candidates: u32,
    },
}

impl serde::Serialize for QueryVariant {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;
        match self {
            QueryVariant::Nearest(vec) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("nearest", vec)?;
                map.end()
            }
            QueryVariant::Sparse(indices, values) => {
                let mut map = serializer.serialize_map(Some(1))?;
                let mut sparse_map = serde_json::Map::new();
                sparse_map.insert(
                    "indices".to_string(),
                    serde_json::to_value(indices).map_err(serde::ser::Error::custom)?,
                );
                sparse_map.insert(
                    "values".to_string(),
                    serde_json::to_value(values).map_err(serde::ser::Error::custom)?,
                );
                map.serialize_entry("nearest", &serde_json::Value::Object(sparse_map))?;
                map.end()
            }
            QueryVariant::Document {
                text,
                model,
                options: _,
            } => {
                let mut map = serializer.serialize_map(Some(1))?;
                if model.is_empty() {
                    map.serialize_entry("nearest", text)?;
                } else {
                    let mut doc_map = serde_json::Map::new();
                    doc_map.insert("text".to_string(), serde_json::Value::String(text.clone()));
                    doc_map.insert(
                        "model".to_string(),
                        serde_json::Value::String(model.clone()),
                    );
                    map.serialize_entry("nearest", &serde_json::Value::Object(doc_map))?;
                }
                map.end()
            }
            QueryVariant::Recommend(input) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("recommend", input)?;
                map.end()
            }
            QueryVariant::Context(input) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("context", input)?;
                map.end()
            }
            QueryVariant::Discover(input) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("discover", input)?;
                map.end()
            }
            QueryVariant::OrderBy(input) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("order_by", input)?;
                map.end()
            }
            QueryVariant::Sample => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("sample", &"random")?;
                map.end()
            }
            QueryVariant::Fusion(ft) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("fusion", ft)?;
                map.end()
            }
            QueryVariant::Rrf(config) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("rrf", config)?;
                map.end()
            }
            QueryVariant::Formula {
                expression,
                defaults: _,
            } => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("formula", expression)?;
                map.end()
            }
            QueryVariant::RelevanceFeedback(input) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("relevance_feedback", input)?;
                map.end()
            }
            QueryVariant::MMR {
                input,
                diversity,
                candidates,
            } => {
                let mut map = serializer.serialize_map(Some(2))?;

                let inner_json = serde_json::to_value(input).map_err(serde::ser::Error::custom)?;
                let inner_vector_input = if let Some(nearest_val) = inner_json.get("nearest") {
                    nearest_val.clone()
                } else {
                    return Err(serde::ser::Error::custom(
                        "MMR inner query must be a nearest query",
                    ));
                };

                map.serialize_entry("nearest", &inner_vector_input)?;
                let mut mmr_opts = serde_json::Map::new();
                mmr_opts.insert("diversity".to_string(), serde_json::json!(diversity));
                mmr_opts.insert(
                    "candidates_limit".to_string(),
                    serde_json::json!(candidates),
                );
                map.serialize_entry("mmr", &serde_json::Value::Object(mmr_opts))?;
                map.end()
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RecommendInput {
    pub positive: Vec<VectorInput>,
    pub negative: Vec<VectorInput>,
    pub strategy: Option<RecommendStrategyType>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ContextInput {
    pub pairs: Vec<ContextPair>,
}

impl serde::Serialize for ContextInput {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.pairs.serialize(serializer)
    }
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

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub enum VectorInput {
    Id(PointId),
    Dense(Vec<f32>),
    Document {
        text: String,
        model: String,
        options: HashMap<String, String>,
    },
}

impl serde::Serialize for VectorInput {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            VectorInput::Id(pid) => pid.serialize(serializer),
            VectorInput::Dense(vec) => vec.serialize(serializer),
            VectorInput::Document {
                text,
                model,
                options,
            } => {
                if model.is_empty() && options.is_empty() {
                    text.serialize(serializer)
                } else {
                    let mut doc_map = serde_json::Map::new();
                    doc_map.insert("text".to_string(), serde_json::Value::String(text.clone()));
                    if !model.is_empty() {
                        doc_map.insert(
                            "model".to_string(),
                            serde_json::Value::String(model.clone()),
                        );
                    }
                    if !options.is_empty() {
                        doc_map.insert(
                            "options".to_string(),
                            serde_json::to_value(options).map_err(serde::ser::Error::custom)?,
                        );
                    }
                    doc_map.serialize(serializer)
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PrefetchQuery {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub prefetch: Vec<PrefetchQuery>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<QueryVariant>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub using: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<Filter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<SearchParams>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score_threshold: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lookup_from: Option<LookupLocation>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct LookupLocation {
    pub collection_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vector_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct WithPayload {
    pub enable: Option<bool>,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
}

impl serde::Serialize for WithPayload {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        if !self.exclude.is_empty() {
            use serde::ser::SerializeMap;
            let mut map = serializer.serialize_map(Some(1))?;
            map.serialize_entry("exclude", &self.exclude)?;
            map.end()
        } else if !self.include.is_empty() {
            use serde::ser::SerializeMap;
            let mut map = serializer.serialize_map(Some(1))?;
            map.serialize_entry("include", &self.include)?;
            map.end()
        } else {
            self.enable.unwrap_or(false).serialize(serializer)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct WithVectors {
    pub enable: Option<bool>,
    pub vectors: Vec<String>,
}

impl serde::Serialize for WithVectors {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        if self.vectors.is_empty() {
            self.enable.unwrap_or(false).serialize(serializer)
        } else {
            self.vectors.serialize(serializer)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct WithLookup {
    pub collection: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryPointsRequest {
    #[serde(skip_serializing)]
    pub collection_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<QueryVariant>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub prefetch: Vec<PrefetchQuery>,
    pub limit: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<SearchParams>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<Filter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_payload: Option<WithPayload>,
    #[serde(rename = "with_vector", skip_serializing_if = "Option::is_none")]
    pub with_vectors: Option<WithVectors>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score_threshold: Option<f32>,
    pub offset: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lookup_from: Option<LookupLocation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub using: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryPointsGroupsRequest {
    #[serde(skip_serializing)]
    pub collection_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<QueryVariant>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub prefetch: Vec<PrefetchQuery>,
    pub limit: u64,
    pub group_by: String,
    pub group_size: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<SearchParams>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<Filter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_payload: Option<WithPayload>,
    #[serde(rename = "with_vector", skip_serializing_if = "Option::is_none")]
    pub with_vectors: Option<WithVectors>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score_threshold: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lookup_from: Option<LookupLocation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_lookup: Option<WithLookup>,
    #[serde(skip_serializing_if = "Option::is_none")]
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
    pub qdrant_filter: Option<Filter>,
    pub score_threshold: Option<f32>,
    pub lookup_from: Option<LookupLocation>,
    pub with_payload: Option<WithPayload>,
    pub with_vectors: Option<WithVectors>,

    pub group_by: String,
    pub group_size: u64,
    pub with_lookup: Option<WithLookup>,

    pub formula: Option<serde_json::Value>,
    pub formula_defaults: HashMap<String, f64>,
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

    pub fn build_flat_request(&self, state: &QueryState) -> Result<QueryPointsRequest, QqlError> {
        let mut prefetches = state.prefetches.clone();
        prefetches.extend(state.manual_prefetches.clone());

        let query = if let Some(ref f) = state.formula {
            let defaults_map: serde_json::Map<String, serde_json::Value> = state
                .formula_defaults
                .iter()
                .map(|(k, v)| (k.clone(), serde_json::json!(v)))
                .collect();
            let defaults = defaults_map
                .into_iter()
                .filter_map(|(key, value)| value.as_f64().map(|value| (key, value)))
                .collect();

            if let Some(ref target) = state.target_query {
                prefetches.push(PrefetchQuery {
                    prefetch: Vec::new(),
                    query: Some(target.clone()),
                    using: if state.vector_name.is_empty() {
                        None
                    } else {
                        Some(state.vector_name.clone())
                    },
                    limit: Some(state.limit),
                    params: None,
                    filter: None,
                    score_threshold: None,
                    lookup_from: None,
                });
            }

            Some(QueryVariant::Formula {
                expression: f.clone(),
                defaults,
            })
        } else {
            state.target_query.clone()
        };
        let prefetch = prefetches;
        let params = state.params.clone();
        let with_payload = state.with_payload.clone();
        let with_vectors = state.with_vectors.clone();
        let lookup_from = state.lookup_from.clone();

        let mut req = QueryPointsRequest {
            collection_name: state.collection_name.clone(),
            query,
            prefetch,
            limit: state.limit,
            params,
            filter: state.qdrant_filter.clone(),
            with_payload,
            with_vectors,
            score_threshold: state.score_threshold,
            offset: state.offset,
            lookup_from,
            using: None,
            timeout: state.request_timeout,
        };

        if !state.vector_name.is_empty() {
            req.using = Some(state.vector_name.clone());
        }

        Ok(req)
    }

    pub fn build_grouped_request(
        &self,
        state: &QueryState,
    ) -> Result<QueryPointsGroupsRequest, QqlError> {
        let flat = self.build_flat_request(state)?;
        let with_lookup = state.with_lookup.clone();

        Ok(QueryPointsGroupsRequest {
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
            with_lookup,
            using: flat.using,
        })
    }
}
