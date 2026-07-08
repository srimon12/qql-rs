pub mod embed_nodes;
pub mod formula_nodes;
pub mod helpers;
pub mod query_nodes;

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::embedder::Embedder;
pub type QdrantFilter = crate::qdrant::Filter;
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryPointsRequest {
    pub collection_name: String,
    pub query: Option<crate::qdrant::Query>,
    pub prefetch: Vec<crate::qdrant::Prefetch>,
    pub limit: u64,
    pub params: Option<crate::qdrant::SearchParams>,
    pub filter: Option<QdrantFilter>,
    pub with_payload: Option<crate::qdrant::WithPayloadInterface>,
    pub with_vectors: Option<crate::qdrant::WithVector>,
    pub score_threshold: Option<f32>,
    pub offset: u64,
    pub lookup_from: Option<crate::qdrant::LookupLocation>,
    pub using: Option<String>,
    pub timeout: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryPointsGroupsRequest {
    pub collection_name: String,
    pub query: Option<crate::qdrant::Query>,
    pub prefetch: Vec<crate::qdrant::Prefetch>,
    pub limit: u64,
    pub group_by: String,
    pub group_size: u64,
    pub params: Option<crate::qdrant::SearchParams>,
    pub filter: Option<QdrantFilter>,
    pub with_payload: Option<crate::qdrant::WithPayloadInterface>,
    pub with_vectors: Option<crate::qdrant::WithVector>,
    pub score_threshold: Option<f32>,
    pub lookup_from: Option<crate::qdrant::LookupLocation>,
    pub with_lookup: Option<crate::qdrant::QueryGroupsRequestWithLookup>,
    pub using: Option<String>,
}

impl From<VectorInput> for crate::qdrant::VectorInput {
    fn from(vi: VectorInput) -> Self {
        match vi {
            VectorInput::Dense(v) => crate::qdrant::VectorInput::Variant0(v),
            VectorInput::Id(id) => crate::qdrant::VectorInput::Variant3(id.into()),
            VectorInput::Document { .. } => crate::qdrant::VectorInput::Variant0(vec![]),
        }
    }
}

impl From<RecommendInput> for crate::qdrant::RecommendQuery {
    fn from(rec: RecommendInput) -> Self {
        crate::qdrant::RecommendQuery {
            recommend: crate::qdrant::RecommendInput {
                positive: rec.positive.into_iter().map(|v| v.into()).collect(),
                negative: rec.negative.into_iter().map(|v| v.into()).collect(),
                strategy: rec.strategy.map(|s| crate::qdrant::RecommendInputStrategy {
                    subtype_0: Some(match s {
                        RecommendStrategyType::AverageVector => {
                            crate::qdrant::RecommendStrategy::AverageVector
                        }
                        RecommendStrategyType::BestScore => {
                            crate::qdrant::RecommendStrategy::BestScore
                        }
                        RecommendStrategyType::SumScores => {
                            crate::qdrant::RecommendStrategy::SumScores
                        }
                    }),
                    subtype_1: None,
                }),
            },
        }
    }
}

impl From<ContextPair> for crate::qdrant::ContextPair {
    fn from(cp: ContextPair) -> Self {
        crate::qdrant::ContextPair {
            positive: cp
                .positive
                .map(|v| v.into())
                .unwrap_or(crate::qdrant::VectorInput::Variant0(vec![])),
            negative: cp
                .negative
                .map(|v| v.into())
                .unwrap_or(crate::qdrant::VectorInput::Variant0(vec![])),
        }
    }
}

impl From<OrderByInput> for crate::qdrant::OrderBy {
    fn from(ob: OrderByInput) -> Self {
        crate::qdrant::OrderBy {
            key: ob.key,
            direction: Some(crate::qdrant::OrderByDirection {
                subtype_0: Some(match ob.direction {
                    OrderByDirection::Asc => crate::qdrant::Direction::Asc,
                    OrderByDirection::Desc => crate::qdrant::Direction::Desc,
                }),
                subtype_1: None,
            }),
            start_from: None,
        }
    }
}

impl TryFrom<QueryVariant> for crate::qdrant::Query {
    type Error = QqlError;
    fn try_from(qv: QueryVariant) -> Result<Self, Self::Error> {
        match qv {
            QueryVariant::Nearest(v) => Ok(crate::qdrant::Query::NearestQuery(
                crate::qdrant::NearestQuery {
                    nearest: crate::qdrant::VectorInput::Variant0(v),
                    mmr: None,
                },
            )),
            QueryVariant::Sparse(indices, values) => Ok(crate::qdrant::Query::NearestQuery(
                crate::qdrant::NearestQuery {
                    nearest: crate::qdrant::VectorInput::Variant1(crate::qdrant::SparseVector {
                        indices,
                        values,
                    }),
                    mmr: None,
                },
            )),
            QueryVariant::Recommend(rec) => Ok(crate::qdrant::Query::RecommendQuery(rec.into())),
            QueryVariant::Context(ctx) => Ok(crate::qdrant::Query::ContextQuery(
                crate::qdrant::ContextQuery {
                    context: crate::qdrant::ContextInput {
                        subtype_0: None,
                        subtype_1: Some(ctx.pairs.into_iter().map(|p| p.into()).collect()),
                        subtype_2: None,
                    },
                },
            )),
            QueryVariant::Discover(dis) => Ok(crate::qdrant::Query::DiscoverQuery(
                crate::qdrant::DiscoverQuery {
                    discover: crate::qdrant::DiscoverInput {
                        target: dis.target.into(),
                        context: crate::qdrant::DiscoverInputContext {
                            subtype_0: None,
                            subtype_1: Some(
                                dis.context.pairs.into_iter().map(|p| p.into()).collect(),
                            ),
                            subtype_2: None,
                        },
                    },
                },
            )),
            QueryVariant::OrderBy(ob) => Ok(crate::qdrant::Query::OrderByQuery(
                crate::qdrant::OrderByQuery {
                    order_by: crate::qdrant::OrderByInterface::OrderBy(ob.into()),
                },
            )),
            QueryVariant::Sample => Ok(crate::qdrant::Query::SampleQuery(
                crate::qdrant::SampleQuery {
                    sample: crate::qdrant::Sample::Random,
                },
            )),
            QueryVariant::Fusion(fusion) => {
                let f = match fusion {
                    FusionType::Rrf => crate::qdrant::Fusion::Rrf,
                    FusionType::Dbsf => crate::qdrant::Fusion::Dbsf,
                };
                Ok(crate::qdrant::Query::FusionQuery(
                    crate::qdrant::FusionQuery { fusion: f },
                ))
            }
            QueryVariant::Rrf(rrf) => {
                let k_nz = rrf.k.and_then(std::num::NonZeroU32::new);
                Ok(crate::qdrant::Query::RrfQuery(crate::qdrant::RrfQuery {
                    rrf: crate::qdrant::Rrf {
                        k: k_nz,
                        weights: rrf.weights,
                    },
                }))
            }
            QueryVariant::RelevanceFeedback(rf) => {
                let strategy = rf
                    .strategy
                    .map(|s| {
                        crate::qdrant::FeedbackStrategy(crate::qdrant::NaiveFeedbackStrategy {
                            naive: crate::qdrant::NaiveFeedbackStrategyParams {
                                a: s.a,
                                b: s.b,
                                c: s.c,
                            },
                        })
                    })
                    .unwrap_or(crate::qdrant::FeedbackStrategy(
                        crate::qdrant::NaiveFeedbackStrategy {
                            naive: crate::qdrant::NaiveFeedbackStrategyParams {
                                a: 1.0,
                                b: 1.0,
                                c: 1.0,
                            },
                        },
                    ));

                Ok(crate::qdrant::Query::RelevanceFeedbackQuery(
                    crate::qdrant::RelevanceFeedbackQuery {
                        relevance_feedback: crate::qdrant::RelevanceFeedbackInput {
                            feedback: rf
                                .feedback
                                .into_iter()
                                .map(|item| crate::qdrant::FeedbackItem {
                                    example: item.example.into(),
                                    score: item.score,
                                })
                                .collect(),
                            strategy,
                            target: rf.target.into(),
                        },
                    },
                ))
            }
            QueryVariant::MMR { .. } => Err(QqlError::runtime(
                "MMR queries must be resolved to nearest search before building request",
            )),
            QueryVariant::Document { .. } => Err(QqlError::runtime(
                "Document queries must be embedded before building request",
            )),
        }
    }
}

impl TryFrom<PrefetchQuery> for crate::qdrant::Prefetch {
    type Error = QqlError;
    fn try_from(pq: PrefetchQuery) -> Result<Self, Self::Error> {
        let query_gen: Option<crate::qdrant::Query> =
            pq.query.map(crate::qdrant::Query::try_from).transpose()?;
        let params_gen: Option<crate::qdrant::SearchParams> = pq
            .params
            .map(crate::qdrant::SearchParams::try_from)
            .transpose()?;

        let prefetch_gen = if !pq.prefetch.is_empty() {
            let list: Vec<crate::qdrant::Prefetch> = pq
                .prefetch
                .into_iter()
                .map(|p| p.try_into())
                .collect::<Result<_, _>>()?;
            Some(crate::qdrant::PrefetchPrefetch {
                subtype_0: None,
                subtype_1: Some(list),
                subtype_2: None,
            })
        } else {
            None
        };

        Ok(crate::qdrant::Prefetch {
            filter: pq
                .filter
                .map(|f| serde_json::from_value(serde_json::to_value(&f).unwrap()).unwrap()),
            limit: pq.limit.and_then(|l| std::num::NonZeroU32::new(l as u32)),
            lookup_from: pq
                .lookup_from
                .map(|lf| serde_json::from_value(serde_json::to_value(&lf).unwrap()).unwrap()),
            params: params_gen
                .map(|p| serde_json::from_value(serde_json::to_value(&p).unwrap()).unwrap()),
            prefetch: prefetch_gen,
            query: query_gen
                .map(|q| serde_json::from_value(serde_json::to_value(&q).unwrap()).unwrap()),
            score_threshold: pq.score_threshold,
            using: pq.using,
        })
    }
}

impl TryFrom<SearchParams> for crate::qdrant::SearchParams {
    type Error = QqlError;
    fn try_from(sp: SearchParams) -> Result<Self, Self::Error> {
        Ok(crate::qdrant::SearchParams {
            acorn: None,
            exact: sp.exact,
            hnsw_ef: sp.hnsw_ef.map(|val| val as u32),
            indexed_only: sp.indexed_only,
            quantization: sp
                .quantization
                .map(|q| crate::qdrant::SearchParamsQuantization {
                    subtype_0: Some(crate::qdrant::QuantizationSearchParams {
                        ignore: q.ignore,
                        oversampling: q.oversampling,
                        rescore: q.rescore,
                    }),
                    subtype_1: None,
                }),
        })
    }
}

impl From<LookupLocation> for crate::qdrant::LookupLocation {
    fn from(ll: LookupLocation) -> Self {
        crate::qdrant::LookupLocation {
            collection: ll.collection_name,
            shard_key: None,
            vector: ll.vector_name,
        }
    }
}

impl From<WithPayload> for crate::qdrant::WithPayloadInterface {
    fn from(wp: WithPayload) -> Self {
        if !wp.exclude.is_empty() {
            crate::qdrant::WithPayloadInterface {
                subtype_0: None,
                subtype_1: None,
                subtype_2: Some(crate::qdrant::PayloadSelector::Exclude(
                    crate::qdrant::PayloadSelectorExclude {
                        exclude: wp.exclude,
                    },
                )),
            }
        } else if !wp.include.is_empty() {
            crate::qdrant::WithPayloadInterface {
                subtype_0: None,
                subtype_1: None,
                subtype_2: Some(crate::qdrant::PayloadSelector::Include(
                    crate::qdrant::PayloadSelectorInclude {
                        include: wp.include,
                    },
                )),
            }
        } else {
            crate::qdrant::WithPayloadInterface {
                subtype_0: wp.enable,
                subtype_1: None,
                subtype_2: None,
            }
        }
    }
}

impl From<WithVectors> for crate::qdrant::WithVector {
    fn from(wv: WithVectors) -> Self {
        if !wv.vectors.is_empty() {
            crate::qdrant::WithVector::Array(wv.vectors)
        } else {
            crate::qdrant::WithVector::Boolean(wv.enable.unwrap_or(false))
        }
    }
}

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

    pub formula: Option<serde_json::Value>,
    pub formula_defaults: HashMap<String, f64>,
}

