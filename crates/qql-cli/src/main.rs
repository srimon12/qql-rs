use clap::Parser;

mod commands;
mod convert;
mod dump;
mod output;
mod script;

#[derive(Parser)]
#[command(name = "qql", about = "Qdrant Query Language CLI")]
struct Cli {
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
    Connect {
        /// Qdrant gRPC URL
        #[arg(long, default_value = "http://localhost:6334")]
        url: String,
    },
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
    },
    /// Show version
    Version,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Command::Exec { query, json } => commands::handle_exec(&query, json),
        Command::Execute {
            file,
            stop_on_error,
        } => commands::handle_execute_file(&file, stop_on_error),
        Command::Explain { query } => commands::handle_explain(&query),
        Command::Connect { url } => commands::handle_connect(&url),
        Command::Convert { file } => commands::handle_convert(file.as_deref()),
        Command::Dump {
            collection,
            output,
            batch_size,
        } => {
            let msg = commands::handle_dump(&collection, &output, batch_size)?;
            println!("{}", msg);
            Ok(())
        }
        Command::Version => commands::handle_version(),
    }
}
