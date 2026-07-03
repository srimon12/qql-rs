use super::{FilterExpr, Value};
use alloc::boxed::Box;
use alloc::vec::Vec;

#[derive(Debug, Clone, PartialEq)]
pub enum FormulaExpr<'a> {
    Constant {
        value: f64,
    },
    Variable {
        name: &'a str,
    },
    Sum {
        left: Box<FormulaExpr<'a>>,
        right: Box<FormulaExpr<'a>>,
    },
    Sub {
        left: Box<FormulaExpr<'a>>,
        right: Box<FormulaExpr<'a>>,
    },
    Mul {
        left: Box<FormulaExpr<'a>>,
        right: Box<FormulaExpr<'a>>,
    },
    Div {
        left: Box<FormulaExpr<'a>>,
        right: Box<FormulaExpr<'a>>,
        by_zero_default: Option<f64>,
    },
    Neg {
        operand: Box<FormulaExpr<'a>>,
    },
    Abs {
        x: Box<FormulaExpr<'a>>,
    },
    Sqrt {
        x: Box<FormulaExpr<'a>>,
    },
    Log {
        x: Box<FormulaExpr<'a>>,
    },
    Ln {
        x: Box<FormulaExpr<'a>>,
    },
    Exp {
        x: Box<FormulaExpr<'a>>,
    },
    Pow {
        base: Box<FormulaExpr<'a>>,
        exponent: Box<FormulaExpr<'a>>,
    },
    GeoDistance {
        lat: f64,
        lon: f64,
        field: &'a str,
    },
    Decay {
        kind: &'a str,
        x: Box<FormulaExpr<'a>>,
        target: Option<Box<FormulaExpr<'a>>>,
        scale: Option<f64>,
        midpoint: Option<f64>,
    },
    Case {
        cond: Box<FilterExpr<'a>>,
        then_: Box<FormulaExpr<'a>>,
        else_: Box<FormulaExpr<'a>>,
    },
    MatchCondition {
        field: &'a str,
        values: Vec<Value<'a>>,
    },
    Datetime {
        value: &'a str,
    },
    DatetimeKey {
        key: &'a str,
    },
}
