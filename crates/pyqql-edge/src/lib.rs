//! pyqql-edge — local QQL execution via qdrant-edge + fastembed.
//!
//! Zero network.  No Qdrant server required.  Parser + in-process HNSW.
//!
//! ```python
//! import pyqql_edge
//!
//! # ── Parser (same API as pyqql) ──
//! stmt = pyqql_edge.parse("QUERY 'hello' FROM docs LIMIT 10")
//! tokens = pyqql_edge.tokenize("QUERY 'test' FROM docs")
//! plan = pyqql_edge.explain("QUERY 'hello' FROM docs LIMIT 10")
//!
//! # ── Edge execution ──
//! exec = pyqql_edge.local_executor("./qdrant_data")
//! result = exec.execute("QUERY 'hello' FROM docs LIMIT 10")
//! ```

use pyo3::exceptions::PySyntaxError;
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyList};
use qql_core::ast::{self, ComparisonOp, Value};
use qql_core::lexer::Lexer;
use qql_core::parser::Parser;

// ═══════════════════════════════════════════════════════════════════
//  Stmt class — mirrors pyqql.PyStmt
// ═══════════════════════════════════════════════════════════════════

#[pyclass(name = "Stmt")]
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
        let cmp = str_to_comparison_op(op);
        ast::inject_filter(&mut self.inner, field, cmp, val)
            .map_err(|e| PySyntaxError::new_err(e.to_string()))?;
        Ok(())
    }

    #[getter]
    fn shard_key(&self) -> Option<String> {
        match &self.inner {
            ast::Stmt::Query(q) => q.shard_key.clone(),
            ast::Stmt::Count(c) => c.shard_key.clone(),
            ast::Stmt::Scroll(s) => s.shard_key.clone(),
            ast::Stmt::Upsert(u) => u.shard_key.clone(),
            ast::Stmt::Delete(d) => d.shard_key.clone(),
            _ => None,
        }
    }

    #[setter]
    fn set_shard_key(&mut self, key: Option<String>) {
        let key = key.filter(|k| !k.is_empty());
        match &mut self.inner {
            ast::Stmt::Query(q) => q.shard_key = key,
            ast::Stmt::Count(c) => c.shard_key = key,
            ast::Stmt::Scroll(s) => s.shard_key = key,
            ast::Stmt::Upsert(u) => u.shard_key = key,
            ast::Stmt::Delete(d) => d.shard_key = key,
            _ => {}
        }
    }

    fn to_json(&self) -> PyResult<String> {
        serde_json::to_string(&self.inner).map_err(|e| PySyntaxError::new_err(e.to_string()))
    }

    fn to_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        pythonize::pythonize(py, &self.inner).map_err(|e| PySyntaxError::new_err(e.to_string()))
    }
}

// ═══════════════════════════════════════════════════════════════════
//  Parser functions
// ═══════════════════════════════════════════════════════════════════

#[pyfunction]
fn parse(input: &str) -> PyResult<PyStmt> {
    let stmt = Parser::parse(input).map_err(|e| PySyntaxError::new_err(e.to_string()))?;
    Ok(PyStmt { inner: stmt })
}

#[pyfunction]
fn parse_all(input: &str) -> PyResult<Vec<PyStmt>> {
    let stmts = Parser::parse_all(input).map_err(|e| PySyntaxError::new_err(e.to_string()))?;
    Ok(stmts.into_iter().map(|s| PyStmt { inner: s }).collect())
}

#[pyfunction]
fn parse_batch(queries: Vec<String>) -> PyResult<Vec<PyStmt>> {
    let mut results = Vec::with_capacity(queries.len());
    for q in queries {
        let stmt = Parser::parse(&q).map_err(|e| PySyntaxError::new_err(e.to_string()))?;
        results.push(PyStmt { inner: stmt });
    }
    Ok(results)
}

#[pyfunction]
fn is_valid(input: &str) -> bool {
    Parser::try_parse(input).is_ok()
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
    let cmp = str_to_comparison_op(op);
    if let Ok(mut py_stmt) = query.extract::<PyRefMut<'_, PyStmt>>() {
        ast::inject_filter(&mut py_stmt.inner, field, cmp, val)
            .map_err(|e| PySyntaxError::new_err(e.to_string()))?;
        Ok(py_stmt.clone())
    } else if let Ok(query_str) = query.extract::<String>() {
        let mut stmt =
            Parser::parse(&query_str).map_err(|e| PySyntaxError::new_err(e.to_string()))?;
        ast::inject_filter(&mut stmt, field, cmp, val)
            .map_err(|e| PySyntaxError::new_err(e.to_string()))?;
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
    let mut result = Vec::new();
    for token_result in lexer {
        let token = token_result.map_err(|e| PySyntaxError::new_err(e.to_string()))?;
        let d = PyDict::new(py);
        d.set_item("kind", token.kind.as_str())?;
        d.set_item("text", token.text)?;
        d.set_item("pos", token.span.start as i64)?;
        result.push(d);
    }
    Ok(result)
}

