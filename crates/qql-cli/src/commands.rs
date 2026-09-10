#[cfg(feature = "edge")]
use std::io::IsTerminal;

/// Resolve dense embedder settings from env (`EMBED_*`) over persisted config.
/// Returns `(endpoint, model, expected_dim, dim_source)`.
fn resolve_embed_settings() -> (Option<String>, String, usize, String) {
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
fn classify_backend_failure(code: &str, message: &str) -> &'static str {
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

fn dense_vector_sizes(info: &qql::client::CollectionInfo) -> Vec<(String, u64)> {
    info.schema
        .vectors
        .iter()
        .map(|v| (v.name.clone().unwrap_or_default(), v.size))
        .collect()
}

#[cfg(feature = "rest")]
async fn probe_embed_dim(
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

pub async fn handle_doctor(
    url: &str,
    use_edge: bool,
    json: bool,
    quiet: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = executor(url, use_edge)?;
    let hosts = doctor_host_summary(executor.config(), use_edge);
    let ping = executor
        .execute("SHOW COLLECTIONS", qql::executor::OnError::Stop)
        .await;
    let mut qdrant_ok = true;
    let mut qdrant_code: Option<String> = None;
    let mut qdrant_detail = String::new();
    let mut qdrant_class = "ok";
    if let Err(e) = &ping {
        qdrant_ok = false;
        qdrant_code = Some(e.code.to_string());
        qdrant_detail = e.to_string();
        qdrant_class = classify_backend_failure(&e.code, &e.message);
    }

    // Embed probe: sample request against EMBED_URL, report the real dim.
    let (endpoint_opt, model, expected_dim, dim_source) = resolve_embed_settings();
    let mut embed_line = String::from("embed: no EMBED_URL configured, literal vectors only");
    // Updated only by the REST embed probe; grpc-only builds keep the defaults.
    #[cfg_attr(not(feature = "rest"), allow(unused_mut))]
    let mut embed_observed: Option<usize> = None;
    #[cfg_attr(not(feature = "rest"), allow(unused_mut))]
    let mut embed_ok = true;
    #[cfg_attr(not(feature = "rest"), allow(unused_mut))]
    let mut embed_code: Option<String> = None;
    if let Some(endpoint) = endpoint_opt.as_deref() {
        #[cfg(feature = "rest")]
        {
            match probe_embed_dim(endpoint, &model, expected_dim).await {
                Ok(real) => {
                    embed_observed = Some(real);
                    if real == expected_dim {
                        embed_line = format!(
                            "embed: {endpoint} model={model} dim={real} (matches {dim_source}={expected_dim})"
                        );
                    } else {
                        embed_ok = false;
                        embed_code = Some("QQL-EMBEDDING-DIM".to_string());
                        embed_line = format!(
                            "[QQL-EMBEDDING-DIM] embed: {endpoint} model={model} returned dim={real} but {dim_source}={expected_dim}; fix: set EMBED_DIM={real}"
                        );
                    }
                }
                Err(e) => {
                    embed_ok = false;
                    embed_code = Some(e.code.to_string());
                    embed_line = format!(
                        "[{}] embed: probe of {endpoint} failed: {e}; fix: run `ollama serve`, `ollama pull {model}`, and check EMBED_URL",
                        e.code
                    );
                }
            }
        }
        #[cfg(not(feature = "rest"))]
        {
            let _ = (endpoint, expected_dim);
            embed_line =
                "embed: HTTP embedding probe needs the rest feature; rebuild with --features rest"
                    .to_string();
        }
    }

    // Collection vector size compare against the probed dim.
    let mut dim_mismatches: Vec<String> = Vec::new();
    let mut collections_note = String::new();
    let mut edge_indexing: Vec<IndexingState> = Vec::new();
    if qdrant_ok && !use_edge {
        match executor.client().list_collections().await {
            Ok(names) => {
                for name in names.iter().take(20) {
                    let Ok(info) = executor.client().get_collection_info(name).await else {
                        continue;
                    };
                    for (vec_name, size) in dense_vector_sizes(&info) {
                        if let Some(real) = embed_observed
                            && size as usize != real
                        {
                            let label = if vec_name.is_empty() {
                                "<default>".to_string()
                            } else {
                                vec_name.clone()
                            };
                            dim_mismatches.push(format!(
                                "[QQL-BACKEND-DIMENSION-MISMATCH] collection '{name}' vector '{label}' size={size} != embed dim={real}; fix: set EMBED_DIM={real} or recreate with VECTOR({real}, ...)"
                            ));
                        }
                    }
                }
                if names.is_empty() {
                    collections_note = "collections: none yet".to_string();
                } else {
                    collections_note =
                        format!("collections: {} ({})", names.len(), names.join(", "));
                }
            }
            Err(e) => {
                collections_note = format!("collections: list failed [{}]: {e}", e.code);
            }
        }
    } else if use_edge && qdrant_ok {
        match collect_indexing_states(executor.client()).await {
            Ok(states) => {
                collections_note = if states.is_empty() {
                    "collections: edge backend (local, none yet)".to_string()
                } else {
                    format!("collections: edge backend (local, {})", states.len())
                };
                edge_indexing = states;
            }
            Err(e) => {
                collections_note = format!("collections: edge list failed [{}]: {e}", e.code);
            }
        }
    } else if use_edge {
        collections_note = "collections: edge backend (local)".to_string();
    }

    let healthy = qdrant_ok && embed_ok && dim_mismatches.is_empty();
    let target = if use_edge {
        "the local edge backend".to_string()
    } else {
        format!("Qdrant at {url}")
    };

    if json {
        let message = if healthy {
            format!("Connected to {target}")
        } else if !qdrant_ok {
            match qdrant_class {
                "unreachable" => format!(
                    "Qdrant unreachable at {url} [{}]: {qdrant_detail}; fix: start Qdrant (docker run -p 6333:6333 qdrant/qdrant:v1.19.0) or set QDRANT_URL",
                    qdrant_code.clone().unwrap_or_default()
                ),
                "auth" => format!(
                    "Qdrant auth failed at {url} [QQL-BACKEND-AUTH]: {qdrant_detail}; fix: set QDRANT_API_KEY to a valid key"
                ),
                _ => format!(
                    "Doctor failed for {target}: {qdrant_detail} {embed_line} {}",
                    dim_mismatches.join("; ")
                ),
            }
        } else if !embed_ok || !dim_mismatches.is_empty() {
            format!("{} {}", embed_line, dim_mismatches.join("; "))
        } else {
            format!("Connected to {target}")
        };
        println!(
            "{}",
            serde_json::json!({
                "ok": healthy,
                "healthy": healthy,
                "message": message,
                "hosts": hosts,
                "qdrant_ok": qdrant_ok,
                "qdrant_error_code": qdrant_code,
                "embed_endpoint": endpoint_opt,
                "embed_model": model,
                "embed_expected_dim": expected_dim,
                "embed_dim_source": dim_source,
                "embed_observed_dim": embed_observed,
                "embed_error_code": embed_code,
                "dim_mismatches": dim_mismatches,
                "collections_note": collections_note,
                "edge_indexing": edge_indexing,
            })
        );
    } else if !quiet {
        if qdrant_ok {
            println!("Connected to {target} (healthy)");
        } else {
            match qdrant_class {
                "unreachable" => println!(
                    "Qdrant unreachable at {url} [{}]: {qdrant_detail}; fix: start Qdrant (docker run -p 6333:6333 qdrant/qdrant:v1.19.0) or set QDRANT_URL",
                    qdrant_code.clone().unwrap_or_default()
                ),
                "auth" => println!(
                    "Qdrant auth failed at {url} [QQL-BACKEND-AUTH]: {qdrant_detail}; fix: set QDRANT_API_KEY to a valid key"
                ),
                _ => println!("Failed to connect to {target}: {qdrant_detail}"),
            }
        }
        println!("{embed_line}");
        for mismatch in &dim_mismatches {
            println!("{mismatch}");
        }
        if !collections_note.is_empty() {
            println!("{collections_note}");
        }
        for state in &edge_indexing {
            if let Some(nudge) = &state.nudge {
                println!("{nudge}");
            }
        }
        print_doctor_hosts(&hosts);
    }
    executor.close().await?;
    if quiet && healthy {
        return Ok(());
    }
    if quiet && !healthy {
        let detail = if !qdrant_ok {
            qdrant_detail.clone()
        } else {
            format!("{embed_line} {}", dim_mismatches.join("; "))
        };
        return Err(format!("Doctor failed for {target}: {detail}").into());
    }
    if healthy {
        Ok(())
    } else {
        Err(format!("Doctor failed for {target}").into())
    }
}

/// Snapshot of embedding / rerank hosts for doctor UX (no model download).
fn doctor_host_summary(
    config: Option<&qql::config::QqlConfig>,
    use_edge: bool,
) -> serde_json::Value {
    let Some(cfg) = config else {
        return serde_json::json!({
            "backend": if use_edge { "edge" } else { "remote" },
            "dense": false,
            "multi": false,
            "image": false,
            "cross_rerank": false,
            "hints": ["no QqlConfig on executor — embedding hosts unknown"],
        });
    };
    let dense = cfg.embedding_model.as_ref().is_some_and(|m| !m.is_empty())
        || cfg
            .embedding_endpoint
            .as_ref()
            .is_some_and(|e| !e.trim().is_empty())
        || use_edge;
    let multi = cfg
        .multi_embedding_model
        .as_ref()
        .is_some_and(|m| !m.is_empty())
        || cfg
            .multi_embedding_endpoint
            .as_ref()
            .is_some_and(|e| !e.trim().is_empty());
    let image = cfg
        .image_embedding_model
        .as_ref()
        .is_some_and(|m| !m.is_empty())
        || cfg
            .image_embedding_endpoint
            .as_ref()
            .is_some_and(|e| !e.trim().is_empty());
    let cross = cfg.rerank_model.as_ref().is_some_and(|m| !m.is_empty())
        || cfg
            .rerank_endpoint
            .as_ref()
            .is_some_and(|e| !e.trim().is_empty());

    let mut hints = Vec::new();
    if !multi {
        hints.push(
            "ColBERT / AS MULTI / multivector RERANK needs multi_model or multi_embedding_* config",
        );
    }
    if !image {
        hints.push("IMAGE / CLIP vision needs image_model or image_embedding_* config");
    }
    if !cross {
        hints.push("CROSS RERANK needs reranker_model or rerank_endpoint / rerank_model");
    }
    if use_edge {
        hints.push("edge has no SHARD routing or shard-key DDL; ALTER COLLECTION covers global HNSW/optimizers and per-vector HNSW only");
    }

    serde_json::json!({
        "backend": if use_edge { "edge" } else { "remote" },
        "dense": dense,
        "dense_model": cfg.embedding_model,
        "multi": multi,
        "multi_model": cfg.multi_embedding_model,
        "image": image,
        "image_model": cfg.image_embedding_model,
        "cross_rerank": cross,
        "rerank_model": cfg.rerank_model,
        "hints": hints,
    })
}

fn print_doctor_hosts(hosts: &serde_json::Value) {
    println!(
        "Hosts: dense={} multi={} image={} cross_rerank={}",
        hosts
            .get("dense")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        hosts
            .get("multi")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        hosts
            .get("image")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        hosts
            .get("cross_rerank")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    );
    if let Some(m) = hosts.get("dense_model").and_then(|v| v.as_str()) {
        println!("  dense_model: {m}");
    }
    if let Some(m) = hosts.get("multi_model").and_then(|v| v.as_str()) {
        println!("  multi_model: {m}");
    }
    if let Some(m) = hosts.get("image_model").and_then(|v| v.as_str()) {
        println!("  image_model: {m}");
    }
    if let Some(m) = hosts.get("rerank_model").and_then(|v| v.as_str()) {
        println!("  rerank_model: {m}");
    }
    if let Some(hints) = hosts.get("hints").and_then(|v| v.as_array()) {
        for h in hints {
            if let Some(s) = h.as_str() {
                println!("  hint: {s}");
            }
        }
    }
}
use crate::convert;
use crate::dump;
use crate::migrate;
use crate::output;
use crate::script;

const VERSION: &str = env!("CARGO_PKG_VERSION");

// ── Public handlers ───────────────────────────────────────────

pub async fn handle_exec(
    url: &str,
    use_edge: bool,
    query: &str,
    params: Option<&serde_json::Value>,
    json: bool,
    quiet: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = executor(url, use_edge)?;
    let result = match params {
        None => executor.execute(query, qql::executor::OnError::Stop).await,
        Some(serde_json::Value::Object(obj)) => {
            let mut map = std::collections::HashMap::new();
            for (k, v) in obj {
                map.insert(k.clone(), qql_core::ast::Value::from_json(v.clone())?);
            }
            executor
                .execute_with_params(query, &map, qql::executor::OnError::Stop)
                .await
        }
        Some(serde_json::Value::Array(arr)) => {
            let items: Vec<qql_core::ast::Value> = arr
                .iter()
                .cloned()
                .map(qql_core::ast::Value::from_json)
                .collect::<Result<_, _>>()?;
            executor
                .execute_with_positional_params(query, &items, qql::executor::OnError::Stop)
                .await
        }
        Some(_) => {
            return Err("--params-file must contain a JSON object or array".into());
        }
    };
    executor.close().await?;
    let report = result?;
    if !quiet {
        crate::table::render_report(&report, json)?;
    }
    Ok(())
}

pub async fn handle_execute_file(
    url: &str,
    use_edge: bool,
    path: &str,
    stop_on_error: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let statements = script::read_script(path).map_err(|e| format!("{}", e))?;
    let executor = executor(url, use_edge)?;
    let mut ok_count = 0;
    let mut fail_count = 0;
    let mut fatal_error = None;

    for (index, statement) in statements.iter().enumerate() {
        let on_err = if stop_on_error {
            qql::executor::OnError::Stop
        } else {
            qql::executor::OnError::Continue
        };
        match executor.execute(statement, on_err).await {
            Ok(report) => {
                ok_count += report.succeeded;
                fail_count += report.failed;
                for result in report.results.iter().filter(|result| !result.ok) {
                    output::print_error(&format!(
                        "statement {} ({}): {}",
                        index + 1,
                        result.operation,
                        result.message
                    ));
                }
            }
            Err(error) => {
                fail_count += 1;
                output::print_error(&format!("statement {}: {}", index + 1, error));
                if stop_on_error {
                    fatal_error = Some(format!("statement {} failed: {}", index + 1, error));
                    break;
                }
            }
        }
    }
    executor.close().await?;
    if let Some(error) = fatal_error {
        return Err(error.into());
    }

    let msg = format!(
        "Executed script {} ({} succeeded, {} failed)",
        path, ok_count, fail_count
    );

    let resp = output::ScriptResponse {
        ok: fail_count == 0,
        command: "execute".to_string(),
        path: path.to_string(),
        succeeded: ok_count,
        failed: fail_count,
        message: msg.clone(),
    };
    let s = serde_json::to_string_pretty(&resp)?;
    println!("{}", s);
    Ok(())
}

pub fn handle_explain(
    query: &str,
    params: Option<&serde_json::Value>,
    json: bool,
    quiet: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let plan = explain_query_bound(query, params)?;
    if quiet {
        return Ok(());
    }
    if json {
        let resp = output::ExplainResponse {
            ok: true,
            query: query.to_string(),
            plan,
        };
        let s = serde_json::to_string_pretty(&resp)?;
        println!("{}", s);
    } else {
        println!("{}", plan);
    }
    Ok(())
}

/// Explain a query, binding `params` on the AST — the same binding `exec`
/// uses — so placeholders (`:rows`, `:q`, `?`) explain exactly as executed.
fn explain_query_bound(query: &str, params: Option<&serde_json::Value>) -> Result<String, String> {
    let Some(params) = params else {
        return explain_query(query);
    };
    let mut statements = qql_core::parser::Parser::parse_all(query).map_err(|e| e.to_string())?;
    for stmt in &mut statements {
        qql_core::params_json::bind_stmt_with_params(stmt, params).map_err(|e| e.to_string())?;
    }
    Ok(qql_core::explain::explain_nodes(&statements))
}

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

fn executor(
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
fn edge_executor() -> Result<qql::executor::Executor, Box<dyn std::error::Error>> {
    let config = crate::config::EdgeConfig::load()?.apply_environment();
    let wal_segment_capacity = wal_segment_capacity_bytes(config.wal_segment_mb)?;
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

pub fn handle_convert(path: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    let input = if let Some(p) = path {
        std::fs::read_to_string(p).map_err(|e| format!("cannot read file: {}", e))?
    } else {
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)
            .map_err(|e| format!("cannot read stdin: {}", e))?;
        buf
    };

    let input = input.trim().to_string();
    if input.is_empty() {
        return Err("no input provided".into());
    }

    let statements = convert::json_to_qql(&input)?;

    for stmt in &statements {
        println!("{}", stmt);
    }

    Ok(())
}

/// Format QQL source into canonical form.
///
/// Reads from `path` (or stdin when `None`). In `check` mode the formatted
/// output is compared against the input and a non-zero exit indicates the
/// source is not formatted. With `write` the formatted output is written back
/// to the file; otherwise it is printed to stdout.
pub fn handle_fmt(
    path: Option<&str>,
    check: bool,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let input = if let Some(p) = path {
        std::fs::read_to_string(p).map_err(|e| format!("cannot read file '{}': {}", p, e))?
    } else {
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)
            .map_err(|e| format!("cannot read stdin: {}", e))?;
        buf
    };

    let formatted = qql_core::fmt::format(&input)?;

    if check {
        if input.trim_end() != formatted {
            let target = path.unwrap_or("<stdin>");
            return Err(format!("{} is not formatted (run `qql fmt` to fix)", target).into());
        }
        return Ok(());
    }

    if write && let Some(p) = path {
        std::fs::write(p, format!("{}\n", formatted))
            .map_err(|e| format!("cannot write '{}': {}", p, e))?;
        return Ok(());
    }

    println!("{}", formatted);
    Ok(())
}

pub async fn handle_dump(
    url: &str,
    use_edge: bool,
    collection: &str,
    output: &str,
    batch_size: u32,
    progress: Option<&(dyn Fn(dump::DumpProgress) + Sync)>,
) -> Result<dump::DumpStats, Box<dyn std::error::Error>> {
    let executor = executor(url, use_edge)?;
    let result = dump::dump_collection(&executor, collection, output, batch_size, progress).await;
    executor.close().await?;
    result
}

pub async fn handle_migrate(
    source_url: &str,
    source_edge: bool,
    target_url: &str,
    target_edge: bool,
    target_api_key: Option<String>,
    opts: migrate::MigrateOptions,
    progress: Option<&(dyn Fn(migrate::MigrateProgress) + Sync)>,
) -> Result<migrate::MigrateStats, Box<dyn std::error::Error>> {
    migrate::validate_endpoints(source_edge, target_edge)?;
    let source = executor_for(source_url, source_edge, None)?;
    let target = executor_for(target_url, target_edge, target_api_key)?;
    let result = migrate::migrate_collection(&source, &target, opts, progress).await;
    let close_source = source.close().await;
    let close_target = target.close().await;
    let stats = result?;
    close_source?;
    close_target?;
    Ok(stats)
}

pub fn handle_configure_edge(
    config: crate::config::EdgeConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    if config.embedder != "fastembed" && config.embedder != "http" {
        return Err("edge embedder must be 'fastembed' or 'http'".into());
    }
    if config.embedder == "http" && config.embed_url.is_none() {
        return Err("--embed-url is required when --embedder http is selected".into());
    }
    if config.embed_dimension == 0 {
        return Err("--embed-dim must be greater than zero".into());
    }
    if config.wal_segment_mb == Some(0) {
        return Err(
            "--wal-segment-mb must be greater than zero; omit it for the qdrant-edge 32 MiB default"
                .into(),
        );
    }
    // Fail closed at save time: k1 > 0, b in [0, 1], avg_len > 0, all finite.
    qql::embedder::Bm25Params::resolve(config.bm25_k1, config.bm25_b, config.bm25_avg_len)?;
    let path = config.save()?;
    println!("Saved edge configuration to {}", path.display());
    println!("Use it with: qql --edge exec \"SHOW COLLECTIONS\"");
    Ok(())
}

/// Convert the CLI's MiB WAL knob to the byte capacity qdrant-edge expects.
///
/// `None` keeps the engine default; a zero or overflowing value fails closed
/// instead of silently producing a nonsensical WAL capacity.
#[cfg(feature = "edge")]
pub(crate) fn wal_segment_capacity_bytes(
    mb: Option<u64>,
) -> Result<Option<usize>, Box<dyn std::error::Error>> {
    match mb {
        None => Ok(None),
        Some(0) => Err(
            "wal_segment_mb must be greater than zero; omit it for the qdrant-edge 32 MiB default"
                .into(),
        ),
        Some(mb) => {
            let bytes = usize::try_from(mb)
                .ok()
                .and_then(|mb| mb.checked_mul(1024 * 1024))
                .ok_or("wal_segment_mb is too large for this platform")?;
            Ok(Some(bytes))
        }
    }
}

/// Per-collection indexing counters for the doctor / check readout.
#[derive(serde::Serialize)]
struct IndexingState {
    collection: String,
    points_count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    indexed_vectors_count: Option<u64>,
    segments_count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    nudge: Option<String>,
}

/// Read indexing counters for every collection and build the
/// "run `qql edge optimize`" nudge when indexed vectors lag points.
async fn collect_indexing_states(
    client: &dyn qql::client::QdrantOps,
) -> Result<Vec<IndexingState>, qql_core::error::QqlError> {
    let names = client.list_collections().await?;
    let mut states = Vec::new();
    for name in names {
        // A collection that disappears mid-scan is skipped, not fatal.
        let Ok(info) = client.get_collection_info(&name).await else {
            continue;
        };
        let nudge = match info.indexed_vectors_count {
            Some(indexed) if indexed < info.points_count => Some(format!(
                "collection '{name}': {} points, {} indexed — qdrant-edge only indexes during optimize; run `qql edge optimize {name}` after bulk writes (segments below indexing_threshold stay brute-force)",
                info.points_count, indexed
            )),
            _ => None,
        };
        states.push(IndexingState {
            collection: name,
            points_count: info.points_count,
            indexed_vectors_count: info.indexed_vectors_count,
            segments_count: info.segments_count,
            nudge,
        });
    }
    Ok(states)
}

fn count_label(count: Option<u64>) -> String {
    count.map_or_else(|| "?".to_string(), |value| value.to_string())
}

#[cfg(feature = "edge")]
fn info_counts(info: &qql::client::CollectionInfo) -> serde_json::Value {
    serde_json::json!({
        "points": info.points_count,
        "indexed": info.indexed_vectors_count,
        "segments": info.segments_count,
    })
}

/// Run the qdrant-edge optimizers on a local collection.
///
/// Builds `EdgeQdrant` directly instead of going through the embedding-aware
/// executor, so no model is initialized or downloaded. qdrant-edge has no
/// background optimizer: this is what merges segments and builds HNSW/sparse
/// indexes.
#[cfg(feature = "edge")]
pub async fn handle_edge_optimize(
    collection: &str,
    json: bool,
    quiet: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    use qql::client::QdrantOps;

    let config = crate::config::EdgeConfig::load()?.apply_environment();
    let wal_segment_capacity = wal_segment_capacity_bytes(config.wal_segment_mb)?;
    let backend = qql_edge::EdgeQdrant::new(config.data_dir, config.on_disk_payload)
        .with_wal_segment_capacity(wal_segment_capacity);

    let outcome = async {
        let before = backend.get_collection_info(collection).await?;
        let optimized = backend.optimize_collection(collection).await?;
        let after = backend.get_collection_info(collection).await?;
        Ok::<_, qql_core::error::QqlError>((before, optimized, after))
    }
    .await;
    let closed = backend.close().await;
    let (before, optimized, after) = outcome?;
    closed?;

    let message = if optimized {
        format!(
            "Optimized '{collection}': {} → {} segments, {} points, indexed {} → {}",
            before.segments_count,
            after.segments_count,
            after.points_count,
            count_label(before.indexed_vectors_count),
            count_label(after.indexed_vectors_count)
        )
    } else {
        format!(
            "'{collection}' is already optimal: {} segments, {} points, indexed {}",
            after.segments_count,
            after.points_count,
            count_label(after.indexed_vectors_count)
        )
    };

    if json {
        println!(
            "{}",
            serde_json::json!({
                "ok": true,
                "operation": "edge-optimize",
                "collection": collection,
                "optimized": optimized,
                "before": info_counts(&before),
                "after": info_counts(&after),
                "message": message,
            })
        );
    } else if !quiet {
        println!("{message}");
    }
    Ok(())
}

/// Seed a local edge collection from a remote Qdrant shard snapshot.
///
/// Streams `GET /collections/{c}/shards/{id}/snapshot`, unpacks it with the
/// engine's snapshot API into a staging directory, verifies it by loading the
/// shard, and only then swaps it into the edge data directory. An existing
/// local collection is replaced only with `--force`; until the swap, it is
/// untouched. The snapshot carries config, built HNSW indexes and quantized
/// data — this is the documented "offload indexing" seed flow, not a
/// statement-based copy.
#[cfg(feature = "edge")]
pub async fn handle_edge_bootstrap(
    from: &str,
    api_key: Option<String>,
    collection: &str,
    shard_id: Option<u32>,
    force: bool,
    json: bool,
    quiet: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = crate::config::EdgeConfig::load()?.apply_environment();
    if collection == ".qql-bootstrap" {
        return Err("collection name '.qql-bootstrap' is reserved for snapshot staging".into());
    }
    let target = config.data_dir.join(collection);
    let replaced = target.exists();
    if replaced && !force {
        return Err(format!(
            "local collection '{collection}' already exists at {}; pass --force to replace it \
             (the remote snapshot is downloaded and verified before anything is moved)",
            target.display()
        )
        .into());
    }
    std::fs::create_dir_all(&config.data_dir)?;

    // Staging lives beside collections, never inside one. A leftover
    // `previous/` from a half-finished --force swap is restored first so a
    // retry cannot delete the only copy of the old collection.
    let workspace = config.data_dir.join(".qql-bootstrap").join(collection);
    let leftover = workspace.join("previous");
    if leftover.exists() && !target.exists() {
        std::fs::rename(&leftover, &target)?;
    }
    if workspace.exists() {
        std::fs::remove_dir_all(&workspace)?;
    }
    std::fs::create_dir_all(&workspace)?;
    let snapshot_path = workspace.join("shard.snapshot");
    let stage = workspace.join("stage");

    let run = async {
        let api_key = api_key.filter(|key| !key.is_empty()).or_else(|| {
            std::env::var("QDRANT_API_KEY")
                .ok()
                .filter(|key| !key.is_empty())
        });
        let client = qql::snapshots::RemoteSnapshotClient::new(from, api_key)?;
        let listing = client.list_shards(collection).await?;
        let chosen = qql::snapshots::select_shard_id(&listing, shard_id)?;
        let bytes = client
            .download_shard_snapshot(collection, chosen, &snapshot_path)
            .await?;
        qql_edge::unpack_snapshot(&snapshot_path, &stage)?;
        let verified = qql_edge::inspect_shard(&stage)?;
        Ok::<_, qql_core::error::QqlError>((listing, chosen, bytes, verified))
    }
    .await;

    // On any download/unpack/verify failure the existing collection is untouched.
    let (listing, chosen, bytes, verified) = match run {
        Ok(value) => value,
        Err(error) => {
            let _ = std::fs::remove_dir_all(&workspace);
            return Err(error.into());
        }
    };

    // Commit. Only now is the existing collection touched.
    let previous = workspace.join("previous");
    if target.exists() {
        std::fs::rename(&target, &previous)?;
    }
    if let Err(error) = std::fs::rename(&stage, &target) {
        if previous.exists() {
            std::fs::rename(&previous, &target).map_err(|restore| {
                format!(
                    "failed to move the verified snapshot into {} ({error}); \
                     previous collection is still at {} (restore failed: {restore})",
                    target.display(),
                    previous.display()
                )
            })?;
        }
        let _ = std::fs::remove_dir_all(&workspace);
        return Err(format!(
            "failed to move the verified snapshot into {}: {error}",
            target.display()
        )
        .into());
    }

    // Verify the shard loads from its final path before reporting success.
    // A failed inspect after a successful rename restores the previous
    // collection when one existed; the workspace is not deleted if restore
    // fails, so the previous copy stays on disk.
    let summary = match qql_edge::inspect_shard(&target) {
        Ok(summary) => summary,
        Err(error) => {
            if previous.exists() {
                let _ = std::fs::remove_dir_all(&target);
                if let Err(restore) = std::fs::rename(&previous, &target) {
                    return Err(format!(
                        "snapshot failed to load ({error}); previous collection is still at {} \
                         (restore failed: {restore})",
                        previous.display()
                    )
                    .into());
                }
            }
            let _ = std::fs::remove_dir_all(&workspace);
            return Err(error.into());
        }
    };
    if previous.exists() {
        let _ = std::fs::remove_dir_all(&previous);
    }
    let _ = std::fs::remove_dir_all(&workspace);

    let message = format!(
        "Bootstrapped '{collection}' from {from} [shard {chosen}]: {} points, {} indexed, {} segments ({bytes} bytes) → {}",
        summary.points_count,
        summary.indexed_vectors_count,
        summary.segments_count,
        target.display()
    );
    if json {
        println!(
            "{}",
            serde_json::json!({
                "ok": true,
                "operation": "edge-bootstrap",
                "collection": collection,
                "from": from,
                "shard_id": chosen,
                "shard_count": listing.shard_count,
                "local_shard_ids": listing.local_shard_ids,
                "remote_shards": listing.remote_shard_ids.len(),
                "snapshot_bytes": bytes,
                "points_count": summary.points_count,
                "indexed_vectors_count": summary.indexed_vectors_count,
                "segments_count": summary.segments_count,
                "path": target.display().to_string(),
                "replaced": replaced,
                "staged": info_counts_from_summary(&verified),
                "message": message,
            })
        );
    } else if !quiet {
        println!("{message}");
        if summary.indexed_vectors_count < summary.points_count {
            println!(
                "hint: run `qql edge optimize {collection}` to merge segments and build indexes (segments below the collection's indexing_threshold stay brute-force)"
            );
        }
    }
    Ok(())
}

#[cfg(feature = "edge")]
fn info_counts_from_summary(summary: &qql_edge::ShardSummary) -> serde_json::Value {
    serde_json::json!({
        "points": summary.points_count,
        "indexed": summary.indexed_vectors_count,
        "segments": summary.segments_count,
    })
}

/// Triage one statement through the documented debug loop:
/// format, offline explain, embed probe, topology, doctor.
pub async fn handle_check(
    url: &str,
    use_edge: bool,
    query: &str,
    params: Option<&serde_json::Value>,
    json: bool,
    quiet: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut stages: Vec<serde_json::Value> = Vec::new();
    let mut failed = false;
    let mut push = |stage: &str, status: &str, code: Option<String>, message: String| {
        stages.push(serde_json::json!({
            "stage": stage,
            "status": status,
            "code": code,
            "message": message,
        }));
    };

    // Stage 1: format check of the input.
    match qql_core::fmt::format(query) {
        Ok(formatted) => {
            if query.trim_end() == formatted {
                push("format", "ok", None, "format: parses cleanly".to_string());
            } else {
                push(
                    "format",
                    "ok",
                    None,
                    "format: parses cleanly but is not canonical; fix: run `qql fmt`".to_string(),
                );
            }
        }
        Err(e) => {
            failed = true;
            push(
                "format",
                "fail",
                Some(e.code.to_string()),
                format!(
                    "[{}] format: {e}; fix: correct the syntax at the reported span",
                    e.code
                ),
            );
        }
    }

    // Stage 2: offline explain.
    let mut stmts: Option<Vec<qql_core::ast::Stmt>> = None;
    match explain_query_bound(query, params) {
        Ok(_) => {
            push(
                "explain",
                "ok",
                None,
                "explain: offline plan built without a backend".to_string(),
            );
            if let Ok(parsed) = parse_and_bind(query, params) {
                stmts = Some(parsed);
            }
        }
        Err(msg) => {
            failed = true;
            push(
                "explain",
                "fail",
                None,
                format!("explain: {msg}; fix: address the reported QQL-* code"),
            );
            if let Ok(parsed) = parse_and_bind(query, params) {
                stmts = Some(parsed);
            }
        }
    }

    // Stage 3: embed endpoint dim probe when the statement needs embeddings.
    let needs_embed = stmts
        .as_ref()
        .is_some_and(|s| statements_need_embeddings(s));
    // Updated only by the REST embed probe; grpc-only builds keep the default.
    #[cfg_attr(not(feature = "rest"), allow(unused_mut))]
    let mut observed_dim: Option<usize> = None;
    if !needs_embed {
        push(
            "embed",
            "skip",
            None,
            "embed: literal vectors only, no embedder needed".to_string(),
        );
    } else if use_edge {
        push(
            "embed",
            "skip",
            None,
            "embed: edge backend provides local embeddings".to_string(),
        );
    } else {
        let (endpoint_opt, model, expected_dim, dim_source) = resolve_embed_settings();
        match endpoint_opt {
            None => {
                failed = true;
                push(
                    "embed",
                    "fail",
                    Some("QQL-EMBEDDING".to_string()),
                    "embed: [QQL-EMBEDDING] statement needs text embeddings but no EMBED_URL is set; fix: export EMBED_URL=http://localhost:11434/v1/embeddings and EMBED_DIM=384".to_string(),
                );
            }
            Some(endpoint) => {
                #[cfg(feature = "rest")]
                {
                    match probe_embed_dim(&endpoint, &model, expected_dim).await {
                        Ok(real) => {
                            observed_dim = Some(real);
                            if real == expected_dim {
                                push(
                                    "embed",
                                    "ok",
                                    None,
                                    format!(
                                        "embed: {endpoint} model={model} dim={real} (matches {dim_source})"
                                    ),
                                );
                            } else {
                                failed = true;
                                push(
                                    "embed",
                                    "fail",
                                    Some("QQL-EMBEDDING-DIM".to_string()),
                                    format!(
                                        "[QQL-EMBEDDING-DIM] embed: {endpoint} returned dim={real} but {dim_source}={expected_dim}; fix: set EMBED_DIM={real}"
                                    ),
                                );
                            }
                        }
                        Err(e) => {
                            failed = true;
                            push(
                                "embed",
                                "fail",
                                Some(e.code.to_string()),
                                format!(
                                    "[{}] embed: probe of {endpoint} failed: {e}; fix: run `ollama serve` and `ollama pull {model}`",
                                    e.code
                                ),
                            );
                        }
                    }
                }
                #[cfg(not(feature = "rest"))]
                {
                    let _ = (endpoint, expected_dim, model, dim_source);
                    push(
                        "embed",
                        "skip",
                        None,
                        "embed: HTTP probe needs the rest feature".to_string(),
                    );
                }
            }
        }
    }

    // Stages 4-5 need a backend. Build once and reuse for topology + doctor.
    let executor = executor(url, use_edge).ok();
    let mut backend_reachable = false;
    let mut backend_note = String::new();
    if let Some(exec) = executor.as_ref() {
        match exec
            .execute("SHOW COLLECTIONS", qql::executor::OnError::Stop)
            .await
        {
            Ok(_) => backend_reachable = true,
            Err(e) => {
                let class = classify_backend_failure(&e.code, &e.message);
                backend_note = match class {
                    "unreachable" => format!(
                        "[{}] backend unreachable at {url}: {e}; fix: start Qdrant (docker run -p 6333:6333 qdrant/qdrant:v1.19.0) or set QDRANT_URL",
                        e.code
                    ),
                    "auth" => format!(
                        "[QQL-BACKEND-AUTH] backend auth failed at {url}: {e}; fix: set QDRANT_API_KEY"
                    ),
                    _ => format!("[{}] backend check failed: {e}", e.code),
                };
            }
        }
    } else {
        backend_note =
            "backend: executor init failed; fix: reinstall with default features".to_string();
    }

    // Stage 4: USING and vector-name topology check against the live collection.
    if let Some(statements) = stmts.as_ref() {
        let collections = stmt_collections(statements);
        if collections.is_empty() {
            push(
                "topology",
                "skip",
                None,
                "topology: no target collection in this statement".to_string(),
            );
        } else if !backend_reachable {
            push(
                "topology",
                "skip",
                None,
                format!("topology: backend unreachable, cannot verify USING ({backend_note})"),
            );
        } else if let Some(exec) = executor.as_ref() {
            let mut ok_all = true;
            for collection in &collections {
                match exec.client().get_collection_info(collection).await {
                    Err(e) => {
                        ok_all = false;
                        failed = true;
                        push(
                            "topology",
                            "fail",
                            Some(e.code.to_string()),
                            format!(
                                "[{}] topology: collection '{collection}' lookup failed: {e}; fix: create it or check the name",
                                e.code
                            ),
                        );
                    }
                    Ok(info) => {
                        let dense: Vec<String> = info
                            .schema
                            .vectors
                            .iter()
                            .filter_map(|v| v.name.clone())
                            .collect();
                        let sparse: Vec<String> = info
                            .schema
                            .sparse_vectors
                            .iter()
                            .map(|v| v.name.clone())
                            .collect();
                        let unnamed = info.schema.vectors.iter().any(|v| v.name.is_none());
                        match check_using_names(statements, &dense, &sparse, unnamed) {
                            Ok(()) => {
                                if let Some(real) = observed_dim {
                                    let mut mismatch = false;
                                    for (vec_name, size) in dense_vector_sizes(&info) {
                                        if size as usize != real {
                                            mismatch = true;
                                            failed = true;
                                            ok_all = false;
                                            let label = if vec_name.is_empty() {
                                                "<default>".to_string()
                                            } else {
                                                vec_name
                                            };
                                            push(
                                                "topology",
                                                "fail",
                                                Some("QQL-BACKEND-DIMENSION-MISMATCH".to_string()),
                                                format!(
                                                    "[QQL-BACKEND-DIMENSION-MISMATCH] topology: collection '{collection}' vector '{label}' size={size} != embed dim={real}; fix: set EMBED_DIM={real} or recreate with VECTOR({real}, ...)"
                                                ),
                                            );
                                        }
                                    }
                                    if !mismatch {
                                        push(
                                            "topology",
                                            "ok",
                                            None,
                                            format!(
                                                "topology: USING names resolve on '{collection}' and dim={real} matches"
                                            ),
                                        );
                                    }
                                } else {
                                    push(
                                        "topology",
                                        "ok",
                                        None,
                                        format!("topology: USING names resolve on '{collection}'"),
                                    );
                                }
                            }
                            Err((code, msg)) => {
                                ok_all = false;
                                failed = true;
                                push(
                                    "topology",
                                    "fail",
                                    Some(code.clone()),
                                    format!(
                                        "[{code}] topology: {msg}; fix: use one of the listed vectors"
                                    ),
                                );
                            }
                        }
                    }
                }
            }
            if ok_all && collections.len() > 1 {
                // Individual per-collection ok lines already pushed; nothing more.
            }
        } else {
            push(
                "topology",
                "skip",
                None,
                "topology: no executor available".to_string(),
            );
        }
    } else {
        push(
            "topology",
            "skip",
            None,
            "topology: skipped because the statement did not parse".to_string(),
        );
    }

    // Stage 4b: edge indexing state. qdrant-edge never indexes in the
    // background, so lag here explains "writes are not searchable yet" and
    // points at `qql edge optimize`.
    if use_edge
        && backend_reachable
        && let Some(exec) = executor.as_ref()
    {
        match collect_indexing_states(exec.client()).await {
            Ok(states) if states.is_empty() => push(
                "edge-indexing",
                "ok",
                None,
                "edge-indexing: no local collections yet".to_string(),
            ),
            Ok(states) => {
                for state in states {
                    match state.nudge {
                        Some(nudge) => push(
                            "edge-indexing",
                            "warn",
                            None,
                            format!("edge-indexing: {nudge}"),
                        ),
                        None => push(
                            "edge-indexing",
                            "ok",
                            None,
                            format!(
                                "edge-indexing: '{}' {} points, indexed {}",
                                state.collection,
                                state.points_count,
                                count_label(state.indexed_vectors_count)
                            ),
                        ),
                    }
                }
            }
            Err(e) => push(
                "edge-indexing",
                "warn",
                Some(e.code.to_string()),
                format!("edge-indexing: readout failed: {e}"),
            ),
        }
    }

    // Stage 5: backend doctor.
    if backend_reachable {
        push(
            "doctor",
            "ok",
            None,
            format!("doctor: backend at {url} answers SHOW COLLECTIONS"),
        );
    } else {
        failed = true;
        push("doctor", "fail", None, format!("doctor: {backend_note}"));
    }

    if let Some(exec) = executor.as_ref() {
        let _ = exec.close().await;
    }

    if json {
        println!(
            "{}",
            serde_json::json!({
                "ok": !failed,
                "operation": "check",
                "query": query,
                "stages": stages,
            })
        );
    } else if !quiet {
        for stage in &stages {
            let status = stage.get("status").and_then(|v| v.as_str()).unwrap_or("?");
            let message = stage.get("message").and_then(|v| v.as_str()).unwrap_or("");
            println!("[{status}] {message}");
        }
    }
    if failed {
        return Err("qql check failed; fix the first [fail] stage above".into());
    }
    Ok(())
}

fn parse_and_bind(
    query: &str,
    params: Option<&serde_json::Value>,
) -> Result<Vec<qql_core::ast::Stmt>, String> {
    let mut statements = qql_core::parser::Parser::parse_all(query).map_err(|e| e.to_string())?;
    if let Some(p) = params {
        for stmt in &mut statements {
            qql_core::params_json::bind_stmt_with_params(stmt, p).map_err(|e| e.to_string())?;
        }
    }
    Ok(statements)
}

fn statements_need_embeddings(stmts: &[qql_core::ast::Stmt]) -> bool {
    stmts.iter().any(stmt_needs_embeddings)
}

fn stmt_needs_embeddings(stmt: &qql_core::ast::Stmt) -> bool {
    use qql_core::ast::Stmt;
    match stmt {
        Stmt::Query(q) => query_stmt_needs_embeddings(q),
        Stmt::Upsert(u) => u.embedding.is_some() || !u.embed.is_empty(),
        _ => false,
    }
}

fn query_stmt_needs_embeddings(q: &qql_core::ast::QueryStmt) -> bool {
    q.ctes.iter().any(|c| query_stmt_needs_embeddings(&c.query))
        || query_expr_needs_embeddings(&q.expression)
}

fn query_expr_needs_embeddings(e: &qql_core::ast::QueryExpr) -> bool {
    use qql_core::ast::{QueryExpr, QueryInput, VectorValue};
    let input_needs = |input: &QueryInput| match input {
        QueryInput::Text { .. }
        | QueryInput::Image { .. }
        | QueryInput::Param(..)
        | QueryInput::PositionalParam(..) => true,
        QueryInput::Vector(VectorValue::Param(..) | VectorValue::PositionalParam(..)) => true,
        QueryInput::Vector(_) | QueryInput::Point(_) => false,
    };
    let prefetch_needs = |list: &[qql_core::ast::Prefetch]| {
        list.iter().any(|p| match &p.source {
            qql_core::ast::PrefetchSource::Query(q) => query_stmt_needs_embeddings(q),
            qql_core::ast::PrefetchSource::Cte(_) => false,
        })
    };
    match e {
        QueryExpr::Nearest {
            input, prefetch, ..
        } => input_needs(input) || prefetch_needs(prefetch),
        QueryExpr::Recommend {
            positive,
            negative,
            prefetch,
            ..
        } => {
            positive.iter().any(input_needs)
                || negative.iter().any(input_needs)
                || prefetch_needs(prefetch)
        }
        QueryExpr::Context {
            pairs, prefetch, ..
        } => {
            pairs
                .iter()
                .any(|p| input_needs(&p.positive) || input_needs(&p.negative))
                || prefetch_needs(prefetch)
        }
        QueryExpr::Discover {
            target,
            context,
            prefetch,
            ..
        } => {
            input_needs(target)
                || context
                    .iter()
                    .any(|p| input_needs(&p.positive) || input_needs(&p.negative))
                || prefetch_needs(prefetch)
        }
        QueryExpr::RelevanceFeedback {
            target,
            feedback,
            prefetch,
            ..
        } => {
            input_needs(target)
                || feedback.iter().any(|f| input_needs(&f.example))
                || prefetch_needs(prefetch)
        }
        QueryExpr::Hybrid { .. } => true,
        QueryExpr::Rerank {
            input, prefetch, ..
        } => input_needs(input) || prefetch_needs(prefetch),
        QueryExpr::CrossRerank { prefetch, .. }
        | QueryExpr::Fusion { prefetch, .. }
        | QueryExpr::Formula { prefetch, .. } => prefetch_needs(prefetch),
        QueryExpr::Points { .. } | QueryExpr::OrderBy { .. } | QueryExpr::SampleRandom => false,
    }
}

fn stmt_collections(stmts: &[qql_core::ast::Stmt]) -> Vec<String> {
    use qql_core::ast::{QueryCollection, Stmt};
    let mut out: Vec<String> = Vec::new();
    let mut push = |name: &str| {
        if !name.is_empty() && !out.iter().any(|v| v == name) {
            out.push(name.to_string());
        }
    };
    for stmt in stmts {
        match stmt {
            Stmt::Query(q) => {
                if let QueryCollection::Explicit(name) = &q.collection {
                    push(name);
                }
                for cte in &q.ctes {
                    if let QueryCollection::Explicit(name) = &cte.query.collection {
                        push(name);
                    }
                }
            }
            Stmt::Scroll(s) => push(&s.collection),
            Stmt::Upsert(u) => push(&u.collection),
            Stmt::Delete(d) => push(&d.collection),
            Stmt::ClearPayload(s) => push(&s.collection),
            Stmt::DeletePayload(s) => push(&s.collection),
            Stmt::DeleteVector(s) => push(&s.collection),
            Stmt::UpdateVector(s) => push(&s.collection),
            Stmt::UpdatePayload(s) => push(&s.collection),
            Stmt::Count(c) => {
                if let QueryCollection::Explicit(name) = &c.collection {
                    push(name);
                }
            }
            Stmt::Facet(f) => {
                if let QueryCollection::Explicit(name) = &f.collection {
                    push(name);
                }
            }
            _ => {}
        }
    }
    out
}

fn check_using_names(
    stmts: &[qql_core::ast::Stmt],
    dense: &[String],
    sparse: &[String],
    unnamed: bool,
) -> Result<(), (String, String)> {
    for stmt in stmts {
        if let qql_core::ast::Stmt::Query(q) = stmt {
            check_query_using(q, dense, sparse, unnamed)?;
        }
    }
    Ok(())
}

fn check_query_using(
    q: &qql_core::ast::QueryStmt,
    dense: &[String],
    sparse: &[String],
    unnamed: bool,
) -> Result<(), (String, String)> {
    for cte in &q.ctes {
        check_query_using(&cte.query, dense, sparse, unnamed)?;
    }
    check_expr_using(&q.expression, dense, sparse, unnamed)
}

fn check_expr_using(
    e: &qql_core::ast::QueryExpr,
    dense: &[String],
    sparse: &[String],
    unnamed: bool,
) -> Result<(), (String, String)> {
    use qql_core::ast::{PrefetchSource, QueryExpr, VectorKind};
    let check_target =
        |target: &Option<qql_core::ast::VectorTarget>| -> Result<(), (String, String)> {
            if let Some(t) = target {
                let in_dense = dense.iter().any(|n| n == &t.name);
                let in_sparse = sparse.iter().any(|n| n == &t.name);
                if !in_dense && !in_sparse {
                    if t.kind.is_some() {
                        return Ok(());
                    }
                    let mut available: Vec<String> =
                        dense.iter().chain(sparse.iter()).cloned().collect();
                    if unnamed {
                        available.push("<default>".to_string());
                    }
                    return Err((
                        "QQL-UNKNOWN-VECTOR".to_string(),
                        format!(
                            "no vector named '{}'. Available vectors: {}",
                            t.name,
                            available.join(", ")
                        ),
                    ));
                }
                if let Some(kind) = t.kind {
                    let actual = if in_dense {
                        VectorKind::Dense
                    } else {
                        VectorKind::Sparse
                    };
                    if kind != actual {
                        return Err((
                            "QQL-VECTOR-KIND".to_string(),
                            format!(
                                "vector '{}' is {} on the collection",
                                t.name,
                                if in_dense { "dense" } else { "sparse" }
                            ),
                        ));
                    }
                }
            }
            Ok(())
        };
    let check_prefetches = |list: &[qql_core::ast::Prefetch]| -> Result<(), (String, String)> {
        for p in list {
            if let PrefetchSource::Query(q) = &p.source {
                check_query_using(q, dense, sparse, unnamed)?;
            }
        }
        Ok(())
    };
    match e {
        QueryExpr::Nearest {
            using, prefetch, ..
        }
        | QueryExpr::Recommend {
            using, prefetch, ..
        }
        | QueryExpr::Context {
            using, prefetch, ..
        }
        | QueryExpr::Discover {
            using, prefetch, ..
        }
        | QueryExpr::RelevanceFeedback {
            using, prefetch, ..
        } => {
            check_target(using)?;
            check_prefetches(prefetch)
        }
        QueryExpr::Rerank {
            using, prefetch, ..
        } => {
            check_target(using)?;
            check_prefetches(prefetch)
        }
        QueryExpr::Hybrid {
            dense_vector,
            sparse_vector,
            ..
        } => {
            if let Some(name) = dense_vector
                && !dense.iter().any(|n| n == name)
            {
                return Err((
                    "QQL-UNKNOWN-VECTOR".to_string(),
                    format!(
                        "no dense vector named '{name}'. Available dense: {}",
                        dense.join(", ")
                    ),
                ));
            }
            if let Some(name) = sparse_vector
                && !sparse.iter().any(|n| n == name)
            {
                return Err((
                    "QQL-UNKNOWN-VECTOR".to_string(),
                    format!(
                        "no sparse vector named '{name}'. Available sparse: {}",
                        sparse.join(", ")
                    ),
                ));
            }
            Ok(())
        }
        QueryExpr::CrossRerank { prefetch, .. }
        | QueryExpr::Fusion { prefetch, .. }
        | QueryExpr::Formula { prefetch, .. } => check_prefetches(prefetch),
        QueryExpr::Points { .. } | QueryExpr::OrderBy { .. } | QueryExpr::SampleRandom => Ok(()),
    }
}

pub fn handle_version() -> Result<(), Box<dyn std::error::Error>> {
    let resp = output::VersionResponse {
        ok: true,
        command: "version".to_string(),
        version: VERSION.to_string(),
        message: format!("qql version {}", VERSION),
    };
    let s = serde_json::to_string_pretty(&resp)?;
    println!("{}", s);
    Ok(())
}

// ── Explain implementation ────────────────────────────────────

fn explain_query(query: &str) -> Result<String, String> {
    // Try multi-statement first — if the input has semicolons we get a
    // per-statement breakdown.  Falls back to single-statement for simple
    // queries (parse_all rejects them with a confusing semicolon error).
    match qql::executor::Executor::explain_all(query) {
        Ok(plan) if !plan.is_empty() => Ok(plan),
        Ok(_) | Err(_) => qql::executor::Executor::explain(query).map_err(|e| e.to_string()),
    }
}
