use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict};

#[pyclass(name = "HttpEmbedder", frozen, from_py_object)]
#[derive(Clone)]
pub struct PyHttpEmbedder {
    pub endpoint: String,
    pub api_key: String,
    pub model: String,
    pub dimension: usize,
}

#[pymethods]
impl PyHttpEmbedder {
    #[new]
    #[pyo3(signature = (endpoint, model, dimension, api_key=None))]
    pub fn new(
        endpoint: &str,
        model: &str,
        dimension: usize,
        api_key: Option<String>,
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
        Ok(PyHttpEmbedder {
            endpoint: endpoint.to_string(),
            api_key: api_key.unwrap_or_default(),
            model: model.to_string(),
            dimension,
        })
    }
}

#[allow(clippy::type_complexity)]
pub fn extract_embedder_config(
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
        } else if let Ok(dict) = emb.cast::<PyDict>() {
            ep = Some(
                dict.get_item("endpoint")?
                    .ok_or_else(|| {
                        pyo3::exceptions::PyValueError::new_err("embedder.endpoint is required")
                    })?
                    .extract::<String>()?,
            );
            model = Some(
                dict.get_item("model")?
                    .ok_or_else(|| {
                        pyo3::exceptions::PyValueError::new_err("embedder.model is required")
                    })?
                    .extract::<String>()?,
            );
            dim = Some(
                dict.get_item("dimension")?
                    .ok_or_else(|| {
                        pyo3::exceptions::PyValueError::new_err("embedder.dimension is required")
                    })?
                    .extract::<usize>()?,
            );
            ep_key = dict
                .get_item("api_key")?
                .map(|value| value.extract::<String>())
                .transpose()?;

            if ep.as_ref().is_some_and(|value| value.trim().is_empty()) {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "embedder.endpoint must not be empty",
                ));
            }
            if model.as_ref().is_some_and(|value| value.trim().is_empty()) {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "embedder.model must not be empty",
                ));
            }
            if dim == Some(0) {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "embedder.dimension must be positive",
                ));
            }
        } else {
            return Err(pyo3::exceptions::PyTypeError::new_err(
                "embedder must be an HttpEmbedder or dict",
            ));
        }
    }
    Ok((ep, ep_key, model, dim))
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

    let (ep, ep_key, model, dim) = extract_embedder_config(embedder)?;

    let mut config = qql::config::QqlConfig {
        url: url.to_string(),
        secret: api_key.clone(),
        ..Default::default()
    };

    if let Some(endpoint) = ep {
        config.embedding_endpoint = Some(endpoint);
        config.embedding_api_key = ep_key;
        config.embedding_model = model;
        config.embedding_dimension = dim.unwrap_or(0);
    }

    if let Some(emb) = embedder
        && let Ok(dict) = emb.cast::<PyDict>()
    {
        if let Ok(Some(v)) = dict.get_item("multi_endpoint") {
            config.multi_embedding_endpoint = Some(v.extract::<String>()?);
        }
        if let Ok(Some(v)) = dict.get_item("multi_api_key") {
            config.multi_embedding_api_key = Some(v.extract::<String>()?);
        }
        if let Ok(Some(v)) = dict.get_item("multi_model") {
            config.multi_embedding_model = Some(v.extract::<String>()?);
        }
        if let Ok(Some(v)) = dict.get_item("multi_dimension") {
            config.multi_embedding_dimension = v.extract::<usize>()?;
        }
        if let Ok(Some(v)) = dict.get_item("image_endpoint") {
            config.image_embedding_endpoint = Some(v.extract::<String>()?);
        }
        if let Ok(Some(v)) = dict.get_item("image_api_key") {
            config.image_embedding_api_key = Some(v.extract::<String>()?);
        }
        if let Ok(Some(v)) = dict.get_item("image_model") {
            config.image_embedding_model = Some(v.extract::<String>()?);
        }
        if let Ok(Some(v)) = dict.get_item("image_dimension") {
            config.image_embedding_dimension = v.extract::<usize>()?;
        }
        if let Ok(Some(v)) = dict.get_item("rerank_endpoint") {
            config.rerank_endpoint = Some(v.extract::<String>()?);
        }
        if let Ok(Some(v)) = dict.get_item("rerank_api_key") {
            config.rerank_api_key = Some(v.extract::<String>()?);
        }
        if let Ok(Some(v)) = dict.get_item("rerank_model") {
            config.rerank_model = Some(v.extract::<String>()?);
        }
    }

    let client: Box<dyn qql::client::QdrantOps> = if use_grpc {
        #[cfg(feature = "grpc")]
        {
            let mut grpc = qql::grpc::GrpcQdrant::from_url(url, api_key)
                .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
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
