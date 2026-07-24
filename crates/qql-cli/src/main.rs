use clap::Parser;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

mod commands;
mod convert;
mod dump;
mod output;
mod script;
mod table;

#[derive(Parser)]
#[command(name = "qql", about = "Qdrant Query Language CLI")]
struct Cli {
    /// Qdrant REST URL. Overrides QDRANT_URL when supplied.
    #[arg(long, global = true)]
    url: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Execute a QQL query
    Exec {
        /// QQL query string (e.g., "QUERY 'hello' FROM docs LIMIT 5")
        query: String,
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
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Quiet mode
        #[arg(long, short)]
        quiet: bool,
    },
    /// Start interactive REPL connected to Qdrant
    Connect,
    /// Convert REST JSON payload to QQL
    Convert {
        /// Path to JSON file (or stdin if omitted)
        file: Option<String>,
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
    /// Run a QQL query against a local qdrant-edge instance (no server needed)
    #[cfg(feature = "edge")]
    Edge {
        /// QQL statement to execute
        query: String,
        /// Directory for persistent qdrant-edge data
        #[arg(long, default_value = "/tmp/qql-edge")]
        data_dir: String,
        /// Store payload on disk instead of memory
        #[arg(long)]
        on_disk: bool,
        /// Embedder backend: 'fastembed' (local ONNX) or 'http' (external provider)
        #[arg(long, default_value = "fastembed")]
        embedder: String,
        /// HTTP embedder endpoint URL (when --embedder http)
        #[arg(long)]
        embed_url: Option<String>,
        /// API key for HTTP embedder (when --embedder http)
        #[arg(long, default_value = "")]
        embed_key: String,
        /// Model name for HTTP embedder (when --embedder http)
        #[arg(long, default_value = "nomic-embed-text")]
        embed_model: String,
        /// Expected embedding dimension for HTTP embedder (when --embedder http)
        #[arg(long, default_value = "768")]
        embed_dim: usize,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show version
    Version,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let url = cli
        .url
        .or_else(|| std::env::var("QDRANT_URL").ok())
        .unwrap_or_else(|| "http://localhost:6333".to_string());

    match cli.command {
        Command::Exec { query, json, quiet } => {
            commands::handle_exec(&url, &query, json, quiet).await
        }
        Command::Execute {
            file,
            stop_on_error,
        } => commands::handle_execute_file(&url, &file, stop_on_error).await,
        Command::Explain {
            query,
            json: _,
            quiet: _,
        } => commands::handle_explain(&query),
        Command::Connect => commands::handle_connect(&url).await,
        Command::Convert { file } => commands::handle_convert(file.as_deref()),
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
            let stats =
                commands::handle_dump(&url, &collection, &output, batch_size, progress_cb).await?;
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
        Command::Doctor { json, quiet: _ } => commands::handle_doctor(&url, json).await,
        #[cfg(feature = "edge")]
        Command::Edge {
            query,
            data_dir,
            on_disk,
            embedder,
            embed_url,
            embed_key,
            embed_model,
            embed_dim,
            json,
        } => {
            commands::handle_edge(
                &query,
                &data_dir,
                on_disk,
                &embedder,
                embed_url.as_deref(),
                &embed_key,
                &embed_model,
                embed_dim,
                json,
            )
            .await
        }
        Command::Version => commands::handle_version(),
    }
}
