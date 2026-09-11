//! Target schema construction: overrides, bulk-load handshake, and DDL apply.

use std::collections::HashSet;
use std::error::Error;
use std::time::Duration;

use qql::backend::CollectionInfo;
use qql::executor::{Executor, OnError};
use qql_core::ast::{ShardKey, Stmt};
use qql_core::parser::Parser;
use qql_plan::filter::top_level_filter;
use qql_plan::types::FilterExpression;
use qql_plan::{
    BinaryQuantization, ProductQuantization, QuantizationConfig, ScalarQuantization,
    TurboQuantization,
};
use serde_json::Value;

use super::checkpoint::Checkpoint;
use super::discover::create_shard_key_sql;
use super::options::{
    DEFAULT_INDEXING_THRESHOLD, MigrateOptions, MigratePlan, QuantizeKind, QuantizeSpec,
};
use crate::dump::{format_ident, generate_create_statement, generate_index_statements};

/// Standalone Qdrant rejects `CreateShardKey` but the RPC can stall the ingest
/// pipeline; fail closed well before that.
const SHARD_KEY_RPC_TIMEOUT: Duration = Duration::from_secs(15);
const PROBE_SHARD_KEY: &str = "__qql_migrate_probe__";

/// Apply reshard / quantize / custom-sharding overrides onto a cloned schema.
pub fn apply_overrides(info: &mut CollectionInfo, opts: &MigrateOptions) {
    if let Some(n) = opts.shard_number {
        info.schema.params.shard_number = Some(n);
    }
    if let Some(r) = opts.replication_factor {
        info.schema.params.replication_factor = Some(r);
    }
    if let Some(ref method) = opts.sharding_method {
        info.schema.params.sharding_method = Some(method.clone());
    } else if opts.uses_custom_sharding() {
        info.schema.params.sharding_method = Some("custom".into());
    }
    if let Some(ref spec) = opts.quantize {
        let q = quantize_config(spec);
        let q_value = serde_json::to_value(&q).unwrap_or(Value::Null);
        info.schema.quantization = Some(q);
        for vector in &mut info.schema.vectors {
            vector.quantization = Some(q_value.clone());
        }
    }
}

/// Raise `indexing_threshold` so HNSW is not built concurrently with ingest.
/// Returns the original threshold (if any) for later restore.
pub fn suppress_indexing(info: &mut CollectionInfo, threshold_kb: u64) -> Option<u64> {
    let optimizers = info.schema.optimizers.get_or_insert_with(Default::default);
    let original = optimizers.indexing_threshold;
    optimizers.indexing_threshold = Some(threshold_kb);
    original
}

/// Typed quantization config for a migrate `--quantize` override.
pub fn quantize_config(spec: &QuantizeSpec) -> QuantizationConfig {
    let always_ram = Some(spec.always_ram);
    match spec.kind {
        QuantizeKind::Scalar => QuantizationConfig::Scalar {
            scalar: ScalarQuantization {
                qtype: "int8".into(),
                quantile: Some(spec.quantile),
                always_ram,
                memory: None,
            },
        },
        QuantizeKind::Binary => QuantizationConfig::Binary {
            binary: BinaryQuantization {
                always_ram,
                encoding: Some(spec.encoding.clone()),
                query_encoding: None,
                memory: None,
            },
        },
        QuantizeKind::Product => QuantizationConfig::Product {
            product: ProductQuantization {
                compression: spec.compression.clone(),
                always_ram,
                memory: None,
            },
        },
        QuantizeKind::Turbo => QuantizationConfig::Turbo {
            turbo: TurboQuantization {
                bits: Some(turbo_bits_label(&spec.bits)),
                always_ram,
                memory: None,
            },
        },
    }
}

/// Map the CLI's numeric turbo bits (`1` / `1.5` / `2` / `4`) onto the OpenAPI
/// `bitsN` label the plan serializes.
fn turbo_bits_label(bits: &str) -> String {
    match bits {
        "1" => "bits1".into(),
        "1.5" => "bits1_5".into(),
        "2" => "bits2".into(),
        "4" => "bits4".into(),
        other => format!("bits{other}"),
    }
}

/// `ALTER COLLECTION … WITH OPTIMIZERS (indexing_threshold = N)`.
pub fn restore_optimizers_statement(collection: &str, original: Option<u64>) -> String {
    format!(
        "ALTER COLLECTION {} WITH OPTIMIZERS (indexing_threshold = {})",
        format_ident(collection),
        original.unwrap_or(DEFAULT_INDEXING_THRESHOLD)
    )
}

