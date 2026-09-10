use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyList};
use qql_core::error::QqlError;
use std::sync::atomic::AtomicBool;

use crate::PyClient;
use pyqql_common::qql_py_value_error;

/// Parse ``wal_segment_mb`` (whole MiB) into the byte capacity
/// [`qql_edge::LocalExecutorOptions::wal_segment_capacity`] expects.
///
/// ``None`` keeps the qdrant-edge default (32 MiB). Zero, negative,
/// fractional, non-finite, or overflowing values fail closed with
/// ``QQL-VALIDATION-CONFIG`` instead of silently rounding or clamping.
#[cfg(feature = "fastembed-local")]
fn wal_segment_capacity(mb: Option<f64>) -> Result<Option<usize>, QqlError> {
    let Some(mb) = mb else {
        return qql_edge::wal_segment_capacity_bytes(None);
    };
    if !mb.is_finite() || mb.fract() != 0.0 || mb < 1.0 {
        return Err(QqlError::validation(
            "QQL-VALIDATION-CONFIG",
            "wal_segment_mb must be a positive whole number of MiB; omit it for the qdrant-edge 32 MiB default",
            None,
        ));
    }
    // `as` saturates; the shared helper rejects any byte count that overflows.
    qql_edge::wal_segment_capacity_bytes(Some(mb as u64))
}

/// List dense ONNX models available for ``local_executor(model=...)``.
///
/// Returns a list of dicts: ``{name, model_code, dim, description}``.
#[cfg(feature = "fastembed-local")]
#[pyfunction]
pub fn list_embedding_models(py: Python<'_>) -> PyResult<Bound<'_, PyList>> {
    let models = qql_edge::list_embedding_models();
    let out = PyList::empty(py);
    let name_key = pyo3::intern!(py, "name");
    let multi_key = pyo3::intern!(py, "multi");
    let image_key = pyo3::intern!(py, "image");
    let model_code_key = pyo3::intern!(py, "model_code");
    let dim_key = pyo3::intern!(py, "dim");
    let description_key = pyo3::intern!(py, "description");
    for m in models {
        let d = PyDict::new(py);
        d.set_item(name_key, m.name)?;
        d.set_item(multi_key, m.multi)?;
        d.set_item(image_key, m.image)?;
        d.set_item(model_code_key, m.model_code)?;
        d.set_item(dim_key, m.dim)?;
        d.set_item(description_key, m.description)?;
        out.append(d)?;
    }
    Ok(out)
}

#[cfg(feature = "fastembed-local")]
#[pyfunction]
#[allow(clippy::too_many_arguments)]
#[pyo3(signature = (data_dir, on_disk_payload=true, *, model=None, sparse_model=None, multi_model=None, image_model=None, reranker_model=None, cache_dir=None, show_download_progress=false, wal_segment_mb=None))]
pub fn local_executor(
    data_dir: &str,
    on_disk_payload: bool,
    model: Option<String>,
    sparse_model: Option<String>,
    multi_model: Option<String>,
    image_model: Option<String>,
    reranker_model: Option<String>,
    cache_dir: Option<String>,
    show_download_progress: bool,
    wal_segment_mb: Option<f64>,
) -> PyResult<PyClient> {
    let wal_segment_capacity = wal_segment_capacity(wal_segment_mb).map_err(qql_py_value_error)?;
    let exec = qql_edge::local_executor_with_options(
        data_dir,
        qql_edge::LocalExecutorOptions {
            on_disk_payload,
            wal_segment_capacity,
            model,
            sparse_model,
            multi_model,
            image_model,
            reranker_model,
            cache_dir: cache_dir.map(std::path::PathBuf::from),
            show_download_progress,
        },
    )
    .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
    Ok(PyClient {
        inner: std::sync::Arc::new(exec),
        runtime: rt,
        closed: AtomicBool::new(false),
    })
}

/// One-shot local execution. Prefer a long-lived `Client` for repeated calls
/// so the model and edge shards stay open.
#[cfg(feature = "fastembed-local")]
#[pyfunction]
#[allow(clippy::too_many_arguments)]
#[pyo3(signature = (query, *, params=None, data_dir="./qdrant_data", on_disk_payload=true, model=None, sparse_model=None, multi_model=None, image_model=None, reranker_model=None, cache_dir=None, show_download_progress=false, on_error="stop"))]
pub fn execute<'py>(
    py: Python<'py>,
    query: &Bound<'_, PyAny>,
    params: Option<&Bound<'_, PyAny>>,
    data_dir: &str,
    on_disk_payload: bool,
    model: Option<String>,
    sparse_model: Option<String>,
    multi_model: Option<String>,
    image_model: Option<String>,
    reranker_model: Option<String>,
    cache_dir: Option<String>,
    show_download_progress: bool,
    on_error: &str,
) -> PyResult<Bound<'py, PyAny>> {
    let client = local_executor(
        data_dir,
        on_disk_payload,
        model,
        sparse_model,
        multi_model,
        image_model,
        reranker_model,
        cache_dir,
        show_download_progress,
        None,
    )?;
    let res = client.execute(py, query, params, on_error);
    let _ = client.close();
    res
}

