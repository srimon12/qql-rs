//! pyqql — native Python bindings for the QQL parser and runtime.
//!
//! The parser/parameter surface (Stmt, parse/tokenize/bind/explain/compile)
//! lives in `pyqql-common`, shared with `pyqql-edge` so the two SDKs cannot
//! drift; this crate keeps the REST/gRPC client, the HTTP embedder, and the
//! module-level one-shot helpers.

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::PyAny;

use pyqql_common as common;

mod embedder;
pub use embedder::*;

#[pyclass(name = "Client", subclass)]
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
            .map_err(common::qql_py_error)
    }

    /// Whether `close()` has been called on this client.
    #[getter]
    fn is_closed(&self) -> bool {
        self.inner.is_closed()
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
        let oe = common::parse_on_error(on_error)?;
        let input = common::prepare_input(query, params)?;
        let out = py.detach(|| common::run_input(&self.inner, &self.runtime, input, oe))?;
        pythonize::pythonize(py, &out).map_err(|e| PyRuntimeError::new_err(e.to_string()))
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
        let oe = common::parse_on_error(on_error)?;
        let input = common::prepare_input(&query, params)?;
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let val = common::run_async(&inner, input, oe)
                .await
                .map_err(common::qql_py_error)?;
            Python::attach(|py| {
                pythonize::pythonize(py, &val)
                    .map(|b| b.unbind())
                    .map_err(|e| PyRuntimeError::new_err(e.to_string()))
            })
        })
    }

    fn explain(&self, query: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        let py = query.py();
        common::do_explain(py, query)
    }

    /// Compile a QQL query to its transport route without executing (parity with nqql).
    /// Optionally accepts `params` to bind before compiling.
    #[pyo3(signature = (query, params=None))]
    fn compile<'py>(
        &self,
        py: Python<'py>,
        query: &str,
        params: Option<&Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        common::compile_query(py, query, params)
    }
}

// ── module-level one-shots ────────────────────────────────────────

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

#[pymodule]
fn pyqql(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    common::register_error_module("pyqql");
    m.add_class::<common::PyStmt>()?;
    m.add_class::<PyHttpEmbedder>()?;
    m.add_class::<PyClient>()?;
    m.add_function(wrap_pyfunction!(execute, m)?)?;
    m.add_function(wrap_pyfunction!(execute_async, m)?)?;
    m.add_function(wrap_pyfunction!(common::bind, m)?)?;
    m.add_function(wrap_pyfunction!(common::explain, m)?)?;
    m.add_function(wrap_pyfunction!(common::parse, m)?)?;
    m.add_function(wrap_pyfunction!(common::parse_json, m)?)?;
    m.add_function(wrap_pyfunction!(common::is_valid, m)?)?;
    m.add_function(wrap_pyfunction!(common::inject_filter, m)?)?;
    m.add_function(wrap_pyfunction!(common::tokenize, m)?)?;
    m.add_function(wrap_pyfunction!(common::compile_query, m)?)?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