/// Build the declarative target plan from source info + overrides.
pub fn build_plan(info: &CollectionInfo, opts: &MigrateOptions) -> MigratePlan {
    let mut info = info.clone();
    apply_overrides(&mut info, opts);
    let original_threshold = info
        .schema
        .optimizers
        .as_ref()
        .and_then(|optimizers| optimizers.indexing_threshold);
    if opts.fast_bulk {
        suppress_indexing(&mut info, opts.bulk_indexing_threshold);
    }
    let create = generate_create_statement(&opts.target_collection, &info);
    let mut indexes =
        generate_index_statements(&opts.target_collection, &info.schema.payload_indexes);
    if let Some(field) = opts.shard_key_field.as_deref() {
        promote_tenant_index(&mut indexes, &opts.target_collection, field);
    }
    let shard_keys = match opts.shard_key.as_deref() {
        Some(key) => vec![format!(
            "CREATE SHARD KEY {} ON COLLECTION {}",
            super::options::parse_shard_key_literal(key),
            format_ident(&opts.target_collection)
        )],
        None => Vec::new(),
    };
    let restore_optimizers = if opts.fast_bulk {
        Some(restore_optimizers_statement(
            &opts.target_collection,
            original_threshold,
        ))
    } else {
        None
    };
    MigratePlan {
        create,
        indexes,
        shard_keys,
        restore_optimizers,
    }
}

/// Parse a user `--where` fragment into a plan-layer filter.
pub fn parse_where_filter(
    collection: &str,
    where_clause: &str,
) -> Result<FilterExpression, Box<dyn Error>> {
    let clause = where_clause.trim();
    let clause = clause
        .strip_prefix("WHERE")
        .or_else(|| clause.strip_prefix("where"))
        .unwrap_or(clause)
        .trim();
    if clause.is_empty() {
        return Err("WHERE clause is empty".into());
    }
    let sql = format!(
        "SCROLL FROM {} WHERE {} LIMIT 1;",
        format_ident(collection),
        clause
    );
    let stmt = Parser::parse(&sql)?;
    match stmt {
        Stmt::Scroll(scroll) => {
            let filter = scroll
                .filter
                .ok_or("WHERE clause produced no filter expression")?;
            Ok(top_level_filter(&filter))
        }
        _ => Err("internal error: expected SCROLL while parsing WHERE".into()),
    }
}

/// Create collection / indexes / shard keys on the target (idempotent).
pub async fn prepare_target(
    target: &Executor,
    source_info: &CollectionInfo,
    opts: &MigrateOptions,
    discovered_keys: &[ShardKey],
    checkpoint: &mut Checkpoint,
) -> Result<MigratePlan, Box<dyn Error>> {
    let mut info = source_info.clone();
    apply_overrides(&mut info, opts);
    let original_threshold = if opts.fast_bulk {
        suppress_indexing(&mut info, opts.bulk_indexing_threshold)
    } else {
        info.schema
            .optimizers
            .as_ref()
            .and_then(|optimizers| optimizers.indexing_threshold)
    };
    checkpoint.original_indexing_threshold = original_threshold;

    let plan = build_plan(source_info, opts);
    let ops = target.ops();
    let exists = ops.collection_exists(&opts.target_collection).await?;

    if opts.recreate && exists {
        run_sql(
            target,
            &format!("DROP COLLECTION {};", format_ident(&opts.target_collection)),
        )
        .await?;
        run_sql(target, &format!("{};", plan.create)).await?;
    } else if !exists {
        run_sql(target, &format!("{};", plan.create)).await?;
    }

    let target_info = ops.get_collection_info(&opts.target_collection).await?;
    if opts.uses_custom_sharding() {
        let method = target_info
            .schema
            .params
            .sharding_method
            .as_deref()
            .unwrap_or("auto");
        if !method.eq_ignore_ascii_case("custom") {
            return Err(format!(
                "target collection '{}' must use sharding_method = 'custom' when a shard key is configured (got '{method}')",
                opts.target_collection
            )
            .into());
        }
        // Empty discovery still probes so standalone fails in schema, not ingest.
        if discovered_keys.is_empty() {
            probe_custom_sharding(target, &opts.target_collection).await?;
        }
    }

    let existing_indexes: HashSet<String> = target_info
        .schema
        .payload_indexes
        .iter()
        .map(|idx| idx.field.clone())
        .collect();
    for spec in &source_info.schema.payload_indexes {
        if existing_indexes.contains(&spec.field) {
            continue;
        }
        let stmts = generate_index_statements(&opts.target_collection, std::slice::from_ref(spec));
        for stmt in stmts {
            run_sql(target, &format!("{};", stmt)).await?;
        }
    }
    if let Some(field) = opts.shard_key_field.as_deref()
        && !existing_indexes.contains(field)
        && !source_info
            .schema
            .payload_indexes
            .iter()
            .any(|idx| idx.field == field)
    {
        let stmt = tenant_index_statement(&opts.target_collection, field);
        run_sql(target, &format!("{};", stmt)).await?;
    }

    let mut plan = plan;
    if !discovered_keys.is_empty() {
        plan.shard_keys = discovered_keys
            .iter()
            .map(|k| create_shard_key_sql(&opts.target_collection, k))
            .collect();
    }
    for stmt in &plan.shard_keys {
        ensure_shard_key(target, stmt).await?;
    }

    Ok(plan)
}

