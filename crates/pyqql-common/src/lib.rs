//! Shared PyO3 logic for the `pyqql` and `pyqql-edge` bindings.
//!
//! The two Python SDKs expose different transports (REST/gRPC vs qdrant-edge)
//! but ship an identical parser/parameter surface. Every piece of that logic
//! lives here so the SDKs cannot drift: the crates register the shared classes
//! and functions in their `#[pymodule]` and keep only transport-specific
//! client construction.
//!
//! Parameter dispatch follows the shared batch contract in
//! [`qql_core::params_json::plan_statement_params`]: a params list whose
//! entries are all objects/arrays is statement-scoped (exact length match,
//! `QQL-BIND-BATCH-LENGTH` otherwise); every other shape applies to every
//! statement identically.

use pyo3::exceptions::{PyRuntimeError, PySyntaxError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyList};
use qql_core::ast::{self, Value};
use qql_core::error::QqlError;
use qql_core::lexer::Lexer;
use qql_core::parser::Parser;

pub mod dispatch;

pub use dispatch::{
    Input, OnError, parse_on_error, prepare_input, run_async, run_input, wrap_execution_report,
};

// ═══════════════════════════════════════════════════════════════════
//  Error mapping
// ═══════════════════════════════════════════════════════════════════

/// Map a [`QqlError`] to a Python `RuntimeError` carrying `.code` / `.kind` /
/// `.span`.
pub fn qql_py_error(error: QqlError) -> PyErr {
    attach_qql_error(PyRuntimeError::new_err(error.to_string()), error)
}

/// Map a [`QqlError`] to a Python `SyntaxError` (parse / validation surface).
pub fn qql_py_syntax_error(error: QqlError) -> PyErr {
    attach_qql_error(PySyntaxError::new_err(error.to_string()), error)
}

/// Map a [`QqlError`] to a Python `ValueError` (binding / value surface).
pub fn qql_py_value_error(error: QqlError) -> PyErr {
    attach_qql_error(PyValueError::new_err(error.to_string()), error)
}

fn attach_qql_error(py_error: PyErr, error: QqlError) -> PyErr {
    Python::attach(|py| {
        let value = py_error.value(py);
        let _ = value.setattr("code", error.code.as_ref());
        let _ = value.setattr("kind", format!("{:?}", error.kind));
        let span = if let Some(span) = error.span {
            let span_dict = PyDict::new(py);
            let _ = span_dict.set_item("start", span.start);
            let _ = span_dict.set_item("end", span.end);
            span_dict.into_any()
        } else {
            py.None().into_bound(py)
        };
        let _ = value.setattr("span", span);
    });
    py_error
}

// ═══════════════════════════════════════════════════════════════════
//  Python value conversion — the single Py → serde_json path
// ═══════════════════════════════════════════════════════════════════

/// Convert a Python value to a JSON-compatible `serde_json::Value`.
///
/// Bools are checked before ints (Python `bool` subclasses `int`), so
/// `True` binds as a boolean, not `1`.
pub fn py_to_json(value: &Bound<'_, PyAny>) -> PyResult<serde_json::Value> {
    if value.is_none() {
        return Ok(serde_json::Value::Null);
    }
    if let Ok(v) = value.extract::<bool>() {
        return Ok(serde_json::Value::Bool(v));
    }
    if let Ok(v) = value.extract::<i64>() {
        return Ok(serde_json::Value::Number(v.into()));
    }
    if let Ok(v) = value.extract::<f64>() {
        if !v.is_finite() {
            // serde_json would silently serialize NaN/infinity as `null`;
            // reject instead so the QQL-BIND-INVALID-FLOAT contract holds.
            return Err(PyValueError::new_err(format!(
                "cannot bind non-finite float value '{v}'"
            )));
        }
        return Ok(serde_json::json!(v));
    }
    if let Ok(s) = value.extract::<String>() {
        return Ok(serde_json::Value::String(s));
    }
    if let Ok(list) = value.cast::<PyList>() {
        let mut items = Vec::with_capacity(list.len());
        for item in list.iter() {
            items.push(py_to_json(&item)?);
        }
        return Ok(serde_json::Value::Array(items));
    }
    if let Ok(dict) = value.cast::<pyo3::types::PyDict>() {
        let mut map = serde_json::Map::with_capacity(dict.len());
        for (key, item) in dict.iter() {
            let key = key
                .extract::<String>()
                .map_err(|_| PySyntaxError::new_err("dict keys must be strings"))?;
            map.insert(key, py_to_json(&item)?);
        }
        return Ok(serde_json::Value::Object(map));
    }
    Err(PySyntaxError::new_err("unsupported filter value type"))
}

