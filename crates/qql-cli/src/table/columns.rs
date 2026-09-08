//! Schema inference, column detection, and hit extraction for query outputs.

use std::collections::BTreeSet;

use super::cell::Cell;

/// The source location of a column's data within a point record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QueryColumnSource {
    Metadata(&'static str),
    Payload(String),
}

/// A detected query column with its display header label and source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueryColumn {
    pub label: String,
    pub source: QueryColumnSource,
}

/// Extracts search hits as key-value JSON maps from an operation response data envelope.
pub fn extract_hits(
    data: &Option<serde_json::Value>,
) -> Vec<serde_json::Map<String, serde_json::Value>> {
    let Some(data) = data else { return Vec::new() };

    // 1. Direct array of search hit objects (QUERY, GET_POINTS, CROSS_RERANK)
    if let Some(arr) = data.as_array() {
        return arr.iter().filter_map(|v| v.as_object().cloned()).collect();
    }

    // 2. Points inside a scroll envelope: {"result": {"points": [...]}} or {"points": [...]}
    let points = data
        .get("result")
        .and_then(|r| r.get("points"))
        .or_else(|| data.get("points"))
        .and_then(|p| p.as_array());

    if let Some(arr) = points {
        return arr.iter().filter_map(|v| v.as_object().cloned()).collect();
    }

    // 3. Array directly nested inside "result": {"result": [...]}
    if let Some(arr) = data.get("result").and_then(|r| r.as_array()) {
        return arr.iter().filter_map(|v| v.as_object().cloned()).collect();
    }

    Vec::new()
}

/// Automatically detects query columns from a slice of search hits.
///
/// Ensures metadata columns (`id`, `score`) appear first, followed by unique payload keys.
/// Colliding payload keys are renamed with a `payload.` prefix so the table is unambiguous.
pub fn detect_query_columns(
    hits: &[serde_json::Map<String, serde_json::Value>],
) -> Vec<QueryColumn> {
    let mut cols = vec![QueryColumn {
        label: "id".into(),
        source: QueryColumnSource::Metadata("id"),
    }];

    if hits.iter().any(|hit| hit.contains_key("score")) {
        cols.push(QueryColumn {
            label: "score".into(),
            source: QueryColumnSource::Metadata("score"),
        });
    }

    if hits.iter().any(|hit| hit.contains_key("collection")) {
        cols.push(QueryColumn {
            label: "collection".into(),
            source: QueryColumnSource::Metadata("collection"),
        });
    }

    // Payload keys form the remaining logical columns. Inspect every result:
    // sampling silently loses fields that occur only in later hits.
    let mut payload_keys = BTreeSet::new();
    for hit in hits {
        if let Some(payload) = hit.get("payload").and_then(|p| p.as_object()) {
            for key in payload.keys() {
                payload_keys.insert(key.clone());
            }
        }
    }

    for key in payload_keys {
        // `id`, `score`, and `collection` identify point metadata. Prefix colliding payload
        // keys so the table remains unambiguous.
        let mut label = if matches!(key.as_str(), "id" | "score" | "collection") {
            format!("payload.{key}")
        } else {
            key.clone()
        };
        while cols.iter().any(|column| column.label == label) {
            label = format!("payload.{label}");
        }
        cols.push(QueryColumn {
            label,
            source: QueryColumnSource::Payload(key),
        });
    }

    cols
}

/// Extracts a formatted `Cell` from a search hit for the specified column.
pub fn query_cell(hit: &serde_json::Map<String, serde_json::Value>, column: &QueryColumn) -> Cell {
    let value = match &column.source {
        QueryColumnSource::Metadata(key) => hit.get(*key),
        QueryColumnSource::Payload(key) => hit
            .get("payload")
            .and_then(serde_json::Value::as_object)
            .and_then(|payload| payload.get(key)),
    };
    Cell::from_json(value)
}
