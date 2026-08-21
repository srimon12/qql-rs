use pyo3::exceptions::{PySyntaxError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyList};
use qql_core::ast::{self, ComparisonOp, Value};
use qql_core::lexer::Lexer;
use qql_core::parser::Parser;

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
        ast::inject_filter(&mut self.inner, field, cmp, val)
            .map_err(|e| PySyntaxError::new_err(e.to_string()))?;
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
        let val =
            serde_json::to_value(&self.inner).map_err(|e| PySyntaxError::new_err(e.to_string()))?;
        pythonize::pythonize(py, &val).map_err(|e| PySyntaxError::new_err(e.to_string()))
    }
}

/// Parse a QQL source into a list of Stmt objects.
/// Accepts single statements and semicolon-delimited scripts.
#[pyfunction]
fn parse(input: &str) -> PyResult<Vec<PyStmt>> {
    let stmts = Parser::parse_all(input).map_err(|e| PySyntaxError::new_err(e.to_string()))?;
    Ok(stmts.into_iter().map(|s| PyStmt { inner: s }).collect())
}

/// Parse a QQL source and return the canonical AST as a JSON string without
/// creating Python objects for every node (parity with `nqql.parseJson`).
#[pyfunction]
fn parse_json(input: &str) -> PyResult<String> {
    let statements = Parser::parse_all(input).map_err(|e| PySyntaxError::new_err(e.to_string()))?;
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

#[pyclass(name = "HttpEmbedder")]
#[derive(Clone)]
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
        if endpoint.trim().is_empty() {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "embedding endpoint is required",
            ));
        }
        if model.trim().is_empty() {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "embedding model is required",
            ));
        }
        if dimension == 0 {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "embedding dimension must be positive",
            ));
        }
        Ok(PyHttpEmbedder {
            endpoint: endpoint.to_string(),
            api_key: api_key.unwrap_or_default(),
            model: model.to_string(),
            dimension,
        })
    }
}

#[allow(clippy::type_complexity)]
fn extract_embedder_config(
    embedder: Option<&Bound<'_, PyAny>>,
) -> PyResult<(
    Option<String>,
    Option<String>,
    Option<String>,
    Option<usize>,
)> {
    let mut ep = None;
    let mut ep_key = None;
    let mut model = None;
    let mut dim = None;

    if let Some(emb) = embedder {
        if let Ok(py_emb) = emb.extract::<PyRef<PyHttpEmbedder>>() {
            ep = Some(py_emb.endpoint.clone());
            ep_key = Some(py_emb.api_key.clone());
            model = Some(py_emb.model.clone());
            dim = Some(py_emb.dimension);
        } else if let Ok(dict) = emb.downcast::<PyDict>() {
            ep = Some(
                dict.get_item("endpoint")?
                    .ok_or_else(|| {
                        pyo3::exceptions::PyValueError::new_err("embedder.endpoint is required")
                    })?
                    .extract::<String>()?,
            );
            model = Some(
                dict.get_item("model")?
                    .ok_or_else(|| {
                        pyo3::exceptions::PyValueError::new_err("embedder.model is required")
                    })?
                    .extract::<String>()?,
            );
            dim = Some(
                dict.get_item("dimension")?
                    .ok_or_else(|| {
                        pyo3::exceptions::PyValueError::new_err("embedder.dimension is required")
                    })?
                    .extract::<usize>()?,
            );
            ep_key = dict
                .get_item("api_key")?
                .map(|value| value.extract::<String>())
                .transpose()?;
            // multi_* keys are applied later on config; parse into side channel via attributes
            // is handled in create_executor when building HttpEmbedderOptions from full dict.
            if ep.as_ref().is_some_and(|value| value.trim().is_empty()) {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "embedder.endpoint must not be empty",
                ));
            }
            if model.as_ref().is_some_and(|value| value.trim().is_empty()) {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "embedder.model must not be empty",
                ));
            }
            if dim == Some(0) {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "embedder.dimension must be positive",
                ));
            }
        } else {
            return Err(pyo3::exceptions::PyTypeError::new_err(
                "embedder must be an HttpEmbedder or dict",
            ));
        }
    }
    Ok((ep, ep_key, model, dim))
}

