use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyList};
use qql_core::error::QqlError;
use std::sync::atomic::AtomicBool;

use crate::PyClient;
use pyqql_common::{qql_py_value_error, validate_bm25_text};

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
#[pyo3(signature = (data_dir, on_disk_payload=true, *, model=None, sparse_model=None, multi_model=None, image_model=None, reranker_model=None, cache_dir=None, show_download_progress=false, wal_segment_mb=None, bm25_k1=None, bm25_b=None, bm25_avg_len=None, bm25_language=None, bm25_tokenizer=None, bm25_lowercase=None, bm25_ascii_folding=None, bm25_min_token_len=None, bm25_max_token_len=None, bm25_stopwords=None, bm25_stemmer=None, bm25_stopwords_languages=None))]
pub fn local_executor(
    py: Python<'_>,
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
    bm25_k1: Option<f64>,
    bm25_b: Option<f64>,
    bm25_avg_len: Option<f64>,
    bm25_language: Option<String>,
    bm25_tokenizer: Option<String>,
    bm25_lowercase: Option<bool>,
    bm25_ascii_folding: Option<bool>,
    bm25_min_token_len: Option<usize>,
    bm25_max_token_len: Option<usize>,
    bm25_stopwords: Option<Vec<String>>,
    bm25_stemmer: Option<String>,
    bm25_stopwords_languages: Option<Vec<String>>,
) -> PyResult<PyClient> {
    let wal_segment_capacity = wal_segment_capacity(wal_segment_mb).map_err(qql_py_value_error)?;
    validate_bm25_text(
        bm25_k1,
        bm25_b,
        bm25_avg_len,
        bm25_language.as_deref(),
        bm25_tokenizer.as_deref(),
        bm25_lowercase,
        bm25_ascii_folding,
        bm25_stopwords.clone(),
        bm25_stemmer.as_deref(),
        bm25_min_token_len,
        bm25_max_token_len,
        bm25_stopwords_languages.clone(),
    )?;
    // A cold fastembed load downloads ONNX weights from HuggingFace; release
    // the GIL for the whole construction so no other Python thread freezes,
    // and make sure the process-wide runtime exists before returning.
    let exec = py.detach(|| {
        pyqql_common::shared_runtime()?;
        qql_edge::local_executor_with_options(
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
                bm25_k1,
                bm25_b,
                bm25_avg_len,
                bm25_language,
                bm25_tokenizer,
                bm25_lowercase,
                bm25_ascii_folding,
                bm25_min_token_len,
                bm25_max_token_len,
                bm25_stopwords,
                bm25_stemmer,
                bm25_stopwords_languages,
            },
        )
        .map_err(|e| PyRuntimeError::new_err(e.to_string()))
    })?;
    Ok(PyClient {
        inner: std::sync::Arc::new(exec),
        closed: AtomicBool::new(false),
    })
}

/// One-shot local execution. Prefer a long-lived `Client` for repeated calls
/// so the model and edge shards stay open. Close errors are propagated
/// (matching the async one-shot) after a successful statement.
#[cfg(feature = "fastembed-local")]
#[pyfunction]
#[allow(clippy::too_many_arguments)]
#[pyo3(signature = (query, *, params=None, data_dir="./qdrant_data", on_disk_payload=true, model=None, sparse_model=None, multi_model=None, image_model=None, reranker_model=None, cache_dir=None, show_download_progress=false, bm25_k1=None, bm25_b=None, bm25_avg_len=None, bm25_language=None, bm25_tokenizer=None, bm25_lowercase=None, bm25_ascii_folding=None, bm25_min_token_len=None, bm25_max_token_len=None, bm25_stopwords=None, bm25_stemmer=None, bm25_stopwords_languages=None, on_error="stop"))]
pub fn execute<'py>(
    py: Python<'py>,
    query: &Bound<'py, PyAny>,
    params: Option<&Bound<'py, PyAny>>,
    data_dir: &str,
    on_disk_payload: bool,
    model: Option<String>,
    sparse_model: Option<String>,
    multi_model: Option<String>,
    image_model: Option<String>,
    reranker_model: Option<String>,
    cache_dir: Option<String>,
    show_download_progress: bool,
    bm25_k1: Option<f64>,
    bm25_b: Option<f64>,
    bm25_avg_len: Option<f64>,
    bm25_language: Option<String>,
    bm25_tokenizer: Option<String>,
    bm25_lowercase: Option<bool>,
    bm25_ascii_folding: Option<bool>,
    bm25_min_token_len: Option<usize>,
    bm25_max_token_len: Option<usize>,
    bm25_stopwords: Option<Vec<String>>,
    bm25_stemmer: Option<String>,
    bm25_stopwords_languages: Option<Vec<String>>,
    on_error: &str,
) -> PyResult<Bound<'py, PyAny>> {
    let client = local_executor(
        py,
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
        bm25_k1,
        bm25_b,
        bm25_avg_len,
        bm25_language,
        bm25_tokenizer,
        bm25_lowercase,
        bm25_ascii_folding,
        bm25_min_token_len,
        bm25_max_token_len,
        bm25_stopwords,
        bm25_stemmer,
        bm25_stopwords_languages,
    )?;
    // Statement errors win over a secondary close error; a successful
    // statement still fails when the flush does (no silent data loss).
    let res = client.execute(py, query, params, on_error);
    match (res, client.close()) {
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Ok(report), Ok(())) => Ok(report),
    }
}

