//! pyqql-edge — local QQL execution via qdrant-edge + fastembed.
//!
//! Zero network.  No Qdrant server required.  Parser + in-process HNSW.
//!
//! ```python
//! import pyqql_edge
//!
//! # ── Parser (same API as pyqql) ──
//! stmt = pyqql_edge.parse("QUERY 'hello' FROM docs LIMIT 10")[0]
//! tokens = pyqql_edge.tokenize("QUERY 'test' FROM docs")
//! plan = pyqql_edge.explain("QUERY 'hello' FROM docs LIMIT 10")
//!
//! # ── Edge execution ──
//! exec = pyqql_edge.local_executor("./qdrant_data", model="BGESmallENV15")
//! result = exec.execute("QUERY 'hello' FROM docs LIMIT 10")
//! models = pyqql_edge.list_embedding_models()
//! ```

use pyo3::exceptions::{PyRuntimeError, PySyntaxError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyList};
use qql_core::ast::{self, ComparisonOp, Value};
use qql_core::lexer::Lexer;
use qql_core::parser::Parser;
use std::sync::atomic::{AtomicBool, Ordering};

// ═══════════════════════════════════════════════════════════════════
//  Stmt class — mirrors pyqql.PyStmt
// ═══════════════════════════════════════════════════════════════════

#[pyclass(name = "Stmt", from_py_object)]
#[derive(Clone)]
pub struct PyStmt {
    pub inner: qql_core::ast::Stmt,
}

#[pymethods]
impl PyStmt {
    fn inject_filter(&mut self, field: &str, op: &str, value: &Bound<'_, PyAny>) -> PyResult<()> {
        if op == "!=" || op == "neq" || op == "<>" {
            return Err(PySyntaxError::new_err(
                "inject_filter does not support '!='; inject equality and wrap with NOT, or rewrite the query",
            ));
        }
        let val = py_to_value(value)?;
        let cmp = str_to_comparison_op(op)?;
        ast::inject_filter(&mut self.inner, field, cmp, val).map_err(qql_py_syntax_error)?;
        Ok(())
    }

    /// QQL `SHARD '…'` routing key (request-level). Prefer the clause in QQL.
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
}

// ═══════════════════════════════════════════════════════════════════
//  Parser functions
// ═══════════════════════════════════════════════════════════════════

#[pyfunction]
fn parse(input: &str) -> PyResult<Vec<PyStmt>> {
    let stmts = Parser::parse_all(input).map_err(qql_py_syntax_error)?;
    Ok(stmts.into_iter().map(|s| PyStmt { inner: s }).collect())
}

/// Parse a script and return the canonical AST JSON without creating Python
/// objects for every node.
#[pyfunction]
fn parse_json(input: &str) -> PyResult<String> {
    let statements = Parser::parse_all(input).map_err(qql_py_error)?;
    serde_json::to_string(&statements)
        .map_err(|error| PyRuntimeError::new_err(format!("serialize AST: {error}")))
}

#[pyfunction]
fn is_valid(input: &str) -> bool {
    Parser::parse_all(input).is_ok()
}

#[pyfunction]
fn inject_filter(
    query: &Bound<'_, PyAny>,
    field: &str,
    op: &str,
    value: &Bound<'_, PyAny>,
) -> PyResult<PyStmt> {
    if op == "!=" || op == "neq" || op == "<>" {
        return Err(PySyntaxError::new_err(
            "inject_filter does not support '!='; inject equality and wrap with NOT, or rewrite the query",
        ));
    }
    let val = py_to_value(value)?;
    let cmp = str_to_comparison_op(op)?;
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

#[pyfunction]
fn tokenize<'py>(input: &str, py: Python<'py>) -> PyResult<Vec<Bound<'py, PyDict>>> {
    let lexer = Lexer::new(input);
    let mut result = Vec::with_capacity(input.len() / 4 + 1);
    let kind_key = pyo3::intern!(py, "kind");
    let text_key = pyo3::intern!(py, "text");
    let pos_key = pyo3::intern!(py, "pos");
    for token_result in lexer {
        let token = token_result.map_err(qql_py_syntax_error)?;
        let d = PyDict::new(py);
        d.set_item(kind_key, token.kind.as_str())?;
        d.set_item(text_key, token.text)?;
        d.set_item(pos_key, token.span.start as i64)?;
        result.push(d);
    }
    Ok(result)
}

