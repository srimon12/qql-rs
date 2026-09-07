//! `qql` — Qdrant Query Language CLI: query exec, scripts, explain, REPL,
//! REST→QQL conversion, collection dump, formatting, and edge configuration.
use clap::Parser;
use std::path::PathBuf;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

mod commands;
mod config;
mod convert;
mod dump;
mod output;
mod repl;
mod script;
mod table;

#[derive(Parser)]
#[command(name = "qql", about = "Qdrant Query Language CLI")]
struct Cli {
    /// Qdrant REST URL. Overrides QDRANT_URL when supplied.
    #[arg(long, global = true)]
    url: Option<String>,
    /// Execute supported commands against the configured in-process edge backend.
    #[arg(long, global = true)]
    edge: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Execute a QQL query
    Exec {
        /// QQL query string (e.g., "QUERY 'hello' FROM docs LIMIT 5")
        query: String,
        /// Parameter in key=value format (can be specified multiple times)
        #[arg(long = "param", short = 'p')]
        params: Vec<String>,
        /// Path to JSON file containing parameter map or positional array
        #[arg(long = "params-file")]
        params_file: Option<PathBuf>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Quiet mode
        #[arg(long, short)]
        quiet: bool,
    },
    /// Execute multiple QQL queries from a file
    Execute {
        /// Path to .qql script file
        file: String,
        /// Stop on first error
        #[arg(long)]
        stop_on_error: bool,
    },
    /// Explain a QQL query (show execution plan)
    Explain {
        query: String,
        /// Parameter in key=value format (can be specified multiple times)
        #[arg(long = "param", short = 'p')]
        params: Vec<String>,
        /// Path to JSON file containing parameter map or positional array
        #[arg(long = "params-file")]
        params_file: Option<PathBuf>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Quiet mode
        #[arg(long, short)]
        quiet: bool,
    },
    /// Start interactive REPL connected to Qdrant
    #[command(alias = "repl")]
    Connect,
    /// Convert REST JSON payload to QQL
    Convert {
        /// Path to JSON file (or stdin if omitted)
        file: Option<String>,
    },
    /// Format QQL source into canonical form
    Fmt {
        /// Path to .qql file (or stdin if omitted)
        file: Option<String>,
        /// Check formatting without writing; exit non-zero if changes are needed
        #[arg(long)]
        check: bool,
        /// Write the formatted output back to the file
        #[arg(long)]
        write: bool,
    },
    /// Dump collection to .qql file
    Dump {
        collection: String,
        output: String,
        #[arg(long, default_value = "100")]
        batch_size: u32,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Quiet mode
        #[arg(long, short)]
        quiet: bool,
    },
    /// Check Qdrant connection health
    Doctor {
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Quiet mode
        #[arg(long, short)]
        quiet: bool,
    },
    /// Configure persistent CLI settings.
    Config {
        #[command(subcommand)]
        command: Box<ConfigCommand>,
    },
    /// Show version
    Version,
}

