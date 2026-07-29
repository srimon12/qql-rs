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

use pyo3::exceptions::{PyRuntimeError, PySyntaxError};
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyList};
use qql_core::ast::{self, ComparisonOp, Value};
use qql_core::lexer::Lexer;
use qql_core::parser::Parser;
use std::sync::atomic::{AtomicBool, Ordering};

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
        let cmp = str_to_comparison_op(op)?;
        ast::inject_filter(&mut self.inner, field, cmp, val).map_err(qql_py_syntax_error)?;
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
            ast::Stmt::ClearPayload(c) => c.shard_key.clone(),
            ast::Stmt::DeleteVector(d) => d.shard_key.clone(),
            ast::Stmt::UpdateVector(u) => u.shard_key.clone(),
            ast::Stmt::UpdatePayload(u) => u.shard_key.clone(),
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
            ast::Stmt::ClearPayload(c) => c.shard_key = key,
            ast::Stmt::DeleteVector(d) => d.shard_key = key,
            ast::Stmt::UpdateVector(u) => u.shard_key = key,
            ast::Stmt::UpdatePayload(u) => u.shard_key = key,
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
    let mut result = Vec::new();
    for token_result in lexer {
        let token = token_result.map_err(qql_py_syntax_error)?;
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
    let stmt = Parser::parse(input).map_err(qql_py_syntax_error)?;
    let (stmt_type, route) = qql_plan::routing::compile_statement(&stmt)
        .map_err(|e| PySyntaxError::new_err(e.to_string()))?;
    let result = serde_json::json!({
        "stmt_type": stmt_type,
        "method": route.method.as_str(),
        "path": route.path,
        "payload": route.body_json().unwrap_or(serde_json::Value::Null),
    });
    pythonize::pythonize(py, &result).map_err(|e| PySyntaxError::new_err(e.to_string()))
}

fn do_explain(py: Python<'_>, query: &Bound<'_, PyAny>) -> PyResult<PyObject> {
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
            dict.set_item("ok", true)?;
            dict.set_item("query", query_str)?;
            dict.set_item("plan", plan)?;
        }
        Err(e) => {
            dict.set_item("ok", false)?;
            dict.set_item("query", query_str)?;
            dict.set_item("error", e.to_string())?;
        }
    }
    Ok(dict.into())
}

#[pyfunction]
fn explain(query: &Bound<'_, PyAny>) -> PyResult<PyObject> {
    let py = query.py();
    do_explain(py, query)
}

// ═══════════════════════════════════════════════════════════════════
//  Edge Client — wraps qql-edge Executor
// ═══════════════════════════════════════════════════════════════════

#[pyclass(name = "Client")]
struct PyClient {
    inner: std::sync::Arc<qql::executor::Executor>,
    runtime: tokio::runtime::Runtime,
    closed: AtomicBool,
}

