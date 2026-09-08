//! Query IR types: `/points/query`, groups, prefetch, and output selectors.

use crate::filter_types::*;
use crate::semantic::*;
use alloc::string::String;
use alloc::vec::Vec;
use serde::Serialize;

/// Wraps multiple `QueryRequest`s for Qdrant's `/points/query/batch` endpoint.
/// All queries must target the same collection.
#[derive(Debug, Clone, Serialize)]
pub struct QueryBatchRequest {
    /// Lowered request bodies, one per `POST /points/query` call.
    pub searches: Vec<QueryRequest>,
}

// ── Query types ────────────────────────────────────────────────

/// Body for `POST /collections/{c}/points/query` (single search request).
#[derive(Debug, Clone, Serialize)]
pub struct QueryRequest {
    /// Lowered query expression (nearest, recommend, fusion, formula, …).
    pub query: QueryVariant,
    /// Named vector to search; defaults to the collection default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub using: Option<String>,
    /// Multi-stage candidate queries run before the main expression.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub prefetch: Vec<PrefetchRequest>,
    /// Filter applied before scoring.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<FilterExpression>,
    /// Search tuning (`hnsw_ef`, `exact`, ACORN, quantization, IDF).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<SearchParamsRequest>,
    /// Minimum similarity score required to keep a hit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score_threshold: Option<f64>,
    /// Payload selection for returned points.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_payload: Option<PayloadSelectorReq>,
    /// Vector selection for returned points.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_vector: Option<VectorSelectorReq>,
    /// Maximum number of hits to return.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
    /// Number of hits to skip before returning results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<u64>,
    /// Alternate collection/vector used to resolve point-ID inputs.
    #[serde(skip_serializing_if = "Option::is_none", rename = "lookup_from")]
    pub lookup_from: Option<LookupRequest>,
    /// Cluster shard routing for custom-sharded collections.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_key: Option<crate::semantic::PlanShardKey>,
    /// OpenAPI query param / proto field — not body JSON.
    #[serde(skip)]
    pub timeout: Option<u64>,
    /// OpenAPI query param / proto field — not body JSON.
    #[serde(skip)]
    pub consistency: Option<ReadConsistencyParam>,
}

/// Body for `POST /collections/{c}/points/query/groups` (grouped search).
#[derive(Debug, Clone, Serialize)]
pub struct QueryGroupsRequest {
    /// Lowered query expression (nearest, recommend, fusion, formula, …).
    pub query: QueryVariant,
    /// Named vector to search; defaults to the collection default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub using: Option<String>,
    /// Multi-stage candidate queries run before the main expression.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub prefetch: Vec<PrefetchRequest>,
    /// Filter applied before scoring.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<FilterExpression>,
    /// Search tuning (`hnsw_ef`, `exact`, ACORN, quantization, IDF).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<SearchParamsRequest>,
    /// Minimum similarity score required to keep a hit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score_threshold: Option<f64>,
    /// Payload selection for returned points.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_payload: Option<PayloadSelectorReq>,
    /// Vector selection for returned points.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_vector: Option<VectorSelectorReq>,
    /// Payload key the groups are formed from (`group_by`).
    pub group_by: String,
    /// Maximum points returned per group (`group_size`).
    pub group_size: u64,
    /// Groups to scan: user LIMIT + OFFSET (skipping done via `group_offset`).
    pub limit: u64,
    /// Collection to look up group hits from (bare name or full selector).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_lookup: Option<WithLookupValue>,
    /// Alternate collection/vector used to resolve point-ID inputs.
    #[serde(skip_serializing_if = "Option::is_none", rename = "lookup_from")]
    pub lookup_from: Option<LookupRequest>,
    /// Cluster shard routing for custom-sharded collections.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_key: Option<crate::semantic::PlanShardKey>,
    /// OpenAPI query param / proto field — not body JSON.
    #[serde(skip)]
    pub timeout: Option<u64>,
    /// OpenAPI query param / proto field — not body JSON.
    #[serde(skip)]
    pub consistency: Option<ReadConsistencyParam>,
    /// Plan-only (not serialized): user OFFSET for response trimming.
    #[serde(skip)]
    pub group_offset: Option<u64>,
}

