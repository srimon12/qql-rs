//! Atomic JSON checkpoint for crash-safe resume.

use std::error::Error;
use std::fs;
use std::path::Path;

use qql_plan::semantic::PlanPointId;
use serde::{Deserialize, Serialize};

use super::options::MigrateOptions;

/// Durable migrator phases. Resume picks up at the stored phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Create collection, indexes, and known shard keys.
    Schema,
    /// Scroll source and upsert to target.
    Ingest,
    /// Restore optimizer `indexing_threshold` after bulk load.
    Optimize,
    /// Exact count comparison.
    Verify,
    /// Point an alias at the target collection.
    Cutover,
    /// Terminal success.
    Done,
}

impl Phase {
    /// Human-readable label for progress output.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Schema => "schema",
            Self::Ingest => "ingest",
            Self::Optimize => "optimize",
            Self::Verify => "verify",
            Self::Cutover => "cutover",
            Self::Done => "done",
        }
    }
}

/// On-disk resume state. Written atomically (tmp + rename) after each window.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Checkpoint {
    /// Format version.
    pub version: u32,
    /// Source collection.
    pub source_collection: String,
    /// Target collection.
    pub target_collection: String,
    /// Source URL identity.
    pub source_url: String,
    /// Target URL identity.
    pub target_url: String,
    /// Schema-affecting options fingerprint.
    pub fingerprint: String,
    /// Current phase.
    pub phase: Phase,
    /// Next scroll offset (exclusive). `None` means start-of-collection.
    pub cursor: Option<PlanPointId>,
    /// Points upserted so far.
    pub written: usize,
    /// Points skipped so far.
    pub skipped: usize,
    /// Batches submitted so far.
    pub batches: usize,
    /// Exact source count captured at start.
    pub source_count: u64,
    /// Optimizer threshold to restore after bulk load.
    pub original_indexing_threshold: Option<u64>,
    /// Whether fast-bulk suppression was applied.
    pub fast_bulk: bool,
}

impl Checkpoint {
    /// Fresh checkpoint at the schema phase.
    pub fn new(opts: &MigrateOptions, source_count: u64) -> Self {
        Self {
            version: 1,
            source_collection: opts.source_collection.clone(),
            target_collection: opts.target_collection.clone(),
            source_url: opts.source_url.clone(),
            target_url: opts.target_url.clone(),
            fingerprint: opts.fingerprint(),
            phase: Phase::Schema,
            cursor: None,
            written: 0,
            skipped: 0,
            batches: 0,
            source_count,
            original_indexing_threshold: None,
            fast_bulk: opts.fast_bulk,
        }
    }
}

/// Load a checkpoint if the file exists.
pub fn load(path: &str) -> Result<Option<Checkpoint>, Box<dyn Error>> {
    if !Path::new(path).exists() {
        return Ok(None);
    }
    let data = fs::read_to_string(path)?;
    let cp: Checkpoint = serde_json::from_str(&data)?;
    if cp.version != 1 {
        return Err(format!(
            "unsupported checkpoint version {} at '{}'",
            cp.version, path
        )
        .into());
    }
    Ok(Some(cp))
}

/// Atomically persist a checkpoint (write `.tmp`, then rename).
pub fn save(path: &str, cp: &Checkpoint) -> Result<(), Box<dyn Error>> {
    let path = Path::new(path);
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let data = serde_json::to_vec_pretty(cp)?;
    fs::write(&tmp, data)?;
    if let Err(rename_err) = fs::rename(&tmp, path) {
        if let Err(copy_err) = fs::copy(&tmp, path) {
            return Err(format!(
                "failed to persist checkpoint at '{}': rename error: {rename_err}; copy error: {copy_err}",
                path.display()
            )
            .into());
        }
        let _ = fs::remove_file(&tmp);
    }
    Ok(())
}

/// Remove a checkpoint file. Missing files are ignored.
pub fn remove(path: &str) -> Result<(), Box<dyn Error>> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.into()),
    }
}

/// Endpoint identity for checkpoint compare. Trailing slashes and host case
/// do not change where a migration points.
pub fn normalize_endpoint(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
}

fn same_endpoint(a: &str, b: &str) -> bool {
    normalize_endpoint(a).eq_ignore_ascii_case(&normalize_endpoint(b))
}

fn path_hash(source_url: &str, source: &str, target_url: &str, target: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for part in [source_url, source, target_url, target] {
        for byte in part.bytes() {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash ^= 0xff;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{:08x}", (hash >> 32) ^ (hash & 0xffff_ffff))
}

/// Default checkpoint path includes both cluster identities to avoid collisions.
pub fn default_path(source_url: &str, source: &str, target_url: &str, target: &str) -> String {
    format!(
        ".qql-migrate/{}__{}__{}__{}__{}.json",
        sanitize(&normalize_endpoint(source_url)),
        sanitize(source),
        sanitize(&normalize_endpoint(target_url)),
        sanitize(target),
        path_hash(
            &normalize_endpoint(source_url),
            source,
            &normalize_endpoint(target_url),
            target
        )
    )
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Validate that a loaded checkpoint matches this run's identity.
pub fn compatible(cp: &Checkpoint, opts: &MigrateOptions) -> Result<(), Box<dyn Error>> {
    if cp.source_collection != opts.source_collection {
        return Err(format!(
            "checkpoint source collection '{}' does not match '{}'",
            cp.source_collection, opts.source_collection
        )
        .into());
    }
    if cp.target_collection != opts.target_collection {
        return Err(format!(
            "checkpoint target collection '{}' does not match '{}'",
            cp.target_collection, opts.target_collection
        )
        .into());
    }
    if !same_endpoint(&cp.source_url, &opts.source_url)
        || !same_endpoint(&cp.target_url, &opts.target_url)
    {
        return Err(format!(
            "checkpoint clusters do not match this run (stored '{}→{}', current '{}→{}'); pass --restart or a different --checkpoint",
            cp.source_url, cp.target_url, opts.source_url, opts.target_url
        )
        .into());
    }
    if cp.fingerprint != opts.fingerprint() {
        return Err(format!(
            "checkpoint options do not match this run (stored '{}', current '{}'); pass --restart to start over",
            cp.fingerprint,
            opts.fingerprint()
        )
        .into());
    }
    Ok(())
}