// ═══════════════════════════════════════════════════════════════════
//  Parameter binding over Python objects
// ═══════════════════════════════════════════════════════════════════

/// Bind parameters into a statement AST in place.
///
/// `params` is a dict (named `:name`) or a list (positional `?`); anything
/// else raises `ValueError`, mirroring the shared
/// [`QQL-BIND-INVALID-PARAMS`](qql_core::params_json) contract.
pub fn bind_py_stmt(stmt: &mut ast::Stmt, params: Option<&Bound<'_, PyAny>>) -> PyResult<()> {
    let Some(p) = params else {
        return Ok(());
    };
    if p.is_none() {
        return Ok(());
    }
    let json_params = py_to_json(p)?;
    qql_core::params_json::bind_stmt_with_params(stmt, &json_params).map_err(qql_py_value_error)
}

/// Bind parameters into a query string; `truncate_vectors` renders compact
/// `[0.1, 0.2, ... (N dims)]` previews of long vector literals.
pub fn bind_py_params(
    query: &str,
    params: Option<&Bound<'_, PyAny>>,
    truncate_vectors: bool,
) -> PyResult<String> {
    let Some(p) = params else {
        return Ok(query.to_string());
    };
    if p.is_none() {
        return Ok(query.to_string());
    }
    let json_params = py_to_json(p)?;
    let plan = qql_core::params_json::plan_statement_params(&json_params, 1)
        .map_err(qql_py_value_error)?;
    qql_core::params_json::bind_str_with_params(
        query,
        qql_core::params_json::param_for(&plan, 0),
        truncate_vectors,
    )
    .map_err(qql_py_value_error)
}

// ═══════════════════════════════════════════════════════════════════
//  Stmt class — identical surface in pyqql and pyqql-edge
// ═══════════════════════════════════════════════════════════════════

/// A parsed QQL statement handle (mirrors `nqql`'s `Stmt` and `qql-wasm`'s).
#[pyclass(name = "Stmt", from_py_object)]
#[derive(Clone)]
pub struct PyStmt {
    /// The typed AST backing this handle.
    pub inner: qql_core::ast::Stmt,
}

#[pymethods]
impl PyStmt {
    fn __str__(&self) -> String {
        format!("{}", self.inner)
    }

    fn __repr__(&self) -> String {
        format!(
            "<Stmt: {}>",
            qql_core::fmt::format_stmt_readable(&self.inner)
        )
    }

    /// Bind parameters into this statement and return a new bound Stmt.
    #[pyo3(signature = (params=None))]
    fn bind(&self, params: Option<&Bound<'_, PyAny>>) -> PyResult<PyStmt> {
        let mut inner = self.inner.clone();
        bind_py_stmt(&mut inner, params)?;
        Ok(PyStmt { inner })
    }

    fn inject_filter(&mut self, field: &str, op: &str, value: &Bound<'_, PyAny>) -> PyResult<()> {
        let val =
            py_to_json(value).and_then(|v| Value::from_json(v).map_err(qql_py_syntax_error))?;
        let cmp = qql_core::ast::ComparisonOp::parse_inject_op(op).map_err(qql_py_syntax_error)?;
        ast::inject_filter(&mut self.inner, field, cmp, val).map_err(qql_py_syntax_error)?;
        Ok(())
    }

    /// QQL `SHARD '…'` routing key on this statement (request-level; not a filter).
    /// Prefer writing `SHARD 'tenant'` in QQL; use the setter only when the host
    /// resolves the key after parse. Empty / None clears. Recurses into CTEs.
    #[getter]
    fn shard_key(&self) -> Option<String> {
        self.inner.shard_key().map(str::to_owned)
    }

    #[setter]
    fn set_shard_key(&mut self, key: Option<String>) -> PyResult<()> {
        if !self.inner.set_shard_key(key) {
            return Err(PyValueError::new_err(
                "cannot set shard_key on statement type that does not support sharding (e.g. DDL statements)",
            ));
        }
        Ok(())
    }

    fn to_json(&self) -> PyResult<String> {
        serde_json::to_string(&self.inner).map_err(|e| PySyntaxError::new_err(e.to_string()))
    }

