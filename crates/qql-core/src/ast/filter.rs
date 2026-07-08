use super::Value;
use alloc::boxed::Box;
use alloc::vec::Vec;

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum FilterExpr<'a> {
    Compare {
        field: &'a str,
        op: &'a str,
        value: Value<'a>,
    },
    Between {
        field: &'a str,
        low: Value<'a>,
        high: Value<'a>,
    },
    In {
        field: &'a str,
        values: Vec<Value<'a>>,
    },
    NotIn {
        field: &'a str,
        values: Vec<Value<'a>>,
    },
    IsNull {
        field: &'a str,
    },
    IsNotNull {
        field: &'a str,
    },
    IsEmpty {
        field: &'a str,
    },
    IsNotEmpty {
        field: &'a str,
    },
    MatchText {
        field: &'a str,
        text: &'a str,
    },
    MatchAny {
        field: &'a str,
        text: &'a str,
    },
    MatchPhrase {
        field: &'a str,
        text: &'a str,
    },
    And {
        operands: Vec<FilterExpr<'a>>,
    },
    Or {
        operands: Vec<FilterExpr<'a>>,
    },
    Not {
        operand: Box<FilterExpr<'a>>,
    },
    Nested {
        path: &'a str,
        filter: Box<FilterExpr<'a>>,
    },
    HasVector {
        name: &'a str,
    },
    ValuesCount {
        field: &'a str,
        op: &'a str,
        count: i64,
    },
    GeoBoundingBox {
        field: &'a str,
        top_left_lat: f64,
        top_left_lon: f64,
        bottom_right_lat: f64,
        bottom_right_lon: f64,
    },
    GeoRadius {
        field: &'a str,
        lat: f64,
        lon: f64,
        radius: f64,
    },
}
