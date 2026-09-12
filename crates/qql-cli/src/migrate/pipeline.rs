//! Crash-safe scroll → upsert pipeline with a concurrent write window.

use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::future::Future;
use std::pin::Pin;

type UpsertFut<'a> = Pin<Box<dyn Future<Output = Result<(), Box<dyn Error>>> + 'a>>;

use qql::executor::Executor;
use qql_core::ast::{ShardKey, Value};
use qql_plan::semantic::PlanPointId;
use qql_plan::types::FilterExpression;

use super::checkpoint::{self, Checkpoint, Phase};
use super::options::{MigrateOptions, MigrateProgress, MissingShardKey};
use super::retry::{self, Circuit};
use super::schema::ensure_shard_key;
use crate::dump::{ScrollPages, format_ident, point_to_upsert_object};

#[derive(Debug)]
pub(crate) struct IngestBatch {
    pub shard_key: Option<ShardKey>,
    pub rows: Vec<Value>,
}

struct WindowPage {
    next_after: Option<PlanPointId>,
    written: usize,
    skipped: usize,
    points: Vec<serde_json::Value>,
}

/// Scroll the source and upsert into the target, checkpointing after each window.
///
/// A bounded channel lets the next source page fill while the current window
/// upserts, so both network pipes stay busy.
pub async fn stream_points(
    source: &Executor,
    target: &Executor,
    opts: &MigrateOptions,
    filter: Option<FilterExpression>,
    precreated: &[ShardKey],
    checkpoint: &mut Checkpoint,
    progress: Option<&(dyn Fn(MigrateProgress) + Sync)>,
) -> Result<(), Box<dyn Error>> {
    let after = checkpoint.cursor.clone();
    let mut prepared: HashMap<String, qql::executor::PreparedStatement> = HashMap::new();
    let mut created_keys: HashSet<String> = HashSet::new();
    for key in precreated {
        created_keys.insert(shard_cache_key(Some(key)));
    }

    let cap = opts.workers.max(1).saturating_mul(2);
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Vec<WindowPage>>(cap);

    let produce = async move {
        let mut pages = ScrollPages::new(source.ops(), &opts.source_collection, opts.batch_size)
            .filter(filter)
            .resume(after);
        loop {
            let window = fill_window(&mut pages, opts).await?;
            if window.is_empty() {
                break;
            }
            if tx.send(window).await.is_err() {
                break;
            }
        }
        Ok::<(), Box<dyn Error>>(())
    };

    let consume = async {
        let mut circuit = Circuit::default();
        while let Some(window) = rx.recv().await {
            let missing = upsert_window(
                target,
                opts,
                &window,
                &mut prepared,
                &mut created_keys,
                &mut circuit,
            )
            .await?;
            let (next_after, written, skipped, batches) = window_totals(&window);
            checkpoint.written += written.saturating_sub(missing);
            checkpoint.skipped += skipped + missing;
            checkpoint.batches += batches;
            checkpoint.cursor = next_after.clone();
            checkpoint.phase = Phase::Ingest;
            checkpoint::save(&opts.checkpoint_path, checkpoint)?;
            if let Some(cb) = progress {
                cb(MigrateProgress {
                    phase: Phase::Ingest.as_str().into(),
                    collection: opts.target_collection.clone(),
                    written: checkpoint.written,
                    skipped: checkpoint.skipped,
                    batches: checkpoint.batches,
                    source_count: checkpoint.source_count,
                });
            }
            if opts.batch_delay_ms > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(opts.batch_delay_ms)).await;
            }
        }
        Ok::<(), Box<dyn Error>>(())
    };

    // Cancel the producer if ingest fails (or finishes); `join!` would keep
    // scrolling into a full channel while CREATE SHARD KEY/upsert is stuck.
    tokio::pin!(produce);
    tokio::pin!(consume);
    tokio::select! {
        result = &mut consume => result,
        result = &mut produce => {
            result?;
            consume.await
        }
    }
}

fn window_totals(window: &[WindowPage]) -> (Option<PlanPointId>, usize, usize, usize) {
    let written = window.iter().map(|p| p.written).sum();
    let skipped = window.iter().map(|p| p.skipped).sum();
    let batches = window.len();
    let next_after = window.last().and_then(|p| p.next_after.clone());
    (next_after, written, skipped, batches)
}

