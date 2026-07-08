use std::borrow::Cow;

use pyo3::exceptions::PySyntaxError;
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyList};
use qql_core::ast::{self, Value};
use qql_core::lexer::Lexer;
use qql_core::parser::Parser;

#[pyfunction]
fn parse<'py>(py: Python<'py>, input: &str) -> PyResult<Bound<'py, PyAny>> {
    match Parser::parse(input) {
        Ok(stmt) => {
            pythonize::pythonize(py, &stmt).map_err(|e| PySyntaxError::new_err(e.to_string()))
        }
        Err(e) => Err(PySyntaxError::new_err(e.to_string())),
    }
}

#[pyfunction]
fn parse_all<'py>(py: Python<'py>, input: &str) -> PyResult<Bound<'py, PyAny>> {
    match Parser::parse_all(input) {
        Ok(stmts) => {
            pythonize::pythonize(py, &stmts).map_err(|e| PySyntaxError::new_err(e.to_string()))
        }
        Err(e) => Err(PySyntaxError::new_err(e.to_string())),
    }
}

#[pyfunction]
fn parse_batch<'py>(py: Python<'py>, queries: Vec<String>) -> PyResult<Bound<'py, PyAny>> {
    let list = pyo3::types::PyList::empty(py);
    for q in queries {
        match Parser::parse(&q) {
            Ok(stmt) => {
                let obj = pythonize::pythonize(py, &stmt)
                    .map_err(|e| PySyntaxError::new_err(e.to_string()))?;
                list.append(obj)?;
            }
            Err(e) => return Err(PySyntaxError::new_err(e.to_string())),
        }
    }
    Ok(list.into_any())
}

#[pyfunction]
fn is_valid(input: &str) -> bool {
    Parser::try_parse(input).is_ok()
}

#[pyfunction]
fn inject_filter<'py>(
    py: Python<'py>,
    query: &str,
    field: &str,
    op: &str,
    value: &Bound<'_, PyAny>,
) -> PyResult<Bound<'py, PyAny>> {
    let value = py_to_value(value)?;
    let mut stmt = Parser::parse(query).map_err(|e| PySyntaxError::new_err(e.to_string()))?;
    ast::inject_filter(&mut stmt, field, op, &value);
    pythonize::pythonize(py, &stmt).map_err(|e| PySyntaxError::new_err(e.to_string()))
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
fn pyqql(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(parse, m)?)?;
    m.add_function(wrap_pyfunction!(parse_all, m)?)?;
    m.add_function(wrap_pyfunction!(parse_batch, m)?)?;
    m.add_function(wrap_pyfunction!(is_valid, m)?)?;
    m.add_function(wrap_pyfunction!(inject_filter, m)?)?;
    m.add_function(wrap_pyfunction!(tokenize, m)?)?;
    Ok(())
}

fn py_to_value(value: &Bound<'_, PyAny>) -> PyResult<Value<'static>> {
    if value.is_none() {
        return Ok(Value::Null);
    }
    if let Ok(v) = value.extract::<bool>() {
        return Ok(Value::Bool(v));
    }
    if let Ok(v) = value.extract::<i64>() {
        return Ok(Value::Int(v));
    }
    if let Ok(v) = value.extract::<f64>() {
        return Ok(Value::Float(v));
    }
    if let Ok(s) = value.extract::<String>() {
        return Ok(json_to_value(&s).unwrap_or(Value::Str(Cow::Owned(s))));
    }
    if let Ok(list) = value.downcast::<PyList>() {
        let mut items = Vec::with_capacity(list.len());
        for item in list.iter() {
            items.push(py_to_value(&item)?);
        }
        return Ok(Value::List(items));
    }
    if let Ok(dict) = value.downcast::<PyDict>() {
        let mut items = Vec::with_capacity(dict.len());
        for (key, item) in dict.iter() {
            let key = key
                .extract::<String>()
                .map_err(|_| PySyntaxError::new_err("dict keys must be strings"))?;
            items.push((Cow::Owned(key), py_to_value(&item)?));
        }
        return Ok(Value::Dict(items));
    }
    Err(PySyntaxError::new_err("unsupported filter value type"))
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
            } else {
                n.as_f64().map(Value::Float)
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
            if map.len() == 1 {
                if let Some((tag, inner)) = map.iter().next() {
                    match tag.as_str() {
                        "str" => return inner.as_str().map(|s| Value::Str(Cow::Owned(s.into()))),
                        "int" => return inner.as_i64().map(Value::Int),
                        "float" => return inner.as_f64().map(Value::Float),
                        "bool" => return inner.as_bool().map(Value::Bool),
                        "null" if inner.is_null() => return Some(Value::Null),
                        "list" => return serde_json_to_value(inner.clone()),
                        "dict" => return serde_json_to_value(inner.clone()),
                        _ => {}
                    }
                }
            }
            let mut pairs = Vec::with_capacity(map.len());
            for (k, v) in map {
                pairs.push((Cow::Owned(k), serde_json_to_value(v)?));
            }
            Some(Value::Dict(pairs))
        }
    }
}