    fn to_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        // Must go through serde_json::Value: pythonize maps Rust tuples to
        // Python tuples, while JSON arrays become lists. AST dicts are
        // `Vec<(String, Value)>` and host tests walk those as lists.
        let val =
            serde_json::to_value(&self.inner).map_err(|e| PySyntaxError::new_err(e.to_string()))?;
        pythonize::pythonize(py, &val).map_err(|e| PySyntaxError::new_err(e.to_string()))
    }

    /// Compile this Stmt directly to its transport route without re-parsing.
    /// Optionally accepts `params` to bind before compiling.
    #[pyo3(signature = (params=None))]
    fn compile_route<'py>(
        &self,
        py: Python<'py>,
        params: Option<&Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let mut stmt = self.inner.clone();
        bind_py_stmt(&mut stmt, params)?;
        let compiled = qql_plan::routing::compile_statement(&stmt).map_err(qql_py_syntax_error)?;
        let result = compiled_route_json(&compiled);
        pythonize::pythonize(py, &result).map_err(|e| PySyntaxError::new_err(e.to_string()))
    }

    fn explain<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let plan = qql_core::explain::explain_node(&self.inner);
        let dict = PyDict::new(py);
        dict.set_item(pyo3::intern!(py, "ok"), true)?;
        dict.set_item(pyo3::intern!(py, "query"), format!("{}", self.inner))?;
        dict.set_item(pyo3::intern!(py, "plan"), plan)?;
        Ok(dict.into_any())
    }
}

/// Shared route JSON shape for `compile_route` / `compile_query`.
pub fn compiled_route_json(compiled: &qql_plan::routing::CompiledStatement) -> serde_json::Value {
    let (method, path, payload) = match &compiled.route {
        Some(route) => {
            let payload = route.body_json().unwrap_or(serde_json::Value::Null);
            (
                serde_json::Value::String(route.method.as_str().into()),
                serde_json::Value::String(route.path.clone()),
                payload,
            )
        }
        None => (
            serde_json::Value::Null,
            serde_json::Value::Null,
            serde_json::Value::Null,
        ),
    };
    serde_json::json!({
        "stmt_type": compiled.stmt_type,
        "method": method,
        "path": path,
        "payload": payload,
    })
}

// ═══════════════════════════════════════════════════════════════════
//  Free functions — registered by both SDK modules
// ═══════════════════════════════════════════════════════════════════

/// Parse a QQL source into a list of Stmt objects.
/// Accepts single statements and semicolon-delimited scripts.
#[pyfunction]
pub fn parse(input: &str) -> PyResult<Vec<PyStmt>> {
    let stmts = Parser::parse_all(input).map_err(qql_py_syntax_error)?;
    Ok(stmts.into_iter().map(|s| PyStmt { inner: s }).collect())
}

/// Parse a QQL source and return the canonical AST as a JSON string without
/// creating Python objects for every node (parity with `nqql.parseJson`).
#[pyfunction]
pub fn parse_json(input: &str) -> PyResult<String> {
    let statements = Parser::parse_all(input).map_err(qql_py_syntax_error)?;
    serde_json::to_string(&statements).map_err(|e| PySyntaxError::new_err(e.to_string()))
}

/// Full frontend validity gate: parse + plan — the same contract as execution
/// and the language conformance suite.
#[pyfunction]
pub fn is_valid(input: &str) -> bool {
    // Full frontend gate: parse + plan — same contract as execution and the
    // language conformance suite.
    qql_plan::parse_and_plan(input).is_ok()
}

/// Inject a WHERE filter into a query string or Stmt; returns the new Stmt.
#[pyfunction]
pub fn inject_filter(
    query: &Bound<'_, PyAny>,
    field: &str,
    op: &str,
    value: &Bound<'_, PyAny>,
) -> PyResult<PyStmt> {
    let val = py_to_json(value).and_then(|v| Value::from_json(v).map_err(qql_py_syntax_error))?;
    let cmp = qql_core::ast::ComparisonOp::parse_inject_op(op).map_err(qql_py_syntax_error)?;
    if let Ok(mut py_stmt) = query.extract::<PyRefMut<'_, PyStmt>>() {
        ast::inject_filter(&mut py_stmt.inner, field, cmp, val).map_err(qql_py_syntax_error)?;
        Ok(py_stmt.clone())
    } else if let Ok(query_str) = query.extract::<String>() {
        let mut stmt = Parser::parse(&query_str).map_err(qql_py_syntax_error)?;
        ast::inject_filter(&mut stmt, field, cmp, val).map_err(qql_py_syntax_error)?;
        Ok(PyStmt { inner: stmt })
    } else {
        Err(pyo3::exceptions::PyTypeError::new_err(
            "query must be a string or a Stmt object",
        ))
    }
}

