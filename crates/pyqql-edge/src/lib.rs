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
            return Err(PyRuntimeError::new_err("client is closed"));
        }
        let oe = common::parse_on_error(on_error)?;
        let input = common::prepare_input(query, params)?;
        let out = py.detach(|| common::run_input(&self.inner, &self.runtime, input, oe))?;
        let dict =
            pythonize::pythonize(py, &out).map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        common::wrap_execution_report(py, dict, "pyqql_edge")
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
        let oe = common::parse_on_error(on_error)?;
        let input = common::prepare_input(&query, params)?;
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let val = common::run_async(&inner, input, oe)
                .await
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
            Python::attach(|py| {
                let dict = pythonize::pythonize(py, &val)
                    .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
                let report = common::wrap_execution_report(py, dict, "pyqql_edge")?;
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
        let oe = common::parse_on_error(on_error)?;
        let input = common::prepare_input(&query, params)?;
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let val = common::run_async(&inner, input, oe)
                .await
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
            Python::attach(|py| {
                let dict = pythonize::pythonize(py, &val)
                    .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
                let report = common::wrap_execution_report(py, dict, "pyqql_edge")?;
                let hits = report.call_method1("hits", (0,))?;
                Ok(hits.unbind())
            })
        })
    }

    fn explain(&self, query: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        let py = query.py();
        common::do_explain(py, query)
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

    /// Flush and release edge storage. Idempotent.
    fn close(&self) -> PyResult<()> {
        if self.closed.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        Python::attach(|py| py.detach(|| self.runtime.block_on(self.inner.close())))
            .map_err(common::qql_py_error)
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
