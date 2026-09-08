//! Migration options, quantization overrides, and progress/stats types.

/// Default upsert batch size (Qdrant bulk-upload guidance: 64–256).
pub const DEFAULT_BATCH_SIZE: u32 = 128;
/// Default concurrent upsert window. Official guidance is 2–4 upload streams.
pub const DEFAULT_WORKERS: usize = 2;
/// Default optimizer `indexing_threshold` (KB) during bulk load (~2 GB).
///
/// Official guidance is "very high" so HNSW is not built concurrently with
/// ingest. 10 GB OOMs small nodes; 2 GB is high enough to suppress indexing
/// on typical migrations without pinning unbounded RAM. Override with
/// `--bulk-threshold-kb`.
pub const DEFAULT_BULK_INDEXING_THRESHOLD: u64 = 2_000_000;
/// Qdrant default `indexing_threshold` (20 MB) restored after bulk load.
pub const DEFAULT_INDEXING_THRESHOLD: u64 = 20_000;

/// What to do when `--shard-key-field` is missing on a point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MissingShardKey {
    /// Abort the migration (default).
    Error,
    /// Skip the point and count it as skipped.
    Skip,
    /// Route the point to this key instead.
    Default(qql_core::ast::ShardKey),
}

impl MissingShardKey {
    /// Parse `error`, `skip`, or `default=<key>` (`101` is numeric).
    pub fn parse(raw: &str) -> Result<Self, String> {
        let raw = raw.trim();
        if raw.eq_ignore_ascii_case("error") {
            return Ok(Self::Error);
        }
        if raw.eq_ignore_ascii_case("skip") {
            return Ok(Self::Skip);
        }
        if let Some(value) = raw
            .strip_prefix("default=")
            .or_else(|| raw.strip_prefix("DEFAULT="))
        {
            return Ok(Self::Default(parse_shard_key_literal(value)));
        }
        Err("expected error, skip, or default=<key> (e.g. default=unknown or default=0)".into())
    }
}

/// Parse a CLI shard-key literal: all-digit → Number, otherwise Keyword.
pub fn parse_shard_key_literal(raw: &str) -> qql_core::ast::ShardKey {
    let raw = raw.trim();
    if !raw.is_empty()
        && raw.bytes().all(|b| b.is_ascii_digit())
        && let Ok(n) = raw.parse::<u64>()
    {
        return qql_core::ast::ShardKey::Number(n);
    }
    qql_core::ast::ShardKey::Keyword(raw.to_string())
}

/// In-flight quantization applied to `CREATE COLLECTION` on the target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuantizeKind {
    /// Scalar int8 quantization.
    Scalar,
    /// Binary quantization.
    Binary,
    /// Product quantization.
    Product,
    /// Turbo quantization.
    Turbo,
}

impl QuantizeKind {
    /// Canonical name used in QQL `type = '…'` and fingerprints.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Scalar => "scalar",
            Self::Binary => "binary",
            Self::Product => "product",
            Self::Turbo => "turbo",
        }
    }
}

/// Quantization parameters applied during schema creation.
#[derive(Debug, Clone, PartialEq)]
pub struct QuantizeSpec {
    /// Quantization family.
    pub kind: QuantizeKind,
    /// Keep quantized vectors in RAM.
    pub always_ram: bool,
    /// Scalar quantile (default 0.99).
    pub quantile: f64,
    /// Product compression ratio (`x4` / `x8` / `x16` / `x32`).
    pub compression: String,
    /// Binary encoding (`one_bit` / `two_bits` / `one_and_half_bits`).
    pub encoding: String,
    /// Turbo bit width (`1` / `1.5` / `2` / `4`).
    pub bits: String,
}

impl QuantizeSpec {
    /// Defaults matching Qdrant's recommended starting points per family.
    pub fn new(kind: QuantizeKind) -> Self {
        Self {
            kind,
            always_ram: true,
            quantile: 0.99,
            compression: "x16".into(),
            encoding: "one_bit".into(),
            bits: "2".into(),
        }
    }
}

