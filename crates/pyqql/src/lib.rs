//! pyqql — native Python bindings for the QQL parser and runtime.
//!
//! The parser/parameter surface (Stmt, parse/tokenize/bind/explain/compile)
//! lives in `pyqql-common`, shared with `pyqql-edge` so the two SDKs cannot
//! drift; this crate keeps the REST/gRPC client and the module-level one-shot
//! helpers.

use pyo3::prelude::*;
use pyo3::types::PyAny;

use pyqql_common as common;

mod embedder;
pub use embedder::*;

#[pyclass(name = "Client", subclass)]
struct PyClient {
    inner: std::sync::Arc<qql::executor::Executor>,
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
        let exec = create_executor(url, api_key, use_grpc, embedder, route_affinity.clone())?;
        Ok(PyClient {
            inner: std::sync::Arc::new(exec),
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
        let runtime = common::shared_runtime()?;
        Python::attach(|py| py.detach(|| runtime.block_on(self.inner.close())))
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
    /// Point payloads are included by default (pass `WITH PAYLOAD false` to strip them).
    /// Lists of same-collection QUERY statements are automatically batched into
    /// a single network call.
    ///
    /// Note on Formula Queries (vs Qdrant SDK):
    /// In QQL, custom formula scoring uses declarative query strings:
    /// `client.execute("QUERY FORMULA score * 0.8 + views * 0.2 FROM docs LIMIT 10")`
    /// Do not wrap QQL strings in `qdrant_client.models.FormulaQuery`, which expects an
    /// object tree of Expression classes rather than query text. Run them directly here.
    /// Always prefer bare `score` over `$score` to prevent shell variable expansion.
    #[pyo3(signature = (query, *, params=None, on_error="stop"))]
    fn execute<'py>(
        &self,
        py: Python<'py>,
        query: &Bound<'py, PyAny>,
        params: Option<&Bound<'py, PyAny>>,
        on_error: &str,
    ) -> PyResult<Bound<'py, PyAny>> {
        common::client::run_execute(py, &self.inner, query, params, on_error)
    }

    /// Async variant — accepts the same input types as `execute`.
    ///
    /// The request runs on the process-wide Tokio runtime shared with the
    /// transport, so it cannot be killed by collecting the client. Cancelling
    /// the returned awaitable detaches the request instead of aborting it: it
    /// runs to completion.
    #[pyo3(signature = (query, *, params=None, on_error="stop"))]
    fn execute_async<'py>(
        &self,
        py: Python<'py>,
        query: Bound<'py, PyAny>,
        params: Option<&Bound<'py, PyAny>>,
        on_error: &str,
    ) -> PyResult<Bound<'py, PyAny>> {
        common::client::run_execute_async(py, &self.inner, query, params, on_error)
    }

    fn explain(&self, query: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        let py = query.py();
        common::do_explain(py, query)
    }

    /// Analyze a single QQL query string or pre-parsed Stmt: static plan
    /// plus measured execution (per-phase client timings, server time, and
    /// hardware/inference usage). Returns a plain dict (no `ExecutionReport`
    /// wrapper — the shape differs). Batch inputs fail closed.
    #[pyo3(signature = (query, *, params=None, on_error="stop"))]
    fn explain_analyze<'py>(
        &self,
        py: Python<'py>,
        query: &Bound<'py, PyAny>,
        params: Option<&Bound<'py, PyAny>>,
        on_error: &str,
    ) -> PyResult<Bound<'py, PyAny>> {
        common::client::run_explain_analyze(py, &self.inner, query, params, on_error)
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
        common::compile(py, query, params)
    }

    /// Bulk ingest: `rows` is a list of point dicts
    /// (`{id, vector, …payload}`) spliced through the `:rows` point-splice
    /// path in `batch_size` chunks (default 100). Row values convert exactly
    /// like `bind` params — nested dicts, lists, and 1-D float buffers
    /// (numpy, `array.array`, memoryviews) all compose.
    #[pyo3(signature = (collection, rows, *, batch_size=100, on_error="stop"))]
    fn upsert_many<'py>(
        &self,
        py: Python<'py>,
        collection: &str,
        rows: &Bound<'py, PyAny>,
        batch_size: usize,
        on_error: &str,
    ) -> PyResult<Bound<'py, PyAny>> {
        common::client::run_upsert_many(py, &self.inner, collection, rows, batch_size, on_error)
    }
}

#[pymodule]
fn pyqql(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    common::register_error_module("pyqql");
    common::register_report_classes(m)?;
    m.add_class::<common::PyStmt>()?;
    m.add_class::<common::PyHttpEmbedder>()?;
    m.add_class::<PyClient>()?;
    m.add_function(wrap_pyfunction!(common::bind, m)?)?;
    m.add_function(wrap_pyfunction!(common::explain, m)?)?;
    m.add_function(wrap_pyfunction!(common::parse, m)?)?;
    m.add_function(wrap_pyfunction!(common::parse_json, m)?)?;
    m.add_function(wrap_pyfunction!(common::is_valid, m)?)?;
    m.add_function(wrap_pyfunction!(common::inject_filter, m)?)?;
    m.add_function(wrap_pyfunction!(common::tokenize, m)?)?;
    m.add_function(wrap_pyfunction!(common::compile, m)?)?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
