use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

pub use crate::semantic::{
    PlanFormula, PlanPointId, PlanPointVectors, PlanQueryInput, PlanVectorValue,
};
pub use qql_core::ast::{MemoryPlacement, VectorDatatype};

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

// ── Filter types ───────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FilterExpression {
    Single(Box<FilterClause>),
    Compound(FilterCompound),
}

/// Transport-neutral filter compound.
///
/// Matches Qdrant `Filter` on **both** protocols:
/// REST `Filter` object and gRPC `qdrant.Filter` — only must/should/must_not
/// (and min_should). Shard routing is **not** a filter field; it lives on the
/// operation request as `shard_key` / gRPC `ShardKeySelector`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterCompound {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub must: Vec<FilterClause>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub must_not: Vec<FilterClause>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub should: Vec<FilterClause>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_should: Option<usize>,
}

/// Wraps multiple `QueryRequest`s for Qdrant's `/points/query/batch` endpoint.
/// All queries must target the same collection.
#[derive(Debug, Clone, Serialize)]
pub struct QueryBatchRequest {
    pub searches: Vec<QueryRequest>,
}

/// Wire body for Qdrant's `POST /collections/{c}/points/batch` (mutation batch).
/// Maps to OpenAPI `UpdateOperations` / gRPC `UpdateBatchPoints`.
#[derive(Debug, Clone, Serialize)]
pub struct UpdateBatchRequest {
    pub operations: Vec<UpdateOperation>,
}

/// One entry in `UpdateOperations.operations` — OpenAPI `UpdateOperation`.
/// Each variant is a single-key object matching the wire format exactly.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum UpdateOperation {
    Upsert { upsert: UpsertRequest },
    Delete { delete: DeleteRequest },
    SetPayload { set_payload: UpdatePayloadRequest },
    ClearPayload { clear_payload: ClearPayloadRequest },
    UpdateVectors { update_vectors: UpdateVectorRequest },
    DeleteVectors { delete_vectors: DeleteVectorRequest },
}