#[derive(clap::Subcommand)]
enum ConfigCommand {
    /// Configure the local qdrant-edge backend used by --edge.
    Edge {
        /// Directory for persistent qdrant-edge data.
        #[arg(long)]
        data_dir: Option<PathBuf>,
        /// Keep payloads in memory instead of persisting them to disk.
        #[arg(long)]
        in_memory: bool,
        /// Embedding backend: fastembed or an OpenAI-compatible HTTP endpoint.
        #[arg(long, default_value = "fastembed")]
        embedder: String,
        /// Local FastEmbed dense model name or alias.
        #[arg(long)]
        model: Option<String>,
        /// Offline sparse model for fastembed (e.g. splade, bge-m3).
        #[arg(long)]
        sparse_model: Option<String>,
        /// Offline multivector model for fastembed (e.g. bge-m3).
        #[arg(long)]
        multi_model: Option<String>,
        /// Offline CLIP vision model for fastembed (e.g. clip-vision).
        #[arg(long)]
        image_model: Option<String>,
        /// Offline cross-encoder model (e.g. bge-reranker-base).
        #[arg(long)]
        reranker_model: Option<String>,
        /// Directory used for downloaded FastEmbed models.
        #[arg(long)]
        cache_dir: Option<PathBuf>,
        /// Show model download progress.
        #[arg(long)]
        show_download_progress: bool,
        /// OpenAI-compatible embedding endpoint used by the HTTP backend.
        #[arg(long)]
        embed_url: Option<String>,
        /// API key used by the HTTP embedding backend.
        #[arg(long, default_value = "")]
        embed_key: String,
        /// Model name sent to the HTTP embedding backend.
        #[arg(long, default_value = "nomic-embed-text")]
        embed_model: String,
        /// Expected HTTP embedding dimension.
        #[arg(long, default_value_t = 768)]
        embed_dim: usize,
        /// Optional multi/ColBERT HTTP embedding endpoint.
        #[arg(long)]
        multi_embed_url: Option<String>,
        /// API key for the multi embedding endpoint.
        #[arg(long)]
        multi_embed_key: Option<String>,
        /// Multi/ColBERT model name for HTTP multi embeds.
        #[arg(long)]
        multi_embed_model: Option<String>,
        /// Per-token dimension for multi embeds (0 = skip check).
        #[arg(long, default_value_t = 0)]
        multi_embed_dim: usize,
        /// Optional image/CLIP vision HTTP embedding endpoint.
        #[arg(long)]
        image_embed_url: Option<String>,
        /// API key for the image embedding endpoint.
        #[arg(long)]
        image_embed_key: Option<String>,
        /// Image/CLIP vision model name for HTTP image embeds.
        #[arg(long)]
        image_embed_model: Option<String>,
        /// Dense dimension for image embeds (CLIP = 512; 0 = use dense dim).
        #[arg(long, default_value_t = 0)]
        image_embed_dim: usize,
    },
}