/// User-facing migration configuration.
#[derive(Debug, Clone)]
pub struct MigrateOptions {
    /// Source collection name.
    pub source_collection: String,
    /// Target collection name.
    pub target_collection: String,
    /// Source URL (checkpoint identity only).
    pub source_url: String,
    /// Target URL (checkpoint identity only).
    pub target_url: String,
    /// Scroll / upsert batch size.
    pub batch_size: u32,
    /// Concurrent upsert window size.
    pub workers: usize,
    /// Override target `shard_number`.
    pub shard_number: Option<u64>,
    /// Override target `replication_factor`.
    pub replication_factor: Option<u64>,
    /// Override target `sharding_method` (`auto` / `custom`).
    pub sharding_method: Option<String>,
    /// In-flight quantization. `None` copies the source config.
    pub quantize: Option<QuantizeSpec>,
    /// Fixed custom-shard key applied to every upsert.
    pub shard_key: Option<String>,
    /// Payload field used as the per-point custom shard key.
    pub shard_key_field: Option<String>,
    /// Policy when `--shard-key-field` is absent on a point.
    pub missing_shard_key: MissingShardKey,
    /// Optimizer indexing threshold (KB) used during bulk load.
    pub bulk_indexing_threshold: u64,
    /// After verify, point this alias at the target collection.
    pub cutover_alias: Option<String>,
    /// After a successful cutover, drop the source collection.
    pub drop_source_after_cutover: bool,
    /// Optional QQL `WHERE` clause restricting the source scroll.
    pub where_clause: Option<String>,
    /// Checkpoint file path.
    pub checkpoint_path: String,
    /// Resume from an existing checkpoint.
    pub resume: bool,
    /// Ignore any existing checkpoint and start over.
    pub restart: bool,
    /// Print the plan and exit without writing.
    pub dry_run: bool,
    /// Temporarily raise `indexing_threshold` during ingest.
    pub fast_bulk: bool,
    /// Compare exact source vs target counts after ingest.
    pub verify: bool,
    /// `WAIT true` on every upsert (durable default).
    pub wait: bool,
    /// Drop the target collection before creating it.
    pub recreate: bool,
}

impl MigrateOptions {
    /// Identity of schema-affecting options. Resume refuses a mismatched checkpoint.
    pub fn fingerprint(&self) -> String {
        let quant = self
            .quantize
            .as_ref()
            .map(|q| q.kind.as_str())
            .unwrap_or("-");
        format!(
            "to={},shards={:?},repl={:?},method={:?},quant={},shard_key={:?},shard_field={:?},missing={:?},bulk={},cutover={:?},where={:?},fast_bulk={}",
            self.target_collection,
            self.shard_number,
            self.replication_factor,
            self.sharding_method,
            quant,
            self.shard_key,
            self.shard_key_field,
            self.missing_shard_key,
            self.bulk_indexing_threshold,
            self.cutover_alias,
            self.where_clause,
            self.fast_bulk,
        )
    }

    /// True when the target uses custom sharding (explicit or implied by shard keys).
    pub fn uses_custom_sharding(&self) -> bool {
        self.shard_key.is_some()
            || self.shard_key_field.is_some()
            || self
                .sharding_method
                .as_deref()
                .is_some_and(|m| m.eq_ignore_ascii_case("custom"))
    }
}

/// Result summary of a migrate run.
#[derive(Debug, Clone, PartialEq)]
pub struct MigrateStats {
    /// Points successfully upserted.
    pub written: usize,
    /// Scroll hits dropped for missing `id`.
    pub skipped: usize,
    /// Number of upsert batches submitted.
    pub batches: usize,
    /// Exact source count (with `--where`, if any).
    pub source_count: u64,
    /// Exact target count after verify, when run.
    pub target_count: Option<u64>,
    /// Whether count verification ran and passed.
    pub verified: bool,
    /// Whether this run continued from a checkpoint.
    pub resumed: bool,
    /// Whether this was a dry-run (no writes).
    pub dry_run: bool,
    /// Alias that now points at the target, when `--cutover` ran.
    pub cutover_alias: Option<String>,
    /// Whether the source collection was dropped after cutover.
    pub source_dropped: bool,
    /// Schema plan (always filled; used by `--dry-run` and JSON output).
    pub plan: MigratePlan,
}

/// Declarative target schema the migrator will (or did) apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigratePlan {
    /// `CREATE COLLECTION` statement.
    pub create: String,
    /// `CREATE INDEX` statements, payload indexes first.
    pub indexes: Vec<String>,
    /// `CREATE SHARD KEY` statements known up front (fixed `--shard-key`).
    pub shard_keys: Vec<String>,
    /// `ALTER COLLECTION` restoring optimizer indexing after bulk load.
    pub restore_optimizers: Option<String>,
}

/// Progress emitted after each ingest window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrateProgress {
    /// Current phase label (`schema`, `ingest`, `optimize`, `verify`).
    pub phase: String,
    /// Target collection.
    pub collection: String,
    /// Points upserted so far.
    pub written: usize,
    /// Points skipped so far.
    pub skipped: usize,
    /// Batches submitted so far.
    pub batches: usize,
    /// Exact source count when known.
    pub source_count: u64,
}
