use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

pub use crate::semantic::{
    PlanFormula, PlanPointId, PlanPointVectors, PlanQueryInput, PlanVectorValue,
};
pub use qql_core::ast::{MemoryPlacement, VectorDatatype};

// ── Method ──────────────────────────────────────────────────────

/// HTTP verb of a projected REST route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// HTTP `GET`.
    Get,
    /// HTTP `POST`.
    Post,
    /// HTTP `PUT`.
    Put,
    /// HTTP `PATCH`.
    Patch,
    /// HTTP `DELETE`.
    Delete,
}

impl Method {
    /// Uppercase verb string, e.g. `"POST"`.
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

/// Filter as carried on the wire: a single condition or a must/should compound.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FilterExpression {
    /// One bare condition (Qdrant accepts a bare `Condition` as a `Filter`).
    Single(Box<FilterClause>),
    /// Full `Filter` object with `must` / `should` / `must_not` lists.
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
    /// Clauses that must all match.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub must: Vec<FilterClause>,
    /// Clauses that must all fail.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub must_not: Vec<FilterClause>,
    /// Clauses of which at least one should match.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub should: Vec<FilterClause>,
    /// At-least-N `should` threshold, when set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_should: Option<usize>,
}

/// Wraps multiple `QueryRequest`s for Qdrant's `/points/query/batch` endpoint.
/// All queries must target the same collection.
#[derive(Debug, Clone, Serialize)]
pub struct QueryBatchRequest {
    /// Lowered request bodies, one per `POST /points/query` call.
    pub searches: Vec<QueryRequest>,
}

/// Wire body for Qdrant's `POST /collections/{c}/points/batch` (mutation batch).
/// Maps to OpenAPI `UpdateOperations` / gRPC `UpdateBatchPoints`.
#[derive(Debug, Clone, Serialize)]
pub struct UpdateBatchRequest {
    /// Ordered mutation operations executed in a single backend call.
    pub operations: Vec<UpdateOperation>,
}

/// One entry in `UpdateOperations.operations` — OpenAPI `UpdateOperation`.
/// Each variant is a single-key object matching the wire format exactly.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum UpdateOperation {
    /// `{ "upsert": … }` — insert or overwrite points.
    Upsert {
        /// Points to insert or overwrite.
        upsert: UpsertRequest,
    },
    /// `{ "delete": … }` — remove points by ID or filter.
    Delete {
        /// Selector for the points to remove.
        delete: DeleteRequest,
    },
    /// `{ "set_payload": … }` — merge payload keys.
    SetPayload {
        /// Payload keys to merge into the targeted points.
        set_payload: UpdatePayloadRequest,
    },
    /// `{ "clear_payload": … }` — remove all payload.
    ClearPayload {
        /// Selector for the points whose payload is cleared.
        clear_payload: ClearPayloadRequest,
    },
    /// `{ "update_vectors": … }` — replace point vectors.
    UpdateVectors {
        /// Vector sets to replace.
        update_vectors: UpdateVectorRequest,
    },
    /// `{ "delete_vectors": … }` — remove named vectors.
    DeleteVectors {
        /// Named vectors to remove.
        delete_vectors: DeleteVectorRequest,
    },
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

/// OpenAPI `MinShould`: clause list plus the minimum number that must match.
#[derive(Debug, Clone, Serialize)]
pub struct MinShould {
    /// Candidate clauses.
    pub conditions: Vec<FilterClause>,
    /// Minimum number of clauses required to match.
    pub min_count: u64,
}

