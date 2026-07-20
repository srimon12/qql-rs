use pyo3::exceptions::PySyntaxError;
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyList};
use qql_core::ast::{self, Value};
use qql_core::lexer::Lexer;
use qql_core::offline;
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

    let mut config = qql::config::QqlConfig::default();
    config.url = url.to_string();
    config.secret = api_key.clone();

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
    inner: qql::executor::Executor,
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
            inner: exec,
            runtime: rt,
        })
    }

    fn execute<'py>(&self, py: Python<'py>, query: &str) -> PyResult<Bound<'py, PyAny>> {
        let res = self
            .runtime
            .block_on(self.inner.execute(query))
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;

        pythonize::pythonize(py, &res)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
    }

    fn explain(&self, query: &str) -> PyResult<String> {
        qql::executor::Executor::explain(query)
            .map_err(|e| pyo3::exceptions::PySyntaxError::new_err(e.to_string()))
    }
}

// Alias PyExecutor to PyClient for backwards compatibility
type PyExecutor = PyClient;

#[pyfunction]
#[pyo3(signature = (query, url="http://localhost:6333", api_key=None, use_grpc=false, embedder=None))]
fn execute<'py>(
    py: Python<'py>,
    query: &str,
    url: &str,
    api_key: Option<String>,
    use_grpc: bool,
    embedder: Option<&Bound<'_, PyAny>>,
) -> PyResult<Bound<'py, PyAny>> {
    let client = PyClient::new(url, api_key, use_grpc, embedder)?;
    client.execute(py, query)
}

#[pyfunction]
fn explain(query: &str) -> PyResult<String> {
    qql::executor::Executor::explain(query)
        .map_err(|e| pyo3::exceptions::PySyntaxError::new_err(e.to_string()))
}

#[pymodule]
fn pyqql(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyHttpEmbedder>()?;
    m.add_class::<PyClient>()?;
    m.add_function(wrap_pyfunction!(execute, m)?)?;
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
