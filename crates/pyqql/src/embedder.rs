use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict};

#[pyclass(name = "HttpEmbedder", frozen, from_py_object)]
#[derive(Clone)]
pub struct PyHttpEmbedder {
    pub endpoint: String,
    pub api_key: String,
    pub model: String,
    pub dimension: usize,
    pub multi_endpoint: Option<String>,
    pub multi_api_key: Option<String>,
    pub multi_model: Option<String>,
    pub multi_dimension: usize,
    pub image_endpoint: Option<String>,
    pub image_api_key: Option<String>,
    pub image_model: Option<String>,
    pub image_dimension: usize,
    pub rerank_endpoint: Option<String>,
    pub rerank_api_key: Option<String>,
    pub rerank_model: Option<String>,
    pub bm25_k1: Option<f64>,
    pub bm25_b: Option<f64>,
    pub bm25_avg_len: Option<f64>,
}

#[pymethods]
impl PyHttpEmbedder {
    #[new]
    #[pyo3(signature = (endpoint, model, dimension, api_key=None, multi_endpoint=None, multi_api_key=None, multi_model=None, multi_dimension=None, image_endpoint=None, image_api_key=None, image_model=None, image_dimension=None, rerank_endpoint=None, rerank_api_key=None, rerank_model=None, bm25_k1=None, bm25_b=None, bm25_avg_len=None))]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        endpoint: &str,
        model: &str,
        dimension: usize,
        api_key: Option<String>,
        multi_endpoint: Option<String>,
        multi_api_key: Option<String>,
        multi_model: Option<String>,
        multi_dimension: Option<usize>,
        image_endpoint: Option<String>,
        image_api_key: Option<String>,
        image_model: Option<String>,
        image_dimension: Option<usize>,
        rerank_endpoint: Option<String>,
        rerank_api_key: Option<String>,
        rerank_model: Option<String>,
        bm25_k1: Option<f64>,
        bm25_b: Option<f64>,
        bm25_avg_len: Option<f64>,
    ) -> PyResult<Self> {
        if endpoint.trim().is_empty() {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "embedding endpoint is required",
            ));
        }
        if model.trim().is_empty() {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "embedding model is required",
            ));
        }
        if dimension == 0 {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "embedding dimension must be positive",
            ));
        }
        validate_bm25(bm25_k1, bm25_b, bm25_avg_len)?;
        Ok(PyHttpEmbedder {
            endpoint: endpoint.to_string(),
            api_key: api_key.unwrap_or_default(),
            model: model.to_string(),
            dimension,
            multi_endpoint: multi_endpoint.filter(|s| !s.trim().is_empty()),
            multi_api_key,
            multi_model: multi_model.filter(|s| !s.trim().is_empty()),
            multi_dimension: multi_dimension.unwrap_or(0),
            image_endpoint: image_endpoint.filter(|s| !s.trim().is_empty()),
            image_api_key,
            image_model: image_model.filter(|s| !s.trim().is_empty()),
            image_dimension: image_dimension.unwrap_or(0),
            rerank_endpoint: rerank_endpoint.filter(|s| !s.trim().is_empty()),
            rerank_api_key,
            rerank_model: rerank_model.filter(|s| !s.trim().is_empty()),
            bm25_k1,
            bm25_b,
            bm25_avg_len,
        })
    }
}

/// Validate client-side BM25 overrides eagerly so bad values raise
/// `ValueError` at `HttpEmbedder(...)` construction (`QQL-VALIDATION-CONFIG`).
fn validate_bm25(k1: Option<f64>, b: Option<f64>, avg_len: Option<f64>) -> PyResult<()> {
    qql::embedder::Bm25Params::resolve(k1, b, avg_len)
        .map(|_| ())
        .map_err(pyqql_common::qql_py_value_error)
}

/// Full embedder configuration shared by the class and dict paths.
#[derive(Debug, Clone, Default)]
pub struct ParsedEmbedderConfig {
    pub endpoint: Option<String>,
    pub api_key: Option<String>,
    pub model: Option<String>,
    pub dimension: Option<usize>,
    pub multi_endpoint: Option<String>,
    pub multi_api_key: Option<String>,
    pub multi_model: Option<String>,
    pub multi_dimension: usize,
    pub image_endpoint: Option<String>,
    pub image_api_key: Option<String>,
    pub image_model: Option<String>,
    pub image_dimension: usize,
    pub rerank_endpoint: Option<String>,
    pub rerank_api_key: Option<String>,
    pub rerank_model: Option<String>,
    pub bm25_k1: Option<f64>,
    pub bm25_b: Option<f64>,
    pub bm25_avg_len: Option<f64>,
}