/// One-shot asynchronous local execution with the same options as `execute`.
#[cfg(feature = "fastembed-local")]
#[pyfunction]
#[allow(clippy::too_many_arguments)]
#[pyo3(signature = (query, *, params=None, data_dir="./qdrant_data", on_disk_payload=true, model=None, sparse_model=None, multi_model=None, image_model=None, reranker_model=None, cache_dir=None, show_download_progress=false, bm25_k1=None, bm25_b=None, bm25_avg_len=None, bm25_language=None, bm25_tokenizer=None, bm25_lowercase=None, bm25_ascii_folding=None, bm25_min_token_len=None, bm25_max_token_len=None, bm25_stopwords=None, bm25_stemmer=None, bm25_stopwords_languages=None, on_error="stop"))]
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
    bm25_k1: Option<f64>,
    bm25_b: Option<f64>,
    bm25_avg_len: Option<f64>,
    bm25_language: Option<String>,
    bm25_tokenizer: Option<String>,
    bm25_lowercase: Option<bool>,
    bm25_ascii_folding: Option<bool>,
    bm25_min_token_len: Option<usize>,
    bm25_max_token_len: Option<usize>,
    bm25_stopwords: Option<Vec<String>>,
    bm25_stemmer: Option<String>,
    bm25_stopwords_languages: Option<Vec<String>>,
    on_error: &str,
) -> PyResult<Bound<'py, PyAny>> {
    let client = local_executor(
        py,
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
        bm25_k1,
        bm25_b,
        bm25_avg_len,
        bm25_language,
        bm25_tokenizer,
        bm25_lowercase,
        bm25_ascii_folding,
        bm25_min_token_len,
        bm25_max_token_len,
        bm25_stopwords,
        bm25_stemmer,
        bm25_stopwords_languages,
    )?;
    let input = pyqql_common::prepare_input(&query, params)?;
    let on_error = pyqql_common::parse_on_error(on_error)?;
    let inner = client.inner.clone();
    let runtime = pyqql_common::shared_runtime()?;
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let (result, close_result) = runtime
            .spawn(async move {
                let result = pyqql_common::run_async(&inner, input, on_error).await;
                let close_result = inner.close().await;
                (result, close_result)
            })
            .await
            .map_err(|error| {
                PyRuntimeError::new_err(format!("execute_async background task failed: {error}"))
            })?;
        let report = result.map_err(pyqql_common::qql_py_error)?;
        close_result.map_err(pyqql_common::qql_py_error)?;
        Python::attach(|py| {
            Ok(pyqql_common::PyExecutionReport::wrap(py, report)?
                .into_any()
                .unbind())
        })
    })
}

/// Build the `HttpEmbedderOptions` for `http_executor`, forwarding every
/// BM25 knob — including `bm25_stopwords_languages`, the one that used to be
/// validated and then swallowed by `..Default::default()`.
///
/// Exposed for the forwarding test; production callers use `http_executor`.
#[cfg(feature = "http-embedding")]
#[allow(clippy::too_many_arguments)]
fn http_embedder_options(
    url: &str,
    embed_key: &str,
    embed_model: &str,
    embed_dim: usize,
    bm25_k1: Option<f64>,
    bm25_b: Option<f64>,
    bm25_avg_len: Option<f64>,
    bm25_language: Option<String>,
    bm25_tokenizer: Option<String>,
    bm25_lowercase: Option<bool>,
    bm25_ascii_folding: Option<bool>,
    bm25_stopwords: Option<Vec<String>>,
    bm25_stemmer: Option<String>,
    bm25_min_token_len: Option<usize>,
    bm25_max_token_len: Option<usize>,
    bm25_stopwords_languages: Option<Vec<String>>,
) -> qql::embedder::HttpEmbedderOptions {
    qql::embedder::HttpEmbedderOptions {
        endpoint: url.to_string(),
        api_key: embed_key.to_string(),
        model: embed_model.to_string(),
        dimension: embed_dim,
        bm25_k1,
        bm25_b,
        bm25_avg_len,
        bm25_language,
        bm25_tokenizer,
        bm25_lowercase,
        bm25_ascii_folding,
        bm25_stopwords,
        bm25_stemmer,
        bm25_min_token_len,
        bm25_max_token_len,
        bm25_stopwords_languages,
        ..Default::default()
    }
}