impl UpdateOperation {
    /// Human-readable operation name for executor responses.
    pub fn operation_name(&self) -> &'static str {
        match self {
            UpdateOperation::Upsert { .. } => "UPSERT",
            UpdateOperation::Delete { .. } => "DELETE",
            UpdateOperation::SetPayload { .. } => "UPDATE_PAYLOAD",
            UpdateOperation::ClearPayload { .. } => "CLEAR_PAYLOAD",
            UpdateOperation::UpdateVectors { .. } => "UPDATE_VECTOR",
            UpdateOperation::DeleteVectors { .. } => "DELETE_VECTOR",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MinShould {
    pub conditions: Vec<FilterClause>,
    pub min_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FilterClause {
    Field(Box<FieldCondition>),
    IsNull(IsNullCondition),
    IsEmpty(IsEmptyCondition),
    HasId(HasIdCondition),
    HasVector(HasVectorCondition),
    Nested(NestedCondition),
    Filter(Box<FilterCompound>),
    Slice(SliceCondition),
}

/// Deterministic slice of the id space (`hash(id) % total == index`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SliceCondition {
    pub slice: SliceParams,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SliceParams {
    pub total: u64,
    pub index: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MatchValue {
    Value { value: serde_json::Value },
    Text { text: String },
    TextAny { text: String },
    Any { any: Vec<serde_json::Value> },
    Except { except: Vec<serde_json::Value> },
    Phrase { phrase: String },
    Prefix { prefix: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoBoundingBox {
    pub top_left: GeoPoint,
    pub bottom_right: GeoPoint,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoRadius {
    pub center: GeoPoint,
    pub radius: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoPolygon {
    pub exterior: GeoLineString,
    pub interiors: Vec<GeoLineString>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoLineString {
    pub points: Vec<GeoPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoPoint {
    pub lat: f64,
    pub lon: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IsNullCondition {
    pub is_null: KeyOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IsEmptyCondition {
    pub is_empty: KeyOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyOnly {
    pub key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HasIdCondition {
    pub has_id: Vec<crate::semantic::PlanPointId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HasVectorCondition {
    pub has_vector: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NestedCondition {
    pub nested: NestedParams,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// OpenAPI query param / proto field — not body JSON.
    #[serde(skip)]
    pub timeout: Option<u64>,
    /// OpenAPI query param / proto field — not body JSON.
    #[serde(skip)]
    pub consistency: Option<ReadConsistencyParam>,
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
    #[serde(skip)]
    pub timeout: Option<u64>,
    #[serde(skip)]
    pub consistency: Option<ReadConsistencyParam>,
    #[serde(skip)]
    pub group_offset: Option<u64>,
}

/// Wire form of OpenAPI `ReadConsistency` for REST query strings / gRPC.
#[derive(Debug, Clone, PartialEq)]
pub enum ReadConsistencyParam {
    Factor(u64),
    Majority,
    Quorum,
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

/// ACORN search params. OpenAPI defaults `enable` to false, so enabled state
/// must be serialized explicitly as `{"enable": true}`.
#[derive(Debug, Clone, Serialize)]
pub struct AcornSearchParams {
    pub enable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_selectivity: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchParamsRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hnsw_ef: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exact: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acorn: Option<AcornSearchParams>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indexed_only: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantization: Option<QuantizationSearchRequest>,
    /// Per-query IDF corpus for sparse vectors (OpenAPI `IdfParams`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idf: Option<IdfSearchParams>,
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

/// OpenAPI `IdfParams`: `"global"` scope or a corpus filter over which sparse
/// vector IDF statistics are computed.
#[derive(Debug, Clone)]
pub enum IdfSearchParams {
    Global,
    Corpus { corpus: FilterExpression },
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

#[derive(Debug, Clone, Serialize)]
pub struct NearestQuery {
    pub nearest: PlanQueryInput,
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
    pub formula: PlanFormula,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub defaults: Option<serde_json::Map<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RelevanceFeedbackInput {
    pub target: PlanQueryInput,
    pub feedback: Vec<FeedbackItem>,
    pub strategy: FeedbackStrategy,
}

#[derive(Debug, Clone, Serialize)]
pub struct FeedbackItem {
    pub example: PlanQueryInput,
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
    pub positive: Vec<PlanQueryInput>,
    pub negative: Vec<PlanQueryInput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strategy: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContextPair {
    pub positive: PlanQueryInput,
    pub negative: PlanQueryInput,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiscoverQuery {
    pub target: PlanQueryInput,
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
    pub ids: Vec<PlanPointId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_payload: Option<PayloadSelectorReq>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_vector: Option<VectorSelectorReq>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_key: Option<String>,
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
    pub offset: Option<PlanPointId>,
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
    pub id: PlanPointId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vector: Option<PlanPointVectors>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Map<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeleteRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<PlanPointId>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<FilterExpression>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_key: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateVectorRequest {
    pub points: Vec<UpdateVectorPoint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_key: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateVectorPoint {
    pub id: PlanPointId,
    pub vector: PlanPointVectors,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdatePayloadRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<PlanPointId>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<FilterExpression>,
    pub payload: serde_json::Map<String, serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_key: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClearPayloadRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<PlanPointId>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<FilterExpression>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_key: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeletePayloadRequest {
    pub keys: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<PlanPointId>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<FilterExpression>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_key: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeleteVectorRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<PlanPointId>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<FilterExpression>,
    pub vector: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_key: Option<String>,
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

/// HNSW index configuration for collection creation/update.
#[derive(Debug, Clone, Serialize)]
pub struct HnswConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub m: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ef_construct: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_scan_threshold: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_indexing_threads: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_disk: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload_m: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inline_storage: Option<bool>,
    /// Memory placement of the HNSW graph.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemoryPlacement>,
}

/// Segment optimizer configuration for collection creation/update.
#[derive(Debug, Clone, Serialize)]
pub struct OptimizersConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_threshold: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vacuum_min_vector_number: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_segment_number: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_segment_size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memmap_threshold: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indexing_threshold: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flush_interval_sec: Option<u64>,
    /// Either a `u64` number or the string `"auto"` (REST-only; gRPC ignores "auto").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_optimization_threads: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prevent_unoptimized: Option<bool>,
}

/// Vector quantization config (scalar/product/binary/turbo).
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum QuantizationConfig {
    Scalar {
        scalar: ScalarQuantization,
    },
    Product {
        product: ProductQuantization,
    },
    Binary {
        binary: BinaryQuantization,
    },
    /// OpenAPI `TurboQuantization`: `{ "turbo": { "bits": "bits2", … } }`.
    Turbo {
        turbo: TurboQuantization,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct ScalarQuantization {
    /// Qdrant REST/OpenAPI expects `"int8"` for scalar quantization type.
    #[serde(rename = "type")]
    pub qtype: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantile: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub always_ram: Option<bool>,
    /// Memory placement of quantized vectors.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemoryPlacement>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProductQuantization {
    pub compression: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub always_ram: Option<bool>,
    /// Memory placement of quantized vectors.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemoryPlacement>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BinaryQuantization {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub always_ram: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoding: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query_encoding: Option<String>,
    /// Memory placement of quantized vectors.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemoryPlacement>,
}

/// OpenAPI `TurboQuantQuantizationConfig`.
#[derive(Debug, Clone, Serialize)]
pub struct TurboQuantization {
    /// OpenAPI enum: `bits1` | `bits1_5` | `bits2` | `bits4`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bits: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub always_ram: Option<bool>,
    /// Memory placement of quantized vectors.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemoryPlacement>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateCollectionRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vectors: Option<serde_json::Map<String, serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sparse_vectors: Option<serde_json::Map<String, serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hnsw_config: Option<HnswConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub optimizers_config: Option<OptimizersConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantization_config: Option<QuantizationConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vectors_config: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_number: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sharding_method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_keys: Option<Vec<String>>,
    /// OpenAPI `PayloadStorageParams`: `{"memory": "cold"|"cached"}`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateCollectionRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub optimizers_config: Option<OptimizersConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hnsw_config: Option<HnswConfig>,
    /// PATCH envelope for update (`{disabled, quantization_config}`) — JSON.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantization_config: Option<serde_json::Value>,
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

/// Cluster-wide resource quota configuration (`PUT /quotas`).
#[derive(Debug, Clone, Serialize)]
pub struct SetQuotaRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_resident_memory_percent: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_disk_usage_percent: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_margin_percent: Option<u64>,
    /// REST query param (`?wait=`), not body.
    #[serde(skip)]
    pub wait: Option<bool>,
}