/// Wire form of OpenAPI `ReadConsistency` for REST query strings / gRPC.
#[derive(Debug, Clone, PartialEq)]
pub enum ReadConsistencyParam {
    /// Numeric replication factor, sent as-is.
    Factor(u64),
    /// Wait for more than half the replicas.
    Majority,
    /// Wait for a quorum of replicas.
    Quorum,
    /// Wait for every replica.
    All,
}

impl ReadConsistencyParam {
    /// REST query value: integer factor or majority|quorum|all.
    pub fn to_query_value(&self) -> String {
        match self {
            Self::Factor(n) => n.to_string(),
            Self::Majority => "majority".into(),
            Self::Quorum => "quorum".into(),
            Self::All => "all".into(),
        }
    }
}

impl From<&qql_core::ast::ReadConsistency> for ReadConsistencyParam {
    fn from(value: &qql_core::ast::ReadConsistency) -> Self {
        match value {
            qql_core::ast::ReadConsistency::Factor(n) => Self::Factor(*n),
            qql_core::ast::ReadConsistency::Majority => Self::Majority,
            qql_core::ast::ReadConsistency::Quorum => Self::Quorum,
            qql_core::ast::ReadConsistency::All => Self::All,
        }
    }
}

/// OpenAPI `with_lookup`: bare collection name or full lookup selector.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum WithLookupValue {
    /// Bare collection name.
    Collection(String),
    /// Full selector with payload/vector options.
    Full(WithLookup),
}

/// Group lookup source: collection plus its payload/vector selection.
#[derive(Debug, Clone, Serialize)]
pub struct WithLookup {
    /// Collection providing the looked-up points.
    pub collection: String,
    /// Payload selection applied to looked-up points.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_payload: Option<PayloadSelectorReq>,
    /// Vector selection applied to looked-up points.
    pub with_vectors: Option<VectorSelectorReq>,
}

/// ACORN search params. OpenAPI defaults `enable` to false, so enabled state
/// must be serialized explicitly as `{"enable": true}`.
#[derive(Debug, Clone, Serialize)]
pub struct AcornSearchParams {
    /// Whether ACORN graph-aware filtering is enabled.
    pub enable: bool,
    /// Upper bound on ACORN selectivity (0..1).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_selectivity: Option<f64>,
}

/// OpenAPI `SearchParams`: HNSW, exact, ACORN, quantization, and IDF tuning.
#[derive(Debug, Clone, Serialize)]
pub struct SearchParamsRequest {
    /// HNSW candidate list size override (`hnsw_ef`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hnsw_ef: Option<u64>,
    /// Force exact brute-force search (`exact`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exact: Option<bool>,
    /// ACORN filtered-search settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acorn: Option<AcornSearchParams>,
    /// Restrict search to indexed segments (`indexed_only`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indexed_only: Option<bool>,
    /// Quantized-search overrides (`ignore`/`rescore`/`oversampling`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantization: Option<QuantizationSearchRequest>,
    /// Per-query IDF corpus for sparse vectors (OpenAPI `IdfParams`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idf: Option<IdfSearchParams>,
}

/// Quantized-search overrides applied to a single query.
#[derive(Debug, Clone, Serialize)]
pub struct QuantizationSearchRequest {
    /// Ignore quantized vectors and search the originals.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ignore: Option<bool>,
    /// Rescore quantized results with original vectors.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rescore: Option<bool>,
    /// Candidate oversampling factor for rescoring.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oversampling: Option<f64>,
}

/// OpenAPI `IdfParams`: `"global"` scope or a corpus filter over which sparse
/// vector IDF statistics are computed.
#[derive(Debug, Clone)]
pub enum IdfSearchParams {
    /// Corpus-wide IDF statistics (the bare string `"global"`).
    Global,
    /// IDF statistics computed over a filtered subset (`{"corpus": …}`).
    Corpus {
        /// Filter selecting the corpus subset.
        corpus: FilterExpression,
    },
}