#[pymethods]
impl PyClient {
    #[pyo3(signature = (query, *, on_error="stop"))]
    fn execute<'py>(
        &self,
        py: Python<'py>,
        query: &Bound<'_, PyAny>,
        on_error: &str,
    ) -> PyResult<Bound<'py, PyAny>> {
        if self.closed.load(Ordering::Acquire) {
            return Err(PyRuntimeError::new_err("client is closed"));
        }
        let out = self.run(query, parse_on_error(on_error)?)?;
        pythonize::pythonize(py, &out)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
    }

    #[pyo3(signature = (query, *, on_error="stop"))]
    fn execute_async<'py>(
        &self,
        py: Python<'py>,
        query: Bound<'py, PyAny>,
        on_error: &str,
    ) -> PyResult<Bound<'py, PyAny>> {
        if self.closed.load(Ordering::Acquire) {
            return Err(PyRuntimeError::new_err("client is closed"));
        }
        let inner = self.inner.clone();
        let input = classify(&query)?;
        let on_error = parse_on_error(on_error)?;
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let val = run_async(&inner, input, on_error)
                .await
                .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
            Python::with_gil(|py| {
                pythonize::pythonize(py, &val)
                    .map(|b| b.unbind())
                    .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
            })
        })
    }

    fn explain(&self, query: &Bound<'_, PyAny>) -> PyResult<PyObject> {
        let py = query.py();
        do_explain(py, query)
    }

    /// Flush and release edge storage. Idempotent.
    fn close(&self) -> PyResult<()> {
        if self.closed.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        self.runtime
            .block_on(self.inner.close())
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
    fn run(
        &self,
        query: &Bound<'_, PyAny>,
        on_error: qql::executor::OnError,
    ) -> PyResult<serde_json::Value> {
        let stop = matches!(on_error, qql::executor::OnError::Stop);
        match classify(query)? {
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

async fn run_async(
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

// ═══════════════════════════════════════════════════════════════════
//  Executor constructors — edge-only, no REST/gRPC
// ═══════════════════════════════════════════════════════════════════

/// Create a fully-local edge executor backed by fastembed-rs and qdrant-edge.
///
/// Args:
///     data_dir: path for on-disk qdrant-edge storage
///     on_disk_payload: store payloads on disk (default True)
///     model: local ONNX model. Accepts enum names (``BGESmallENV15``), HF
///         codes (``Xenova/bge-small-en-v1.5``), or short aliases
///         (``bge-small-en-v1.5``). Default: BGESmallENV15 (384-d).
///     cache_dir: override model cache directory
///     show_download_progress: show HuggingFace download progress (default False)
#[cfg(feature = "fastembed-local")]
#[pyfunction]
#[allow(clippy::too_many_arguments)]
#[pyo3(signature = (data_dir, on_disk_payload=true, *, model=None, multi_model=None, image_model=None, reranker_model=None, cache_dir=None, show_download_progress=false))]
fn local_executor(
    data_dir: &str,
    on_disk_payload: bool,
    model: Option<String>,
    multi_model: Option<String>,
    image_model: Option<String>,
    reranker_model: Option<String>,
    cache_dir: Option<String>,
    show_download_progress: bool,
) -> PyResult<PyClient> {
    let exec = qql_edge::local_executor_with_options(
        data_dir,
        qql_edge::LocalExecutorOptions {
            on_disk_payload,
            model,
            multi_model,
            image_model,
            reranker_model,
            cache_dir: cache_dir.map(std::path::PathBuf::from),
            show_download_progress,
        },
    )
    .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
    Ok(PyClient {
        inner: std::sync::Arc::new(exec),
        runtime: rt,
        closed: AtomicBool::new(false),
    })
}

/// List dense ONNX models available for ``local_executor(model=...)``.
///
/// Returns a list of dicts: ``{name, model_code, dim, description}``.
#[cfg(feature = "fastembed-local")]
#[pyfunction]
fn list_embedding_models(py: Python<'_>) -> PyResult<Bound<'_, PyList>> {
    let models = qql_edge::list_embedding_models();
    let out = PyList::empty(py);
    for m in models {
        let d = PyDict::new(py);
        d.set_item("name", m.name)?;
        d.set_item("multi", m.multi)?;
        d.set_item("image", m.image)?;
        d.set_item("model_code", m.model_code)?;
        d.set_item("dim", m.dim)?;
        d.set_item("description", m.description)?;
        out.append(d)?;
    }
    Ok(out)
}

/// One-shot local execution. Prefer a long-lived `Client` for repeated calls
/// so the model and edge shards stay open.
#[cfg(feature = "fastembed-local")]
#[pyfunction]
#[allow(clippy::too_many_arguments)]
#[pyo3(signature = (query, *, data_dir="./qdrant_data", on_disk_payload=true, model=None, cache_dir=None, show_download_progress=false, on_error="stop"))]
fn execute<'py>(
    py: Python<'py>,
    query: &Bound<'_, PyAny>,
    data_dir: &str,
    on_disk_payload: bool,
    model: Option<String>,
    cache_dir: Option<String>,
    show_download_progress: bool,
    on_error: &str,
) -> PyResult<Bound<'py, PyAny>> {
    let client = local_executor(
        data_dir,
        on_disk_payload,
        model,
        None,
        None,
        None,
        cache_dir,
        show_download_progress,
    )?;
    let report = client.run(query, parse_on_error(on_error)?)?;
    client.close()?;
    pythonize::pythonize(py, &report).map_err(|error| PyRuntimeError::new_err(error.to_string()))
}

/// One-shot asynchronous local execution with the same options as `execute`.
#[cfg(feature = "fastembed-local")]
#[pyfunction]
#[allow(clippy::too_many_arguments)]
#[pyo3(signature = (query, *, data_dir="./qdrant_data", on_disk_payload=true, model=None, cache_dir=None, show_download_progress=false, on_error="stop"))]
fn execute_async<'py>(
    py: Python<'py>,
    query: Bound<'py, PyAny>,
    data_dir: &str,
    on_disk_payload: bool,
    model: Option<String>,
    cache_dir: Option<String>,
    show_download_progress: bool,
    on_error: &str,
) -> PyResult<Bound<'py, PyAny>> {
    let input = classify(&query)?;
    let on_error = parse_on_error(on_error)?;
    let client = local_executor(
        data_dir,
        on_disk_payload,
        model,
        None,
        None,
        None,
        cache_dir,
        show_download_progress,
    )?;
    let inner = client.inner.clone();
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let result = run_async(&inner, input, on_error).await;
        let close_result = inner.close().await;
        let value = result.map_err(qql_py_error)?;
        close_result.map_err(qql_py_error)?;
        Python::with_gil(|py| {
            pythonize::pythonize(py, &value)
                .map(|bound| bound.unbind())
                .map_err(|error| PyRuntimeError::new_err(error.to_string()))
        })
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
        closed: AtomicBool::new(false),
    })
}

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
    Python::with_gil(|py| {
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
