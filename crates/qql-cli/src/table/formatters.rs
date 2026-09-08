//! Statement-specific table formatters for queries, facets, groups, counts, and metadata.

use super::cell::{Cell, stringify_value};
use super::columns::{detect_query_columns, extract_hits, query_cell};
use super::renderer::Table;

/// Print search hits for QUERY, SCROLL, CROSS_RERANK, and GET_POINTS operations.
pub fn print_query_table(
    data: &Option<serde_json::Value>,
) -> Result<(), Box<dyn std::error::Error>> {
    let hits = extract_hits(data);
    if hits.is_empty() {
        println!("(no results)");
        return Ok(());
    }

    let max_col_width = std::env::var("QQL_MAX_COL_WIDTH")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(80);

    let columns = detect_query_columns(&hits);
    let mut table = Table::new(columns.iter().map(|column| column.label.clone()).collect())
        .with_max_col_width(max_col_width);

    for hit in &hits {
        let mut row = Vec::with_capacity(columns.len());
        for col in &columns {
            row.push(query_cell(hit, col));
        }
        table.add_cells(row);
    }

    table.print()?;
    Ok(())
}

/// Print categorical facet aggregation results for FACET operations.
pub fn print_facet_table(
    data: &Option<serde_json::Value>,
) -> Result<(), Box<dyn std::error::Error>> {
    let hits = data.as_ref().and_then(|d| {
        d.as_array()
            .or_else(|| {
                d.get("result")
                    .and_then(|r| r.get("hits"))
                    .and_then(|h| h.as_array())
            })
            .or_else(|| d.get("hits").and_then(|h| h.as_array()))
    });

    let Some(hits) = hits else {
        if let Some(d) = data {
            println!("{}", serde_json::to_string_pretty(d)?);
        }
        return Ok(());
    };

    if hits.is_empty() {
        println!("(no facet hits)");
        return Ok(());
    }

    let mut table = Table::new(vec!["Value".into(), "Count".into()]);
    for hit in hits {
        let val = hit.get("value");
        let count = hit.get("count");
        table.add_cells(vec![Cell::from_json(val), Cell::from_json(count)]);
    }

    table.print()?;
    Ok(())
}

/// Print grouped query results for QUERY ... GROUP BY operations.
pub fn print_groups_table(
    data: &Option<serde_json::Value>,
) -> Result<(), Box<dyn std::error::Error>> {
    let groups = data
        .as_ref()
        .and_then(|d| d.get("result"))
        .and_then(|r| r.get("groups"))
        .and_then(|g| g.as_array())
        .or_else(|| {
            data.as_ref()
                .and_then(|d| d.get("groups"))
                .and_then(|g| g.as_array())
        });

    let Some(groups) = groups else {
        println!("{}", serde_json::to_string_pretty(data)?);
        return Ok(());
    };

    if groups.is_empty() {
        println!("(no groups)");
        return Ok(());
    }

    let mut table = Table::new(vec!["group_id".into(), "count".into()]);

    for g in groups {
        let id = stringify_value(&g.get("id").cloned().unwrap_or_default());
        let hits = g
            .get("hits")
            .and_then(|h| h.as_array())
            .map(|a| a.len().to_string())
            .unwrap_or_else(|| "0".into());
        table.add_row(vec![id, hits]);
    }

    table.print()?;
    Ok(())
}

/// Print count integer for COUNT operations.
pub fn print_count(data: &Option<serde_json::Value>) {
    println!("  count: {}", count_value(data));
}

/// Print a table of collection names for SHOW COLLECTIONS operations.
pub fn print_collections_list(
    data: &Option<serde_json::Value>,
) -> Result<(), Box<dyn std::error::Error>> {
    let cols = collection_names(data);

    if cols.is_empty() {
        println!("(no collections)");
        return Ok(());
    }

    let mut table = Table::new(vec!["Collection".into()]);
    for name in cols {
        table.add_row(vec![name]);
    }
    table.print()?;
    Ok(())
}

/// Print collection metadata properties for SHOW COLLECTION operations.
pub fn print_collection_info(
    data: &Option<serde_json::Value>,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(obj) = result_value(data).and_then(serde_json::Value::as_object) else {
        if let Some(d) = data {
            println!("{}", serde_json::to_string_pretty(d)?);
        }
        return Ok(());
    };

    let mut table = Table::new(vec!["Property".into(), "Value".into()]);
    for (key, val) in obj {
        table.add_row(vec![key.clone(), stringify_value(val)]);
    }
    table.print()?;
    Ok(())
}

/// Print custom shard keys for SHOW SHARD KEYS operations.
pub fn print_shard_keys_table(
    data: &Option<serde_json::Value>,
) -> Result<(), Box<dyn std::error::Error>> {
    let shard_keys = data.as_ref().and_then(|d| {
        d.get("result")
            .and_then(|r| r.get("shard_keys"))
            .or_else(|| d.get("shard_keys"))
            .and_then(|s| s.as_array())
            .or_else(|| d.as_array())
    });

    let Some(keys) = shard_keys else {
        if let Some(d) = data {
            println!("{}", serde_json::to_string_pretty(d)?);
        }
        return Ok(());
    };

    if keys.is_empty() {
        println!("(no shard keys)");
        return Ok(());
    }

    let mut table = Table::new(vec!["Shard Key".into()]);
    for key in keys {
        table.add_row(vec![stringify_value(key)]);
    }
    table.print()?;
    Ok(())
}

/// Print quota limits and metrics for SHOW QUOTAS operations.
pub fn print_quotas_table(
    data: &Option<serde_json::Value>,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(obj) = result_value(data).and_then(serde_json::Value::as_object) else {
        if let Some(d) = data {
            println!("{}", serde_json::to_string_pretty(d)?);
        }
        return Ok(());
    };

    if obj.is_empty() {
        println!("(no quotas configured)");
        return Ok(());
    }

    let mut table = Table::new(vec!["Quota".into(), "Limit".into()]);
    for (key, val) in obj {
        table.add_row(vec![key.clone(), stringify_value(val)]);
    }
    table.print()?;
    Ok(())
}

// ── helpers ──────────────────────────────────────────────────────

/// Returns Qdrant's `result` object when present, otherwise the response.
pub fn result_value(data: &Option<serde_json::Value>) -> Option<&serde_json::Value> {
    data.as_ref()
        .map(|value| value.get("result").unwrap_or(value))
}

pub fn count_value(data: &Option<serde_json::Value>) -> u64 {
    result_value(data)
        .and_then(|value| value.get("count"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
}

pub fn collection_names(data: &Option<serde_json::Value>) -> Vec<String> {
    result_value(data)
        .and_then(|value| value.get("collections"))
        .and_then(serde_json::Value::as_array)
        .map(|collections| {
            collections
                .iter()
                .filter_map(|collection| {
                    collection
                        .as_str()
                        .or_else(|| collection.get("name").and_then(serde_json::Value::as_str))
                })
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}