/// One filter condition — any OpenAPI `Condition` variant.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FilterClause {
    /// Field-scoped predicates on a payload `key`.
    Field(Box<FieldCondition>),
    /// `{ "is_null": { "key": … } }` — key missing or null.
    IsNull(IsNullCondition),
    /// `{ "is_empty": { "key": … } }` — key missing or empty array.
    IsEmpty(IsEmptyCondition),
    /// `{ "has_id": [...] }` — point-ID membership.
    HasId(HasIdCondition),
    /// `{ "has_vector": "name" }` — named-vector presence.
    HasVector(HasVectorCondition),
    /// `{ "nested": … }` — filter over an array of objects.
    Nested(NestedCondition),
    /// Recursive sub-filter.
    Filter(Box<FilterCompound>),
    /// Deterministic id-space slice.
    Slice(SliceCondition),
}

/// Deterministic slice of the id space (`hash(id) % total == index`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SliceCondition {
    /// Slice parameters (`total`, `index`).
    pub slice: SliceParams,
}

/// Parameters for a deterministic id-space slice.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SliceParams {
    /// Total number of slices.
    pub total: u64,
    /// Zero-based index of this slice.
    pub index: u64,
}

/// Field-scoped condition on a payload `key`: match, range, geo, or count.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldCondition {
    /// Payload key the condition applies to.
    pub key: String,
    /// Exact/text/any/except/phrase/prefix predicate (`match`).
    #[serde(rename = "match", skip_serializing_if = "Option::is_none")]
    pub r#match: Option<MatchValue>,
    /// Numeric bounds comparison (`range`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<RangeParams>,
    /// Rectangle containment check (`geo_bounding_box`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geo_bounding_box: Option<GeoBoundingBox>,
    /// Center-plus-radius containment check (`geo_radius`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geo_radius: Option<GeoRadius>,
    /// Polygon containment check (`geo_polygon`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geo_polygon: Option<GeoPolygon>,
    /// Value-count bounds check (`values_count`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub values_count: Option<ValuesCountParams>,
    /// Set when the key is missing or an empty array (`is_empty`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_empty: Option<bool>,
    /// Set when the key is missing or null (`is_null`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_null: Option<bool>,
}

/// OpenAPI `ValuesCount`: bounds on the number of values under a `key`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValuesCountParams {
    /// Count strictly below this value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lt: Option<u64>,
    /// Count strictly above this value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gt: Option<u64>,
    /// Count at or above this value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gte: Option<u64>,
    /// Count at or below this value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lte: Option<u64>,
}

/// OpenAPI `Match` variants: exact value, text forms, any-of, or exclusion.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MatchValue {
    /// Exact match against any JSON value (`{"value": …}`).
    Value {
        /// Exact JSON value to match.
        value: serde_json::Value,
    },
    /// Full-text match on an indexed text field (`{"text": …}`).
    Text {
        /// Query text for the full-text index.
        text: String,
    },
    /// Text-any match on a text field (`{"text": …}`; gRPC `TextAny` variant).
    TextAny {
        /// Query text for the text-any index.
        text: String,
    },
    /// Match any of the listed values (`{"any": [...]}`).
    Any {
        /// Accepted values (at least one must match).
        any: Vec<serde_json::Value>,
    },
    /// Match none of the listed values (`{"except": [...]}`).
    Except {
        /// Rejected values (none may match).
        except: Vec<serde_json::Value>,
    },
    /// Exact phrase match on a text field (`{"phrase": …}`).
    Phrase {
        /// Exact phrase to match.
        phrase: String,
    },
    /// Token-prefix match on a text field (`{"prefix": …}`).
    Prefix {
        /// Token prefix to match.
        prefix: String,
    },
}

/// OpenAPI `Range`: numeric bounds for a field (`gt`/`gte`/`lt`/`lte`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RangeParams {
    /// Strictly greater than.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gt: Option<serde_json::Value>,
    /// Greater than or equal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gte: Option<serde_json::Value>,
    /// Strictly less than.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lt: Option<serde_json::Value>,
    /// Less than or equal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lte: Option<serde_json::Value>,
}

/// Qdrant `geo_bounding_box` condition: rectangle by opposite corners.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoBoundingBox {
    /// Top-left corner of the rectangle.
    pub top_left: GeoPoint,
    /// Bottom-right corner of the rectangle.
    pub bottom_right: GeoPoint,
}

