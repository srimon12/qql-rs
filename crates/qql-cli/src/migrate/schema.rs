//! Target schema construction: overrides, bulk-load handshake, and DDL apply.

use std::collections::HashSet;
use std::error::Error;

use qql::backend::CollectionInfo;
use qql::executor::{Executor, OnError};
use qql_core::ast::Stmt;
use qql_core::parser::Parser;
use qql_plan::filter::top_level_filter;
use qql_plan::types::FilterExpression;
use serde_json::{Value, json};

use super::checkpoint::Checkpoint;
use super::options::{
    DEFAULT_INDEXING_THRESHOLD, MigrateOptions, MigratePlan, QuantizeKind, QuantizeSpec,
};
use crate::dump::{format_ident, generate_create_statement, generate_index_statements};

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
        let q = quantize_json(spec);
        info.schema.quantization = Some(q.clone());
        for vector in &mut info.schema.vectors {
            vector.quantization = Some(q.clone());
        }
    }
}

/// Raise `indexing_threshold` so HNSW is not built concurrently with ingest.
/// Returns the original threshold (if any) for later restore.
pub fn suppress_indexing(info: &mut CollectionInfo, threshold_kb: u64) -> Option<u64> {
    let original = info
        .schema
        .optimizers
        .as_ref()
        .and_then(|m| m.get("indexing_threshold"))
        .and_then(Value::as_u64);
    let map = info.schema.optimizers.get_or_insert_with(Default::default);
    map.insert("indexing_threshold".into(), json!(threshold_kb));
    original
}

/// Nested REST quantization object (`{ "scalar": { … } }`).
pub fn quantize_json(spec: &QuantizeSpec) -> Value {
    let always = json!(spec.always_ram);
    match spec.kind {
        QuantizeKind::Scalar => json!({
            "scalar": {
                "type": "int8",
                "quantile": spec.quantile,
                "always_ram": always,
            }
        }),
        QuantizeKind::Binary => json!({
            "binary": {
                "always_ram": always,
                "encoding": spec.encoding,
            }
        }),
        QuantizeKind::Product => json!({
            "product": {
                "compression": spec.compression,
                "always_ram": always,
            }
        }),
        QuantizeKind::Turbo => json!({
            "turbo": {
                "bits": spec.bits,
                "always_ram": always,
            }
        }),
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
        .and_then(|m| m.get("indexing_threshold"))
        .and_then(Value::as_u64);
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
            .and_then(|m| m.get("indexing_threshold"))
            .and_then(Value::as_u64)
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
    match target.execute(&sql, OnError::Stop).await {
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
                Err(msg.into())
            }
        }
        Err(err) if already_exists(&err.to_string()) => Ok(()),
        Err(err) => Err(err.into()),
    }
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
