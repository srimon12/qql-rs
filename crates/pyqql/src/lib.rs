use pyo3::exceptions::PySyntaxError;
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyList};
use qql_core::ast::{self, Value};
use qql_core::lexer::Lexer;
use qql_core::offline;
use qql_core::parser::Parser;

#[pyclass(name = "Stmt")]
#[derive(Clone)]
pub struct PyStmt {
    pub inner: qql_core::ast::Stmt,
}

#[pymethods]
impl PyStmt {
    fn inject_filter(&mut self, field: &str, op: &str, value: &Bound<'_, PyAny>) -> PyResult<()> {
        let val = py_to_value(value)?;
        ast::inject_filter(&mut self.inner, field, op, &val);
        Ok(())
    }

    fn to_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        pythonize::pythonize(py, &self.inner).map_err(|e| PySyntaxError::new_err(e.to_string()))
    }
}

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
    let val = py_to_value(value)?;
    if let Ok(mut py_stmt) = query.extract::<PyRefMut<'_, PyStmt>>() {
        ast::inject_filter(&mut py_stmt.inner, field, op, &val);
        Ok(py_stmt.clone())
    } else if let Ok(query_str) = query.extract::<String>() {
        let mut stmt =
            Parser::parse(&query_str).map_err(|e| PySyntaxError::new_err(e.to_string()))?;
        ast::inject_filter(&mut stmt, field, op, &val);
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
        d.set_item("pos", token.pos as i64)?;
        result.push(d);
    }
    Ok(result)
}

#[pyfunction]
fn compile_query<'py>(py: Python<'py>, input: &str) -> PyResult<Bound<'py, PyAny>> {
    let compiled = offline::compile(input).map_err(|e| PySyntaxError::new_err(e.to_string()))?;
    pythonize::pythonize(py, &compiled).map_err(|e| PySyntaxError::new_err(e.to_string()))
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
            if let Some(v) = dict.get_item("endpoint")? {
                ep = v.extract::<String>().ok();
            }
            if let Some(v) = dict.get_item("api_key")? {
                ep_key = v.extract::<String>().ok();
            }
            if let Some(v) = dict.get_item("model")? {
                model = v.extract::<String>().ok();
            }
            if let Some(v) = dict.get_item("dimension")? {
                dim = v.extract::<usize>().ok();
            }
        }
    }
    Ok((ep, ep_key, model, dim))
}

fn create_executor(
    url: &str,
    api_key: Option<String>,
    use_grpc: bool,
    embedder: Option<&Bound<'_, PyAny>>,
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

    let client: Box<dyn qql::client::QdrantOps> = if use_grpc {
        #[cfg(feature = "grpc")]
        {
            Box::new(
                qql::grpc::GrpcQdrant::from_url(url, api_key)
                    .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?,
            )
        }
        #[cfg(not(feature = "grpc"))]
        {
            return Err(pyo3::exceptions::PyRuntimeError::new_err(
                "gRPC feature not enabled in this build",
            ));
        }
    } else {
        Box::new(
            qql::rest::RestQdrant::new(url.to_string(), api_key)
                .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?,
        )
    };

    let embedder_impl = if let Some(endpoint) = &config.embedding_endpoint {
        if !endpoint.trim().is_empty() {
            let api_key = config.embedding_api_key.clone().unwrap_or_default();
            let model = config.embedding_model.clone().unwrap_or_default();
            let dim = config.embedding_dimension;
            let http_emb = qql::embedder::HttpEmbedder::new(endpoint.clone(), api_key, model, dim)
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
}

#[pymethods]
impl PyClient {
    #[new]
    #[pyo3(signature = (url="http://localhost:6333", api_key=None, use_grpc=false, embedder=None))]
    fn new(
        url: &str,
        api_key: Option<String>,
        use_grpc: bool,
        embedder: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let (exec, rt) = create_executor(url, api_key, use_grpc, embedder)?;
        Ok(PyClient {
            inner: std::sync::Arc::new(exec),
            runtime: rt,
        })
    }

    fn execute<'py>(
        &self,
        py: Python<'py>,
        query: &Bound<'_, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let res = if let Ok(py_stmt) = query.extract::<PyRef<PyStmt>>() {
            self.runtime
                .block_on(self.inner.execute_node(py_stmt.inner.clone()))
                .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?
        } else if let Ok(query_str) = query.extract::<String>() {
            self.runtime
                .block_on(self.inner.execute(&query_str))
                .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?
        } else {
            return Err(pyo3::exceptions::PyTypeError::new_err(
                "query must be a string or a Stmt object",
            ));
        };

        pythonize::pythonize(py, &res)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
    }

    #[pyo3(signature = (query))]
    fn execute_async<'py>(
        &self,
        py: Python<'py>,
        query: Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        let is_stmt = query.extract::<PyRef<PyStmt>>().is_ok();
        let query_str = if !is_stmt {
            Some(query.extract::<String>()?)
        } else {
            None
        };
        let py_stmt = if is_stmt {
            Some(query.extract::<PyRef<PyStmt>>()?.inner.clone())
        } else {
            None
        };

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let res = if let Some(stmt) = py_stmt {
                inner
                    .execute_node(stmt)
                    .await
                    .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?
            } else if let Some(q_str) = query_str {
                inner
                    .execute(&q_str)
                    .await
                    .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?
            } else {
                return Err(pyo3::exceptions::PyTypeError::new_err(
                    "query must be a string or a Stmt object",
                ));
            };
            let py_val = Python::with_gil(|py| {
                pythonize::pythonize(py, &res)
                    .map(|b| b.unbind())
                    .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
            })?;
            Ok(py_val)
        })
    }

    fn explain(&self, query: &Bound<'_, PyAny>) -> PyResult<String> {
        if let Ok(py_stmt) = query.extract::<PyRef<PyStmt>>() {
            qql::executor::Executor::explain_node(&py_stmt.inner)
                .map_err(|e| pyo3::exceptions::PySyntaxError::new_err(e.to_string()))
        } else if let Ok(query_str) = query.extract::<String>() {
            qql::executor::Executor::explain(&query_str)
                .map_err(|e| pyo3::exceptions::PySyntaxError::new_err(e.to_string()))
        } else {
            Err(pyo3::exceptions::PyTypeError::new_err(
                "query must be a string or a Stmt object",
            ))
        }
    }
}

