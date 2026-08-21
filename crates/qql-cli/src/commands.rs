#[cfg(feature = "edge")]
use std::io::IsTerminal;

pub async fn handle_doctor(
    url: &str,
    use_edge: bool,
    json: bool,
    quiet: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = executor(url, use_edge)?;
    let hosts = doctor_host_summary(executor.config(), use_edge);
    let result = executor
        .execute("SHOW COLLECTIONS", qql::executor::OnError::Stop)
        .await;
    executor.close().await?;
    match result {
        Ok(_) => {
            if quiet {
                return Ok(());
            }
            let target = if use_edge {
                "the local edge backend".to_string()
            } else {
                format!("Qdrant at {url}")
            };
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "ok": true,
                        "healthy": true,
                        "message": format!("Connected to {target}"),
                        "hosts": hosts,
                    })
                );
            } else {
                println!("Connected to {target} (healthy)");
                print_doctor_hosts(&hosts);
            }
            Ok(())
        }
        Err(e) => {
            let target = if use_edge {
                "the local edge backend".to_string()
            } else {
                format!("Qdrant at {url}")
            };
            if quiet {
                return Err(format!("Failed to connect to {target}: {e}").into());
            }
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "ok": false,
                        "healthy": false,
                        "error": format!("Failed to connect to {target}: {e}"),
                        "hosts": hosts,
                    })
                );
            } else {
                println!("Failed to connect to {target}: {e}");
                print_doctor_hosts(&hosts);
            }
            Err(e.into())
        }
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
        hints.push("edge has no GROUP BY, SHARD keys, ALTER COLLECTION, or ACORN");
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
use crate::output;
use crate::script;

const VERSION: &str = env!("CARGO_PKG_VERSION");

// ── Public handlers ───────────────────────────────────────────

