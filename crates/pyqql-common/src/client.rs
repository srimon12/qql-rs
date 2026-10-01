//! Shared client method bodies for the Python SDKs.
//!
//! `pyqql` and `pyqql-edge` expose the same `Client` surface; the bodies live
//! here so the blocking/async dispatch pattern, the GIL-detach placement, and
//! the error mapping cannot drift. The SDK crates keep thin `#[pymethods]`
//! wrappers (argument parsing plus their transport-specific gates).

use std::sync::Arc;

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::PyAny;

use qql::executor::Executor;
use qql_core::ast::Value;
use qql_core::error::QqlError;

use crate::report::PyExecutionReport;
use crate::{
    parse_on_error, prepare_input, qql_py_error, qql_py_value_error, run_analyze_input, run_async,
    run_input, shared_runtime,
};

/// `Client.execute` body: normalize the input, run it on the shared runtime
/// with the GIL released, and wrap the typed report.
pub fn run_execute<'py>(
    py: Python<'py>,
    executor: &Executor,
    query: &Bound<'py, PyAny>,
    params: Option<&Bound<'py, PyAny>>,
    on_error: &str,
) -> PyResult<Bound<'py, PyAny>> {
    let on_error = parse_on_error(on_error)?;
    let input = prepare_input(query, params)?;
    let runtime = shared_runtime()?;
    let report = py.detach(|| run_input(executor, runtime, input, on_error))?;
    Ok(PyExecutionReport::wrap(py, report)?.into_any())
}

/// `Client.execute_async` body.
///
/// The request is spawned onto the shared runtime — the same runtime every
/// transport is bound to — and awaited through its `JoinHandle`, so the
/// runtime can never be dropped mid-request. Cancelling the returned
/// awaitable detaches the spawned task instead of aborting it (Tokio's
/// contract): the request runs to completion on the shared runtime.
pub fn run_execute_async<'py>(
    py: Python<'py>,
    executor: &Arc<Executor>,
    query: Bound<'py, PyAny>,
    params: Option<&Bound<'py, PyAny>>,
    on_error: &str,
) -> PyResult<Bound<'py, PyAny>> {
    let on_error = parse_on_error(on_error)?;
    let input = prepare_input(&query, params)?;
    let runtime = shared_runtime()?;
    let executor = executor.clone();
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let report = runtime
            .spawn(async move { run_async(&executor, input, on_error).await })
            .await
            .map_err(|error| {
                PyRuntimeError::new_err(format!("execute_async background task failed: {error}"))
            })?
            .map_err(qql_py_error)?;
        Python::attach(|py| Ok(PyExecutionReport::wrap(py, report)?.into_any().unbind()))
    })
}

/// `Client.execute_hits` body: run `execute` and return the typed hits of
/// statement `stmt` (Python-style index resolution).
pub fn run_execute_hits<'py>(
    py: Python<'py>,
    executor: &Executor,
    query: &Bound<'py, PyAny>,
    params: Option<&Bound<'py, PyAny>>,
    on_error: &str,
    stmt: isize,
) -> PyResult<Bound<'py, PyAny>> {
    let report = run_execute(py, executor, query, params, on_error)?;
    report.call_method1("hits", (stmt,))
}

/// `Client.execute_async_hits` body — same contract as [`run_execute_hits`],
/// driven on the shared runtime.
pub fn run_execute_async_hits<'py>(
    py: Python<'py>,
    executor: &Arc<Executor>,
    query: Bound<'py, PyAny>,
    params: Option<&Bound<'py, PyAny>>,
    on_error: &str,
    stmt: isize,
) -> PyResult<Bound<'py, PyAny>> {
    let on_error = parse_on_error(on_error)?;
    let input = prepare_input(&query, params)?;
    let runtime = shared_runtime()?;
    let executor = executor.clone();
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let report = runtime
            .spawn(async move { run_async(&executor, input, on_error).await })
            .await
            .map_err(|error| {
                PyRuntimeError::new_err(format!("execute_async background task failed: {error}"))
            })?
            .map_err(qql_py_error)?;
        Python::attach(|py| {
            let report = PyExecutionReport::wrap(py, report)?;
            Ok(report.call_method1("hits", (stmt,))?.unbind())
        })
    })
}

/// `Client.upsert_many` body: validate the row list, convert each row exactly
/// like `bind` params, and run the chunked `:rows` point-splice path.
pub fn run_upsert_many<'py>(
    py: Python<'py>,
    executor: &Executor,
    collection: &str,
    rows: &Bound<'py, PyAny>,
    batch_size: usize,
    on_error: &str,
) -> PyResult<Bound<'py, PyAny>> {
    let on_error = parse_on_error(on_error)?;
    let items: Vec<Bound<'_, PyAny>> = rows.extract().map_err(|_| {
        qql_py_value_error(QqlError::validation(
            "QQL-BIND-TYPE-MISMATCH",
            "upsert_many rows must be a list of point objects ({id, vector, …payload})",
            None,
        ))
    })?;
    let values: Vec<Value> = items
        .iter()
        .map(crate::py_to_value)
        .collect::<PyResult<_>>()?;
    let runtime = shared_runtime()?;
    let report = py
        .detach(|| runtime.block_on(executor.upsert_many(collection, values, batch_size, on_error)))
        .map_err(qql_py_error)?;
    Ok(PyExecutionReport::wrap(py, report)?.into_any())
}

/// `Client.explain_analyze` body: single-statement measured execution,
/// returned as a pythonized dict (no `ExecutionReport` wrapper).
pub fn run_explain_analyze<'py>(
    py: Python<'py>,
    executor: &Executor,
    query: &Bound<'py, PyAny>,
    params: Option<&Bound<'py, PyAny>>,
    on_error: &str,
) -> PyResult<Bound<'py, PyAny>> {
    let on_error = parse_on_error(on_error)?;
    let input = prepare_input(query, params)?;
    let runtime = shared_runtime()?;
    let out = py.detach(|| run_analyze_input(executor, runtime, input, on_error))?;
    pythonize::pythonize(py, &out).map_err(crate::serialize_error)
}