/// Tokenize a query into `{ kind, text, pos, end, len }` dicts.
#[pyfunction]
pub fn tokenize<'py>(input: &str, py: Python<'py>) -> PyResult<Vec<Bound<'py, PyDict>>> {
    let lexer = Lexer::new(input);
    let mut result = Vec::with_capacity(input.len() / 4 + 1);
    let kind_key = pyo3::intern!(py, "kind");
    let text_key = pyo3::intern!(py, "text");
    let pos_key = pyo3::intern!(py, "pos");
    let end_key = pyo3::intern!(py, "end");
    let len_key = pyo3::intern!(py, "len");
    for token_result in lexer {
        let token = token_result.map_err(qql_py_syntax_error)?;
        let d = PyDict::new(py);
        d.set_item(kind_key, token.kind.as_str())?;
        d.set_item(text_key, token.text)?;
        d.set_item(pos_key, token.span.start as i64)?;
        d.set_item(end_key, token.span.end as i64)?;
        d.set_item(
            len_key,
            token.span.end.saturating_sub(token.span.start) as i64,
        )?;
        result.push(d);
    }
    Ok(result)
}

/// Compile a QQL query to its transport route (non-executing), optionally
/// binding `params` first (parity with `nqql.compileQuery`).
#[pyfunction]
#[pyo3(signature = (input, params=None))]
pub fn compile_query<'py>(
    py: Python<'py>,
    input: &str,
    params: Option<&Bound<'py, PyAny>>,
) -> PyResult<Bound<'py, PyAny>> {
    let mut stmt = Parser::parse(input).map_err(qql_py_syntax_error)?;
    bind_py_stmt(&mut stmt, params)?;
    let compiled = qql_plan::routing::compile_statement(&stmt).map_err(qql_py_syntax_error)?;
    let result = compiled_route_json(&compiled);
    pythonize::pythonize(py, &result).map_err(|e| PySyntaxError::new_err(e.to_string()))
}

/// Tree-formatted plan explanation for a query string or Stmt.
pub fn do_explain(py: Python<'_>, query: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    let query_str: String;
    let plan_result = if let Ok(py_stmt) = query.extract::<PyRef<PyStmt>>() {
        query_str = String::from("<Stmt>");
        Ok(qql_core::explain::explain_node(&py_stmt.inner))
    } else if let Ok(s) = query.extract::<String>() {
        query_str = s.clone();
        qql_core::explain::explain(&s)
    } else {
        return Err(pyo3::exceptions::PyTypeError::new_err(
            "query must be a string or a Stmt object",
        ));
    };

    let dict = PyDict::new(py);
    match plan_result {
        Ok(plan) => {
            dict.set_item(pyo3::intern!(py, "ok"), true)?;
            dict.set_item(pyo3::intern!(py, "query"), query_str)?;
            dict.set_item(pyo3::intern!(py, "plan"), plan)?;
        }
        Err(e) => {
            dict.set_item(pyo3::intern!(py, "ok"), false)?;
            dict.set_item(pyo3::intern!(py, "query"), query_str)?;
            dict.set_item(pyo3::intern!(py, "error"), e.to_string())?;
        }
    }
    Ok(dict.into_any().unbind())
}

/// Explain a query string or Stmt (returns `{ok, query, plan|error}`).
#[pyfunction]
pub fn explain(query: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    let py = query.py();
    do_explain(py, query)
}

/// Substitute `:name` (dict) or `?` (list) placeholders into a query string or Stmt.
///
/// When `truncate_vectors=True`, long vector literals (e.g. 384 dims) are rendered
/// in a compact human-readable format `[0.12, 0.34, ... (384 dims)]` suitable for logging.
#[pyfunction]
#[pyo3(signature = (query, params=None, *, truncate_vectors=false))]
pub fn bind<'py>(
    py: Python<'py>,
    query: &Bound<'py, PyAny>,
    params: Option<&Bound<'py, PyAny>>,
    truncate_vectors: bool,
) -> PyResult<Bound<'py, PyAny>> {
    if let Ok(stmt) = query.extract::<PyRef<'_, PyStmt>>() {
        let mut inner = stmt.inner.clone();
        bind_py_stmt(&mut inner, params)?;
        let py_stmt = PyStmt { inner };
        if truncate_vectors {
            let readable = qql_core::fmt::format_stmt_readable(&py_stmt.inner);
            Ok(readable.into_pyobject(py)?.into_any())
        } else {
            Ok(Bound::new(py, py_stmt)?.into_any())
        }
    } else if let Ok(q_str) = query.extract::<String>() {
        let bound = match params {
            Some(p) => bind_py_params(&q_str, Some(p), truncate_vectors)?,
            None => q_str,
        };
        Ok(bound.into_pyobject(py)?.into_any())
    } else {
        Err(pyo3::exceptions::PyTypeError::new_err(
            "query must be a str or Stmt",
        ))
    }
}