#[cfg(feature = "http-embedding")]
#[pyfunction]
#[allow(clippy::too_many_arguments)]
#[pyo3(signature = (data_dir, url, embed_key, embed_model, embed_dim, on_disk_payload=true, *, bm25_k1=None, bm25_b=None, bm25_avg_len=None, bm25_language=None, bm25_tokenizer=None, bm25_lowercase=None, bm25_ascii_folding=None, bm25_stopwords=None, bm25_stemmer=None, bm25_min_token_len=None, bm25_max_token_len=None, bm25_stopwords_languages=None))]
pub fn http_executor(
    py: Python<'_>,
    data_dir: &str,
    url: &str,
    embed_key: &str,
    embed_model: &str,
    embed_dim: usize,
    on_disk_payload: bool,
    bm25_k1: Option<f64>,
    bm25_b: Option<f64>,
    bm25_avg_len: Option<f64>,
    bm25_language: Option<String>,
    bm25_tokenizer: Option<String>,
    bm25_lowercase: Option<bool>,
    bm25_ascii_folding: Option<bool>,
    bm25_stopwords: Option<Vec<String>>,
    bm25_stemmer: Option<String>,
    bm25_min_token_len: Option<usize>,
    bm25_max_token_len: Option<usize>,
    bm25_stopwords_languages: Option<Vec<String>>,
) -> PyResult<PyClient> {
    validate_bm25_text(
        bm25_k1,
        bm25_b,
        bm25_avg_len,
        bm25_language.as_deref(),
        bm25_tokenizer.as_deref(),
        bm25_lowercase,
        bm25_ascii_folding,
        bm25_stopwords.clone(),
        bm25_stemmer.as_deref(),
        bm25_min_token_len,
        bm25_max_token_len,
        bm25_stopwords_languages.clone(),
    )?;
    let options = http_embedder_options(
        url,
        embed_key,
        embed_model,
        embed_dim,
        bm25_k1,
        bm25_b,
        bm25_avg_len,
        bm25_language,
        bm25_tokenizer,
        bm25_lowercase,
        bm25_ascii_folding,
        bm25_stopwords,
        bm25_stemmer,
        bm25_min_token_len,
        bm25_max_token_len,
        bm25_stopwords_languages,
    );
    let exec = py.detach(|| {
        pyqql_common::shared_runtime()?;
        qql_edge::http_executor_with_options_and_wal(data_dir, on_disk_payload, None, options)
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))
    })?;
    Ok(PyClient {
        inner: std::sync::Arc::new(exec),
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

    #[cfg(feature = "http-embedding")]
    #[test]
    fn http_executor_forwards_every_bm25_knob() {
        let options = http_embedder_options(
            "http://localhost:9/v1/embeddings",
            "key",
            "model",
            3,
            Some(2.0),
            Some(0.5),
            Some(8.0),
            Some("spanish".to_string()),
            Some("whitespace".to_string()),
            Some(false),
            Some(true),
            Some(vec!["el".to_string()]),
            Some("none".to_string()),
            Some(2),
            Some(9),
            Some(vec!["fr".to_string()]),
        );
        assert_eq!(options.bm25_k1, Some(2.0));
        assert_eq!(options.bm25_b, Some(0.5));
        assert_eq!(options.bm25_avg_len, Some(8.0));
        assert_eq!(options.bm25_language.as_deref(), Some("spanish"));
        assert_eq!(options.bm25_tokenizer.as_deref(), Some("whitespace"));
        assert_eq!(options.bm25_lowercase, Some(false));
        assert_eq!(options.bm25_ascii_folding, Some(true));
        assert_eq!(
            options.bm25_stopwords.as_deref(),
            Some(&["el".to_string()][..])
        );
        assert_eq!(options.bm25_stemmer.as_deref(), Some("none"));
        assert_eq!(options.bm25_min_token_len, Some(2));
        assert_eq!(options.bm25_max_token_len, Some(9));
        assert_eq!(
            options.bm25_stopwords_languages.as_deref(),
            Some(&["fr".to_string()][..]),
            "bm25_stopwords_languages must reach the embedder options"
        );
    }
}