impl Serialize for IdfSearchParams {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        match self {
            // OpenAPI `IdfScope`: the bare string `"global"`.
            IdfSearchParams::Global => serializer.serialize_str("global"),
            IdfSearchParams::Corpus { corpus } => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("corpus", corpus)?;
                map.end()
            }
        }
    }
}

/// OpenAPI `NearestQuery`: primary similarity target with optional MMR.
#[derive(Debug, Clone, Serialize)]
pub struct NearestQuery {
    /// Target input: vector, point ID, document, or image.
    pub nearest: PlanQueryInput,
    /// Maximal-marginal-relevance diversification params, when requested.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mmr: Option<MmrQueryParams>,
}

/// MMR diversification parameters for a nearest query.
#[derive(Debug, Clone, Serialize)]
pub struct MmrQueryParams {
    /// Diversity weight (lambda): 0 = relevance, 1 = diversity.
    pub diversity: f64,
    /// Candidate pool size considered before MMR reordering.
    pub candidates_limit: u64,
}

/// OpenAPI `Query` variants lowered from a QQL query expression.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum QueryVariant {
    /// Similarity search against a target (`{"nearest": …}`).
    Nearest(NearestQuery),
    /// Recommendation from positive/negative examples (`{"recommend": …}`).
    Recommend {
        /// Recommendation input: positives, negatives, strategy.
        recommend: RecommendQuery,
    },
    /// Context-pair constraint search (`{"context": [...]}`).
    Context {
        /// Positive and negative context pairs.
        context: Vec<ContextPair>,
    },
    /// Target-plus-context discovery search (`{"discover": …}`).
    Discover {
        /// Target example restricted by context pairs.
        discover: DiscoverQuery,
    },
    /// Payload-key ordering query (`{"order_by": …}`).
    OrderBy {
        /// Order key, direction, and paging.
        order_by: OrderByQuery,
    },
    /// Random sampling (`{"sample": "random"}`).
    Sample {
        /// Sampling method name (only `random`).
        sample: String,
    },
    /// Multi-stage fusion by name, `rrf` or `dbsf` (`{"fusion": …}`).
    Fusion {
        /// Fusion method name (`rrf` or `dbsf`).
        fusion: String,
    },
    /// Reciprocal-rank fusion with explicit parameters (`{"rrf": …}`).
    Rrf(RrfQuery),
    /// Arithmetic formula scoring (`{"formula": …}`).
    Formula(FormulaQuery),
    /// Rocchio-style relevance feedback (`{"relevance_feedback": …}`).
    RelevanceFeedback {
        /// Positive/negative feedback inputs with weights.
        relevance_feedback: RelevanceFeedbackInput,
    },
}

/// Reciprocal-rank-fusion parameters.
#[derive(Debug, Clone, Serialize)]
pub struct RrfParams {
    /// RRF smoothing constant (`k`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub k: Option<u64>,
    /// Per-prefetch weights aligned with the prefetch order.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weights: Option<Vec<f64>>,
}

/// Explicit `rrf` query wrapper.
#[derive(Debug, Clone, Serialize)]
pub struct RrfQuery {
    /// Reciprocal-rank-fusion parameters.
    pub rrf: RrfParams,
}

/// Formula-based scoring over payload keys and `$score`.
#[derive(Debug, Clone, Serialize)]
pub struct FormulaQuery {
    /// Typed formula expression serialized to the OpenAPI wire form.
    pub formula: PlanFormula,
    /// Default values for variables referenced by the formula.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub defaults: Option<serde_json::Map<String, serde_json::Value>>,
}

/// Relevance-feedback query: target input, scored examples, and strategy.
#[derive(Debug, Clone, Serialize)]
pub struct RelevanceFeedbackInput {
    /// Base query input to refine with feedback.
    pub target: PlanQueryInput,
    /// Scored example inputs (positive or negative relevance).
    pub feedback: Vec<FeedbackItem>,
    /// Feedback strategy; currently the `naive` linear combination.
    pub strategy: FeedbackStrategy,
}

