use alloc::string::String;
use alloc::vec::Vec;
use serde::Serialize;

// ── Method ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

impl Method {
    pub fn as_str(&self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Patch => "PATCH",
            Method::Delete => "DELETE",
        }
    }
}

// ── Embedding ──────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddingJob {
    pub texts: Vec<String>,
    pub model: Option<String>,
    pub kind: EmbeddingKind,
    pub destinations: Vec<EmbeddingDestination>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddingKind {
    Dense,
    Sparse,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddingDestination {
    pub carrier_name: String,
    pub vector_name: String,
}

// ── Filter types ───────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum FilterExpression {
    Single(Box<FilterClause>),
    Compound(FilterCompound),
}

#[derive(Debug, Clone, Serialize)]
pub struct FilterCompound {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub must: Vec<FilterClause>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub must_not: Vec<FilterClause>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub should: Vec<FilterClause>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_key: Option<String>,
}

/// Wraps multiple `QueryRequest`s for Qdrant's `/points/query/batch` endpoint.
/// All queries must target the same collection.
#[derive(Debug, Clone, Serialize)]
pub struct QueryBatchRequest {
    pub searches: Vec<QueryRequest>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MinShould {
    pub conditions: Vec<FilterClause>,
    pub min_count: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum FilterClause {
    Field(Box<FieldCondition>),
    IsNull(IsNullCondition),
    IsEmpty(IsEmptyCondition),
    HasId(HasIdCondition),
    HasVector(HasVectorCondition),
    Nested(NestedCondition),
    Filter(Box<FilterCompound>),
}

#[derive(Debug, Clone, Serialize)]
pub struct FieldCondition {
    pub key: String,
    #[serde(rename = "match", skip_serializing_if = "Option::is_none")]
    pub r#match: Option<MatchValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<RangeParams>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geo_bounding_box: Option<GeoBoundingBox>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geo_radius: Option<GeoRadius>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geo_polygon: Option<GeoPolygon>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub values_count: Option<ValuesCountParams>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_empty: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_null: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValuesCountParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lt: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gt: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gte: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lte: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum MatchValue {
    Value { value: serde_json::Value },
    Text { text: String },
    TextAny { text: String },
    Any { any: Vec<serde_json::Value> },
    Except { except: Vec<serde_json::Value> },
    Phrase { phrase: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct RangeParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gt: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gte: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lt: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lte: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GeoBoundingBox {
    pub top_left: GeoPoint,
    pub bottom_right: GeoPoint,
}

#[derive(Debug, Clone, Serialize)]
pub struct GeoRadius {
    pub center: GeoPoint,
    pub radius: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct GeoPolygon {
    pub exterior: GeoLineString,
    pub interiors: Vec<GeoLineString>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GeoLineString {
    pub points: Vec<GeoPoint>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GeoPoint {
    pub lat: f64,
    pub lon: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct IsNullCondition {
    pub is_null: KeyOnly,
}

#[derive(Debug, Clone, Serialize)]
pub struct IsEmptyCondition {
    pub is_empty: KeyOnly,
}

#[derive(Debug, Clone, Serialize)]
pub struct KeyOnly {
    pub key: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct HasIdCondition {
    pub has_id: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HasVectorCondition {
    pub has_vector: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct NestedCondition {
    pub nested: NestedParams,
}

#[derive(Debug, Clone, Serialize)]
pub struct NestedParams {
    pub key: String,
    pub filter: Box<FilterExpression>,
}

// ── Query types ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct QueryRequest {
    pub query: QueryVariant,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub using: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub prefetch: Vec<PrefetchRequest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<FilterExpression>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<SearchParamsRequest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score_threshold: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_payload: Option<PayloadSelectorReq>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_vector: Option<VectorSelectorReq>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "lookup_from")]
    pub lookup_from: Option<LookupRequest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_key: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct QueryGroupsRequest {
    pub query: QueryVariant,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub using: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub prefetch: Vec<PrefetchRequest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<FilterExpression>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<SearchParamsRequest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score_threshold: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_payload: Option<PayloadSelectorReq>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_vector: Option<VectorSelectorReq>,
    pub group_by: String,
    pub group_size: u64,
    pub limit: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_lookup: Option<WithLookupValue>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "lookup_from")]
    pub lookup_from: Option<LookupRequest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_key: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum WithLookupValue {
    Collection(String),
    Full(WithLookup),
}

#[derive(Debug, Clone, Serialize)]
pub struct WithLookup {
    pub collection: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_payload: Option<PayloadSelectorReq>,
    pub with_vectors: Option<VectorSelectorReq>,
}

#[derive(Debug, Clone)]
pub struct AcornFlag;

impl Serialize for AcornFlag {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let s = serializer.serialize_struct("AcornFlag", 0)?;
        s.end()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchParamsRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hnsw_ef: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exact: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acorn: Option<AcornFlag>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indexed_only: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantization: Option<QuantizationSearchRequest>,
}

#[derive(Debug, Clone, Serialize)]
pub struct QuantizationSearchRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ignore: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rescore: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oversampling: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NearestQuery {
    pub nearest: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mmr: Option<MmrQueryParams>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MmrQueryParams {
    pub diversity: f64,
    pub candidates_limit: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum QueryVariant {
    Nearest(NearestQuery),
    Recommend {
        recommend: RecommendQuery,
    },
    Context {
        context: Vec<ContextPair>,
    },
    Discover {
        discover: DiscoverQuery,
    },
    OrderBy {
        order_by: OrderByQuery,
    },
    Sample {
        sample: String,
    },
    Fusion {
        fusion: String,
    },
    Rrf(RrfQuery),
    Formula(FormulaQuery),
    RelevanceFeedback {
        relevance_feedback: RelevanceFeedbackInput,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct RrfParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub k: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weights: Option<Vec<f64>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RrfQuery {
    pub rrf: RrfParams,
}

#[derive(Debug, Clone, Serialize)]
pub struct FormulaQuery {
    pub formula: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub defaults: Option<serde_json::Map<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RelevanceFeedbackInput {
    pub target: serde_json::Value,
    pub feedback: Vec<FeedbackItem>,
    pub strategy: FeedbackStrategy,
}

#[derive(Debug, Clone, Serialize)]
pub struct FeedbackItem {
    pub example: serde_json::Value,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct FeedbackStrategy {
    pub naive: NaiveFeedbackStrategyParams,
}

#[derive(Debug, Clone, Serialize)]
pub struct NaiveFeedbackStrategyParams {
    pub a: f64,
    pub b: f64,
    pub c: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecommendQuery {
    pub positive: Vec<serde_json::Value>,
    pub negative: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strategy: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContextPair {
    pub positive: serde_json::Value,
    pub negative: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiscoverQuery {
    pub target: serde_json::Value,
    pub context: Vec<ContextPair>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OrderByQuery {
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direction: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PrefetchRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<QueryVariant>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub using: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<FilterExpression>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<SearchParamsRequest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score_threshold: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lookup_from: Option<LookupRequest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefetch: Option<Vec<PrefetchRequest>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LookupRequest {
    pub collection: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vector: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PointsRequest {
    pub ids: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_payload: Option<PayloadSelectorReq>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_vector: Option<VectorSelectorReq>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum PayloadSelectorReq {
    All(bool),
    Include { include: Vec<String> },
    Exclude { exclude: Vec<String> },
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum VectorSelectorReq {
    All(bool),
    Names(Vec<String>),
}

// ── Scroll ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct ScrollRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<FilterExpression>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_payload: Option<PayloadSelectorReq>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_vector: Option<VectorSelectorReq>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_by: Option<OrderByQuery>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_key: Option<String>,
}

// ── Mutations ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct UpsertRequest {
    pub points: Vec<UpsertPointRequest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_key: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpsertPointRequest {
    pub id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vector: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Map<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeleteRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<FilterExpression>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_key: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateVectorRequest {
    pub points: Vec<UpdateVectorPoint>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateVectorPoint {
    pub id: serde_json::Value,
    pub vector: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdatePayloadRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<FilterExpression>,
    pub payload: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClearPayloadRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<FilterExpression>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeleteVectorRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<FilterExpression>,
    pub vector: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CountRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<FilterExpression>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exact: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateCollectionRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vectors: Option<serde_json::Map<String, serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sparse_vectors: Option<serde_json::Map<String, serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hnsw_config: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub optimizers_config: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantization_config: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vectors_config: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_number: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sharding_method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_keys: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateIndexRequest {
    pub field_name: String,
    pub field_schema: String,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateShardKeyRequest {
    pub shard_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shards_number: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replication_factor: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DropShardKeyRequest {
    pub shard_key: String,
}
