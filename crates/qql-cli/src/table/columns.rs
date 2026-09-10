//! Schema inference, column detection, and cell extraction for query outputs.

use std::collections::BTreeSet;

use qql::executor::SearchHit;

use super::cell::{Alignment, Cell};

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

/// Automatically detects query columns from typed search hits.
///
/// Ensures metadata columns (`id`, then `score` for scored operations) appear
/// first, followed by unique payload keys. Colliding payload keys are renamed
/// with a `payload.` prefix so the table is unambiguous.
pub fn detect_query_columns(hits: &[SearchHit], scored: bool) -> Vec<QueryColumn> {
    let mut cols = vec![QueryColumn {
        label: "id".into(),
        source: QueryColumnSource::Metadata("id"),
    }];

    if scored {
        cols.push(QueryColumn {
            label: "score".into(),
            source: QueryColumnSource::Metadata("score"),
        });
    }

    if hits.iter().any(|hit| hit.collection.is_some()) {
        cols.push(QueryColumn {
            label: "collection".into(),
            source: QueryColumnSource::Metadata("collection"),
        });
    }

    // Payload keys form the remaining logical columns. Inspect every result:
    // sampling silently loses fields that occur only in later hits.
    let mut payload_keys = BTreeSet::new();
    for hit in hits {
        if let Some(payload) = &hit.payload {
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

/// Extracts a formatted `Cell` from a typed search hit for the specified column.
pub fn query_cell(hit: &SearchHit, column: &QueryColumn) -> Cell {
    match &column.source {
        QueryColumnSource::Metadata("id") => Cell::text(hit.id.to_string()),
        QueryColumnSource::Metadata("score") => Cell {
            // Shortest f32 round-trip decimal, matching the serialized report.
            value: hit.score.to_string(),
            alignment: Alignment::Right,
        },
        QueryColumnSource::Metadata("collection") => {
            Cell::text(hit.collection.as_deref().unwrap_or_default())
        }
        QueryColumnSource::Payload(key) => {
            Cell::from_json(hit.payload.as_ref().and_then(|payload| payload.get(key)))
        }
        QueryColumnSource::Metadata(_) => Cell::text(""),
    }
}