#[pyfunction]
fn compile_query<'py>(py: Python<'py>, input: &str) -> PyResult<Bound<'py, PyAny>> {
    let stmt = Parser::parse(input).map_err(qql_py_syntax_error)?;
    let compiled = qql_plan::routing::compile_statement(&stmt)
        .map_err(|e| PySyntaxError::new_err(e.to_string()))?;
    let (method, path, payload) = match compiled.route {
        Some(route) => {
            let payload = route.body_json().unwrap_or(serde_json::Value::Null);
            (
                serde_json::Value::String(route.method.as_str().into()),
                serde_json::Value::String(route.path),
                payload,
            )
        }
        None => (
            serde_json::Value::Null,
            serde_json::Value::Null,
            serde_json::Value::Null,
        ),
    };
    let result = serde_json::json!({
        "stmt_type": compiled.stmt_type,
        "method": method,
        "path": path,
        "payload": payload,
    });
    pythonize::pythonize(py, &result).map_err(|e| PySyntaxError::new_err(e.to_string()))
}

fn do_explain(py: Python<'_>, query: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
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

#[pyfunction]
fn explain(query: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    let py = query.py();
    do_explain(py, query)
}

// ═══════════════════════════════════════════════════════════════════
//  Edge Client — wraps qql-edge Executor
// ═══════════════════════════════════════════════════════════════════

#[pyclass(name = "Client", frozen)]
pub struct PyClient {
    pub(crate) inner: std::sync::Arc<qql::executor::Executor>,
    pub(crate) runtime: tokio::runtime::Runtime,
    pub(crate) closed: AtomicBool,
}

#[pymethods]
impl PyClient {
    #[pyo3(signature = (query, *, params=None, on_error="stop"))]
    fn execute<'py>(
        &self,
        py: Python<'py>,
        query: &Bound<'_, PyAny>,
        params: Option<&Bound<'_, PyAny>>,
        on_error: &str,
    ) -> PyResult<Bound<'py, PyAny>> {
        if self.closed.load(Ordering::Acquire) {
            return Err(PyRuntimeError::new_err("client is closed"));
        }
        let oe = parse_on_error(on_error)?;
        let input = if let Some(p) = params {
            if let Ok(q_str) = query.extract::<String>() {
                let bound = bind_py_params(&q_str, p)?;
                Input::String(bound)
            } else {
                return Err(PyValueError::new_err(
                    "parameter binding requires a query string",
                ));
            }
        } else {
            classify(query)?
        };
        let out = py.detach(|| self.run_input(input, oe))?;
        pythonize::pythonize(py, &out)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
    }

    #[pyo3(signature = (query, *, params=None, on_error="stop"))]
    fn execute_async<'py>(
        &self,
        py: Python<'py>,
        query: Bound<'py, PyAny>,
        params: Option<&Bound<'_, PyAny>>,
        on_error: &str,
    ) -> PyResult<Bound<'py, PyAny>> {
        if self.closed.load(Ordering::Acquire) {
            return Err(PyRuntimeError::new_err("client is closed"));
        }
        let inner = self.inner.clone();
        let input = if let Some(p) = params {
            if let Ok(q_str) = query.extract::<String>() {
                let bound = bind_py_params(&q_str, p)?;
                Input::String(bound)
            } else {
                return Err(PyValueError::new_err(
                    "parameter binding requires a query string",
                ));
            }
        } else {
            classify(&query)?
        };
        let on_error = parse_on_error(on_error)?;
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let val = run_async(&inner, input, on_error)
                .await
                .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
            Python::attach(|py| {
                pythonize::pythonize(py, &val)
                    .map(|b| b.unbind())
                    .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
            })
        })
    }

    fn explain(&self, query: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        let py = query.py();
        do_explain(py, query)
    }

    /// Compile a QQL query to its transport route without executing (parity
    /// with `pyqql.Client.compile` / `nqql-edge` `Client.compile`).
    fn compile<'py>(&self, py: Python<'py>, query: &str) -> PyResult<Bound<'py, PyAny>> {
        compile_query(py, query)
    }

    /// Flush and release edge storage. Idempotent.
    fn close(&self) -> PyResult<()> {
        if self.closed.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        Python::attach(|py| py.detach(|| self.runtime.block_on(self.inner.close())))
            .map_err(qql_py_error)
    }

    fn __enter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __exit__(
        &self,
        _ty: &Bound<'_, PyAny>,
        _value: &Bound<'_, PyAny>,
        _traceback: &Bound<'_, PyAny>,
    ) -> PyResult<bool> {
        self.close()?;
        Ok(false)
    }
}

pub(crate) enum Input {
    String(String),
    Stmt(ast::Stmt),
    StrList(Vec<String>),
    StmtList(Vec<ast::Stmt>),
}