fn resolve_query_params(
    query: &str,
    params: &[String],
    params_file: Option<&PathBuf>,
) -> Result<String, Box<dyn std::error::Error>> {
    if params.is_empty() && params_file.is_none() {
        return Ok(query.to_string());
    }

    let mut map = serde_json::Map::new();

    if let Some(file_path) = params_file {
        let content = std::fs::read_to_string(file_path)?;
        let parsed: serde_json::Value = serde_json::from_str(&content)?;
        match parsed {
            serde_json::Value::Object(obj) => {
                for (k, v) in obj {
                    let key = k.strip_prefix(':').unwrap_or(&k).to_string();
                    map.insert(key, v);
                }
            }
            serde_json::Value::Array(_) => {
                let bound = qql_core::params_json::bind_str_with_params(query, &parsed, false)?;
                return Ok(bound);
            }
            _ => {
                return Err("--params-file must contain a JSON object or array".into());
            }
        }
    }

    for p in params {
        let (key_raw, val_raw) = p
            .split_once('=')
            .ok_or_else(|| format!("parameter must be in key=value format, got '{p}'"))?;
        let key = key_raw
            .trim()
            .strip_prefix(':')
            .unwrap_or(key_raw.trim())
            .to_string();
        let val_trimmed = val_raw.trim();
        let parsed_val: serde_json::Value = serde_json::from_str(val_trimmed)
            .unwrap_or_else(|_| serde_json::Value::String(val_trimmed.to_string()));
        map.insert(key, parsed_val);
    }

    let bound =
        qql_core::params_json::bind_str_with_params(query, &serde_json::Value::Object(map), false)?;
    Ok(bound)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let use_edge = cli.edge;
    let url = cli
        .url
        .or_else(|| std::env::var("QDRANT_URL").ok())
        .unwrap_or_else(|| "http://localhost:6333".to_string());

    match cli.command.unwrap_or(Command::Connect) {
        Command::Exec {
            query,
            params,
            params_file,
            json,
            quiet,
        } => {
            let bound = resolve_query_params(&query, &params, params_file.as_ref())?;
            commands::handle_exec(&url, use_edge, &bound, json, quiet).await
        }
        Command::Execute {
            file,
            stop_on_error,
        } => commands::handle_execute_file(&url, use_edge, &file, stop_on_error).await,
        Command::Explain {
            query,
            params,
            params_file,
            json,
            quiet,
        } => {
            let bound = resolve_query_params(&query, &params, params_file.as_ref())?;
            commands::handle_explain(&bound, json, quiet)
        }
        Command::Connect => commands::handle_connect(&url, use_edge).await,
        Command::Convert { file } => commands::handle_convert(file.as_deref()),
        Command::Fmt { file, check, write } => commands::handle_fmt(file.as_deref(), check, write),
        Command::Dump {
            collection,
            output,
            batch_size,
            json,
            quiet,
        } => {
            use std::io::Write;
            let progress_fn = |p: dump::DumpProgress| {
                eprint!("\rDumped {} points ({} batches)...", p.written, p.batches);
                let _ = std::io::stderr().flush();
            };
            let progress_cb: Option<&(dyn Fn(dump::DumpProgress) + Sync)> = if !json && !quiet {
                Some(&progress_fn)
            } else {
                None
            };
            let stats = commands::handle_dump(
                &url,
                use_edge,
                &collection,
                &output,
                batch_size,
                progress_cb,
            )
            .await?;
            if !json && !quiet && stats.batches > 0 {
                eprintln!();
            }
            let msg = format!(
                "Dumped collection '{}' to {} ({} written, {} skipped, {} batches)",
                collection, output, stats.written, stats.skipped, stats.batches
            );
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "ok": true,
                        "operation": "dump",
                        "collection": collection,
                        "output": output,
                        "written": stats.written,
                        "skipped": stats.skipped,
                        "batches": stats.batches,
                        "message": msg,
                    })
                );
            } else {
                println!("{}", msg);
            }
            Ok(())
        }
        Command::Doctor { json, quiet } => {
            commands::handle_doctor(&url, use_edge, json, quiet).await
        }
        Command::Config { command } => match *command {
            ConfigCommand::Edge {
                data_dir,
                in_memory,
                embedder,
                model,
                sparse_model,
                multi_model,
                image_model,
                reranker_model,
                cache_dir,
                show_download_progress,
                embed_url,
                embed_key,
                embed_model,
                embed_dim,
                multi_embed_url,
                multi_embed_key,
                multi_embed_model,
                multi_embed_dim,
                image_embed_url,
                image_embed_key,
                image_embed_model,
                image_embed_dim,
            } => commands::handle_configure_edge(config::EdgeConfig {
                data_dir: data_dir.unwrap_or_else(|| config::EdgeConfig::default().data_dir),
                on_disk_payload: !in_memory,
                embedder,
                model,
                sparse_model,
                multi_model,
                image_model,
                reranker_model,
                cache_dir,
                show_download_progress,
                embed_url,
                embed_key,
                embed_model,
                embed_dimension: embed_dim,
                multi_embed_url,
                multi_embed_key,
                multi_embed_model,
                multi_embed_dimension: multi_embed_dim,
                image_embed_url,
                image_embed_key,
                image_embed_model,
                image_embed_dimension: image_embed_dim,
            }),
        },
        Command::Version => commands::handle_version(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_query_params_named() {
        let query = "QUERY TEXT :q FROM docs WHERE price < :p LIMIT :l;";
        let params = vec![
            "q=laptop".to_string(),
            ":p=999.50".to_string(),
            "l=5".to_string(),
        ];
        let bound = resolve_query_params(query, &params, None).unwrap();
        assert!(bound.contains("QUERY TEXT 'laptop' FROM docs WHERE price < 999.5"));
        assert!(bound.contains("LIMIT 5"));
    }

    #[test]
    fn test_resolve_query_params_empty() {
        let query = "QUERY TEXT 'test' FROM docs LIMIT 10;";
        let bound = resolve_query_params(query, &[], None).unwrap();
        assert_eq!(bound, query);
    }
}
