use super::Value;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum FilterExpr {
    Compare {
        field: String,
        op: String,
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
    NotIn {
        field: String,
        values: Vec<Value>,
    },
    IsNull {
        field: String,
    },
    IsNotNull {
        field: String,
    },
    IsEmpty {
        field: String,
    },
    IsNotEmpty {
        field: String,
    },
    MatchText {
        field: String,
        text: String,
    },
    MatchAny {
        field: String,
        text: String,
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
        op: String,
        count: i64,
    },
    GeoBoundingBox {
        field: String,
        top_left_lat: f64,
        top_left_lon: f64,
        bottom_right_lat: f64,
        bottom_right_lon: f64,
    },
    GeoRadius {
        field: String,
        lat: f64,
        lon: f64,
        radius: f64,
    },
}