pub(crate) fn classify(query: &Bound<'_, PyAny>) -> PyResult<Input> {
    if let Ok(list) = query.cast::<pyo3::types::PyList>() {
        if list.is_empty() {
            return Ok(Input::StrList(Vec::new()));
        }
        let first = list.get_item(0)?;
        if first.extract::<PyRef<'_, PyStmt>>().is_ok() {
            let stmts: Vec<ast::Stmt> = list
                .iter()
                .map(|i| Ok(i.extract::<PyRef<'_, PyStmt>>()?.inner.clone()))
                .collect::<PyResult<_>>()?;
            return Ok(Input::StmtList(stmts));
        }
        let strs: Vec<String> = list
            .iter()
            .map(|i| i.extract::<String>())
            .collect::<PyResult<_>>()
            .map_err(|_| {
                pyo3::exceptions::PyTypeError::new_err("list items must be strings or Stmt objects")
            })?;
        return Ok(Input::StrList(strs));
    }
    if let Ok(stmt) = query.extract::<PyRef<'_, PyStmt>>() {
        return Ok(Input::Stmt(stmt.inner.clone()));
    }
    let s = query.extract::<String>().map_err(|_| {
        pyo3::exceptions::PyTypeError::new_err(
            "query must be a str, Stmt, list[str], or list[Stmt]",
        )
    })?;
    Ok(Input::String(s))
}

impl PyClient {
    fn run_input(
        &self,
        input: Input,
        on_error: qql::executor::OnError,
    ) -> PyResult<serde_json::Value> {
        let stop = matches!(on_error, qql::executor::OnError::Stop);
        match input {
            Input::String(s) => {
                let report = self
                    .runtime
                    .block_on(self.inner.execute(&s, on_error))
                    .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
                Ok(serde_json::to_value(&report).unwrap_or_default())
            }
            Input::Stmt(s) => {
                let results = self
                    .runtime
                    .block_on(self.inner.execute_batch_nodes(vec![s], stop))
                    .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
                let report = qql::executor::ExecutionReport::from_results(results);
                Ok(serde_json::to_value(&report).unwrap_or_default())
            }
            Input::StrList(strs) => {
                let refs: Vec<&str> = strs.iter().map(|s| s.as_str()).collect();
                let report = self
                    .runtime
                    .block_on(self.inner.execute_batch(&refs, on_error))
                    .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
                Ok(serde_json::to_value(&report).unwrap_or_default())
            }
            Input::StmtList(stmts) => {
                let results = self
                    .runtime
                    .block_on(self.inner.execute_batch_nodes(stmts, stop))
                    .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
                let report = qql::executor::ExecutionReport::from_results(results);
                Ok(serde_json::to_value(&report).unwrap_or_default())
            }
        }
    }
}

pub(crate) async fn run_async(
    inner: &qql::executor::Executor,
    input: Input,
    on_error: qql::executor::OnError,
) -> Result<serde_json::Value, qql_core::error::QqlError> {
    let stop = matches!(on_error, qql::executor::OnError::Stop);
    match input {
        Input::String(s) => {
            let report = inner.execute(&s, on_error).await?;
            Ok(serde_json::to_value(&report).unwrap_or_default())
        }
        Input::Stmt(s) => {
            let results = inner.execute_batch_nodes(vec![s], stop).await?;
            let report = qql::executor::ExecutionReport::from_results(results);
            Ok(serde_json::to_value(&report).unwrap_or_default())
        }
        Input::StrList(strs) => {
            let refs: Vec<&str> = strs.iter().map(|s| s.as_str()).collect();
            let report = inner.execute_batch(&refs, on_error).await?;
            Ok(serde_json::to_value(&report).unwrap_or_default())
        }
        Input::StmtList(stmts) => {
            let results = inner.execute_batch_nodes(stmts, stop).await?;
            let report = qql::executor::ExecutionReport::from_results(results);
            Ok(serde_json::to_value(&report).unwrap_or_default())
        }
    }
}

/// Substitute named (:name) or positional (?) parameters into a query string.
#[pyfunction]
#[pyo3(signature = (query, params=None))]
fn bind(query: &str, params: Option<&Bound<'_, PyAny>>) -> PyResult<String> {
    match params {
        Some(p) => bind_py_params(query, p),
        None => Ok(query.to_string()),
    }
}