impl Default for QueryState {
    fn default() -> Self {
        QueryState {
            query_text: String::new(),
            prefetches: Vec::new(),
            manual_prefetches: Vec::new(),
            target_query: None,
            params: None,
            fusion_config: None,
            has_mmr: false,
            mmr_candidates: 0,
            mmr_diversity: 0.0,
            local_embed: false,
            embedder: None,
            cloud_model_options: HashMap::new(),
            dense_model: String::new(),
            doc_options: None,
            request_timeout: None,
            collection_name: String::new(),
            vector_name: String::new(),
            limit: 0,
            offset: 0,
            qdrant_filter: None,
            score_threshold: None,
            lookup_from: None,
            with_payload: None,
            with_vectors: None,
            group_by: String::new(),
            group_size: 0,
            with_lookup: None,
            formula: None,
            formula_defaults: HashMap::new(),
        }
    }
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
            let formula_val = serde_json::json!({
                "formula": f,
                "defaults": defaults_map
            });
            let formula_query: crate::qdrant::FormulaQuery = serde_json::from_value(formula_val)
                .map_err(|e| QqlError::runtime(e.to_string()))?;
            Some(crate::qdrant::Query::FormulaQuery(formula_query))
        } else {
            state
                .target_query
                .as_ref()
                .map(|q| q.clone().try_into())
                .transpose()?
        };
        let prefetch = prefetches
            .into_iter()
            .map(|p| p.try_into())
            .collect::<Result<Vec<_>, _>>()?;
        let params = state
            .params
            .as_ref()
            .map(|p| p.clone().try_into())
            .transpose()?;
        let with_payload = state.with_payload.as_ref().map(|wp| wp.clone().into());
        let with_vectors = state.with_vectors.as_ref().map(|wv| wv.clone().into());
        let lookup_from = state.lookup_from.as_ref().map(|lf| lf.clone().into());

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
        let with_lookup = state
            .with_lookup
            .as_ref()
            .map(|wl| {
                let wl_val = serde_json::json!({
                    "collection": wl.collection
                });
                serde_json::from_value(wl_val).map_err(|e| QqlError::runtime(e.to_string()))
            })
            .transpose()?;

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
