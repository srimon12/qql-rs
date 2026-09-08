//! Cluster-to-cluster collection migrator.
//!
//! Copies **schema + points** (not RocksDB snapshots), so the target can be
//! any Qdrant minor version, any shard count, and any quantization config.
//!
//! Protocol (Qdrant bulk-load guidance):
//! 1. `CREATE COLLECTION` with optional in-flight overrides
//! 2. `CREATE INDEX` before points (filterable HNSW links)
//! 3. Suppress `indexing_threshold` during ingest
//! 4. Stream scroll → `:rows` upsert with an atomic checkpoint
//! 5. Restore optimizer threshold and verify exact counts

mod checkpoint;
mod cutover;
mod options;
mod pipeline;
mod schema;
mod verify;

#[cfg(test)]
mod tests;

use std::error::Error;

use qql::executor::Executor;

use checkpoint::Phase;
use schema::build_plan;

pub use checkpoint::default_path as default_checkpoint_path;
pub use options::{
    DEFAULT_BATCH_SIZE, DEFAULT_BULK_INDEXING_THRESHOLD, DEFAULT_WORKERS, MigrateOptions,
    MigrateProgress, MigrateStats, MissingShardKey, QuantizeKind, QuantizeSpec,
};

/// Run a collection migration from `source` to `target`.
pub async fn migrate_collection(
    source: &Executor,
    target: &Executor,
    opts: MigrateOptions,
    progress: Option<&(dyn Fn(MigrateProgress) + Sync)>,
) -> Result<MigrateStats, Box<dyn Error>> {
    validate_options(&opts)?;

    let source_ops = source.ops();
    if !source_ops
        .collection_exists(&opts.source_collection)
        .await?
    {
        return Err(format!(
            "source collection '{}' does not exist",
            opts.source_collection
        )
        .into());
    }

    let source_info = source_ops
        .get_collection_info(&opts.source_collection)
        .await?;
    let plan = build_plan(&source_info, &opts);
    let source_count = verify::exact_count(
        source,
        &opts.source_collection,
        opts.where_clause.as_deref(),
    )
    .await?;

    if opts.dry_run {
        return Ok(MigrateStats {
            written: 0,
            skipped: 0,
            batches: 0,
            source_count,
            target_count: None,
            verified: false,
            resumed: false,
            dry_run: true,
            cutover_alias: None,
            source_dropped: false,
            plan,
        });
    }

    let (mut checkpoint, resumed) = load_or_init(&opts, source_count)?;
    checkpoint.source_count = source_count;

    let filter = match opts.where_clause.as_deref() {
        Some(clause) => Some(schema::parse_where_filter(&opts.source_collection, clause)?),
        None => None,
    };

    if checkpoint.phase == Phase::Schema {
        emit(
            progress,
            Phase::Schema,
            &opts.target_collection,
            &checkpoint,
        );
        schema::prepare_target(target, &source_info, &opts, &mut checkpoint).await?;
        checkpoint.phase = Phase::Ingest;
        checkpoint::save(&opts.checkpoint_path, &checkpoint)?;
    }

    let restore_needed = opts.fast_bulk;
    let ingest = async {
        if checkpoint.phase == Phase::Ingest {
            tokio::select! {
                result = pipeline::stream_points(
                    source,
                    target,
                    &opts,
                    filter,
                    &mut checkpoint,
                    progress,
                ) => result,
                _ = tokio::signal::ctrl_c() => {
                    Err("migration interrupted (Ctrl+C); optimizer threshold will be restored; resume with the same command".into())
                }
            }?;
            checkpoint.phase = if restore_needed {
                Phase::Optimize
            } else {
                Phase::Verify
            };
            checkpoint::save(&opts.checkpoint_path, &checkpoint)?;
        }
        Ok::<(), Box<dyn Error>>(())
    }
    .await;

    if restore_needed && matches!(checkpoint.phase, Phase::Ingest | Phase::Optimize) {
        emit(
            progress,
            Phase::Optimize,
            &opts.target_collection,
            &checkpoint,
        );
        let restored = schema::restore_optimizers(
            target,
            &opts.target_collection,
            checkpoint.original_indexing_threshold,
        )
        .await;
        if let Err(err) = restored {
            if ingest.is_ok() {
                return Err(err);
            }
        } else {
            checkpoint.phase = Phase::Verify;
            checkpoint::save(&opts.checkpoint_path, &checkpoint)?;
        }
    }
    ingest?;

    let mut target_count = None;
    let mut verified = false;
    if checkpoint.phase == Phase::Verify {
        emit(
            progress,
            Phase::Verify,
            &opts.target_collection,
            &checkpoint,
        );
        if opts.verify {
            target_count = Some(verify::verify_counts(target, &opts, source_count).await?);
            verified = true;
        } else {
            target_count = verify::exact_count(target, &opts.target_collection, None)
                .await
                .ok();
        }
        checkpoint.phase = if opts.cutover_alias.is_some() {
            Phase::Cutover
        } else {
            Phase::Done
        };
        checkpoint::save(&opts.checkpoint_path, &checkpoint)?;
    }

    let mut cutover_alias = None;
    let mut source_dropped = false;
    if checkpoint.phase == Phase::Cutover {
        if let Some(alias) = opts.cutover_alias.as_deref() {
            emit(
                progress,
                Phase::Cutover,
                &opts.target_collection,
                &checkpoint,
            );
            cutover::cutover_alias(target, &opts, alias).await?;
            cutover_alias = Some(alias.to_string());
            if opts.drop_source_after_cutover {
                cutover::drop_source(source, &opts.source_collection).await?;
                source_dropped = true;
            }
        }
        checkpoint.phase = Phase::Done;
        checkpoint::save(&opts.checkpoint_path, &checkpoint)?;
    }

    checkpoint::remove(&opts.checkpoint_path)?;

    Ok(MigrateStats {
        written: checkpoint.written,
        skipped: checkpoint.skipped,
        batches: checkpoint.batches,
        source_count,
        target_count,
        verified,
        resumed,
        dry_run: false,
        cutover_alias,
        source_dropped,
        plan,
    })
}