#[allow(clippy::too_many_arguments)]
fn create_executor(
    url: &str,
    api_key: Option<String>,
    use_grpc: bool,
    embedder: Option<&Bound<'_, PyAny>>,
    route_affinity: Option<String>,
) -> PyResult<(qql::executor::Executor, tokio::runtime::Runtime)> {
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;

    let (ep, ep_key, model, dim) = extract_embedder_config(embedder)?;

    let mut config = qql::config::QqlConfig {
        url: url.to_string(),
        secret: api_key.clone(),
        ..Default::default()
    };

    if let Some(endpoint) = ep {
        config.embedding_endpoint = Some(endpoint);
        config.embedding_api_key = ep_key;
        config.embedding_model = model;
        config.embedding_dimension = dim.unwrap_or(0);
    }
    // Optional multi/ColBERT fields from embedder dict.
    if let Some(emb) = embedder {
        if let Ok(dict) = emb.downcast::<PyDict>() {
            if let Ok(Some(v)) = dict.get_item("multi_endpoint") {
                config.multi_embedding_endpoint = Some(v.extract::<String>()?);
            }
            if let Ok(Some(v)) = dict.get_item("multi_api_key") {
                config.multi_embedding_api_key = Some(v.extract::<String>()?);
            }
            if let Ok(Some(v)) = dict.get_item("multi_model") {
                config.multi_embedding_model = Some(v.extract::<String>()?);
            }
            if let Ok(Some(v)) = dict.get_item("multi_dimension") {
                config.multi_embedding_dimension = v.extract::<usize>()?;
            }
            if let Ok(Some(v)) = dict.get_item("image_endpoint") {
                config.image_embedding_endpoint = Some(v.extract::<String>()?);
            }
            if let Ok(Some(v)) = dict.get_item("image_api_key") {
                config.image_embedding_api_key = Some(v.extract::<String>()?);
            }
            if let Ok(Some(v)) = dict.get_item("image_model") {
                config.image_embedding_model = Some(v.extract::<String>()?);
            }
            if let Ok(Some(v)) = dict.get_item("image_dimension") {
                config.image_embedding_dimension = v.extract::<usize>()?;
            }
            if let Ok(Some(v)) = dict.get_item("rerank_endpoint") {
                config.rerank_endpoint = Some(v.extract::<String>()?);
            }
            if let Ok(Some(v)) = dict.get_item("rerank_api_key") {
                config.rerank_api_key = Some(v.extract::<String>()?);
            }
            if let Ok(Some(v)) = dict.get_item("rerank_model") {
                config.rerank_model = Some(v.extract::<String>()?);
            }
        }
    }

    let client: Box<dyn qql::client::QdrantOps> = if use_grpc {
        #[cfg(feature = "grpc")]
        {
            let mut grpc = qql::grpc::GrpcQdrant::from_url(url, api_key)
                .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
            if let Some(affinity) = route_affinity.as_deref() {
                grpc = grpc.with_route_affinity(affinity);
            }
            Box::new(grpc)
        }
        #[cfg(not(feature = "grpc"))]
        {
            return Err(pyo3::exceptions::PyRuntimeError::new_err(
                "gRPC feature not enabled in this build",
            ));
        }
    } else {
        let mut rest = qql::rest::RestQdrant::new(url.to_string(), api_key);
        if let Some(affinity) = route_affinity.as_deref() {
            rest = rest.with_route_affinity(affinity);
        }
        Box::new(rest)
    };

    let embedder_impl = if let Some(endpoint) = &config.embedding_endpoint {
        if !endpoint.trim().is_empty() {
            let http_emb =
                qql::embedder::HttpEmbedder::try_with_options(qql::embedder::HttpEmbedderOptions {
                    endpoint: endpoint.clone(),
                    api_key: config.embedding_api_key.clone().unwrap_or_default(),
                    model: config.embedding_model.clone().unwrap_or_default(),
                    dimension: config.embedding_dimension,
                    multi_endpoint: config.multi_embedding_endpoint.clone(),
                    multi_api_key: config.multi_embedding_api_key.clone(),
                    multi_model: config.multi_embedding_model.clone(),
                    multi_dimension: config.multi_embedding_dimension,
                    image_endpoint: config.image_embedding_endpoint.clone(),
                    image_api_key: config.image_embedding_api_key.clone(),
                    image_model: config.image_embedding_model.clone(),
                    image_dimension: config.image_embedding_dimension,
                    rerank_endpoint: config.rerank_endpoint.clone(),
                    rerank_api_key: config.rerank_api_key.clone(),
                    rerank_model: config.rerank_model.clone(),
                })
                .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
            Some(std::sync::Arc::new(http_emb) as std::sync::Arc<dyn qql::embedder::Embedder>)
        } else {
            None
        }
    } else {
        None
    };

    let exec = qql::executor::Executor::with_embedder(client, Some(config), embedder_impl);
    Ok((exec, rt))
}

