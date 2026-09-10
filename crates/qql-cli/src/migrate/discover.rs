//! Discover custom shard keys from the source before ingest.
//!
//! Topology (`CREATE SHARD KEY`) is applied in the schema phase. Ingest still
//! creates a key if a value was not discovered (truncated facet, or a point
//! written after FACET).

use std::collections::HashMap;
use std::error::Error;

use qql::executor::{Executor, OnError};
use qql_core::ast::ShardKey;
use qql_plan::types::{FilterExpression, PayloadSelectorReq, VectorSelectorReq};
use serde_json::Value;

use super::options::{MigrateOptions, MissingShardKey};
use crate::dump::{ScrollPages, format_ident};

/// FACET `LIMIT`. If this many hits come back, discovery falls back to a
/// payload-only scroll so the key set is complete.
pub(crate) const FACET_LIMIT: u64 = 10_000;
const SCROLL_BATCH: u32 = 512;

/// `CREATE SHARD KEY <key> ON COLLECTION <c>`.
pub fn create_shard_key_sql(collection: &str, key: &ShardKey) -> String {
    format!(
        "CREATE SHARD KEY {key} ON COLLECTION {}",
        format_ident(collection)
    )
}

/// Unique shard keys the target must have before ingest.
///
/// `--shard-key` is a single literal. `--shard-key-field` is FACET, then a
/// payload-only scroll if FACET is truncated or has no index.
pub async fn discover_shard_keys(
    source: &Executor,
    opts: &MigrateOptions,
    filter: Option<FilterExpression>,
) -> Result<Vec<ShardKey>, Box<dyn Error>> {
    let mut keys: HashMap<String, ShardKey> = HashMap::new();
    if let Some(ref raw) = opts.shard_key {
        insert_key(&mut keys, super::options::parse_shard_key_literal(raw));
        return Ok(sorted(keys));
    }
    let Some(ref field) = opts.shard_key_field else {
        return Ok(Vec::new());
    };

    match facet_keys(
        source,
        &opts.source_collection,
        field,
        opts.where_clause.as_deref(),
    )
    .await
    {
        Ok((found, truncated)) => {
            for key in found {
                insert_key(&mut keys, key);
            }
            if truncated || keys.is_empty() {
                for key in scroll_field_keys(source, &opts.source_collection, field, filter.clone())
                    .await?
                {
                    insert_key(&mut keys, key);
                }
            }
        }
        Err(err) if facet_missing_index(&err.to_string()) => {
            for key in scroll_field_keys(source, &opts.source_collection, field, filter).await? {
                insert_key(&mut keys, key);
            }
        }
        Err(err) => return Err(err),
    }

    if let MissingShardKey::Default(ref key) = opts.missing_shard_key {
        insert_key(&mut keys, key.clone());
    }
    if keys.is_empty() && !matches!(opts.missing_shard_key, MissingShardKey::Skip) {
        return Err(format!(
            "no shard-key values found for payload field '{field}' on '{}'",
            opts.source_collection
        )
        .into());
    }
    Ok(sorted(keys))
}

async fn facet_keys(
    source: &Executor,
    collection: &str,
    field: &str,
    where_clause: Option<&str>,
) -> Result<(Vec<ShardKey>, bool), Box<dyn Error>> {
    let coll = format_ident(collection);
    let ident = format_ident(field);
    let sql = match where_clause {
        Some(clause) if !clause.trim().is_empty() => {
            let clause = strip_where(clause);
            format!("FACET {ident} FROM {coll} WHERE {clause} LIMIT {FACET_LIMIT} EXACT true;")
        }
        _ => format!("FACET {ident} FROM {coll} LIMIT {FACET_LIMIT} EXACT true;"),
    };
    let report = source.execute(&sql, OnError::Stop).await?;
    let resp = report.results.first().ok_or("FACET returned no result")?;
    if !resp.ok {
        return Err(resp.message.clone().into());
    }
    let hits = resp.facet().unwrap_or_default();
    let truncated = hits.len() as u64 >= FACET_LIMIT;
    let mut keys = Vec::new();
    for (value, _) in hits {
        if let Some(key) = facet_value_to_shard_key(&value) {
            keys.push(key);
        }
    }
    Ok((keys, truncated))
}

async fn scroll_field_keys(
    source: &Executor,
    collection: &str,
    field: &str,
    filter: Option<FilterExpression>,
) -> Result<Vec<ShardKey>, Box<dyn Error>> {
    let mut pages = ScrollPages::new(source.ops(), collection, SCROLL_BATCH)
        .filter(filter)
        .select(
            PayloadSelectorReq::Include {
                include: vec![field.to_string()],
            },
            VectorSelectorReq::All(false),
        );
    let mut keys: HashMap<String, ShardKey> = HashMap::new();
    while let Some(points) = pages.next().await? {
        for rec in &points {
            if let Some(key) = rec
                .get("payload")
                .and_then(|p| p.get(field))
                .and_then(json_to_shard_key)
                .or_else(|| rec.get(field).and_then(json_to_shard_key))
            {
                insert_key(&mut keys, key);
            }
        }
    }
    Ok(sorted(keys))
}

pub(crate) fn json_to_shard_key(value: &Value) -> Option<ShardKey> {
    match value {
        Value::String(s) if !s.is_empty() => Some(ShardKey::Keyword(s.clone())),
        Value::Number(n) => {
            if let Some(u) = n.as_u64() {
                Some(ShardKey::Number(u))
            } else {
                n.as_i64()
                    .filter(|i| *i >= 0)
                    .map(|i| ShardKey::Number(i as u64))
            }
        }
        _ => None,
    }
}

/// Typed FACET value → shard key. Bool facet values are not shard keys.
fn facet_value_to_shard_key(value: &qql::PlanFacetValue) -> Option<ShardKey> {
    match value {
        qql::PlanFacetValue::Keyword(text) if !text.is_empty() => {
            Some(ShardKey::Keyword(text.clone()))
        }
        qql::PlanFacetValue::Integer(number) if *number >= 0 => {
            Some(ShardKey::Number(*number as u64))
        }
        _ => None,
    }
}

fn facet_missing_index(msg: &str) -> bool {
    let lower = msg.to_ascii_lowercase();
    lower.contains("index does not exist") || lower.contains("no payload index")
}

fn strip_where(clause: &str) -> &str {
    let trimmed = clause.trim();
    trimmed
        .strip_prefix("WHERE")
        .or_else(|| trimmed.strip_prefix("where"))
        .unwrap_or(trimmed)
        .trim()
}

fn insert_key(keys: &mut HashMap<String, ShardKey>, key: ShardKey) {
    keys.insert(key.to_string(), key);
}

fn sorted(keys: HashMap<String, ShardKey>) -> Vec<ShardKey> {
    let mut out: Vec<(String, ShardKey)> = keys.into_iter().collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out.into_iter().map(|(_, k)| k).collect()
}
