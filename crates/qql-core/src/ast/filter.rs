use super::{PointId, Value};
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum ComparisonOp {
    Eq,
    Gt,
    Gte,
    Lt,
    Lte,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum PointIdPredicate {
    Eq(PointId),
    In(Vec<PointId>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct GeoPoint {
    pub lat: f64,
    pub lon: f64,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum FilterExpr {
    PointId(PointIdPredicate),
    Compare {
        field: String,
        op: ComparisonOp,
        value: Value,
    },
    Between {
        field: String,
        low: Value,
        high: Value,
    },
    In {
        field: String,
        values: Vec<Value>,
    },
    IsNull {
        field: String,
    },
    IsEmpty {
        field: String,
    },
    MatchText {
        field: String,
        text: String,
    },
    MatchAny {
        field: String,
        values: Vec<Value>,
    },
    MatchPhrase {
        field: String,
        text: String,
    },
    And {
        operands: Vec<FilterExpr>,
    },
    Or {
        operands: Vec<FilterExpr>,
    },
    Not {
        operand: Box<FilterExpr>,
    },
    Nested {
        path: String,
        filter: Box<FilterExpr>,
    },
    HasVector {
        name: String,
    },
    ValuesCount {
        field: String,
        op: ComparisonOp,
        count: u64,
    },
    GeoBoundingBox {
        field: String,
        top_left: GeoPoint,
        bottom_right: GeoPoint,
    },
    GeoRadius {
        field: String,
        center: GeoPoint,
        radius: f64,
    },
}
