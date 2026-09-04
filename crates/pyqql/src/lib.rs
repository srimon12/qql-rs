use pyo3::exceptions::{PyRuntimeError, PySyntaxError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyList};
use qql_core::ast::{self, ComparisonOp, Value};
use qql_core::error::QqlError;
use qql_core::lexer::Lexer;
use qql_core::parser::Parser;

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
    fn compile_route<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let compiled =
            qql_plan::routing::compile_statement(&self.inner).map_err(qql_py_syntax_error)?;
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

/// Parse a QQL source into a list of Stmt objects.
/// Accepts single statements and semicolon-delimited scripts.
#[pyfunction]
fn parse(input: &str) -> PyResult<Vec<PyStmt>> {
    let stmts = Parser::parse_all(input).map_err(qql_py_syntax_error)?;
    Ok(stmts.into_iter().map(|s| PyStmt { inner: s }).collect())
}

/// Parse a QQL source and return the canonical AST as a JSON string without
/// creating Python objects for every node (parity with `nqql.parseJson`).
#[pyfunction]
fn parse_json(input: &str) -> PyResult<String> {
    let statements = Parser::parse_all(input).map_err(qql_py_syntax_error)?;
    serde_json::to_string(&statements).map_err(|e| PySyntaxError::new_err(e.to_string()))
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

mod embedder;
pub use embedder::*;

#[pyclass(name = "Client", frozen)]
struct PyClient {
    inner: std::sync::Arc<qql::executor::Executor>,
    runtime: tokio::runtime::Runtime,
    /// Normalized `X-Qdrant-Route-Affinity` / gRPC metadata value; `None` = unset.
    route_affinity: Option<String>,
}

#[pymethods]
impl PyClient {
    #[new]
    #[pyo3(signature = (url="http://localhost:6333", api_key=None, use_grpc=false, embedder=None, route_affinity=None))]
    fn new(
        url: &str,
        api_key: Option<String>,
        use_grpc: bool,
        embedder: Option<&Bound<'_, PyAny>>,
        route_affinity: Option<String>,
    ) -> PyResult<Self> {
        let route_affinity = route_affinity.filter(|s| !s.is_empty());
        let (exec, rt) = create_executor(url, api_key, use_grpc, embedder, route_affinity.clone())?;
        Ok(PyClient {
            inner: std::sync::Arc::new(exec),
            runtime: rt,
            route_affinity,
        })
    }

    /// Read affinity key pinning reads to a stable replica
    /// (`X-Qdrant-Route-Affinity` header / gRPC metadata, Qdrant 1.19+).
    /// Set at construction via `Client(..., route_affinity=...)`.
    #[getter]
    fn route_affinity(&self) -> Option<String> {
        self.route_affinity.clone()
    }

    /// Close the client and release underlying connections.
    fn close(&self) -> PyResult<()> {
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

    /// Execute a QQL query string, a pre-parsed Stmt, or a list of either.
    ///
    /// Supports all QQL retrieval, mutation, DDL, and aggregation operations
    /// (`QUERY`, `SCROLL`, `COUNT`, `FACET`, `UPSERT`, `UPDATE`, `DELETE`, etc.).
    /// Queries include point payloads by default (`WITH PAYLOAD true`).
    /// Lists of same-collection QUERY statements are automatically batched into
    /// a single network call.
    #[pyo3(signature = (query, *, params=None, on_error="stop"))]
    fn execute<'py>(
        &self,
        py: Python<'py>,
        query: &Bound<'_, PyAny>,
        params: Option<&Bound<'_, PyAny>>,
        on_error: &str,
    ) -> PyResult<Bound<'py, PyAny>> {
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
            self.classify(query)?
        };
        let out = py.detach(|| self.run_input(input, oe))?;
        pythonize::pythonize(py, &out)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
    }

    /// Async variant — accepts the same input types as `execute`.
    ///
    /// Executes the QQL pipeline asynchronously on the Tokio background runtime.
    #[pyo3(signature = (query, *, params=None, on_error="stop"))]
    fn execute_async<'py>(
        &self,
        py: Python<'py>,
        query: Bound<'py, PyAny>,
        params: Option<&Bound<'_, PyAny>>,
        on_error: &str,
    ) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
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
            self.classify(&query)?
        };
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let val = Self::run_async(&inner, input, oe)
                .await
                .map_err(qql_py_error)?;
            Python::attach(|py| {
                pythonize::pythonize(py, &val)
                    .map(|b| b.unbind())
                    .map_err(|e| PyRuntimeError::new_err(e.to_string()))
            })
        })
    }

    fn explain(&self, query: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        let py = query.py();
        do_explain(py, query)
    }

    /// Compile a QQL query to its transport route without executing (parity with nqql).
    fn compile<'py>(&self, py: Python<'py>, query: &str) -> PyResult<Bound<'py, PyAny>> {
        compile_query(py, query)
    }
}
// ── internal dispatch (plain impl) ────────────────────────────────

enum Input {
    String(String),
    Stmt(ast::Stmt),
    StrList(Vec<String>),
    StmtList(Vec<ast::Stmt>),
}