pub(crate) fn bind_py_params(query: &str, params: &Bound<'_, PyAny>) -> PyResult<String> {
    if params.is_none() {
        return Ok(query.to_string());
    }
    if let Ok(dict) = params.cast::<PyDict>() {
        let mut map = std::collections::HashMap::new();
        for (k, v) in dict.iter() {
            let key = k
                .extract::<String>()
                .map_err(|_| PySyntaxError::new_err("parameter dict keys must be strings"))?;
            let val = py_to_value(&v)?;
            map.insert(key, val);
        }
        qql_core::params::bind_named(query, |k| map.get(k).cloned())
            .map_err(|e| PyValueError::new_err(e.to_string()))
    } else if let Ok(list) = params.cast::<PyList>() {
        let mut items = Vec::with_capacity(list.len());
        for item in list.iter() {
            items.push(py_to_value(&item)?);
        }
        qql_core::params::bind_positional(query, &items)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    } else {
        Err(PyValueError::new_err(
            "params must be a dict for named parameters (:name) or a list for positional parameters (?)",
        ))
    }
}

mod models;
pub use models::*;

// ═══════════════════════════════════════════════════════════════════
//  Module init
// ═══════════════════════════════════════════════════════════════════

#[pymodule]
fn pyqql_edge(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyStmt>()?;
    m.add_class::<PyClient>()?;
    #[cfg(feature = "fastembed-local")]
    m.add_function(wrap_pyfunction!(local_executor, m)?)?;
    #[cfg(feature = "fastembed-local")]
    m.add_function(wrap_pyfunction!(list_embedding_models, m)?)?;
    #[cfg(feature = "fastembed-local")]
    m.add_function(wrap_pyfunction!(execute, m)?)?;
    #[cfg(feature = "fastembed-local")]
    m.add_function(wrap_pyfunction!(execute_async, m)?)?;
    #[cfg(feature = "http-embedding")]
    m.add_function(wrap_pyfunction!(http_executor, m)?)?;
    m.add_function(wrap_pyfunction!(bind, m)?)?;
    m.add_function(wrap_pyfunction!(explain, m)?)?;
    m.add_function(wrap_pyfunction!(parse, m)?)?;
    m.add_function(wrap_pyfunction!(parse_json, m)?)?;
    m.add_function(wrap_pyfunction!(is_valid, m)?)?;
    m.add_function(wrap_pyfunction!(inject_filter, m)?)?;
    m.add_function(wrap_pyfunction!(tokenize, m)?)?;
    m.add_function(wrap_pyfunction!(compile_query, m)?)?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}

fn qql_py_error(error: qql_core::error::QqlError) -> pyo3::PyErr {
    attach_qql_error(PyRuntimeError::new_err(error.to_string()), error)
}

fn qql_py_syntax_error(error: qql_core::error::QqlError) -> pyo3::PyErr {
    attach_qql_error(PySyntaxError::new_err(error.to_string()), error)
}

fn attach_qql_error(py_error: pyo3::PyErr, error: qql_core::error::QqlError) -> pyo3::PyErr {
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

fn parse_on_error(value: &str) -> PyResult<qql::executor::OnError> {
    match value {
        "stop" => Ok(qql::executor::OnError::Stop),
        "continue" => Ok(qql::executor::OnError::Continue),
        _ => Err(pyo3::exceptions::PyValueError::new_err(
            "on_error must be 'stop' or 'continue'",
        )),
    }
}

// ═══════════════════════════════════════════════════════════════════
//  Helpers
// ═══════════════════════════════════════════════════════════════════

fn py_to_value(value: &Bound<'_, PyAny>) -> PyResult<Value> {
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
        return Ok(Value::Str(s));
    }
    if let Ok(list) = value.cast::<PyList>() {
        let mut items = Vec::with_capacity(list.len());
        for item in list.iter() {
            items.push(py_to_value(&item)?);
        }
        return Ok(Value::List(items));
    }
    if let Ok(dict) = value.cast::<PyDict>() {
        let mut items = Vec::with_capacity(dict.len());
        for (key, item) in dict.iter() {
            let key = key
                .extract::<String>()
                .map_err(|_| PySyntaxError::new_err("dict keys must be strings"))?;
            items.push((key, py_to_value(&item)?));
        }
        return Ok(Value::Dict(items));
    }
    Err(PySyntaxError::new_err("unsupported filter value type"))
}

fn str_to_comparison_op(op: &str) -> PyResult<ComparisonOp> {
    match op {
        "=" | "==" | "eq" => Ok(ComparisonOp::Eq),
        ">" | "gt" => Ok(ComparisonOp::Gt),
        ">=" | "gte" => Ok(ComparisonOp::Gte),
        "<" | "lt" => Ok(ComparisonOp::Lt),
        "<=" | "lte" => Ok(ComparisonOp::Lte),
        _ => Err(PySyntaxError::new_err(format!(
            "unsupported comparison operator '{op}' (use =, >, >=, <, <=)"
        ))),
    }
}
