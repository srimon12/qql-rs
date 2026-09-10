//! Statement-specific table formatters for queries, facets, groups, counts, and metadata.
//!
//! Every formatter consumes typed `ExecData` payloads directly — there is no
//! JSON round-trip between the executor and the terminal.

use qql::backend::CollectionInfo;
use qql::executor::{FacetHit, GroupedSearchResult, SearchHit};
use qql::{PlanFacetValue, PlanGroupId, PlanShardKey, QuotaConfig};

use super::cell::{Alignment, Cell};
use super::columns::{detect_query_columns, query_cell};
use super::renderer::Table;

/// Print search hits for QUERY, SCROLL, CROSS_RERANK, and GET_POINTS operations.
///
/// `scored` adds the score column for operations whose backend returns
/// similarity scores (QUERY, CROSS_RERANK); SCROLL / GET_POINTS carry the
/// default 0.0 and omit it.
pub fn print_query_table(
    hits: Option<&[SearchHit]>,
    scored: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(hits) = hits else {
        println!("(no results)");
        return Ok(());
    };
    if hits.is_empty() {
        println!("(no results)");
        return Ok(());
    }

    let max_col_width = std::env::var("QQL_MAX_COL_WIDTH")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(80);

    let columns = detect_query_columns(hits, scored);
    let mut table = Table::new(columns.iter().map(|column| column.label.clone()).collect())
        .with_max_col_width(max_col_width);

    for hit in hits {
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
pub fn print_facet_table(entries: Option<&[FacetHit]>) -> Result<(), Box<dyn std::error::Error>> {
    let Some(entries) = entries else {
        println!("(no facet hits)");
        return Ok(());
    };
    if entries.is_empty() {
        println!("(no facet hits)");
        return Ok(());
    }

    let mut table = Table::new(vec!["Value".into(), "Count".into()]);
    for entry in entries {
        table.add_cells(vec![
            facet_value_cell(&entry.value),
            Cell {
                value: entry.count.to_string(),
                alignment: Alignment::Right,
            },
        ]);
    }

    table.print()?;
    Ok(())
}

/// Print grouped query results for QUERY ... GROUP BY operations.
pub fn print_groups_table(
    groups: Option<&[GroupedSearchResult]>,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(groups) = groups else {
        println!("(no groups)");
        return Ok(());
    };
    if groups.is_empty() {
        println!("(no groups)");
        return Ok(());
    }

    let mut table = Table::new(vec!["group_id".into(), "count".into()]);

    for group in groups {
        table.add_cells(vec![
            group_id_cell(&group.group_id),
            Cell {
                value: group.hits.len().to_string(),
                alignment: Alignment::Right,
            },
        ]);
    }

    table.print()?;
    Ok(())
}

/// Print count integer for COUNT operations.
pub fn print_count(count: Option<u64>) {
    println!("  count: {}", count.unwrap_or(0));
}

/// Print a table of collection names for SHOW COLLECTIONS operations.
pub fn print_collections_list(names: Option<&[String]>) -> Result<(), Box<dyn std::error::Error>> {
    let Some(names) = names else {
        println!("(no collections)");
        return Ok(());
    };
    if names.is_empty() {
        println!("(no collections)");
        return Ok(());
    }

    let mut table = Table::new(vec!["Collection".into()]);
    for name in names {
        table.add_row(vec![name.clone()]);
    }
    table.print()?;
    Ok(())
}

/// Print collection metadata properties for SHOW COLLECTION operations.
pub fn print_collection_info(
    info: Option<&CollectionInfo>,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(info) = info else {
        println!("(no collection info)");
        return Ok(());
    };

    let mut table = Table::new(vec!["Property".into(), "Value".into()]);
    table.add_row(vec!["status".into(), info.status.clone()]);
    table.add_row(vec!["points_count".into(), info.points_count.to_string()]);
    table.add_row(vec![
        "segments_count".into(),
        info.segments_count.to_string(),
    ]);

    let vectors = info
        .schema
        .vectors
        .iter()
        .map(|spec| {
            let name = spec.name.as_deref().unwrap_or("default");
            format!(
                "{name} ({}, {})",
                spec.size,
                spec.distance.to_ascii_uppercase()
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    if !vectors.is_empty() {
        table.add_row(vec!["vectors".into(), vectors]);
    }

    let sparse = info
        .schema
        .sparse_vectors
        .iter()
        .map(|spec| spec.name.clone())
        .collect::<Vec<_>>()
        .join(", ");
    if !sparse.is_empty() {
        table.add_row(vec!["sparse_vectors".into(), sparse]);
    }

    let indexes = info
        .schema
        .payload_indexes
        .iter()
        .map(|spec| format!("{} ({})", spec.field, spec.data_type))
        .collect::<Vec<_>>()
        .join(", ");
    if !indexes.is_empty() {
        table.add_row(vec!["payload_indexes".into(), indexes]);
    }

    let params = &info.schema.params;
    if let Some(shard_number) = params.shard_number {
        table.add_row(vec!["shard_number".into(), shard_number.to_string()]);
    }
    if let Some(method) = &params.sharding_method {
        table.add_row(vec!["sharding_method".into(), method.clone()]);
    }
    if let Some(on_disk) = params.on_disk_payload {
        table.add_row(vec!["on_disk_payload".into(), on_disk.to_string()]);
    }
    if let Some(replication) = params.replication_factor {
        table.add_row(vec!["replication_factor".into(), replication.to_string()]);
    }

    table.print()?;
    Ok(())
}

/// Print custom shard keys for SHOW SHARD KEYS operations.
pub fn print_shard_keys_table(
    keys: Option<&[PlanShardKey]>,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(keys) = keys else {
        println!("(no shard keys)");
        return Ok(());
    };
    if keys.is_empty() {
        println!("(no shard keys)");
        return Ok(());
    }

    let mut table = Table::new(vec!["Shard Key".into()]);
    for key in keys {
        table.add_row(vec![key.to_string()]);
    }
    table.print()?;
    Ok(())
}

/// Print quota limits and metrics for SHOW QUOTAS operations.
pub fn print_quotas_table(config: Option<&QuotaConfig>) -> Result<(), Box<dyn std::error::Error>> {
    let Some(config) = config else {
        println!("(no quotas configured)");
        return Ok(());
    };

    let mut rows: Vec<(&str, String)> = Vec::new();
    if let Some(enabled) = config.enabled {
        rows.push(("enabled", enabled.to_string()));
    }
    if let Some(percent) = config.max_resident_memory_percent {
        rows.push(("max_resident_memory_percent", percent.to_string()));
    }
    if let Some(percent) = config.max_disk_usage_percent {
        rows.push(("max_disk_usage_percent", percent.to_string()));
    }
    if let Some(percent) = config.release_margin_percent {
        rows.push(("release_margin_percent", percent.to_string()));
    }

    if rows.is_empty() {
        println!("(no quotas configured)");
        return Ok(());
    }

    let mut table = Table::new(vec!["Quota".into(), "Limit".into()]);
    for (key, value) in rows {
        table.add_row(vec![key.into(), value]);
    }
    table.print()?;
    Ok(())
}

// ── helpers ──────────────────────────────────────────────────────

/// Render a typed facet value with the alignment its JSON shape would get.
fn facet_value_cell(value: &PlanFacetValue) -> Cell {
    match value {
        PlanFacetValue::Keyword(text) => Cell::text(text.clone()),
        PlanFacetValue::Integer(number) => Cell {
            value: number.to_string(),
            alignment: Alignment::Right,
        },
        PlanFacetValue::Bool(flag) => Cell::text(flag.to_string()),
    }
}

/// Render a typed group id with the alignment its JSON shape would get.
fn group_id_cell(id: &PlanGroupId) -> Cell {
    match id {
        PlanGroupId::Keyword(text) => Cell::text(text.clone()),
        PlanGroupId::Unsigned(number) => Cell {
            value: number.to_string(),
            alignment: Alignment::Right,
        },
        PlanGroupId::Signed(number) => Cell {
            value: number.to_string(),
            alignment: Alignment::Right,
        },
    }
}
