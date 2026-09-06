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

    /// Compile this Stmt directly to its transport route without re-parsing.
    /// Optionally accepts `params` to bind before compiling.
    #[pyo3(signature = (params=None))]
    fn compile_route<'py>(
        &self,
        py: Python<'py>,
        params: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let mut stmt = self.inner.clone();
        bind_py_stmt(&mut stmt, params)?;
        let compiled = qql_plan::routing::compile_statement(&stmt).map_err(qql_py_syntax_error)?;
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
    let statements = Parser::parse_all(input).map_err(qql_py_syntax_error)?;
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

#[pyclass(name = "Client", subclass)]
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
        let input = self.prepare_input(query, params)?;
        let out = py.detach(|| self.run_input(input, oe))?;
        let dict = pythonize::pythonize(py, &out)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
        wrap_execution_report(py, dict)
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
        let oe = parse_on_error(on_error)?;
        let input = self.prepare_input(&query, params)?;
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let val = run_async(&inner, input, oe)
                .await
                .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
            Python::attach(|py| {
                let dict = pythonize::pythonize(py, &val)
                    .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
                let report = wrap_execution_report(py, dict)?;
                Ok(report.unbind())
            })
        })
    }

    #[pyo3(signature = (query, *, params=None, on_error="stop"))]
    fn execute_hits<'py>(
        &self,
        py: Python<'py>,
        query: &Bound<'_, PyAny>,
        params: Option<&Bound<'_, PyAny>>,
        on_error: &str,
    ) -> PyResult<Bound<'py, PyAny>> {
        let rep = self.execute(py, query, params, on_error)?;
        rep.call_method1("hits", (0,))
    }

    #[pyo3(signature = (query, *, params=None, on_error="stop"))]
    fn execute_async_hits<'py>(
        &self,
        py: Python<'py>,
        query: Bound<'py, PyAny>,
        params: Option<&Bound<'_, PyAny>>,
        on_error: &str,
    ) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        let oe = parse_on_error(on_error)?;
        let input = self.prepare_input(&query, params)?;
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let val = run_async(&inner, input, oe)
                .await
                .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
            Python::attach(|py| {
                let dict = pythonize::pythonize(py, &val)
                    .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
                let report = wrap_execution_report(py, dict)?;
                let hits = report.call_method1("hits", (0,))?;
                Ok(hits.unbind())
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

impl PyClient {
    pub(crate) fn prepare_input(
        &self,
        query: &Bound<'_, PyAny>,
        params: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Input> {
        let params_opt = params.filter(|p| !p.is_none());

        // Check if query is a Python list
        if let Ok(list) = query.cast::<pyo3::types::PyList>() {
            if list.is_empty() {
                return Ok(Input::StrList(Vec::new()));
            }

            // Check if params is a list of statement-scoped parameters
            let scoped_params = if let Some(p) = params_opt {
                if let Ok(param_list) = p.cast::<pyo3::types::PyList>() {
                    if param_list.len() == list.len() {
                        let mut v = Vec::with_capacity(param_list.len());
                        for item in param_list.iter() {
                            v.push(item);
                        }
                        Some(v)
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };

            let first = list.get_item(0)?;
            if first.extract::<PyRef<'_, PyStmt>>().is_ok() {
                let mut stmts = Vec::with_capacity(list.len());
                for (i, item) in list.iter().enumerate() {
                    let stmt_params = match &scoped_params {
                        Some(scoped) => Some(&scoped[i]),
                        None => params_opt,
                    };
                    let py_stmt = item.extract::<PyRef<'_, PyStmt>>()?;
                    let mut s = py_stmt.inner.clone();
                    bind_py_stmt(&mut s, stmt_params)?;
                    stmts.push(s);
                }
                return Ok(Input::StmtList(stmts));
            }

            // List of strings
            let mut strs = Vec::with_capacity(list.len());
            for (i, item) in list.iter().enumerate() {
                let s_str = item.extract::<String>().map_err(|_| {
                    pyo3::exceptions::PyTypeError::new_err(
                        "list items must be strings or Stmt objects",
                    )
                })?;
                let stmt_params = match &scoped_params {
                    Some(scoped) => Some(&scoped[i]),
                    None => params_opt,
                };
                let bound = match stmt_params {
                    Some(p) => bind_py_params(&s_str, p, false)?,
                    None => s_str,
                };
                strs.push(bound);
            }
            return Ok(Input::StrList(strs));
        }

        // Single Stmt
        if let Ok(py_stmt) = query.extract::<PyRef<'_, PyStmt>>() {
            let mut stmt = py_stmt.inner.clone();
            bind_py_stmt(&mut stmt, params_opt)?;
            return Ok(Input::Stmt(stmt));
        }

        // Single String (could be a script)
        if let Ok(s) = query.extract::<String>() {
            if let Some(p) = params_opt {
                // If params is a list of statement-scoped dicts, check if it matches the number of statements in the script
                if let Ok(param_list) = p.cast::<pyo3::types::PyList>() {
                    let parsed_stmts = Parser::parse_all(&s).map_err(qql_py_syntax_error)?;
                    if parsed_stmts.len() > 1 && param_list.len() == parsed_stmts.len() {
                        let mut bound_stmts = Vec::with_capacity(parsed_stmts.len());
                        for (i, mut stmt) in parsed_stmts.into_iter().enumerate() {
                            let param_item = param_list.get_item(i)?;
                            bind_py_stmt(&mut stmt, Some(&param_item))?;
                            bound_stmts.push(stmt);
                        }
                        return Ok(Input::StmtList(bound_stmts));
                    }
                }
                let bound = bind_py_params(&s, p, false)?;
                return Ok(Input::String(bound));
            } else {
                return Ok(Input::String(s));
            }
        }

        Err(pyo3::exceptions::PyTypeError::new_err(
            "query must be a str, Stmt, list[str], or list[Stmt]",
        ))
    }
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

/// Substitute named (:name) or positional (?) parameters into a query string or Stmt.
///
/// When `truncate_vectors=True`, long vector literals (e.g. 384 dims) are rendered
/// in a compact human-readable format `[0.12, 0.34, ... (384 dims)]` suitable for logging.
#[pyfunction]
#[pyo3(signature = (query, params=None, *, truncate_vectors=false))]
fn bind<'py>(
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
            Some(p) => bind_py_params(&q_str, p, truncate_vectors)?,
            None => q_str,
        };
        Ok(bound.into_pyobject(py)?.into_any())
    } else {
        Err(pyo3::exceptions::PyTypeError::new_err(
            "query must be a str or Stmt",
        ))
    }
}

pub(crate) fn extract_param_map(
    params: &Bound<'_, PyAny>,
) -> PyResult<(std::collections::HashMap<String, Value>, Vec<Value>)> {
    let mut named = std::collections::HashMap::new();
    let mut positional = Vec::new();

    if params.is_none() {
        return Ok((named, positional));
    }

    if let Ok(dict) = params.cast::<PyDict>() {
        flatten_dict_params(dict, "", &mut named)?;
    } else if let Ok(list) = params.cast::<PyList>() {
        for item in list.iter() {
            positional.push(py_to_value(&item)?);
        }
    } else {
        return Err(PyValueError::new_err(
            "params must be a dict for named parameters (:name) or a list for positional parameters (?)",
        ));
    }

    Ok((named, positional))
}

pub(crate) fn flatten_dict_params(
    dict: &Bound<'_, PyDict>,
    prefix: &str,
    out: &mut std::collections::HashMap<String, Value>,
) -> PyResult<()> {
    for (k, v) in dict.iter() {
        let key = k
            .extract::<String>()
            .map_err(|_| PySyntaxError::new_err("parameter dict keys must be strings"))?;
        let full_key = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };

        let val = py_to_value(&v)?;
        if let Ok(nested_dict) = v.cast::<PyDict>() {
            flatten_dict_params(nested_dict, &full_key, out)?;
        }
        out.insert(full_key, val);
    }
    Ok(())
}

