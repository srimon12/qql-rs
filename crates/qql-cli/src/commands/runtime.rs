//! Shared CLI runtime: executor construction, embed settings, explain.

#[cfg(feature = "edge")]
use std::io::IsTerminal;

/// Resolve dense embedder settings from env (`EMBED_*`) over persisted config.
/// Returns `(endpoint, model, expected_dim, dim_source)`.
pub(crate) fn resolve_embed_settings() -> (Option<String>, String, usize, String) {
    let config = qql::config::QqlConfig::load()
        .ok()
        .flatten()
        .unwrap_or_default();
    let endpoint = std::env::var("EMBED_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| {
            config
                .embedding_endpoint
                .clone()
                .filter(|v| !v.trim().is_empty())
        });
    let model = std::env::var("EMBED_MODEL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| config.embedding_model.clone())
        .unwrap_or_else(|| "all-minilm:l6-v2".to_string());
    if let Ok(raw) = std::env::var("EMBED_DIM")
        && let Ok(dim) = raw.trim().parse::<usize>()
        && dim > 0
    {
        return (endpoint, model, dim, "EMBED_DIM".to_string());
    }
    if config.embedding_dimension > 0 {
        return (
            endpoint,
            model,
            config.embedding_dimension,
            "config embedding_dimension".to_string(),
        );
    }
    (endpoint, model, 384, "default 384".to_string())
}

/// Classify a backend failure as unreachable vs auth vs other for doctor output.
pub(crate) fn classify_backend_failure(code: &str, message: &str) -> &'static str {
    if code == "QQL-TRANSPORT" || code == "QQL-BACKEND-JSON" && message.contains("request id") {
        // Transport covers refused connections, DNS, and timeouts.
        "unreachable"
    } else if code == "QQL-BACKEND-AUTH" {
        "auth"
    } else if message.contains("connection refused")
        || message.contains("Connection refused")
        || message.contains("dns error")
        || message.contains("failed to lookup address")
    {
        "unreachable"
    } else {
        "other"
    }
}

pub(crate) fn dense_vector_sizes(info: &qql::client::CollectionInfo) -> Vec<(String, u64)> {
    info.schema
        .vectors
        .iter()
        .map(|v| (v.name.clone().unwrap_or_default(), v.size))
        .collect()
}

#[cfg(feature = "rest")]
pub(crate) async fn probe_embed_dim(
    endpoint: &str,
    model: &str,
    dim: usize,
) -> Result<usize, qql_core::error::QqlError> {
    let api_key = std::env::var("EMBED_KEY").unwrap_or_default();
    let embedder =
        qql::embedder::HttpEmbedder::try_with_options(qql::embedder::HttpEmbedderOptions {
            endpoint: endpoint.to_string(),
            api_key,
            model: model.to_string(),
            dimension: dim.max(1),
            ..Default::default()
        })?;
    embedder.probe_dimension("qql doctor probe").await
}

/// Explain a query, binding `params` on the AST — the same binding `exec`
/// uses — so placeholders (`:rows`, `:q`, `?`) explain exactly as executed.
pub(crate) fn explain_query_bound(
    query: &str,
    params: Option<&serde_json::Value>,
) -> Result<String, String> {
    let Some(params) = params else {
        return explain_query(query);
    };
    let mut statements = qql_core::parser::Parser::parse_all(query).map_err(|e| e.to_string())?;
    for stmt in &mut statements {
        qql_core::params_json::bind_stmt_with_params(stmt, params).map_err(|e| e.to_string())?;
    }
    Ok(qql_core::explain::explain_nodes(&statements))
}

/// Open the interactive REPL against `url` (or the local edge backend).
pub async fn handle_connect(url: &str, use_edge: bool) -> Result<(), Box<dyn std::error::Error>> {
    let executor = executor(url, use_edge)?;
    let initial = executor
        .execute("SHOW COLLECTIONS", qql::executor::OnError::Stop)
        .await;
    if let Err(error) = initial {
        executor.close().await?;
        return Err(error.into());
    }
    crate::repl::run_repl(url, use_edge, executor).await
}

pub fn explain_query_str(query: &str) -> Result<String, String> {
    explain_query(query)
}

pub(crate) fn executor(
    url: &str,
    use_edge: bool,
) -> Result<qql::executor::Executor, Box<dyn std::error::Error>> {
    executor_for(url, use_edge, None)
}