/// Qdrant `geo_radius` filter condition: center point plus meter radius.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoRadius {
    /// Center of the circle.
    pub center: GeoPoint,
    /// Radius in meters.
    pub radius: f64,
}

/// Qdrant `geo_polygon` condition: exterior ring plus optional holes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoPolygon {
    /// Outer boundary of the polygon.
    pub exterior: GeoLineString,
    /// Interior holes excluded from the polygon.
    pub interiors: Vec<GeoLineString>,
}

/// Ordered `GeoPoint` ring forming a polygon boundary or hole.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoLineString {
    /// Vertices of the ring, in order.
    pub points: Vec<GeoPoint>,
}

/// Wire geographic coordinate (`{"lat": .., "lon": ..}`) in degrees.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoPoint {
    /// Latitude in degrees.
    pub lat: f64,
    /// Longitude in degrees.
    pub lon: f64,
}

/// `{ "is_null": { "key": … } }` — key missing or holding a null value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IsNullCondition {
    /// Key-only wrapper for the check.
    pub is_null: KeyOnly,
}

/// `{ "is_empty": { "key": … } }` — key missing or holding an empty array.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IsEmptyCondition {
    /// Key-only wrapper for the check.
    pub is_empty: KeyOnly,
}

/// Condition wrapper carrying only the payload `key`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyOnly {
    /// Payload key the condition applies to.
    pub key: String,
}

/// `{ "has_id": [...] }` — point-ID membership check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HasIdCondition {
    /// Point IDs a matched point must belong to.
    pub has_id: Vec<crate::semantic::PlanPointId>,
}

/// `{ "has_vector": "name" }` — named-vector presence check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HasVectorCondition {
    /// Vector name a matched point must carry.
    pub has_vector: String,
}

/// `{ "nested": … }` — filter applied over an array of objects.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NestedCondition {
    /// Parameters of the nested check.
    pub nested: NestedParams,
}

/// Nested-filter parameters: object-array `key` plus the applied filter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NestedParams {
    /// Payload key holding the array of objects.
    pub key: String,
    /// Filter applied to each nested object.
    pub filter: Box<FilterExpression>,
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
    pub shard_key: Option<String>,
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
    pub shard_key: Option<String>,
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