pub(crate) fn bind_py_stmt(
    stmt: &mut ast::Stmt,
    params: Option<&Bound<'_, PyAny>>,
) -> PyResult<()> {
    let Some(p) = params else {
        return Ok(());
    };
    if p.is_none() {
        return Ok(());
    }
    let (named, positional) = extract_param_map(p)?;
    qql_core::params::bind_stmt(stmt, |name| named.get(name).cloned(), &positional)
        .map_err(qql_py_value_error)?;
    Ok(())
}

pub(crate) fn bind_py_params(
    query: &str,
    params: &Bound<'_, PyAny>,
    truncate_vectors: bool,
) -> PyResult<String> {
    if params.is_none() {
        return Ok(query.to_string());
    }
    let (named, positional) = extract_param_map(params)?;
    if !positional.is_empty() {
        if truncate_vectors {
            qql_core::params::bind_positional_readable(query, &positional, 2)
                .map_err(qql_py_value_error)
        } else {
            qql_core::params::bind_positional(query, &positional).map_err(qql_py_value_error)
        }
    } else {
        if truncate_vectors {
            qql_core::params::bind_named_readable(query, |k| named.get(k).cloned(), 2)
                .map_err(qql_py_value_error)
        } else {
            qql_core::params::bind_named(query, |k| named.get(k).cloned())
                .map_err(qql_py_value_error)
        }
    }
}

pub(crate) fn wrap_execution_report<'py>(
    py: Python<'py>,
    dict: Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyAny>> {
    if let Ok(pyqql_edge) = py.import("pyqql_edge") {
        if let Ok(report_cls) = pyqql_edge.getattr("ExecutionReport") {
            if let Ok(report) = report_cls.call1((&dict,)) {
                return Ok(report);
            }
        }
    }
    Ok(dict)
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

fn qql_py_value_error(error: qql_core::error::QqlError) -> pyo3::PyErr {
    attach_qql_error(
        pyo3::exceptions::PyValueError::new_err(error.to_string()),
        error,
    )
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