pub(crate) fn executor_for(
    url: &str,
    use_edge: bool,
    api_key: Option<String>,
) -> Result<qql::executor::Executor, Box<dyn std::error::Error>> {
    if use_edge {
        #[cfg(feature = "edge")]
        {
            return edge_executor();
        }
        #[cfg(not(feature = "edge"))]
        {
            return Err(
                "edge support is not installed; reinstall qql-cli with --features edge".into(),
            );
        }
    }

    let config = qql::config::QqlConfig::load()?.unwrap_or_default();

    #[cfg(feature = "grpc")]
    let use_grpc = url.starts_with("grpc://") || url.contains(":6334");
    #[cfg(not(feature = "grpc"))]
    let use_grpc = false;

    let client: Box<dyn qql::client::QdrantOps> = if use_grpc {
        #[cfg(feature = "grpc")]
        {
            Box::new(qql::grpc::GrpcQdrant::from_url(
                url,
                api_key
                    .clone()
                    .or_else(|| std::env::var("QDRANT_API_KEY").ok())
                    .or_else(|| config.secret.clone()),
            )?)
        }
        #[cfg(not(feature = "grpc"))]
        {
            return Err("gRPC support is disabled in this build".into());
        }
    } else {
        #[cfg(feature = "rest")]
        {
            Box::new(qql::rest::RestQdrant::new(
                url.to_owned(),
                api_key
                    .or_else(|| std::env::var("QDRANT_API_KEY").ok())
                    .or_else(|| config.secret.clone()),
            ))
        }
        #[cfg(not(feature = "rest"))]
        {
            return Err(
                "REST support is disabled in this build; use a gRPC URL (:6334) or rebuild with --features rest"
                    .into(),
            );
        }
    };

    let env_url = std::env::var("EMBED_URL").ok();
    let embedder = if let Some(endpoint) = env_url.as_ref().or(config.embedding_endpoint.as_ref()) {
        if !endpoint.trim().is_empty() {
            #[cfg(feature = "rest")]
            {
                let api_key = std::env::var("EMBED_KEY")
                    .ok()
                    .unwrap_or_else(|| config.embedding_api_key.clone().unwrap_or_default());
                let model = std::env::var("EMBED_MODEL").ok().unwrap_or_else(|| {
                    config
                        .embedding_model
                        .clone()
                        .unwrap_or_else(|| "all-minilm:l6-v2".to_string())
                });
                let dimension = std::env::var("EMBED_DIM")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(if config.embedding_dimension > 0 {
                        config.embedding_dimension
                    } else {
                        384
                    });
                let multi_endpoint = std::env::var("MULTI_EMBED_URL")
                    .ok()
                    .or_else(|| config.multi_embedding_endpoint.clone());
                let multi_api_key = std::env::var("MULTI_EMBED_KEY")
                    .ok()
                    .or_else(|| config.multi_embedding_api_key.clone());
                let multi_model = std::env::var("MULTI_EMBED_MODEL")
                    .ok()
                    .or_else(|| config.multi_embedding_model.clone());
                let multi_dimension = std::env::var("MULTI_EMBED_DIM")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(config.multi_embedding_dimension);
                let image_endpoint = std::env::var("IMAGE_EMBED_URL")
                    .ok()
                    .or_else(|| config.image_embedding_endpoint.clone());
                let image_api_key = std::env::var("IMAGE_EMBED_KEY")
                    .ok()
                    .or_else(|| config.image_embedding_api_key.clone());
                let image_model = std::env::var("IMAGE_EMBED_MODEL")
                    .ok()
                    .or_else(|| config.image_embedding_model.clone());
                let image_dimension = std::env::var("IMAGE_EMBED_DIM")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(config.image_embedding_dimension);
                let rerank_endpoint = std::env::var("RERANK_URL")
                    .ok()
                    .or_else(|| config.rerank_endpoint.clone());
                let rerank_api_key = std::env::var("RERANK_KEY")
                    .ok()
                    .or_else(|| config.rerank_api_key.clone());
                let rerank_model = std::env::var("RERANK_MODEL")
                    .ok()
                    .or_else(|| config.rerank_model.clone());
                let http_emb = qql::embedder::HttpEmbedder::try_with_options(
                    qql::embedder::HttpEmbedderOptions {
                        endpoint: endpoint.clone(),
                        api_key,
                        model,
                        dimension,
                        multi_endpoint,
                        multi_api_key,
                        multi_model,
                        multi_dimension,
                        image_endpoint,
                        image_api_key,
                        image_model,
                        image_dimension,
                        rerank_endpoint,
                        rerank_api_key,
                        rerank_model,
                        bm25_k1: config.bm25_k1,
                        bm25_b: config.bm25_b,
                        bm25_avg_len: config.bm25_avg_len,
                    },
                )?;
                Some(std::sync::Arc::new(http_emb) as std::sync::Arc<dyn qql::embedder::Embedder>)
            }
            #[cfg(not(feature = "rest"))]
            {
                let _ = endpoint;
                return Err(
                    "HTTP embedding requires the rest feature; rebuild with --features rest".into(),
                );
            }
        } else {
            None
        }
    } else {
        None
    };

    Ok(qql::executor::Executor::with_embedder(
        client,
        Some(config),
        embedder,
    ))
}

