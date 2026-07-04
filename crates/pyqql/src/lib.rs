use std::borrow::Cow;

use pyo3::exceptions::PySyntaxError;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use qql_core::ast::{self, Value};
use qql_core::lexer::Lexer;
use qql_core::parser::Parser;

#[pyfunction]
fn parse(input: &str) -> PyResult<String> {
    match Parser::parse(input) {
        Ok(stmt) => Ok(format!("{:#?}", stmt)),
        Err(e) => Err(PySyntaxError::new_err(e.to_string())),
    }
}

#[pyfunction]
fn is_valid(input: &str) -> bool {
    Parser::parse(input).is_ok()
}

#[pyfunction]
fn inject_filter(query: &str, field: &str, op: &str, value_json: &str) -> PyResult<String> {
    let value = json_to_value(value_json)
        .ok_or_else(|| PySyntaxError::new_err("invalid value JSON"))?;
    let mut stmt = Parser::parse(query)
        .map_err(|e| PySyntaxError::new_err(e.to_string()))?;
    ast::inject_filter(&mut stmt, field, op, &value);
    Ok(format!("{:#?}", stmt))
}

#[pyfunction]
fn tokenize<'py>(input: &str, py: Python<'py>) -> PyResult<Vec<Bound<'py, PyDict>>> {
    let lexer = Lexer::new(input);
    let mut result = Vec::new();
    for token_result in lexer {
        let token = token_result.map_err(|e| PySyntaxError::new_err(e.to_string()))?;
        let d = PyDict::new(py);
        d.set_item("kind", token.kind.as_str())?;
        d.set_item("text", token.text)?;
        d.set_item("pos", token.pos as i64)?;
        result.push(d);
    }
    Ok(result)
}

#[pymodule]
fn pyqql(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(parse, m)?)?;
    m.add_function(wrap_pyfunction!(is_valid, m)?)?;
    m.add_function(wrap_pyfunction!(inject_filter, m)?)?;
    m.add_function(wrap_pyfunction!(tokenize, m)?)?;
    Ok(())
}

fn json_to_value(json: &str) -> Option<Value<'static>> {
    let jv: serde_json::Value = serde_json::from_str(json).ok()?;
    serde_json_to_value(jv)
}

fn serde_json_to_value(jv: serde_json::Value) -> Option<Value<'static>> {
    match jv {
        serde_json::Value::String(s) => Some(Value::Str(Cow::Owned(s))),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(Value::Int(i))
            } else if let Some(f) = n.as_f64() {
                Some(Value::Float(f))
            } else {
                None
            }
        }
        serde_json::Value::Bool(b) => Some(Value::Bool(b)),
        serde_json::Value::Null => Some(Value::Null),
        serde_json::Value::Array(items) => {
            let mut vals = Vec::with_capacity(items.len());
            for item in items {
                vals.push(serde_json_to_value(item)?);
            }
            Some(Value::List(vals))
        }
        serde_json::Value::Object(map) => {
            let mut pairs = Vec::with_capacity(map.len());
            for (k, v) in map {
                pairs.push((Cow::Owned(k), serde_json_to_value(v)?));
            }
            Some(Value::Dict(pairs))
        }
    }
}
