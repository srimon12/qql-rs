//! Standalone binary for `qql-record` — transparent Qdrant REST recorder and live QQL converter.

use clap::Parser;
use qql_record::{RecordOptions, run};
use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "qql-record",
    about = "Transparent Qdrant REST traffic recorder and live QQL converter (dev tool)",
    version
)]
struct Cli {
    /// Address to listen on (the app points here instead of Qdrant)
    #[arg(long, default_value = "127.0.0.1:6334")]
    listen: SocketAddr,
    /// Upstream Qdrant REST base URL to forward to
    #[arg(long, default_value = "http://127.0.0.1:6333")]
    target: String,
    /// JSONL capture file (created/appended, fsynced per line)
    #[arg(long, default_value = "capture.jsonl")]
    out: PathBuf,
    /// Optional QQL capture file (converted at record time; failures
    /// become `-- ERROR <file:line> <error>` comments)
    #[arg(long)]
    qql_out: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let cli = Cli::parse();
    run(RecordOptions {
        listen: cli.listen,
        target: cli.target,
        out: cli.out,
        qql_out: cli.qql_out,
    })
    .await
}