#[pyfunction]
#[pyo3(signature = (query, url="http://localhost:6333", api_key=None, use_grpc=false, embedder=None))]
fn execute<'py>(
    py: Python<'py>,
    query: &Bound<'_, PyAny>,
    url: &str,
    api_key: Option<String>,
    use_grpc: bool,
    embedder: Option<&Bound<'_, PyAny>>,
) -> PyResult<Bound<'py, PyAny>> {
    let client = PyClient::new(url, api_key, use_grpc, embedder)?;
    client.execute(py, query)
}

#[pyfunction]
#[pyo3(signature = (query, url="http://localhost:6333", api_key=None, use_grpc=false, embedder=None))]
fn execute_async<'py>(
    py: Python<'py>,
    query: Bound<'py, PyAny>,
    url: &str,
    api_key: Option<String>,
    use_grpc: bool,
    embedder: Option<&Bound<'_, PyAny>>,
) -> PyResult<Bound<'py, PyAny>> {
    let (inner, rt) = create_executor(url, api_key, use_grpc, embedder)?;
    let inner_arc = std::sync::Arc::new(inner);
    let is_stmt = query.extract::<PyRef<PyStmt>>().is_ok();
    let query_str = if !is_stmt {
        Some(query.extract::<String>()?)
    } else {
        None
    };
    let py_stmt = if is_stmt {
        Some(query.extract::<PyRef<PyStmt>>()?.inner.clone())
    } else {
        None
    };

    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let res = if let Some(stmt) = py_stmt {
            inner_arc
                .execute_node(stmt)
                .await
                .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?
        } else if let Some(q_str) = query_str {
            inner_arc
                .execute(&q_str)
                .await
                .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?
        } else {
            return Err(pyo3::exceptions::PyTypeError::new_err(
                "query must be a string or a Stmt object",
            ));
        };
        let _rt_keepalive = rt;
        let py_val = Python::with_gil(|py| {
            pythonize::pythonize(py, &res)
                .map(|b| b.unbind())
                .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
        })?;
        Ok(py_val)
    })
}

#[pyfunction]
fn explain(query: &Bound<'_, PyAny>) -> PyResult<String> {
    if let Ok(py_stmt) = query.extract::<PyRef<PyStmt>>() {
        qql::executor::Executor::explain_node(&py_stmt.inner)
            .map_err(|e| pyo3::exceptions::PySyntaxError::new_err(e.to_string()))
    } else if let Ok(query_str) = query.extract::<String>() {
        qql::executor::Executor::explain(&query_str)
            .map_err(|e| pyo3::exceptions::PySyntaxError::new_err(e.to_string()))
    } else {
        Err(pyo3::exceptions::PyTypeError::new_err(
            "query must be a string or a Stmt object",
        ))
    }
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
    m.add_function(wrap_pyfunction!(parse_all, m)?)?;
    m.add_function(wrap_pyfunction!(parse_batch, m)?)?;
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
