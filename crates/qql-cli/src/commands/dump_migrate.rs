//! `qql dump` and `qql migrate` handlers.

use super::runtime::{executor, executor_for};
use crate::dump;
use crate::migrate;

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
