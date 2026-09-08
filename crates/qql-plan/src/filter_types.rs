//! Filter IR types matching OpenAPI `Filter` / `Condition`.

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

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
