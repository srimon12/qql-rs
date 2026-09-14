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
//!
//! The parser/parameter surface (Stmt, parse/tokenize/bind/explain/compile)
//! lives in `pyqql-common`, shared with `pyqql` so the two SDKs cannot drift;
//! this crate keeps the edge client, executor constructors, and one-shot
//! helpers.

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::PyAny;
use std::sync::atomic::{AtomicBool, Ordering};

use pyqql_common as common;

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
            return Err(common::client_closed_error());
        }
        let oe = common::parse_on_error(on_error)?;
        let input = common::prepare_input(query, params)?;
        let report = py.detach(|| common::run_input(&self.inner, &self.runtime, input, oe))?;
        Ok(common::PyExecutionReport::wrap(py, report)?.into_any())
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
            return Err(common::client_closed_error());
        }
        let inner = self.inner.clone();
        let oe = common::parse_on_error(on_error)?;
        let input = common::prepare_input(&query, params)?;
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let report = common::run_async(&inner, input, oe)
                .await
                .map_err(common::qql_py_error)?;
            Python::attach(|py| {
                Ok(common::PyExecutionReport::wrap(py, report)?
                    .into_any()
                    .unbind())
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
        let oe = common::parse_on_error(on_error)?;
        let input = common::prepare_input(&query, params)?;
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let report = common::run_async(&inner, input, oe)
                .await
                .map_err(common::qql_py_error)?;
            Python::attach(|py| {
                let report = common::PyExecutionReport::wrap(py, report)?;
                let hits = report.call_method1("hits", (0,))?;
                Ok(hits.unbind())
            })
        })
    }

    fn explain(&self, query: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        let py = query.py();
        common::do_explain(py, query)
    }

    /// Analyze a single QQL query string or pre-parsed Stmt: static plan
    /// plus measured execution (per-phase client timings, server time, and
    /// hardware/inference usage). Returns a plain dict. Batch inputs fail
    /// closed. (Parity with `pyqql.Client.explain_analyze`.)
    #[pyo3(signature = (query, *, params=None, on_error="stop"))]
    fn explain_analyze<'py>(
        &self,
        py: Python<'py>,
        query: &Bound<'_, PyAny>,
        params: Option<&Bound<'_, PyAny>>,
        on_error: &str,
    ) -> PyResult<Bound<'py, PyAny>> {
        if self.closed.load(Ordering::Acquire) {
            return Err(common::client_closed_error());
        }
        let oe = common::parse_on_error(on_error)?;
        let input = common::prepare_input(query, params)?;
        let out = py.detach(|| common::run_analyze_input(&self.inner, &self.runtime, input, oe))?;
        pythonize::pythonize(py, &out).map_err(|e| PyRuntimeError::new_err(e.to_string()))
    }

    /// Compile a QQL query to its transport route without executing (parity
    /// with `pyqql.Client.compile` / `nqql-edge` `Client.compile`).
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
        rows: &Bound<'_, PyAny>,
        batch_size: usize,
        on_error: &str,
    ) -> PyResult<Bound<'py, PyAny>> {
        if self.closed.load(Ordering::Acquire) {
            return Err(common::client_closed_error());
        }
        let oe = common::parse_on_error(on_error)?;
        let items: Vec<Bound<'_, PyAny>> = rows.extract().map_err(|_| {
            common::qql_py_value_error(qql_core::error::QqlError::validation(
                "QQL-BIND-TYPE-MISMATCH",
                "upsert_many rows must be a list of point objects ({id, vector, …payload})",
                None,
            ))
        })?;
        let values: Vec<qql_core::ast::Value> = items
            .iter()
            .map(common::py_to_value)
            .collect::<PyResult<_>>()?;
        let report = py
            .detach(|| {
                self.runtime
                    .block_on(self.inner.upsert_many(collection, values, batch_size, oe))
            })
            .map_err(common::qql_py_error)?;
        Ok(common::PyExecutionReport::wrap(py, report)?.into_any())
    }

    /// Flush and release edge storage. Idempotent.
    fn close(&self) -> PyResult<()> {
        if self.closed.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        Python::attach(|py| py.detach(|| self.runtime.block_on(self.inner.close())))
            .map_err(common::qql_py_error)
    }

    /// Run backend storage optimizers (segment merge / index build) and return
    /// whether anything was optimized. Edge-only: remote backends answer
    /// `QQL-BACKEND-OPTIMIZE`.
    fn optimize(&self, py: Python<'_>, collection: &str) -> PyResult<bool> {
        if self.closed.load(Ordering::Acquire) {
            return Err(common::client_closed_error());
        }
        py.detach(|| {
            self.runtime
                .block_on(self.inner.client().optimize_collection(collection))
        })
        .map_err(common::qql_py_error)
    }

    /// Whether `close()` has been called on this client.
    #[getter]
    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
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

mod models;
pub use models::*;

// ═══════════════════════════════════════════════════════════════════
//  Module init
// ═══════════════════════════════════════════════════════════════════

#[pymodule]
fn pyqql_edge(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    common::register_error_module("pyqql_edge");
    common::register_report_classes(m)?;
    m.add_class::<common::PyStmt>()?;
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