fn opt_string_key(dict: &Bound<'_, PyDict>, key: &str) -> PyResult<Option<String>> {
    let Some(value) = dict.get_item(key)? else {
        return Ok(None);
    };
    if value.is_none() {
        return Ok(None);
    }
    Ok(Some(value.extract::<String>()?))
}

fn opt_nonempty_string_key(dict: &Bound<'_, PyDict>, key: &str) -> PyResult<Option<String>> {
    Ok(opt_string_key(dict, key)?.filter(|s| !s.trim().is_empty()))
}

fn opt_usize_key(dict: &Bound<'_, PyDict>, key: &str) -> PyResult<usize> {
    let Some(value) = dict.get_item(key)? else {
        return Ok(0);
    };
    if value.is_none() {
        return Ok(0);
    }
    value.extract::<usize>()
}

/// Optional float key: missing/`None` → unset; anything non-numeric is a
/// `TypeError` from PyO3's extractor.
fn opt_f64_key(dict: &Bound<'_, PyDict>, key: &str) -> PyResult<Option<f64>> {
    let Some(value) = dict.get_item(key)? else {
        return Ok(None);
    };
    if value.is_none() {
        return Ok(None);
    }
    Ok(Some(value.extract::<f64>()?))
}

pub fn extract_embedder_config(
    embedder: Option<&Bound<'_, PyAny>>,
) -> PyResult<ParsedEmbedderConfig> {
    let mut out = ParsedEmbedderConfig::default();

    if let Some(emb) = embedder {
        if let Ok(py_emb) = emb.extract::<PyRef<PyHttpEmbedder>>() {
            out.endpoint = Some(py_emb.endpoint.clone());
            out.api_key = Some(py_emb.api_key.clone());
            out.model = Some(py_emb.model.clone());
            out.dimension = Some(py_emb.dimension);
            out.multi_endpoint = py_emb.multi_endpoint.clone();
            out.multi_api_key = py_emb.multi_api_key.clone();
            out.multi_model = py_emb.multi_model.clone();
            out.multi_dimension = py_emb.multi_dimension;
            out.image_endpoint = py_emb.image_endpoint.clone();
            out.image_api_key = py_emb.image_api_key.clone();
            out.image_model = py_emb.image_model.clone();
            out.image_dimension = py_emb.image_dimension;
            out.rerank_endpoint = py_emb.rerank_endpoint.clone();
            out.rerank_api_key = py_emb.rerank_api_key.clone();
            out.rerank_model = py_emb.rerank_model.clone();
            out.bm25_k1 = py_emb.bm25_k1;
            out.bm25_b = py_emb.bm25_b;
            out.bm25_avg_len = py_emb.bm25_avg_len;
        } else if let Ok(dict) = emb.cast::<PyDict>() {
            out.endpoint = Some(
                dict.get_item("endpoint")?
                    .ok_or_else(|| {
                        pyo3::exceptions::PyValueError::new_err("embedder.endpoint is required")
                    })?
                    .extract::<String>()?,
            );
            out.model = Some(
                dict.get_item("model")?
                    .ok_or_else(|| {
                        pyo3::exceptions::PyValueError::new_err("embedder.model is required")
                    })?
                    .extract::<String>()?,
            );
            out.dimension = Some(
                dict.get_item("dimension")?
                    .ok_or_else(|| {
                        pyo3::exceptions::PyValueError::new_err("embedder.dimension is required")
                    })?
                    .extract::<usize>()?,
            );
            out.api_key = opt_string_key(dict, "api_key")?;

            if out
                .endpoint
                .as_ref()
                .is_some_and(|value| value.trim().is_empty())
            {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "embedder.endpoint must not be empty",
                ));
            }
            if out
                .model
                .as_ref()
                .is_some_and(|value| value.trim().is_empty())
            {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "embedder.model must not be empty",
                ));
            }
            if out.dimension == Some(0) {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "embedder.dimension must be positive",
                ));
            }

            // Optional multi/image/rerank groups: empty strings unset, None
            // and missing keys unset, 0 dims skip checks (image 0 falls back
            // to dense dim in the Rust core).
            out.multi_endpoint = opt_nonempty_string_key(dict, "multi_endpoint")?;
            out.multi_api_key = opt_string_key(dict, "multi_api_key")?;
            out.multi_model = opt_nonempty_string_key(dict, "multi_model")?;
            out.multi_dimension = opt_usize_key(dict, "multi_dimension")?;
            out.image_endpoint = opt_nonempty_string_key(dict, "image_endpoint")?;
            out.image_api_key = opt_string_key(dict, "image_api_key")?;
            out.image_model = opt_nonempty_string_key(dict, "image_model")?;
            out.image_dimension = opt_usize_key(dict, "image_dimension")?;
            out.rerank_endpoint = opt_nonempty_string_key(dict, "rerank_endpoint")?;
            out.rerank_api_key = opt_string_key(dict, "rerank_api_key")?;
            out.rerank_model = opt_nonempty_string_key(dict, "rerank_model")?;

            // Optional client-side BM25 document parameters (local sparse path).
            out.bm25_k1 = opt_f64_key(dict, "bm25_k1")?;
            out.bm25_b = opt_f64_key(dict, "bm25_b")?;
            out.bm25_avg_len = opt_f64_key(dict, "bm25_avg_len")?;
        } else {
            return Err(pyo3::exceptions::PyTypeError::new_err(
                "embedder must be an HttpEmbedder or dict",
            ));
        }
    }
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
pub fn create_executor(
    url: &str,
    api_key: Option<String>,
    use_grpc: bool,
    embedder: Option<&Bound<'_, PyAny>>,
    route_affinity: Option<String>,
) -> PyResult<(qql::executor::Executor, tokio::runtime::Runtime)> {
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;

    let parsed = extract_embedder_config(embedder)?;

    let mut config = qql::config::QqlConfig {
        url: url.to_string(),
        secret: api_key.clone(),
        ..Default::default()
    };

    if let Some(endpoint) = parsed.endpoint {
        config.embedding_endpoint = Some(endpoint);
        config.embedding_api_key = parsed.api_key;
        config.embedding_model = parsed.model;
        config.embedding_dimension = parsed.dimension.unwrap_or(0);
        config.multi_embedding_endpoint = parsed.multi_endpoint;
        config.multi_embedding_api_key = parsed.multi_api_key;
        config.multi_embedding_model = parsed.multi_model;
        config.multi_embedding_dimension = parsed.multi_dimension;
        config.image_embedding_endpoint = parsed.image_endpoint;
        config.image_embedding_api_key = parsed.image_api_key;
        config.image_embedding_model = parsed.image_model;
        config.image_embedding_dimension = parsed.image_dimension;
        config.rerank_endpoint = parsed.rerank_endpoint;
        config.rerank_api_key = parsed.rerank_api_key;
        config.rerank_model = parsed.rerank_model;
    }

    // Validate client-side BM25 params once (ValueError before any network).
    validate_bm25(parsed.bm25_k1, parsed.bm25_b, parsed.bm25_avg_len)?;
    config.bm25_k1 = parsed.bm25_k1;
    config.bm25_b = parsed.bm25_b;
    config.bm25_avg_len = parsed.bm25_avg_len;

    let client: Box<dyn qql::client::QdrantOps> = if use_grpc {
        #[cfg(feature = "grpc")]
        {
            // tonic's `connect_lazy` captures the tokio reactor eagerly
            // (hyper-util timer handle, captured at Channel construction), so
            // the channel must be built with the client's runtime entered —
            // otherwise construction panics with "there is no reactor
            // running" on the Python thread.
            let grpc = {
                let _enter = rt.enter();
                qql::grpc::GrpcQdrant::from_url(url, api_key)
                    .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?
            };
            let mut grpc = grpc;
            if let Some(affinity) = route_affinity.as_deref() {
                grpc = grpc.with_route_affinity(affinity);
            }
            Box::new(grpc)
        }
        #[cfg(not(feature = "grpc"))]
        {
            return Err(pyo3::exceptions::PyRuntimeError::new_err(
                "gRPC feature not enabled in this build",
            ));
        }
    } else {
        let mut rest = qql::rest::RestQdrant::new(url.to_string(), api_key);
        if let Some(affinity) = route_affinity.as_deref() {
            rest = rest.with_route_affinity(affinity);
        }
        Box::new(rest)
    };

    let embedder_impl = if let Some(endpoint) = &config.embedding_endpoint {
        if !endpoint.trim().is_empty() {
            let http_emb =
                qql::embedder::HttpEmbedder::try_with_options(qql::embedder::HttpEmbedderOptions {
                    endpoint: endpoint.clone(),
                    api_key: config.embedding_api_key.clone().unwrap_or_default(),
                    model: config.embedding_model.clone().unwrap_or_default(),
                    dimension: config.embedding_dimension,
                    multi_endpoint: config.multi_embedding_endpoint.clone(),
                    multi_api_key: config.multi_embedding_api_key.clone(),
                    multi_model: config.multi_embedding_model.clone(),
                    multi_dimension: config.multi_embedding_dimension,
                    image_endpoint: config.image_embedding_endpoint.clone(),
                    image_api_key: config.image_embedding_api_key.clone(),
                    image_model: config.image_embedding_model.clone(),
                    image_dimension: config.image_embedding_dimension,
                    rerank_endpoint: config.rerank_endpoint.clone(),
                    rerank_api_key: config.rerank_api_key.clone(),
                    rerank_model: config.rerank_model.clone(),
                    bm25_k1: config.bm25_k1,
                    bm25_b: config.bm25_b,
                    bm25_avg_len: config.bm25_avg_len,
                })
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
