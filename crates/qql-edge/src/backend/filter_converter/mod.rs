//! Typed lowering: `qql_plan` filter IR → `qdrant_edge::Filter`.
//!
//! This replaces the former `serde_json` round-trip in `convert_edge_filter`.
//! The same lowering serves every read and mutation response path
//! (`EdgeQdrant::execute_planned`), so filter semantics cannot drift between
//! them.
//!
//! Variant mapping (plan → qdrant-edge):
//!
//! | plan | qdrant-edge |
//! |---|---|
//! | `FilterExpression::Single` | `Filter { must: [condition] }` |
//! | `FilterExpression::Compound` | `Filter { must, must_not, should, min_should }` |
//! | `MatchValue::{Value,Text,Any,Except,Phrase,Prefix}` | `Match::{Value,Text,Any,Except,Phrase,Prefix}` |
//! | `RangeParams` numeric / RFC 3339 bounds | `RangeInterface::{Float,DateTime}` |
//! | `FieldCondition` geo / `values_count` / `is_empty` / `is_null` | same-named edge types |
//! | `IsNull`/`IsEmpty`/`HasId`/`HasVector`/`Nested`/`Filter`/`Slice` | same-named edge conditions |
//!
//! `FilterCompound::min_should` is the legacy integer form ("at least N of
//! `should`"). qdrant-edge 0.8 models the modern `MinShould { conditions,
//! min_count }` object, so the typed lowering moves the `should` clauses into
//! `MinShould::conditions` and clears `Filter::should`. The planner never emits
//! the legacy field today (every construction site passes `None`), so this is a
//! completeness adapter, not a live path.
//!
//! No serde fallback is needed: every plan variant maps directly.
//!
//! Split by size hygiene: `core` (expression dispatch), `matching`
//! (match/range/values-count), `geo` (geo/slice).

mod core;
mod geo;
mod matching;

#[cfg(test)]
mod tests;

pub(crate) use core::{convert_edge_filter, convert_formula_condition};

use qql_core::error::QqlError;

fn lower_key(key: &str) -> Result<qdrant_edge::JsonPath, QqlError> {
    key.parse()
        .map_err(|_| filter_error(format!("invalid payload key '{key}'")))
}

fn filter_error(message: impl Into<String>) -> QqlError {
    QqlError::execution("QQL-EDGE-FILTER-CONVERT", message.into(), None)
}