/// Body for `POST /collections/{c}/points`: retrieve points by ID.
#[derive(Debug, Clone, Serialize)]
pub struct PointsRequest {
    /// Point IDs to fetch.
    pub ids: Vec<PlanPointId>,
    /// Payload selection for returned points.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_payload: Option<PayloadSelectorReq>,
    /// Vector selection for returned points.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_vector: Option<VectorSelectorReq>,
    /// Cluster shard routing for custom-sharded collections.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_key: Option<String>,
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

// ── Scroll ─────────────────────────────────────────────────────

/// Body for `POST /collections/{c}/points/scroll` (keyset pagination).
#[derive(Debug, Clone, Serialize)]
pub struct ScrollRequest {
    /// Filter applied before paging.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<FilterExpression>,
    /// Point ID to resume from (`offset`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<PlanPointId>,
    /// Maximum points returned per page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
    /// Payload selection for returned points.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_payload: Option<PayloadSelectorReq>,
    /// Vector selection for returned points.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_vector: Option<VectorSelectorReq>,
    /// Payload-key ordering instead of ID order.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_by: Option<OrderByQuery>,
    /// Cluster shard routing for custom-sharded collections.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_key: Option<String>,
}

// ── Mutations ──────────────────────────────────────────────────

/// Body for `PUT /collections/{c}/points` (upsert points).
#[derive(Debug, Clone, Serialize)]
pub struct UpsertRequest {
    /// Points to insert or overwrite.
    pub points: Vec<UpsertPointRequest>,
    /// Cluster shard routing for custom-sharded collections.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_key: Option<String>,
}

/// One point in an upsert: ID plus optional vectors and payload.
#[derive(Debug, Clone, Serialize)]
pub struct UpsertPointRequest {
    /// Point ID.
    pub id: PlanPointId,
    /// Vectors to write, unnamed or keyed by vector name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vector: Option<PlanPointVectors>,
    /// Payload object stored with the point.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Map<String, serde_json::Value>>,
}

/// Body for `POST /collections/{c}/points/delete`.
#[derive(Debug, Clone, Serialize)]
pub struct DeleteRequest {
    /// Explicit point IDs to delete (mutually exclusive with `filter`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<PlanPointId>>,
    /// Filter selecting the points to delete.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<FilterExpression>,
    /// Cluster shard routing for custom-sharded collections.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_key: Option<String>,
}

/// Body for `PUT /collections/{c}/points/vectors` (replace point vectors).
#[derive(Debug, Clone, Serialize)]
pub struct UpdateVectorRequest {
    /// Points with the vectors to replace.
    pub points: Vec<UpdateVectorPoint>,
    /// Cluster shard routing for custom-sharded collections.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_key: Option<String>,
}

/// One point in a vector update.
#[derive(Debug, Clone, Serialize)]
pub struct UpdateVectorPoint {
    /// Point ID.
    pub id: PlanPointId,
    /// Replacement vectors, unnamed or keyed by vector name.
    pub vector: PlanPointVectors,
}

/// Body for `POST /collections/{c}/points/payload` (set payload keys).
#[derive(Debug, Clone, Serialize)]
pub struct UpdatePayloadRequest {
    /// Explicit point IDs (mutually exclusive with `filter`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<PlanPointId>>,
    /// Filter selecting the points to update.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<FilterExpression>,
    /// Payload keys to set on the selected points.
    pub payload: serde_json::Map<String, serde_json::Value>,
    /// Cluster shard routing for custom-sharded collections.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_key: Option<String>,
}

/// Body for `POST /collections/{c}/points/payload/clear` (drop all payload).
#[derive(Debug, Clone, Serialize)]
pub struct ClearPayloadRequest {
    /// Explicit point IDs (mutually exclusive with `filter`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<PlanPointId>>,
    /// Filter selecting the points to clear.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<FilterExpression>,
    /// Cluster shard routing for custom-sharded collections.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_key: Option<String>,
}

/// Body for `POST /collections/{c}/points/payload/delete` (remove keys).
#[derive(Debug, Clone, Serialize)]
pub struct DeletePayloadRequest {
    /// Payload keys to delete.
    pub keys: Vec<String>,
    /// Explicit point IDs (mutually exclusive with `filter`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<PlanPointId>>,
    /// Filter selecting the points to update.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<FilterExpression>,
    /// Cluster shard routing for custom-sharded collections.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_key: Option<String>,
}

/// Body for `POST /collections/{c}/points/vectors/delete` (remove vectors).
#[derive(Debug, Clone, Serialize)]
pub struct DeleteVectorRequest {
    /// Explicit point IDs (mutually exclusive with `filter`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<PlanPointId>>,
    /// Filter selecting the points to update.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<FilterExpression>,
    /// Named vectors to remove from the selected points.
    pub vector: Vec<String>,
    /// Cluster shard routing for custom-sharded collections.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_key: Option<String>,
}

/// Body for `POST /collections/{c}/points/count`.
#[derive(Debug, Clone, Serialize)]
pub struct CountRequest {
    /// Filter narrowing the counted points.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<FilterExpression>,
    /// Cluster shard routing for custom-sharded collections.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_key: Option<String>,
    /// Exact count instead of a faster estimate when `true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exact: Option<bool>,
}

/// Request payload for Qdrant's `/collections/{collection}/facet` endpoint.
#[derive(Debug, Clone, Serialize)]
pub struct FacetRequest {
    /// Payload key to facet on.
    pub key: String,
    /// Maximum number of facet hits to return.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    /// Filter expression narrowing points considered for faceting.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<FilterExpression>,
    /// Whether to return exact counts across shards.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exact: Option<bool>,
    /// Shard key for custom tenant routing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_key: Option<String>,
}

/// HNSW index configuration for collection creation/update.
#[derive(Debug, Clone, Serialize)]
pub struct HnswConfig {
    /// Edges per node (`m`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub m: Option<u64>,
    /// Candidate list size while building (`ef_construct`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ef_construct: Option<u64>,
    /// Brute-force fallback below this point count (`full_scan_threshold`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_scan_threshold: Option<u64>,
    /// Indexing thread cap (`max_indexing_threads`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_indexing_threads: Option<u64>,
    /// Store the HNSW graph on disk (`on_disk`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_disk: Option<bool>,
    /// Edges per node for payload-aware indexes (`payload_m`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload_m: Option<u64>,
    /// Keep the graph inline with vector storage (`inline_storage`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inline_storage: Option<bool>,
    /// Memory placement of the HNSW graph.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemoryPlacement>,
}

/// Segment optimizer configuration for collection creation/update.
#[derive(Debug, Clone, Serialize)]
pub struct OptimizersConfig {
    /// Deleted-vector ratio that triggers segment merges (`deleted_threshold`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_threshold: Option<f64>,
    /// Minimum segment size for vacuuming (`vacuum_min_vector_number`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vacuum_min_vector_number: Option<u64>,
    /// Target segment count (`default_segment_number`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_segment_number: Option<u64>,
    /// Maximum segment size in bytes (`max_segment_size`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_segment_size: Option<u64>,
    /// Point count above which segments are memmaped (`memmap_threshold`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memmap_threshold: Option<u64>,
    /// Minimum points before indexing kicks in (`indexing_threshold`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indexing_threshold: Option<u64>,
    /// Background flush interval in seconds (`flush_interval_sec`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flush_interval_sec: Option<u64>,
    /// Either a `u64` number or the string `"auto"` (REST-only; gRPC ignores "auto").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_optimization_threads: Option<serde_json::Value>,
    /// Reject queries over unoptimized segments (`prevent_unoptimized`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prevent_unoptimized: Option<bool>,
}

/// Vector quantization config (scalar/product/binary/turbo).
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum QuantizationConfig {
    /// Scalar (int8) quantization (`{"scalar": …}`).
    Scalar {
        /// Scalar quantization parameters.
        scalar: ScalarQuantization,
    },
    /// Product quantization (`{"product": …}`).
    Product {
        /// Product quantization parameters.
        product: ProductQuantization,
    },
    /// Binary quantization (`{"binary": …}`).
    Binary {
        /// Binary quantization parameters.
        binary: BinaryQuantization,
    },
    /// OpenAPI `TurboQuantization`: `{ "turbo": { "bits": "bits2", … } }`.
    Turbo {
        /// Turbo quantization parameters.
        turbo: TurboQuantization,
    },
}

/// OpenAPI scalar quantization config (type `int8`).
#[derive(Debug, Clone, Serialize)]
pub struct ScalarQuantization {
    /// Qdrant REST/OpenAPI expects `"int8"` for scalar quantization type.
    #[serde(rename = "type")]
    pub qtype: String,
    /// Calibration quantile, e.g. `0.99`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantile: Option<f64>,
    /// Keep quantized vectors in RAM.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub always_ram: Option<bool>,
    /// Memory placement of quantized vectors.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemoryPlacement>,
}

/// OpenAPI product quantization config.
#[derive(Debug, Clone, Serialize)]
pub struct ProductQuantization {
    /// Compression ratio: `x4`, `x8`, `x16`, or `x32`.
    pub compression: String,
    /// Keep quantized vectors in RAM.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub always_ram: Option<bool>,
    /// Memory placement of quantized vectors.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemoryPlacement>,
}

/// OpenAPI binary quantization config.
#[derive(Debug, Clone, Serialize)]
pub struct BinaryQuantization {
    /// Keep quantized vectors in RAM.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub always_ram: Option<bool>,
    /// Bit packing: `one_bit`, `two_bits`, or `one_and_half_bits`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoding: Option<String>,
    /// Query-side encoding: `default`, `binary`, `scalar4bits`, `scalar8bits`.
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
    /// Keep quantized vectors in RAM.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub always_ram: Option<bool>,
    /// Memory placement of quantized vectors.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemoryPlacement>,
}

/// Plan IR for `CREATE COLLECTION`; projected to the OpenAPI body at the edge.
#[derive(Debug, Clone, Serialize)]
pub struct CreateCollectionRequest {
    /// Named dense vector configs (`size`, `distance`, per-vector options).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vectors: Option<serde_json::Map<String, serde_json::Value>>,
    /// Named sparse vector configs (`modifier`, optional `index` settings).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sparse_vectors: Option<serde_json::Map<String, serde_json::Value>>,
    /// Collection-wide HNSW settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hnsw_config: Option<HnswConfig>,
    /// Collection-wide optimizer settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub optimizers_config: Option<OptimizersConfig>,
    /// Collection params (`replication_factor`, `read_fan_out_*`, `payload`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
    /// Vector quantization settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantization_config: Option<QuantizationConfig>,
    /// Flat `vectors_config` (`on_disk`/`memory`/`datatype`) from `WITH VECTOR`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vectors_config: Option<serde_json::Value>,
    /// Number of shards (`shard_number`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_number: Option<u64>,
    /// `"auto"` or `"custom"` sharding method.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sharding_method: Option<String>,
    /// Custom shard keys created via `/shards` after collection create.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_keys: Option<Vec<String>>,
    /// OpenAPI `PayloadStorageParams`: `{"memory": "cold"|"cached"}`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
}

/// Plan IR for `ALTER COLLECTION`; projected to the OpenAPI PATCH body.
#[derive(Debug, Clone, Serialize)]
pub struct UpdateCollectionRequest {
    /// Updated optimizer settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub optimizers_config: Option<OptimizersConfig>,
    /// Updated collection params.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
    /// Updated HNSW settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hnsw_config: Option<HnswConfig>,
    /// PATCH envelope for update (`{disabled, quantization_config}`) — JSON.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantization_config: Option<serde_json::Value>,
}

/// Plan IR for `CREATE INDEX`; extra options stay flattened for gRPC.
#[derive(Debug, Clone, Serialize)]
pub struct CreateIndexRequest {
    /// Payload field to index.
    pub field_name: String,
    /// Schema type: `keyword`, `integer`, `float`, `text`, `bool`, …
    pub field_schema: String,
    /// Extra index options flattened onto the request (tokenizer, …).
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Plan IR for creating a custom shard key on a collection.
#[derive(Debug, Clone, Serialize)]
pub struct CreateShardKeyRequest {
    /// Custom shard key to create.
    pub shard_key: String,
    /// Number of shards backing the key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shards_number: Option<u64>,
    /// Replication factor for the key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replication_factor: Option<u64>,
}

/// Plan IR for dropping a custom shard key from a collection.
#[derive(Debug, Clone, Serialize)]
pub struct DropShardKeyRequest {
    /// Custom shard key to remove.
    pub shard_key: String,
}

/// Cluster-wide resource quota configuration (`PUT /quotas`).
#[derive(Debug, Clone, Serialize)]
pub struct SetQuotaRequest {
    /// Whether quota enforcement is active.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Resident-memory cap as a percent of total (1-100).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_resident_memory_percent: Option<u64>,
    /// Disk-usage cap as a percent of total (1-100).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_disk_usage_percent: Option<u64>,
    /// Margin reclaimed when a cap trips, as a percent (0-100).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_margin_percent: Option<u64>,
    /// REST query param (`?wait=`), not body.
    #[serde(skip)]
    pub wait: Option<bool>,
}