impl PyClient {
    fn classify(&self, query: &Bound<'_, PyAny>) -> PyResult<Input> {
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
                    pyo3::exceptions::PyTypeError::new_err(
                        "list items must be strings or Stmt objects",
                    )
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
                    .map_err(qql_py_error)?;
                Ok(serde_json::to_value(&report).unwrap_or_default())
            }
            Input::Stmt(s) => {
                let results = self
                    .runtime
                    .block_on(self.inner.execute_batch_nodes(vec![s], stop))
                    .map_err(qql_py_error)?;
                let report = qql::executor::ExecutionReport::from_results(results);
                Ok(serde_json::to_value(&report).unwrap_or_default())
            }
            Input::StrList(strs) => {
                let refs: Vec<&str> = strs.iter().map(|s| s.as_str()).collect();
                let report = self
                    .runtime
                    .block_on(self.inner.execute_batch(&refs, on_error))
                    .map_err(qql_py_error)?;
                Ok(serde_json::to_value(&report).unwrap_or_default())
            }
            Input::StmtList(stmts) => {
                let results = self
                    .runtime
                    .block_on(self.inner.execute_batch_nodes(stmts, stop))
                    .map_err(qql_py_error)?;
                let report = qql::executor::ExecutionReport::from_results(results);
                Ok(serde_json::to_value(&report).unwrap_or_default())
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
}

// ── free functions ────────────────────────────────────────────────

#[pyfunction]
#[pyo3(signature = (query, *, params=None, url="http://localhost:6333", api_key=None, use_grpc=false, embedder=None, on_error="stop", route_affinity=None))]
#[allow(clippy::too_many_arguments)]
fn execute<'py>(
    py: Python<'py>,
    query: &Bound<'_, PyAny>,
    params: Option<&Bound<'_, PyAny>>,
    url: &str,
    api_key: Option<String>,
    use_grpc: bool,
    embedder: Option<&Bound<'_, PyAny>>,
    on_error: &str,
    route_affinity: Option<String>,
) -> PyResult<Bound<'py, PyAny>> {
    let client = PyClient::new(url, api_key, use_grpc, embedder, route_affinity)?;
    client.execute(py, query, params, on_error)
}

#[pyfunction]
#[pyo3(signature = (query, *, params=None, url="http://localhost:6333", api_key=None, use_grpc=false, embedder=None, on_error="stop", route_affinity=None))]
#[allow(clippy::too_many_arguments)]
fn execute_async<'py>(
    py: Python<'py>,
    query: Bound<'py, PyAny>,
    params: Option<&Bound<'_, PyAny>>,
    url: &str,
    api_key: Option<String>,
    use_grpc: bool,
    embedder: Option<&Bound<'_, PyAny>>,
    on_error: &str,
    route_affinity: Option<String>,
) -> PyResult<Bound<'py, PyAny>> {
    let client = PyClient::new(url, api_key, use_grpc, embedder, route_affinity)?;
    client.execute_async(py, query, params, on_error)
}

/// Substitute `:name` (dict) or `?` (list) placeholders into a query string.
#[pyfunction]
#[pyo3(signature = (query, params=None))]
fn bind(query: &str, params: Option<&Bound<'_, PyAny>>) -> PyResult<String> {
    match params {
        Some(p) => bind_py_params(query, p),
        None => Ok(query.to_string()),
    }
}

fn bind_py_params(query: &str, params: &Bound<'_, PyAny>) -> PyResult<String> {
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
        qql_core::params::bind_named(query, |k| map.get(k).cloned()).map_err(qql_py_value_error)
    } else if let Ok(list) = params.cast::<PyList>() {
        let mut items = Vec::with_capacity(list.len());
        for item in list.iter() {
            items.push(py_to_value(&item)?);
        }
        qql_core::params::bind_positional(query, &items).map_err(qql_py_value_error)
    } else {
        Err(PyValueError::new_err(
            "params must be a dict for named parameters (:name) or a list for positional parameters (?)",
        ))
    }
}

fn parse_on_error(s: &str) -> PyResult<qql::executor::OnError> {
    match s {
        "stop" => Ok(qql::executor::OnError::Stop),
        "continue" => Ok(qql::executor::OnError::Continue),
        _ => Err(pyo3::exceptions::PyValueError::new_err(
            "on_error must be 'stop' or 'continue'",
        )),
    }
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

#[pymodule]
fn pyqql(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyStmt>()?;
    m.add_class::<PyHttpEmbedder>()?;
    m.add_class::<PyClient>()?;
    m.add_function(wrap_pyfunction!(execute, m)?)?;
    m.add_function(wrap_pyfunction!(execute_async, m)?)?;
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

fn qql_py_error(error: QqlError) -> PyErr {
    attach_qql_error(PyRuntimeError::new_err(error.to_string()), error)
}

fn qql_py_syntax_error(error: QqlError) -> PyErr {
    attach_qql_error(PySyntaxError::new_err(error.to_string()), error)
}

fn qql_py_value_error(error: QqlError) -> PyErr {
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