#[cfg(feature = "edge")]
pub(crate) fn edge_executor() -> Result<qql::executor::Executor, Box<dyn std::error::Error>> {
    let config = crate::config::EdgeConfig::load()?.apply_environment();
    let wal_segment_capacity = qql_edge::wal_segment_capacity_bytes(config.wal_segment_mb)?;
    match config.embedder.as_str() {
        "fastembed" => {
            let is_tty = std::io::stdout().is_terminal();
            let show_progress = config.show_download_progress || is_tty;
            let model_name = config.model.as_deref().unwrap_or("BGESmallENV15");
            if show_progress {
                eprintln!(
                    "ℹ Initializing local edge embedder (model: '{model_name}'). Model weights are downloaded on first run if not cached."
                );
            }
            let options = qql_edge::LocalExecutorOptions {
                on_disk_payload: config.on_disk_payload,
                wal_segment_capacity,
                model: config.model,
                sparse_model: config.sparse_model,
                multi_model: config.multi_model.or(config.multi_embed_model.clone()),
                image_model: config.image_model.or(config.image_embed_model.clone()),
                reranker_model: config.reranker_model.clone(),
                cache_dir: config.cache_dir,
                show_download_progress: show_progress,
                bm25_k1: config.bm25_k1,
                bm25_b: config.bm25_b,
                bm25_avg_len: config.bm25_avg_len,
            };
            qql_edge::local_executor_with_options(config.data_dir, options)
                .map_err(|error| format!("edge initialization failed: {error}").into())
        }
        "http" => {
            let endpoint = config.embed_url.ok_or(
                "the edge HTTP embedder requires embed_url; run `qql config edge --embedder http --embed-url <URL>`",
            )?;
            qql_edge::http_executor_with_options_and_wal(
                config.data_dir,
                config.on_disk_payload,
                wal_segment_capacity,
                qql::embedder::HttpEmbedderOptions {
                    endpoint,
                    api_key: config.embed_key,
                    model: config.embed_model,
                    dimension: config.embed_dimension,
                    multi_endpoint: config.multi_embed_url,
                    multi_api_key: config.multi_embed_key,
                    multi_model: config.multi_embed_model,
                    multi_dimension: config.multi_embed_dimension,
                    image_endpoint: config.image_embed_url,
                    image_api_key: config.image_embed_key,
                    image_model: config.image_embed_model,
                    image_dimension: config.image_embed_dimension,
                    rerank_endpoint: None,
                    rerank_api_key: None,
                    rerank_model: config.reranker_model,
                    bm25_k1: config.bm25_k1,
                    bm25_b: config.bm25_b,
                    bm25_avg_len: config.bm25_avg_len,
                },
            )
            .map_err(|error| format!("edge initialization failed: {error}").into())
        }
        other => Err(format!(
            "unknown configured edge embedder '{other}'; expected 'fastembed' or 'http'"
        )
        .into()),
    }
}

// ── Explain implementation ────────────────────────────────────

pub(crate) fn explain_query(query: &str) -> Result<String, String> {
    // Try multi-statement first — if the input has semicolons we get a
    // per-statement breakdown.  Falls back to single-statement for simple
    // queries (parse_all rejects them with a confusing semicolon error).
    match qql::executor::Executor::explain_all(query) {
        Ok(plan) if !plan.is_empty() => Ok(plan),
        Ok(_) | Err(_) => qql::executor::Executor::explain(query).map_err(|e| e.to_string()),
    }
}
