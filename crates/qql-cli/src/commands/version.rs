//! `qql version`.

use crate::output;

const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn handle_version() -> Result<(), Box<dyn std::error::Error>> {
    let features = [
        ("grpc", cfg!(feature = "grpc")),
        ("rest", cfg!(feature = "rest")),
        ("edge", cfg!(feature = "edge")),
        ("fastembed", cfg!(feature = "fastembed")),
    ]
    .into_iter()
    .filter(|(_, on)| *on)
    .map(|(name, _)| name.to_string())
    .collect::<Vec<_>>();
    let resp = output::VersionResponse {
        ok: true,
        command: "version".to_string(),
        version: VERSION.to_string(),
        features,
        message: format!("qql version {}", VERSION),
    };
    let s = serde_json::to_string_pretty(&resp)?;
    println!("{}", s);
    Ok(())
}
