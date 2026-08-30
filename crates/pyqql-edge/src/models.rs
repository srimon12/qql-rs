use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyList};
use std::sync::atomic::AtomicBool;

use crate::{bind_py_params, classify, parse_on_error, qql_py_error, run_async, Input, PyClient};

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
#[pyo3(signature = (data_dir, on_disk_payload=true, *, model=None, sparse_model=None, multi_model=None, image_model=None, reranker_model=None, cache_dir=None, show_download_progress=false))]
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
) -> PyResult<PyClient> {
    let exec = qql_edge::local_executor_with_options(
        data_dir,
        qql_edge::LocalExecutorOptions {
            on_disk_payload,
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
    let input = if let Some(p) = params {
        if let Ok(q_str) = query.extract::<String>() {
            let bound = bind_py_params(&q_str, p)?;
            Input::String(bound)
        } else {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "parameter binding requires a query string",
            ));
        }
    } else {
        classify(&query)?
    };
    let on_error = parse_on_error(on_error)?;
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
    )?;
    let inner = client.inner.clone();
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let result = run_async(&inner, input, on_error).await;
        let close_result = inner.close().await;
        let value = result.map_err(qql_py_error)?;
        close_result.map_err(qql_py_error)?;
        Python::attach(|py| {
            pythonize::pythonize(py, &value)
                .map(|bound| bound.unbind())
                .map_err(|error| PyRuntimeError::new_err(error.to_string()))
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
