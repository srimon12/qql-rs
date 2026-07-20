use super::{FilterExpr, Value};
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum FormulaExpr {
    Constant {
        value: f64,
    },
    Variable {
        name: String,
    },
    Sum {
        left: Box<FormulaExpr>,
        right: Box<FormulaExpr>,
    },
    Sub {
        left: Box<FormulaExpr>,
        right: Box<FormulaExpr>,
    },
    Mul {
        left: Box<FormulaExpr>,
        right: Box<FormulaExpr>,
    },
    Div {
        left: Box<FormulaExpr>,
        right: Box<FormulaExpr>,
        by_zero_default: Option<f64>,
    },
    Neg {
        operand: Box<FormulaExpr>,
    },
    Abs {
        x: Box<FormulaExpr>,
    },
    Sqrt {
        x: Box<FormulaExpr>,
    },
    Log {
        x: Box<FormulaExpr>,
    },
    Ln {
        x: Box<FormulaExpr>,
    },
    Exp {
        x: Box<FormulaExpr>,
    },
    Pow {
        base: Box<FormulaExpr>,
        exponent: Box<FormulaExpr>,
    },
    GeoDistance {
        lat: f64,
        lon: f64,
        field: String,
    },
    Decay {
        kind: String,
        x: Box<FormulaExpr>,
        target: Option<Box<FormulaExpr>>,
        scale: Option<f64>,
        midpoint: Option<f64>,
    },
    Case {
        cond: Box<FilterExpr>,
        then_: Box<FormulaExpr>,
        else_: Box<FormulaExpr>,
    },
    MatchCondition {
        field: String,
        values: Vec<Value>,
    },
    Datetime {
        value: String,
    },
    DatetimeKey {
        key: String,
    },
}