fn validate_options(opts: &MigrateOptions) -> Result<(), Box<dyn Error>> {
    if opts.batch_size == 0 {
        return Err("batch_size must be >= 1".into());
    }
    if opts.workers == 0 {
        return Err("workers must be >= 1".into());
    }
    if opts.shard_key.is_some() && opts.shard_key_field.is_some() {
        return Err("--shard-key and --shard-key-field cannot be used together".into());
    }
    if same_backend(opts) && opts.source_collection == opts.target_collection {
        return Err(
            "source and target collections must differ when migrating on the same cluster".into(),
        );
    }
    if opts.resume && opts.restart {
        return Err("--resume and --restart cannot be used together".into());
    }
    if opts.bulk_indexing_threshold == 0 {
        return Err("bulk-threshold-kb must be >= 1".into());
    }
    if opts.drop_source_after_cutover && opts.cutover_alias.is_none() {
        return Err("--drop-source-after-cutover requires --cutover".into());
    }
    Ok(())
}

fn same_backend(opts: &MigrateOptions) -> bool {
    opts.source_url == opts.target_url
}

fn load_or_init(
    opts: &MigrateOptions,
    source_count: u64,
) -> Result<(checkpoint::Checkpoint, bool), Box<dyn Error>> {
    let existing = checkpoint::load(&opts.checkpoint_path)?;
    if opts.restart {
        checkpoint::remove(&opts.checkpoint_path)?;
        return Ok((checkpoint::Checkpoint::new(opts, source_count), false));
    }
    if let Some(cp) = existing {
        checkpoint::compatible(&cp, opts)?;
        return Ok((cp, true));
    }
    if opts.resume {
        return Err(format!(
            "no checkpoint found at '{}'; nothing to resume",
            opts.checkpoint_path
        )
        .into());
    }
    Ok((checkpoint::Checkpoint::new(opts, source_count), false))
}

fn emit(
    progress: Option<&(dyn Fn(MigrateProgress) + Sync)>,
    phase: Phase,
    collection: &str,
    checkpoint: &checkpoint::Checkpoint,
) {
    if let Some(cb) = progress {
        cb(MigrateProgress {
            phase: phase.as_str().into(),
            collection: collection.to_string(),
            written: checkpoint.written,
            skipped: checkpoint.skipped,
            batches: checkpoint.batches,
            source_count: checkpoint.source_count,
        });
    }
}