/// One-shot asynchronous local execution with the same options as `execute`.
#[cfg(feature = "fastembed-local")]
#[pyfunction]
#[allow(clippy::too_many_arguments)]
#[pyo3(signature = (query, *, params=None, data_dir="./qdrant_data", on_disk_payload=true, model=None, sparse_model=None, multi_model=None, image_model=None, reranker_model=None, cache_dir=None, show_download_progress=false, on_error="stop"))]
pub fn execute_async<'py>(
    py: Python<'py>,
    query: Bound<'py, PyAny>,
    params: Option<&Bound<'_, PyAny>>,
    data_dir: &str,
    on_disk_payload: bool,
    model: Option<String>,
    sparse_model: Option<String>,
    multi_model: Option<String>,
    image_model: Option<String>,
    reranker_model: Option<String>,
    cache_dir: Option<String>,
    show_download_progress: bool,
    on_error: &str,
) -> PyResult<Bound<'py, PyAny>> {
    let client = local_executor(
        data_dir,
        on_disk_payload,
        model,
        sparse_model,
        multi_model,
        image_model,
        reranker_model,
        cache_dir,
        show_download_progress,
        None,
    )?;
    let input = pyqql_common::prepare_input(&query, params)?;
    let on_error = pyqql_common::parse_on_error(on_error)?;
    let inner = client.inner.clone();
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let result = pyqql_common::run_async(&inner, input, on_error).await;
        let close_result = inner.close().await;
        let report = result.map_err(pyqql_common::qql_py_error)?;
        close_result.map_err(pyqql_common::qql_py_error)?;
        Python::attach(|py| {
            Ok(pyqql_common::PyExecutionReport::wrap(py, report)?
                .into_any()
                .unbind())
        })
    })
}

#[cfg(feature = "http-embedding")]
#[pyfunction]
#[pyo3(signature = (data_dir, url, embed_key, embed_model, embed_dim, on_disk_payload=true))]
pub fn http_executor(
    data_dir: &str,
    url: &str,
    embed_key: &str,
    embed_model: &str,
    embed_dim: usize,
    on_disk_payload: bool,
) -> PyResult<PyClient> {
    let exec = qql_edge::http_executor(
        data_dir,
        on_disk_payload,
        url.to_string(),
        embed_key.to_string(),
        embed_model.to_string(),
        embed_dim,
    )
    .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
    Ok(PyClient {
        inner: std::sync::Arc::new(exec),
        runtime: rt,
        closed: AtomicBool::new(false),
    })
}

#[cfg(test)]
#[cfg(feature = "fastembed-local")]
mod tests {
    use super::*;

    #[test]
    fn wal_segment_mb_converts_to_bytes() {
        assert_eq!(wal_segment_capacity(None).unwrap(), None);
        assert_eq!(wal_segment_capacity(Some(1.0)).unwrap(), Some(1 << 20));
        assert_eq!(wal_segment_capacity(Some(4.0)).unwrap(), Some(4 << 20));
    }

    #[test]
    fn wal_segment_mb_rejects_invalid_values() {
        for bad in [0.0, -1.0, 1.5, f64::NAN, f64::INFINITY, 1e30] {
            let err = wal_segment_capacity(Some(bad))
                .expect_err(&format!("wal_segment_mb={bad} must fail closed"));
            assert_eq!(err.code, "QQL-VALIDATION-CONFIG", "wal_segment_mb={bad}");
        }
    }
}
