//! Remote transport construction for `pyqql`.
//!
//! The `HttpEmbedder` class, dict/class config extraction, and BM25
//! validation live in `pyqql-common`; this module only owns REST/gRPC
//! transport construction over the shared runtime.

use pyo3::prelude::*;

/// Build the transport + configuration for a remote client.
///
/// All blocking construction (tonic's eager `connect_lazy` reactor capture)
/// happens with the process-wide shared runtime entered, so the channel is
/// bound to the same runtime every request is later driven on.
pub fn create_executor(
    url: &str,
    api_key: Option<String>,
    use_grpc: bool,
    embedder: Option<&Bound<'_, PyAny>>,
    route_affinity: Option<String>,
) -> PyResult<qql::executor::Executor> {
    let parsed = pyqql_common::extract_embedder_config(embedder)?;

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
    pyqql_common::validate_bm25_text(
        parsed.bm25_k1,
        parsed.bm25_b,
        parsed.bm25_avg_len,
        parsed.bm25_language.as_deref(),
        parsed.bm25_tokenizer.as_deref(),
        parsed.bm25_lowercase,
        parsed.bm25_ascii_folding,
        parsed.bm25_stopwords.clone(),
        parsed.bm25_stemmer.as_deref(),
        parsed.bm25_min_token_len,
        parsed.bm25_max_token_len,
        parsed.bm25_stopwords_languages.clone(),
    )?;
    config.bm25_k1 = parsed.bm25_k1;
    config.bm25_b = parsed.bm25_b;
    config.bm25_avg_len = parsed.bm25_avg_len;
    config.bm25_language = parsed.bm25_language;
    config.bm25_tokenizer = parsed.bm25_tokenizer;
    config.bm25_lowercase = parsed.bm25_lowercase;
    config.bm25_ascii_folding = parsed.bm25_ascii_folding;
    config.bm25_stopwords = parsed.bm25_stopwords;
    config.bm25_stopwords_languages = parsed.bm25_stopwords_languages;
    config.bm25_stemmer = parsed.bm25_stemmer;
    config.bm25_min_token_len = parsed.bm25_min_token_len;
    config.bm25_max_token_len = parsed.bm25_max_token_len;

    let client: Box<dyn qql::client::QdrantOps> = if use_grpc {
        #[cfg(feature = "grpc")]
        {
            // tonic's `connect_lazy` captures the tokio reactor eagerly
            // (hyper-util timer handle, captured at Channel construction), so
            // the channel must be built with the client's runtime entered —
            // otherwise construction panics with "there is no reactor
            // running" on the Python thread.
            let grpc = {
                let _enter = pyqql_common::shared_runtime()?.enter();
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
                    bm25_language: config.bm25_language.clone(),
                    bm25_tokenizer: config.bm25_tokenizer.clone(),
                    bm25_lowercase: config.bm25_lowercase,
                    bm25_ascii_folding: config.bm25_ascii_folding,
                    bm25_stopwords: config.bm25_stopwords.clone(),
                    bm25_stopwords_languages: config.bm25_stopwords_languages.clone(),
                    bm25_stemmer: config.bm25_stemmer.clone(),
                    bm25_min_token_len: config.bm25_min_token_len,
                    bm25_max_token_len: config.bm25_max_token_len,
                })
                .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
            Some(std::sync::Arc::new(http_emb) as std::sync::Arc<dyn qql::embedder::Embedder>)
        } else {
            None
        }
    } else {
        None
    };

    Ok(qql::executor::Executor::with_embedder(
        client,
        Some(config),
        embedder_impl,
    ))
}
