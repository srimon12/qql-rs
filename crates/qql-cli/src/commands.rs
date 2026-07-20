use crate::convert;
use crate::dump;
use crate::output;
use crate::script;

const VERSION: &str = "0.1.0";

// ── Public handlers ───────────────────────────────────────────

pub async fn handle_exec(
    url: &str,
    query: &str,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = executor(url)?;
    let response = executor.execute(query).await?;
    if json {
        let s = serde_json::to_string_pretty(&response)?;
        println!("{}", s);
    } else {
        println!("{}", response.message);
        if let Some(data) = response.data {
            println!("{}", serde_json::to_string_pretty(&data)?);
        }
    }
    Ok(())
}

pub async fn handle_execute_file(
    url: &str,
    path: &str,
    stop_on_error: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let statements = script::read_script(path).map_err(|e| format!("{}", e))?;
    let executor = executor(url)?;
    let mut ok_count = 0;
    let mut fail_count = 0;

    for (index, statement) in statements.iter().enumerate() {
        match executor.execute(statement).await {
            Ok(_) => ok_count += 1,
            Err(error) => {
                fail_count += 1;
                output::print_error(&format!("statement {}: {}", index + 1, error));
                if stop_on_error {
                    return Err(format!("statement {} failed: {}", index + 1, error).into());
                }
            }
        }
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

pub fn handle_explain(query: &str) -> Result<(), Box<dyn std::error::Error>> {
    let plan = explain_query(query)?;

    let resp = output::ExplainResponse {
        ok: true,
        query: query.to_string(),
        plan: plan.clone(),
    };
    let s = serde_json::to_string_pretty(&resp)?;
    println!("{}", s);
    Ok(())
}

pub async fn handle_connect(url: &str) -> Result<(), Box<dyn std::error::Error>> {
    let executor = executor(url)?;
    executor.execute("SHOW COLLECTIONS").await?;
    output::print_banner();
    output::print_success(&format!("Connected to \x1b[36m{}\x1b[0m", url));
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
            match handle_dump(url, dump_parts[0], dump_parts[1], 50).await {
                Ok(msg) => output::print_success(&msg),
                Err(e) => output::print_error(&format!("dump error: {}", e)),
            }
            continue;
        }

        match executor.execute(&trimmed).await {
            Ok(response) => {
                output::print_success(&response.message);
                if let Some(data) = response.data {
                    println!("{}", serde_json::to_string_pretty(&data)?);
                }
            }
            Err(e) => output::print_error(&format!("execution error: {}", e)),
        }
    }

    Ok(())
}

fn executor(url: &str) -> Result<qql::executor::Executor, Box<dyn std::error::Error>> {
    let config = qql::config::QqlConfig::load()?.unwrap_or_default();

    let client: Box<dyn qql::client::QdrantOps> = if url.contains(":6334") {
        Box::new(qql::grpc::GrpcQdrant::from_url(
            url,
            std::env::var("QDRANT_API_KEY")
                .ok()
                .or_else(|| config.secret.clone()),
        )?)
    } else {
        Box::new(qql::rest::RestQdrant::new(
            url.to_owned(),
            std::env::var("QDRANT_API_KEY")
                .ok()
                .or_else(|| config.secret.clone()),
        )?)
    };

    let embedder = if let Some(endpoint) = &config.embedding_endpoint {
        if !endpoint.trim().is_empty() {
            let api_key = config.embedding_api_key.clone().unwrap_or_default();
            let model = config.embedding_model.clone().unwrap_or_default();
            let dimension = config.embedding_dimension;
            let http_emb =
                qql::embedder::HttpEmbedder::new(endpoint.clone(), api_key, model, dimension)?;
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
        embedder,
    ))
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

pub async fn handle_dump(
    url: &str,
    collection: &str,
    output: &str,
    batch_size: u32,
) -> Result<String, Box<dyn std::error::Error>> {
    let exec = executor(url)?;
    let (written, skipped) = dump::dump_collection(&exec, collection, output, batch_size, "", "").await?;
    Ok(format!(
        "Dumped collection '{}' to {} ({} written, {} skipped)",
        collection, output, written, skipped
    ))
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
    qql::executor::Executor::explain(query).map_err(|e| e.to_string())
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
    println!(
        r#"
\x1b[1mAvailable Statements:\x1b[0m

  \x1b[33mINSERT INTO\x1b[0m COLLECTION <name> \x1b[33mVALUES\x1b[0m {{'text': '...', ...}}

  \x1b[33mCREATE COLLECTION\x1b[0m <name> [\x1b[33mHYBRID\x1b[0m [\x1b[33mRERANK\x1b[0m]]

  \x1b[33mDROP COLLECTION\x1b[0m <name>

  \x1b[33mSHOW COLLECTIONS\x1b[0m

  \x1b[33mQUERY\x1b[0m ['<text>' | <id> | NEAREST '<text>' | ...]
      \x1b[33mFROM\x1b[0m <collection> \x1b[33mLIMIT\x1b[0m <n>

  \x1b[33mSELECT\x1b[0m * \x1b[33mFROM\x1b[0m <name> \x1b[33mWHERE id =\x1b[0m '<id>|<int>'

  \x1b[33mSCROLL FROM\x1b[0m <name> [\x1b[33mWHERE\x1b[0m <filter>] [\x1b[33mAFTER\x1b[0m '<id>'] \x1b[33mLIMIT\x1b[0m <n>

  \x1b[33mDELETE FROM\x1b[0m <name> \x1b[33mWHERE\x1b[0m id = '<id>' | <field> = '<value>'

\x1b[1mBuilt-in Commands:\x1b[0m

  \x1b[36mhelp\x1b[0m, \x1b[36m?\x1b[0m           Show this help
  \x1b[36mexplain <query>\x1b[0m  Show query plan without executing
  \x1b[36mexecute <file>\x1b[0m  Run a .qql script file
  \x1b[36m\e <file>\x1b[0m        Shortcut for execute
  \x1b[36mdump <name> <file>\x1b[0m  Dump a collection to a .qql script file
  \x1b[36mexit\x1b[0m, \x1b[36mquit\x1b[0m      Exit the shell

\x1b[1mKeyboard Shortcuts:\x1b[0m

  Ctrl-C         Cancel current input
  Ctrl-D         Exit shell
"#
    );
}
