//! Post-ingest verification: exact source vs target counts.

use std::error::Error;

use qql::executor::{Executor, OnError};

use super::options::MigrateOptions;
use crate::dump::format_ident;

/// Exact `COUNT … WITH (exact = true)`, optionally restricted by `--where`.
pub async fn exact_count(
    exec: &Executor,
    collection: &str,
    where_clause: Option<&str>,
) -> Result<u64, Box<dyn Error>> {
    let coll = format_ident(collection);
    let sql = match where_clause {
        Some(clause) if !clause.trim().is_empty() => {
            let clause = strip_where(clause);
            format!("COUNT FROM {coll} WHERE {clause} WITH (exact = true);")
        }
        _ => format!("COUNT FROM {coll} WITH (exact = true);"),
    };
    let report = exec.execute(&sql, OnError::Stop).await?;
    report
        .results
        .first()
        .and_then(|r| r.count())
        .ok_or_else(|| format!("COUNT on '{collection}' returned no count").into())
}

/// Compare filtered source count to target count. Target is unfiltered:
/// a migration writes only the selected points into a dedicated collection.
pub async fn verify_counts(
    target: &Executor,
    opts: &MigrateOptions,
    source_count: u64,
) -> Result<u64, Box<dyn Error>> {
    // `--no-wait` returns before WAL apply; poll briefly so verify is not a race.
    let attempts = if opts.wait { 1 } else { 40 };
    let mut target_count = 0;
    for i in 0..attempts {
        target_count = exact_count(target, &opts.target_collection, None).await?;
        if target_count == source_count {
            return Ok(target_count);
        }
        if i + 1 < attempts {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }
    Err(format!(
        "count mismatch after migrate: source '{}' = {source_count}, target '{}' = {target_count}",
        opts.source_collection, opts.target_collection
    )
    .into())
}

fn strip_where(clause: &str) -> &str {
    let trimmed = clause.trim();
    trimmed
        .strip_prefix("WHERE")
        .or_else(|| trimmed.strip_prefix("where"))
        .unwrap_or(trimmed)
        .trim()
}