/// Create a custom shard key, treating "already exists" as success.
pub async fn ensure_shard_key(target: &Executor, stmt: &str) -> Result<(), Box<dyn Error>> {
    let sql = if stmt.trim_end().ends_with(';') {
        stmt.to_string()
    } else {
        format!("{};", stmt)
    };
    let result = match tokio::time::timeout(
        SHARD_KEY_RPC_TIMEOUT,
        target.execute(&sql, OnError::Stop),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => {
            return Err(format!(
                "CREATE SHARD KEY timed out after {}s; {}",
                SHARD_KEY_RPC_TIMEOUT.as_secs(),
                CUSTOM_SHARD_KEY_HINT
            )
            .into());
        }
    };
    match result {
        Ok(report) if report.ok => Ok(()),
        Ok(report) => {
            let msg = report
                .results
                .iter()
                .find(|r| !r.ok)
                .map(|r| r.message.as_str())
                .unwrap_or("CREATE SHARD KEY failed");
            if already_exists(msg) {
                Ok(())
            } else {
                Err(map_shard_key_error(msg).into())
            }
        }
        Err(err) if already_exists(&err.to_string()) => Ok(()),
        Err(err) => Err(map_shard_key_error(&err.to_string()).into()),
    }
}

const CUSTOM_SHARD_KEY_HINT: &str = "custom shard keys require Qdrant distributed mode; \
     this target is standalone. Remove --shard-key / --shard-key-field, \
     or point --target-url at a clustered Qdrant";

/// True when the backend cannot create custom shard keys (standalone Qdrant).
pub(crate) fn shard_key_backend_unsupported(msg: &str) -> bool {
    let lower = msg.to_ascii_lowercase();
    lower.contains("standalone")
        || lower.contains("not implemented")
        || lower.contains("unsupported-shard-key")
        || lower.contains("unsupported_shard_key")
}

pub(crate) fn map_shard_key_error(msg: &str) -> String {
    if shard_key_backend_unsupported(msg) {
        CUSTOM_SHARD_KEY_HINT.to_string()
    } else {
        msg.to_string()
    }
}

async fn probe_custom_sharding(target: &Executor, collection: &str) -> Result<(), Box<dyn Error>> {
    let coll = format_ident(collection);
    let create = format!("CREATE SHARD KEY '{PROBE_SHARD_KEY}' ON COLLECTION {coll}");
    ensure_shard_key(target, &create).await?;
    let drop = format!("DROP SHARD KEY '{PROBE_SHARD_KEY}' ON COLLECTION {coll};");
    let _ = run_sql(target, &drop).await;
    Ok(())
}

/// Restore optimizer indexing after ingest.
pub async fn restore_optimizers(
    target: &Executor,
    collection: &str,
    original: Option<u64>,
) -> Result<(), Box<dyn Error>> {
    let sql = format!("{};", restore_optimizers_statement(collection, original));
    run_sql(target, &sql).await
}

pub(crate) async fn run_sql(exec: &Executor, sql: &str) -> Result<(), Box<dyn Error>> {
    let report = exec.execute(sql, OnError::Stop).await?;
    if report.ok {
        return Ok(());
    }
    let msg = report
        .results
        .iter()
        .find(|r| !r.ok)
        .map(|r| r.message.clone())
        .unwrap_or_else(|| format!("statement failed: {sql}"));
    Err(msg.into())
}

fn already_exists(msg: &str) -> bool {
    let lower = msg.to_ascii_lowercase();
    lower.contains("already exists") || lower.contains("already exist")
}

fn tenant_index_statement(collection: &str, field: &str) -> String {
    format!(
        "CREATE INDEX ON COLLECTION {} FOR {} TYPE keyword WITH (is_tenant = true)",
        format_ident(collection),
        format_ident(field)
    )
}

fn promote_tenant_index(indexes: &mut Vec<String>, collection: &str, field: &str) {
    let marker = format!("FOR {} TYPE", format_ident(field));
    if let Some(existing) = indexes.iter_mut().find(|s| s.contains(&marker)) {
        if !existing.contains("is_tenant") {
            if existing.contains(" WITH (") {
                *existing = existing.replacen(" WITH (", " WITH (is_tenant = true, ", 1);
            } else {
                existing.push_str(" WITH (is_tenant = true)");
            }
        }
    } else {
        indexes.push(tenant_index_statement(collection, field));
    }
}
