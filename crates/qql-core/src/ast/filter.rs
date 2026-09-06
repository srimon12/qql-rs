use super::{PointId, Value};
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

/// Scalar comparison operator for `field <op> value` and `VALUES_COUNT` predicates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ComparisonOp {
    /// `=` — equal.
    Eq,
    /// `>` — strictly greater than.
    Gt,
    /// `>=` — greater than or equal.
    Gte,
    /// `<` — strictly less than.
    Lt,
    /// `<=` — less than or equal.
    Lte,
}

impl ComparisonOp {
    /// Parse a host-language comparison operator string for
    /// [`inject_filter`](crate::ast::inject_filter) (`=`, `==`, `eq`, `>`,
    /// `gt`, …). `!=` / `neq` / `<>` are rejected with guidance to inject
    /// equality and wrap with `NOT`; the single source of this contract lives
    /// here so every binding surfaces the same message and code.
    pub fn parse_inject_op(op: &str) -> Result<Self, crate::error::QqlError> {
        match op {
            "=" | "==" | "eq" => Ok(Self::Eq),
            ">" | "gt" => Ok(Self::Gt),
            ">=" | "gte" => Ok(Self::Gte),
            "<" | "lt" => Ok(Self::Lt),
            "<=" | "lte" => Ok(Self::Lte),
            "!=" | "neq" | "<>" => Err(crate::error::QqlError::validation(
                "QQL-VALIDATION-FILTER-INJECT",
                "inject_filter does not support '!='; inject equality and wrap with NOT, or rewrite the query",
                None,
            )),
            other => Err(crate::error::QqlError::validation(
                "QQL-VALIDATION-FILTER-INJECT",
                alloc::format!("unsupported comparison operator '{other}' (use =, >, >=, <, <=)"),
                None,
            )),
        }
    }
}

/// Point-ID predicate of a `PointId` filter clause.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PointIdPredicate {
    /// `id = <point-id>` — exact single-ID match.
    Eq(PointId),
    /// `id IN (…)` — match any of the listed point IDs.
    In(Vec<PointId>),
}

/// Geographic coordinate, WGS-84 decimal degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GeoPoint {
    /// Latitude in degrees, `-90.0..=90.0`.
    pub lat: f64,
    /// Longitude in degrees, `-180.0..=180.0`.
    pub lon: f64,
}

/// `WHERE` predicate tree, mirroring Qdrant's `Filter` schema.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum FilterExpr {
    /// `id = …` / `id IN (…)` point-ID predicate.
    PointId(PointIdPredicate),
    /// `field <op> value` scalar comparison.
    Compare {
        /// Payload field path.
        field: String,
        /// Comparison operator.
        op: ComparisonOp,
        /// Right-hand literal value.
        value: Value,
    },
    /// `field BETWEEN low AND high` — inclusive range test.
    Between {
        /// Payload field path.
        field: String,
        /// Inclusive lower bound.
        low: Value,
        /// Inclusive upper bound.
        high: Value,
    },
    /// `field IN (…)` — match any of the listed values.
    In {
        /// Payload field path.
        field: String,
        /// Accepted literal values.
        values: Vec<Value>,
    },
    /// `field IS NULL` — field missing or holding `null`.
    IsNull {
        /// Payload field path.
        field: String,
    },
    /// `field IS EMPTY` — field present but with no values.
    IsEmpty {
        /// Payload field path.
        field: String,
    },
    /// `field MATCH 'text'` — full-text match.
    MatchText {
        /// Payload field path.
        field: String,
        /// Text to match.
        text: String,
    },
    /// `field MATCH ANY (…)` — match any of the listed values.
    MatchAny {
        /// Payload field path.
        field: String,
        /// Accepted literal values.
        values: Vec<Value>,
    },
    /// `field MATCH PHRASE 'text'` — exact phrase (ordered tokens) match.
    MatchPhrase {
        /// Payload field path.
        field: String,
        /// Phrase to match.
        text: String,
    },
    /// `field MATCH PREFIX 'prefix'` — prefix match (keyword `prefix` index).
    MatchPrefix {
        /// Payload field path.
        field: String,
        /// Required value prefix.
        prefix: String,
    },
    /// `… AND …` — all operands must hold.
    And {
        /// Conjoined sub-filters.
        operands: Vec<FilterExpr>,
    },
    /// `… OR …` — at least one operand must hold.
    Or {
        /// Disjoined sub-filters.
        operands: Vec<FilterExpr>,
    },
    /// `NOT …` — logical negation.
    Not {
        /// Negated sub-filter.
        operand: Box<FilterExpr>,
    },
    /// `NESTED 'path', <filter>` — apply a filter to an object path.
    Nested {
        /// Payload object path.
        path: String,
        /// Sub-filter applied under `path`.
        filter: Box<FilterExpr>,
    },
    /// `HAS_VECTOR <name>` — point has the named vector.
    HasVector {
        /// Named vector to check.
        name: String,
    },
    /// Deterministic sampling: a point belongs to slice `index` iff
    /// `hash(id) % total == index`. `total >= 1`, `index < total`.
    Slice {
        /// Total slice count; must be `>= 1`.
        total: u64,
        /// Slice to select; must be `< total`.
        index: u64,
    },
    /// `field VALUES_COUNT <op> n` — compare the number of stored values.
    ValuesCount {
        /// Payload field path.
        field: String,
        /// Comparison applied to the count.
        op: ComparisonOp,
        /// Reference count.
        count: u64,
    },
    /// `field GEO_BBOX (…)` — point inside the lat/lon rectangle.
    GeoBoundingBox {
        /// Payload field path.
        field: String,
        /// North-west corner.
        top_left: GeoPoint,
        /// South-east corner.
        bottom_right: GeoPoint,
    },
    /// `field GEO_RADIUS (…)` — point within a circle.
    GeoRadius {
        /// Payload field path.
        field: String,
        /// Circle center.
        center: GeoPoint,
        /// Radius in meters.
        radius: f64,
    },
    /// `field GEO_POLYGON (…)` — point inside the polygon.
    GeoPolygon {
        /// Payload field path.
        field: String,
        /// Outer ring vertices.
        exterior: Vec<GeoPoint>,
        /// Hole rings cut out of the polygon.
        interiors: Vec<Vec<GeoPoint>>,
    },
}