#[pyclass(name = "Client")]
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

    /// Execute a QQL query string, a pre-parsed Stmt, or a list of either.
    /// Lists of same-collection QUERY statements are automatically batched
    /// into a single network call.
    #[pyo3(signature = (query, *, on_error="stop"))]
    fn execute<'py>(
        &self,
        py: Python<'py>,
        query: &Bound<'_, PyAny>,
        on_error: &str,
    ) -> PyResult<Bound<'py, PyAny>> {
        let oe = parse_on_error(on_error)?;
        let out = self.classify_and_run(query, oe)?;
        pythonize::pythonize(py, &out)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
    }

    /// Async variant — accepts the same input types as `execute`.
    #[pyo3(signature = (query, *, on_error="stop"))]
    fn execute_async<'py>(
        &self,
        py: Python<'py>,
        query: Bound<'py, PyAny>,
        on_error: &str,
    ) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        let oe = parse_on_error(on_error)?;
        let classified = self.classify(&query)?;
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let val = Self::run_async(&inner, classified, oe)
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

    fn classify_and_run(
        &self,
        query: &Bound<'_, PyAny>,
        on_error: qql::executor::OnError,
    ) -> PyResult<serde_json::Value> {
        let stop = matches!(on_error, qql::executor::OnError::Stop);
        match self.classify(query)? {
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
#[pyo3(signature = (query, *, url="http://localhost:6333", api_key=None, use_grpc=false, embedder=None, on_error="stop", route_affinity=None))]
#[allow(clippy::too_many_arguments)]
fn execute<'py>(
    py: Python<'py>,
    query: &Bound<'_, PyAny>,
    url: &str,
    api_key: Option<String>,
    use_grpc: bool,
    embedder: Option<&Bound<'_, PyAny>>,
    on_error: &str,
    route_affinity: Option<String>,
) -> PyResult<Bound<'py, PyAny>> {
    let client = PyClient::new(url, api_key, use_grpc, embedder, route_affinity)?;
    client.execute(py, query, on_error)
}

#[pyfunction]
#[pyo3(signature = (query, *, url="http://localhost:6333", api_key=None, use_grpc=false, embedder=None, on_error="stop", route_affinity=None))]
#[allow(clippy::too_many_arguments)]
fn execute_async<'py>(
    py: Python<'py>,
    query: Bound<'py, PyAny>,
    url: &str,
    api_key: Option<String>,
    use_grpc: bool,
    embedder: Option<&Bound<'_, PyAny>>,
    on_error: &str,
    route_affinity: Option<String>,
) -> PyResult<Bound<'py, PyAny>> {
    let client = PyClient::new(url, api_key, use_grpc, embedder, route_affinity)?;
    client.execute_async(py, query, on_error)
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

#[pymodule]
fn pyqql(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyStmt>()?;
    m.add_class::<PyHttpEmbedder>()?;
    m.add_class::<PyClient>()?;
    m.add_function(wrap_pyfunction!(execute, m)?)?;
    m.add_function(wrap_pyfunction!(execute_async, m)?)?;
    m.add_function(wrap_pyfunction!(explain, m)?)?;
    m.add_function(wrap_pyfunction!(parse, m)?)?;
    m.add_function(wrap_pyfunction!(parse_json, m)?)?;
    m.add_function(wrap_pyfunction!(is_valid, m)?)?;
    m.add_function(wrap_pyfunction!(inject_filter, m)?)?;
    m.add_function(wrap_pyfunction!(tokenize, m)?)?;
    m.add_function(wrap_pyfunction!(compile_query, m)?)?;
    Ok(())
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
