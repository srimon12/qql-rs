use clap::Parser;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

mod commands;
mod convert;
mod dump;
mod output;
mod script;

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
    Explain { query: String },
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
        Command::Exec { query, json, quiet: _ } => commands::handle_exec(&url, &query, json).await,
        Command::Execute {
            file,
            stop_on_error,
        } => commands::handle_execute_file(&url, &file, stop_on_error).await,
        Command::Explain { query } => commands::handle_explain(&query),
        Command::Connect => commands::handle_connect(&url).await,
        Command::Convert { file } => commands::handle_convert(file.as_deref()),
        Command::Dump {
            collection,
            output,
            batch_size,
            json,
        } => {
            let msg = commands::handle_dump(&url, &collection, &output, batch_size).await?;
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "ok": true,
                        "operation": "dump",
                        "message": msg,
                    })
                );
            } else {
                println!("{}", msg);
            }
            Ok(())
        }
        Command::Version => commands::handle_version(),
    }
}