#[pyfunction]
fn compile_query<'py>(py: Python<'py>, input: &str) -> PyResult<Bound<'py, PyAny>> {
    let stmt = Parser::parse(input).map_err(|e| PySyntaxError::new_err(e.to_string()))?;
    let route = qql_plan::routing::route(&stmt);
    let result = serde_json::json!({
        "method": route.method.as_str(),
        "path": route.path,
        "payload": route.body_json().unwrap_or(serde_json::Value::Null),
    });
    pythonize::pythonize(py, &result).map_err(|e| PySyntaxError::new_err(e.to_string()))
}

#[pyfunction]
fn explain(query: &Bound<'_, PyAny>) -> PyResult<String> {
    if let Ok(py_stmt) = query.extract::<PyRef<PyStmt>>() {
        Ok(qql_core::explain::explain_node(&py_stmt.inner))
    } else if let Ok(query_str) = query.extract::<String>() {
        qql_core::explain::explain(&query_str)
            .map_err(|e| pyo3::exceptions::PySyntaxError::new_err(e.to_string()))
    } else {
        Err(pyo3::exceptions::PyTypeError::new_err(
            "query must be a string or a Stmt object",
        ))
    }
}

// ═══════════════════════════════════════════════════════════════════
//  Edge Client — wraps qql-edge Executor
// ═══════════════════════════════════════════════════════════════════

#[pyclass(name = "Client")]
struct PyClient {
    inner: std::sync::Arc<qql::executor::Executor>,
    runtime: tokio::runtime::Runtime,
}

#[pymethods]
impl PyClient {
    fn execute<'py>(
        &self,
        py: Python<'py>,
        query: &Bound<'_, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let out = self.run(query)?;
        pythonize::pythonize(py, &out)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
    }

    fn execute_async<'py>(
        &self,
        py: Python<'py>,
        query: Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        let input = classify(&query)?;
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let val = run_async(&inner, input)
                .await
                .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
            Python::with_gil(|py| {
                pythonize::pythonize(py, &val)
                    .map(|b| b.unbind())
                    .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
            })
        })
    }

    fn explain(&self, query: &Bound<'_, PyAny>) -> PyResult<String> {
        if let Ok(py_stmt) = query.extract::<PyRef<PyStmt>>() {
            Ok(qql_core::explain::explain_node(&py_stmt.inner))
        } else if let Ok(query_str) = query.extract::<String>() {
            qql_core::explain::explain(&query_str)
                .map_err(|e| pyo3::exceptions::PySyntaxError::new_err(e.to_string()))
        } else {
            Err(pyo3::exceptions::PyTypeError::new_err(
                "query must be a string or a Stmt object",
            ))
        }
    }
}

enum Input {
    String(String),
    Stmt(ast::Stmt),
    StrList(Vec<String>),
    StmtList(Vec<ast::Stmt>),
}

