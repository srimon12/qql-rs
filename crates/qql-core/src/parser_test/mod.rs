// This file is auto-generated during test file split.
mod errors;
mod filters;
mod formulas;
mod inserts_updates_scroll;
mod queries;
mod readme;

use alloc::vec::Vec;

use crate::ast::{Stmt, Value};
use crate::error::QqlError;
use crate::parser::Parser;

pub(crate) fn parse(input: &str) -> Result<Stmt, QqlError> {
    Parser::parse(input)
}

pub(crate) fn assert_parse_ok(input: &str) -> Stmt {
    parse(input).unwrap_or_else(|e| panic!("failed to parse '{}': {}", input, e))
}

pub(crate) fn assert_parse_err(input: &str) {
    assert!(parse(input).is_err(), "expected parse error for: {}", input);
}

pub(crate) fn i64_val(v: i64) -> Value {
    Value::Int(v)
}

pub(crate) fn str_val(s: &'static str) -> Value {
    Value::Str(s.to_string())
}

pub(crate) fn float_val(f: f64) -> Value {
    Value::Float(f)
}

pub(crate) fn make_payload(pairs: &[(String, Value)]) -> Vec<(String, Value)> {
    pairs.to_vec()
}