pub async fn handle_exec(
    url: &str,
    use_edge: bool,
    query: &str,
    json: bool,
    quiet: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = executor(url, use_edge)?;
    let result = executor.execute(query, qql::executor::OnError::Stop).await;
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
    json: bool,
    quiet: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let plan = explain_query(query)?;
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

pub async fn handle_connect(url: &str, use_edge: bool) -> Result<(), Box<dyn std::error::Error>> {
    let executor = executor(url, use_edge)?;
    let initial = executor
        .execute("SHOW COLLECTIONS", qql::executor::OnError::Stop)
        .await;
    if let Err(error) = initial {
        executor.close().await?;
        return Err(error.into());
    }
    output::print_banner();
    let target = if use_edge { "local edge" } else { url };
    output::print_success(&format!("Connected to \x1b[36m{}\x1b[0m", target));
    println!("Type \x1b[1mhelp\x1b[0m for available commands or \x1b[1mexit\x1b[0m to quit.\n");

    let mut rl = rustyline::DefaultEditor::new()?;

    loop {
        let prompt = "\x1b[32m\x1b[1mqql>\x1b[0m ";
        let line = match rl.readline(prompt) {
            Ok(l) => l,
            Err(_) => {
                println!("\nBye.");
                break;
            }
        };

        let trimmed = line.trim().to_string();
        if trimmed.is_empty() {
            continue;
        }

        let _ = rl.add_history_entry(&trimmed);

        let lower = trimmed.to_lowercase();

        if lower == "exit" || lower == "quit" || lower == "\\q" || lower == ":q" {
            println!("Bye.");
            break;
        }

        if lower == "help" || lower == "\\h" || lower == "?" {
            print_repl_help();
            continue;
        }

        if let Some(args) = cut_command_prefix(&trimmed, "explain") {
            match explain_query(&args) {
                Ok(plan) => {
                    println!("\x1b[1mQuery Plan\x1b[0m");
                    println!("{}", plan);
                }
                Err(e) => output::print_error(&format!("explain error: {}", e)),
            }
            continue;
        }

        if let Some(args) = cut_command_prefix(&trimmed, "execute") {
            match script::read_script(&args) {
                Ok(stmts) => {
                    let (ok, fail) = script::execute_script(stmts, false, |stmt| {
                        explain_query(stmt).map_err(|e| e.to_string())
                    })?;
                    output::print_success(&format!(
                        "Executed script {} ({} succeeded, {} failed)",
                        args, ok, fail
                    ));
                }
                Err(e) => output::print_error(&format!("execute error: {}", e)),
            }
            continue;
        }

        if let Some(args) = cut_command_prefix(&trimmed, "\\e") {
            match script::read_script(&args) {
                Ok(stmts) => {
                    let (ok, fail) = script::execute_script(stmts, false, |stmt| {
                        explain_query(stmt).map_err(|e| e.to_string())
                    })?;
                    output::print_success(&format!(
                        "Executed script {} ({} succeeded, {} failed)",
                        args, ok, fail
                    ));
                }
                Err(e) => output::print_error(&format!("execute error: {}", e)),
            }
            continue;
        }

        if let Some(args) = cut_command_prefix(&trimmed, "dump") {
            let parts: Vec<&str> = args.split_whitespace().collect();
            let dump_parts = if parts.len() >= 3 && parts[0].eq_ignore_ascii_case("collection") {
                &parts[1..]
            } else {
                &parts
            };
            if dump_parts.len() != 2 {
                output::print_error("dump error: usage DUMP [COLLECTION] <name> <output.qql>");
                continue;
            }
            match dump::dump_collection(&executor, dump_parts[0], dump_parts[1], 50, None).await {
                Ok(stats) => output::print_success(&format!(
                    "Dumped collection '{}' to {} ({} written, {} skipped, {} batches)",
                    dump_parts[0], dump_parts[1], stats.written, stats.skipped, stats.batches
                )),
                Err(e) => output::print_error(&format!("dump error: {}", e)),
            }
            continue;
        }

        match executor
            .execute(&trimmed, qql::executor::OnError::Stop)
            .await
        {
            Ok(report) => {
                if let Err(e) = crate::table::render_report(&report, false) {
                    output::print_error(&format!("display error: {}", e));
                }
            }
            Err(e) => output::print_error(&format!("execution error: {}", e)),
        }
    }

    executor.close().await?;
    Ok(())
}

fn executor(
    url: &str,
    use_edge: bool,
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
    let use_grpc = url.contains(":6334");
    #[cfg(not(feature = "grpc"))]
    let use_grpc = false;

    let client: Box<dyn qql::client::QdrantOps> = if use_grpc {
        #[cfg(feature = "grpc")]
        {
            Box::new(qql::grpc::GrpcQdrant::from_url(
                url,
                std::env::var("QDRANT_API_KEY")
                    .ok()
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
                std::env::var("QDRANT_API_KEY")
                    .ok()
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
    match config.embedder.as_str() {
        "fastembed" => {
            let is_tty = std::io::stdout().is_terminal();
            let show_progress = config.show_download_progress || is_tty;
            let model_name = config.model.as_deref().unwrap_or("BGESmallENV15");
            if show_progress {
                eprintln!("ℹ Initializing local edge embedder (model: '{model_name}'). Model weights are downloaded on first run if not cached.");
            }
            let options = qql_edge::LocalExecutorOptions {
                on_disk_payload: config.on_disk_payload,
                model: config.model,
                sparse_model: config.sparse_model,
                multi_model: config.multi_model.or(config.multi_embed_model.clone()),
                image_model: config.image_model.or(config.image_embed_model.clone()),
                reranker_model: config.reranker_model.clone(),
                cache_dir: config.cache_dir,
                show_download_progress: show_progress,
            };
            qql_edge::local_executor_with_options(config.data_dir, options)
                .map_err(|error| format!("edge initialization failed: {error}").into())
        }
        "http" => {
            let endpoint = config.embed_url.ok_or(
                "the edge HTTP embedder requires embed_url; run `qql config edge --embedder http --embed-url <URL>`",
            )?;
            qql_edge::http_executor_with_options(
                config.data_dir,
                config.on_disk_payload,
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

    if write {
        if let Some(p) = path {
            std::fs::write(p, format!("{}\n", formatted))
                .map_err(|e| format!("cannot write '{}': {}", p, e))?;
            return Ok(());
        }
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
    let path = config.save()?;
    println!("Saved edge configuration to {}", path.display());
    println!("Use it with: qql --edge exec \"SHOW COLLECTIONS\"");
    Ok(())
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

// ── REPL helpers ──────────────────────────────────────────────

fn cut_command_prefix(input: &str, prefix: &str) -> Option<String> {
    let input_trimmed = input.trim();
    let lower = input_trimmed.to_lowercase();

    if lower.len() <= prefix.len() || !lower.starts_with(prefix) {
        return None;
    }

    let after = &input_trimmed[prefix.len()..];
    if after.starts_with(' ') {
        Some(after.trim().to_string())
    } else {
        None
    }
}

fn print_repl_help() {
    // NOTE: must be a normal (non-raw) string so the \x1b[..m escapes are
    // processed — a raw string would print the literal escape text.
    let help = "\x1b[1mAvailable Statements:\x1b[0m\n\
\n  \x1b[33mUPSERT INTO\x1b[0m <name> \x1b[33mVALUES\x1b[0m {id: 1, text: '...', ...}\n\
\n  \x1b[33mCREATE COLLECTION\x1b[0m <name> [\x1b[33mHYBRID\x1b[0m [\x1b[33mRERANK\x1b[0m]]\n\
\n  \x1b[33mDROP COLLECTION\x1b[0m <name>\n\
\n  \x1b[33mSHOW COLLECTIONS\x1b[0m\n\
\n  \x1b[33mQUERY\x1b[0m ['<text>' | NEAREST POINT <id> | ...]\n\
      \x1b[33mFROM\x1b[0m <collection> [\x1b[33mUSING\x1b[0m <vector> [\x1b[33mAS DENSE|SPARSE\x1b[0m]] \x1b[33mLIMIT\x1b[0m <n>\n\
\n  \x1b[33mQUERY POINTS\x1b[0m (<id>, ...) \x1b[33mFROM\x1b[0m <name> [\x1b[33mWITH PAYLOAD true\x1b[0m]\n\
\n  \x1b[33mSCROLL FROM\x1b[0m <name> [\x1b[33mWHERE\x1b[0m <filter>] [\x1b[33mAFTER\x1b[0m '<id>'] [\x1b[33mWITH VECTOR\x1b[0m] \x1b[33mLIMIT\x1b[0m <n>\n\
\n  \x1b[33mDELETE FROM\x1b[0m <name> \x1b[33mWHERE\x1b[0m id = '<id>' | <field> = '<value>'\n\
\n\x1b[1mBuilt-in Commands:\x1b[0m\n\
\n  \x1b[36mhelp\x1b[0m, \x1b[36m?\x1b[0m           Show this help\n\
  \x1b[36mexplain <query>\x1b[0m  Show query plan without executing\n\
  \x1b[36mexecute <file>\x1b[0m  Run a .qql script file\n\
  \x1b[36m\\e <file>\x1b[0m        Shortcut for execute\n\
  \x1b[36mdump <name> <file>\x1b[0m  Dump collection (schema + vectors + payload) to .qql\n\
  \x1b[36mexit\x1b[0m, \x1b[36mquit\x1b[0m      Exit the shell\n\
\n\x1b[1mKeyboard Shortcuts:\x1b[0m\n\
\n  Ctrl-C         Cancel current input\n\
  Ctrl-D         Exit shell\n";
    println!("{}", help);
}
