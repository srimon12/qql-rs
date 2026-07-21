use alloc::string::String;
use alloc::vec::Vec;
use serde::Serialize;

// ── Route ──────────────────────────────────────────────────────

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

#[derive(Debug, Clone, PartialEq)]
pub struct Route<T> {
    pub method: Method,
    pub path: String,
    pub query: Vec<(String, String)>,
    pub body: Option<T>,
}

impl<T: Serialize> Route<T> {
    pub fn body_json(&self) -> Option<serde_json::Value> {
        self.body.as_ref().map(|b| serde_json::to_value(b).unwrap())
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
    Single(FilterClause),
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
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum FilterClause {
    Field(FieldCondition),
    IsNull(IsNullCondition),
    IsEmpty(IsEmptyCondition),
    HasId(HasIdCondition),
    HasVector(HasVectorCondition),
    Nested(NestedCondition),
    ValuesCount(ValuesCountCondition),
}

#[derive(Debug, Clone, Serialize)]
pub struct FieldCondition {
    pub key: String,
    #[serde(flatten)]
    pub condition: FieldMatch,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum FieldMatch {
    Match(MatchCondition),
    Range(RangeCondition),
    GeoBoundingBox(GeoBoundingBoxCondition),
    GeoRadius(GeoRadiusCondition),
}

#[derive(Debug, Clone, Serialize)]
pub struct MatchCondition {
    #[serde(rename = "match")]
    pub match_: MatchValue,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum MatchValue {
    Value { value: serde_json::Value },
    Text { text: String },
    Any { any: Vec<serde_json::Value> },
}

#[derive(Debug, Clone, Serialize)]
pub struct RangeCondition {
    pub range: RangeParams,
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
pub struct GeoBoundingBoxCondition {
    pub geo_bounding_box: GeoBoundingBox,
}

#[derive(Debug, Clone, Serialize)]
pub struct GeoBoundingBox {
    pub top_left: GeoPoint,
    pub bottom_right: GeoPoint,
}

#[derive(Debug, Clone, Serialize)]
pub struct GeoRadiusCondition {
    pub geo_radius: GeoRadius,
}

#[derive(Debug, Clone, Serialize)]
pub struct GeoRadius {
    pub center: GeoPoint,
    pub radius: f64,
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

#[derive(Debug, Clone, Serialize)]
pub struct ValuesCountCondition {
    pub key: String,
    #[serde(rename = "values_count")]
    pub values_count: RangeParams,
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
    pub group_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_request: Option<GroupRequest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_payload: Option<PayloadSelectorReq>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_vector: Option<VectorSelectorReq>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GroupRequest {
    pub with_lookup: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchParamsRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hnsw_ef: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exact: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acorn: Option<bool>,
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
    Recommend { recommend: RecommendQuery },
    Context { context: Vec<ContextPair> },
    Discover { discover: DiscoverQuery },
    OrderBy { order_by: OrderByQuery },
    Sample { sample: String },
    Fusion { fusion: String },
    Formula { formula: serde_json::Value },
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
    pub filter: Option<FilterExpression>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score_threshold: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lookup: Option<LookupRequest>,
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
    pub limit: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_payload: Option<PayloadSelectorReq>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_vector: Option<VectorSelectorReq>,
}

// ── Mutations ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct UpsertRequest {
    pub points: Vec<UpsertPointRequest>,
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

// ── DDL ────────────────────────────────────────────────────────

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
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateIndexRequest {
    pub field_name: String,
    pub field_schema: String,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}