/// One scored relevance-feedback example.
#[derive(Debug, Clone, Serialize)]
pub struct FeedbackItem {
    /// Example query input.
    pub example: PlanQueryInput,
    /// Relevance score of the example (positive or negative).
    pub score: f64,
}

/// OpenAPI `FeedbackStrategy` wrapper.
#[derive(Debug, Clone, Serialize)]
pub struct FeedbackStrategy {
    /// Naive linear-combination parameters.
    pub naive: NaiveFeedbackStrategyParams,
}

/// `naive` strategy weights over target and example similarities.
#[derive(Debug, Clone, Serialize)]
pub struct NaiveFeedbackStrategyParams {
    /// Weight applied to the target similarity.
    pub a: f64,
    /// Weight applied to positive-example similarities.
    pub b: f64,
    /// Weight applied to negative-example similarities.
    pub c: f64,
}

/// Recommendation query over positive/negative example inputs.
#[derive(Debug, Clone, Serialize)]
pub struct RecommendQuery {
    /// Examples the results should be similar to.
    pub positive: Vec<PlanQueryInput>,
    /// Examples the results should steer away from.
    pub negative: Vec<PlanQueryInput>,
    /// Aggregation strategy: `average_vector`, `best_score`, or `sum_scores`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strategy: Option<String>,
}

/// One positive/negative pair forming a context constraint.
#[derive(Debug, Clone, Serialize)]
pub struct ContextPair {
    /// Input the results should be similar to.
    pub positive: PlanQueryInput,
    /// Input the results should be dissimilar to.
    pub negative: PlanQueryInput,
}

/// Discovery query: anchor target refined by context pairs.
#[derive(Debug, Clone, Serialize)]
pub struct DiscoverQuery {
    /// Primary anchor input.
    pub target: PlanQueryInput,
    /// Positive/negative constraints applied around the target.
    pub context: Vec<ContextPair>,
}

/// Payload-key ordering for order-by queries.
#[derive(Debug, Clone, Serialize)]
pub struct OrderByQuery {
    /// Payload key to order by.
    pub key: String,
    /// Sort direction: `"asc"` or `"desc"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direction: Option<String>,
}

/// One multi-stage prefetch stage; nested stages recurse via `prefetch`.
#[derive(Debug, Clone, Serialize)]
pub struct PrefetchRequest {
    /// Stage query expression, omitted for filter-only stages.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<QueryVariant>,
    /// Named vector used by this stage.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub using: Option<String>,
    /// Filter applied before this stage scores.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<FilterExpression>,
    /// Stage search tuning.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<SearchParamsRequest>,
    /// Stage score cutoff.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score_threshold: Option<f64>,
    /// Candidate count produced by this stage.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
    /// Alternate collection/vector used to resolve point-ID inputs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lookup_from: Option<LookupRequest>,
    /// Nested stages run before this stage.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefetch: Option<Vec<PrefetchRequest>>,
}

/// Alternate source collection/vector for resolving point-ID inputs.
#[derive(Debug, Clone, Serialize)]
pub struct LookupRequest {
    /// Collection holding the referenced points.
    pub collection: String,
    /// Named vector used for the lookup.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vector: Option<String>,
}

/// OpenAPI `PayloadSelector`: all on/off, include-list, or exclude-list.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum PayloadSelectorReq {
    /// `true` returns full payload, `false` returns none.
    All(bool),
    /// Field whitelist (`{"include": [...]}`).
    Include {
        /// Payload keys to return.
        include: Vec<String>,
    },
    /// Field blacklist (`{"exclude": [...]}`).
    Exclude {
        /// Payload keys to omit.
        exclude: Vec<String>,
    },
}

/// OpenAPI vector selector: all on/off or a named-vector list.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum VectorSelectorReq {
    /// `true` returns all vectors, `false` returns none.
    All(bool),
    /// Return only the listed named vectors.
    Names(Vec<String>),
}