fn classify(query: &Bound<'_, PyAny>) -> PyResult<Input> {
    if let Ok(list) = query.downcast::<pyo3::types::PyList>() {
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
    fn run(&self, query: &Bound<'_, PyAny>) -> PyResult<serde_json::Value> {
        match classify(query)? {
            Input::String(s) => {
                let res = self
                    .runtime
                    .block_on(self.inner.execute(&s))
                    .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
                Ok(serde_json::to_value(&res).unwrap_or_default())
            }
            Input::Stmt(s) => {
                let res = self
                    .runtime
                    .block_on(self.inner.execute_node(s))
                    .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
                Ok(serde_json::to_value(&res).unwrap_or_default())
            }
            Input::StrList(strs) => {
                let refs: Vec<&str> = strs.iter().map(|s| s.as_str()).collect();
                let results = self
                    .runtime
                    .block_on(self.inner.execute_batch(&refs, true))
                    .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
                Ok(serde_json::to_value(&results).unwrap_or_default())
            }
            Input::StmtList(stmts) => {
                let results = self
                    .runtime
                    .block_on(self.inner.execute_batch_nodes(stmts, true))
                    .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
                Ok(serde_json::to_value(&results).unwrap_or_default())
            }
        }
    }
}

async fn run_async(
    inner: &qql::executor::Executor,
    input: Input,
) -> Result<serde_json::Value, qql_core::error::QqlError> {
    match input {
        Input::String(s) => {
            let res = inner.execute(&s).await?;
            Ok(serde_json::to_value(&res).unwrap_or_default())
        }
        Input::Stmt(s) => {
            let res = inner.execute_node(s).await?;
            Ok(serde_json::to_value(&res).unwrap_or_default())
        }
        Input::StrList(strs) => {
            let refs: Vec<&str> = strs.iter().map(|s| s.as_str()).collect();
            let results = inner.execute_batch(&refs, true).await?;
            Ok(serde_json::to_value(&results).unwrap_or_default())
        }
        Input::StmtList(stmts) => {
            let results = inner.execute_batch_nodes(stmts, true).await?;
            Ok(serde_json::to_value(&results).unwrap_or_default())
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
//  HttpEmbedder — for use with http_executor
// ═══════════════════════════════════════════════════════════════════

#[pyclass(name = "HttpEmbedder")]
#[derive(Clone)]
#[allow(dead_code)]
struct PyHttpEmbedder {
    endpoint: String,
    api_key: String,
    model: String,
    dimension: usize,
}

#[pymethods]
impl PyHttpEmbedder {
    #[new]
    #[pyo3(signature = (endpoint, model, dimension, api_key=None))]
    fn new(
        endpoint: &str,
        model: &str,
        dimension: usize,
        api_key: Option<String>,
    ) -> PyResult<Self> {
        Ok(PyHttpEmbedder {
            endpoint: endpoint.to_string(),
            api_key: api_key.unwrap_or_default(),
            model: model.to_string(),
            dimension,
        })
    }
}

// ═══════════════════════════════════════════════════════════════════
//  Executor constructors — edge-only, no REST/gRPC
// ═══════════════════════════════════════════════════════════════════

#[pyfunction]
#[pyo3(signature = (data_dir, on_disk_payload=true))]
fn local_executor(data_dir: &str, on_disk_payload: bool) -> PyResult<PyClient> {
    let exec = qql_edge::local_executor(data_dir, on_disk_payload)
        .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
    Ok(PyClient {
        inner: std::sync::Arc::new(exec),
        runtime: rt,
    })
}

#[cfg(feature = "http-embedding")]
#[pyfunction]
#[pyo3(signature = (data_dir, url, embed_key, embed_model, embed_dim, on_disk_payload=true))]
fn http_executor(
    data_dir: &str,
    url: &str,
    embed_key: &str,
    embed_model: &str,
    embed_dim: usize,
    on_disk_payload: bool,
) -> PyResult<PyClient> {
    let exec = qql_edge::http_executor(
        data_dir,
        on_disk_payload,
        url.to_string(),
        embed_key.to_string(),
        embed_model.to_string(),
        embed_dim,
    )
    .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
    Ok(PyClient {
        inner: std::sync::Arc::new(exec),
        runtime: rt,
    })
}

// ═══════════════════════════════════════════════════════════════════
//  Module init
// ═══════════════════════════════════════════════════════════════════

#[pymodule]
fn pyqql_edge(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyStmt>()?;
    m.add_class::<PyHttpEmbedder>()?;
    m.add_class::<PyClient>()?;
    m.add_function(wrap_pyfunction!(local_executor, m)?)?;
    #[cfg(feature = "http-embedding")]
    m.add_function(wrap_pyfunction!(http_executor, m)?)?;
    m.add_function(wrap_pyfunction!(explain, m)?)?;
    m.add_function(wrap_pyfunction!(parse, m)?)?;
    m.add_function(wrap_pyfunction!(parse_all, m)?)?;
    m.add_function(wrap_pyfunction!(parse_batch, m)?)?;
    m.add_function(wrap_pyfunction!(is_valid, m)?)?;
    m.add_function(wrap_pyfunction!(inject_filter, m)?)?;
    m.add_function(wrap_pyfunction!(tokenize, m)?)?;
    m.add_function(wrap_pyfunction!(compile_query, m)?)?;
    Ok(())
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
            items.push((key, py_to_value(&item)?));
        }
        return Ok(Value::Dict(items));
    }
    Err(PySyntaxError::new_err("unsupported filter value type"))
}

fn str_to_comparison_op(op: &str) -> ComparisonOp {
    match op {
        "=" | "==" | "eq" => ComparisonOp::Eq,
        ">" | "gt" => ComparisonOp::Gt,
        ">=" | "gte" => ComparisonOp::Gte,
        "<" | "lt" => ComparisonOp::Lt,
        "<=" | "lte" => ComparisonOp::Lte,
        _ => ComparisonOp::Eq,
    }
}