async fn fill_window(
    pages: &mut ScrollPages<'_>,
    opts: &MigrateOptions,
) -> Result<Vec<WindowPage>, Box<dyn Error>> {
    let mut window = Vec::with_capacity(opts.workers.max(1));
    for _ in 0..opts.workers.max(1) {
        let Some(points) = pages.next().await? else {
            break;
        };
        let written = points.iter().filter(|p| p.get("id").is_some()).count();
        let skipped = points.len() - written;
        window.push(WindowPage {
            next_after: pages.cursor().cloned(),
            written,
            skipped,
            points,
        });
    }
    Ok(window)
}

#[allow(clippy::map_entry)] // HashMap entry cannot be held across `prepare().await`
async fn upsert_window(
    target: &Executor,
    opts: &MigrateOptions,
    window: &[WindowPage],
    prepared: &mut HashMap<String, qql::executor::PreparedStatement>,
    created_keys: &mut HashSet<String>,
    circuit: &mut Circuit,
) -> Result<usize, Box<dyn Error>> {
    let mut groups: Vec<IngestBatch> = Vec::new();
    let mut missing = 0usize;
    for page in window {
        let mut records = Vec::with_capacity(page.points.len());
        for point in &page.points {
            if let Some(rec) = point_to_upsert_object(point) {
                records.push(rec);
            }
        }
        if records.is_empty() {
            continue;
        }
        let (part, skipped) = split_by_shard(records, opts)?;
        missing += skipped;
        groups.extend(part);
    }
    for group in &groups {
        let cache = shard_cache_key(group.shard_key.as_ref());
        if let Some(ref key) = group.shard_key
            && created_keys.insert(cache.clone())
        {
            let stmt = format!(
                "CREATE SHARD KEY {key} ON COLLECTION {}",
                format_ident(&opts.target_collection)
            );
            retry::retry(circuit, || ensure_shard_key(target, &stmt)).await?;
        }
        if !prepared.contains_key(&cache) {
            let sql = upsert_template(&opts.target_collection, group.shard_key.as_ref(), opts.wait);
            let stmt = retry::retry(circuit, || async {
                target.prepare(&sql).await.map_err(|e| e.into())
            })
            .await?;
            prepared.insert(cache, stmt);
        }
    }
    upsert_groups(target, prepared, groups, opts.workers, circuit).await?;
    Ok(missing)
}

fn upsert_template(collection: &str, shard_key: Option<&ShardKey>, wait: bool) -> String {
    let mut sql = format!("UPSERT INTO {} VALUES :rows", format_ident(collection));
    if let Some(key) = shard_key {
        sql.push_str(&format!(" SHARD {key}"));
    }
    sql.push_str(&format!(" WAIT {}", wait));
    sql
}

pub(crate) fn shard_cache_key(shard_key: Option<&ShardKey>) -> String {
    match shard_key {
        Some(ShardKey::Keyword(s)) => format!("k:{s}"),
        Some(ShardKey::Number(n)) => format!("n:{n}"),
        // Placeholders never occur in migrate-built statements (keys come
        // from record fields); include them deterministically so the match
        // stays exhaustive without panicking.
        Some(ShardKey::Param(name, _)) => format!("p:{name}"),
        Some(ShardKey::PositionalParam(idx, _)) => format!("q:{idx}"),
        None => String::new(),
    }
}

pub(crate) fn split_by_shard(
    records: Vec<serde_json::Value>,
    opts: &MigrateOptions,
) -> Result<(Vec<IngestBatch>, usize), Box<dyn Error>> {
    if let Some(ref key) = opts.shard_key {
        let shard_key = Some(super::options::parse_shard_key_literal(key));
        return Ok((
            vec![IngestBatch {
                shard_key,
                rows: json_rows(records)?,
            }],
            0,
        ));
    }
    let Some(ref field) = opts.shard_key_field else {
        return Ok((
            vec![IngestBatch {
                shard_key: None,
                rows: json_rows(records)?,
            }],
            0,
        ));
    };
    let mut buckets: HashMap<String, (ShardKey, Vec<serde_json::Value>)> = HashMap::new();
    let mut skipped = 0usize;
    for rec in records {
        let key = match shard_key_from_payload(&rec, field) {
            Ok(key) => key,
            Err(err) => match &opts.missing_shard_key {
                MissingShardKey::Error => return Err(err),
                MissingShardKey::Skip => {
                    skipped += 1;
                    continue;
                }
                MissingShardKey::Default(key) => key.clone(),
            },
        };
        buckets
            .entry(key.to_string())
            .or_insert_with(|| (key, Vec::new()))
            .1
            .push(rec);
    }
    let mut out = Vec::with_capacity(buckets.len());
    for (_, (key, recs)) in buckets {
        out.push(IngestBatch {
            shard_key: Some(key),
            rows: json_rows(recs)?,
        });
    }
    Ok((out, skipped))
}

fn shard_key_from_payload(
    rec: &serde_json::Value,
    field: &str,
) -> Result<ShardKey, Box<dyn Error>> {
    let err_msg =
        || format!("payload field '{field}' must be a non-empty string or non-negative integer");
    match rec.get(field) {
        Some(serde_json::Value::String(s)) if !s.is_empty() => Ok(ShardKey::Keyword(s.clone())),
        Some(serde_json::Value::Number(n)) => {
            if let Some(u) = n.as_u64() {
                Ok(ShardKey::Number(u))
            } else if let Some(i) = n.as_i64().filter(|i| *i >= 0) {
                Ok(ShardKey::Number(i as u64))
            } else {
                Err(err_msg().into())
            }
        }
        Some(_) => Err(err_msg().into()),
        None => Err(format!("payload field '{field}' is missing on a migrated point").into()),
    }
}

fn json_rows(records: Vec<serde_json::Value>) -> Result<Vec<Value>, Box<dyn Error>> {
    records
        .into_iter()
        .map(|r| Value::from_json(r).map_err(|e| -> Box<dyn Error> { e.into() }))
        .collect()
}

async fn upsert_groups(
    target: &Executor,
    prepared: &HashMap<String, qql::executor::PreparedStatement>,
    mut groups: Vec<IngestBatch>,
    workers: usize,
    circuit: &Circuit,
) -> Result<(), Box<dyn Error>> {
    let concurrency = workers.max(1);
    while !groups.is_empty() {
        let n = groups.len().min(concurrency);
        let chunk: Vec<IngestBatch> = groups.drain(..n).collect();
        let futs: Vec<UpsertFut<'_>> = chunk
            .into_iter()
            .map(|group| {
                let fut = upsert_one(target, prepared, group, circuit);
                Box::pin(fut) as UpsertFut<'_>
            })
            .collect();
        join_all(futs).await?;
    }
    Ok(())
}

fn join_all(mut futs: Vec<UpsertFut<'_>>) -> UpsertFut<'_> {
    Box::pin(async move {
        match futs.len() {
            0 => Ok(()),
            1 => futs.remove(0).await,
            n => {
                let right = futs.split_off(n / 2);
                let (a, b) = tokio::join!(join_all(futs), join_all(right));
                a?;
                b?;
                Ok(())
            }
        }
    })
}

async fn upsert_one(
    target: &Executor,
    prepared: &HashMap<String, qql::executor::PreparedStatement>,
    group: IngestBatch,
    circuit: &Circuit,
) -> Result<(), Box<dyn Error>> {
    let key = shard_cache_key(group.shard_key.as_ref());
    let stmt = prepared
        .get(&key)
        .ok_or_else(|| format!("missing prepared upsert template for shard key '{key}'"))?;
    let mut params = HashMap::with_capacity(1);
    params.insert("rows".to_string(), Value::List(group.rows));
    retry::retry(circuit, || async {
        let resp = target
            .execute_prepared(stmt, &params)
            .await
            .map_err(|e| -> Box<dyn Error> { e.into() })?;
        if resp.ok {
            Ok(())
        } else {
            Err(resp.message.into())
        }
    })
    .await
}

#[cfg(test)]
pub(crate) fn window_totals_for_test(
    written: &[usize],
    skipped: &[usize],
) -> (usize, usize, usize) {
    let pages: Vec<WindowPage> = written
        .iter()
        .zip(skipped)
        .map(|(w, s)| WindowPage {
            next_after: None,
            written: *w,
            skipped: *s,
            points: Vec::new(),
        })
        .collect();
    let (_, w, s, b) = window_totals(&pages);
    (w, s, b)
}
