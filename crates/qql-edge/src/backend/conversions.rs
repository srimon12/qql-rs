use std::collections::HashMap;

use qdrant_edge::{PointId, Record};
use serde_json::Value;

use qql::executor::{FacetHit, SearchHit};
use qql_core::error::QqlError;
use qql_plan::PlanPointId;

pub(crate) fn to_edge_id(id: impl IntoPlanPointId) -> Result<PointId, QqlError> {
    match id.into_plan_point_id() {
        PlanPointId::Number(n) => Ok(PointId::NumId(n)),
        PlanPointId::String(s) => {
            // qdrant-edge PointId is NumId | Uuid only — bare strings like "doc-1"
            // are not representable. Fail loudly instead of silently dropping the
            // id (the previous filter_map(...ok()) path deleted half a batch).
            uuid::Uuid::parse_str(&s).map(PointId::Uuid).map_err(|e| {
                QqlError::execution(
                    "QQL-EDGE-INVALID-POINT-ID",
                    format!(
                        "invalid point id '{s}': edge mode accepts unsigned integers or UUIDs only ({e})"
                    ),
                    None,
                )
            })
        }
    }
}

/// Convert a list of plan point IDs, propagating the first conversion error.
pub(crate) fn to_edge_ids<I, T>(ids: I) -> Result<Vec<PointId>, QqlError>
where
    I: IntoIterator<Item = T>,
    T: IntoPlanPointId,
{
    ids.into_iter().map(to_edge_id).collect()
}

/// Accept typed plan point IDs (owned or borrowed).
pub(crate) trait IntoPlanPointId {
    fn into_plan_point_id(self) -> PlanPointId;
}

impl IntoPlanPointId for PlanPointId {
    fn into_plan_point_id(self) -> PlanPointId {
        self
    }
}

impl IntoPlanPointId for &PlanPointId {
    fn into_plan_point_id(self) -> PlanPointId {
        self.clone()
    }
}

/// Typed plan ID from a `qdrant-edge` point ID (no JSON hop).
pub(crate) fn from_edge_plan_id(id: &PointId) -> PlanPointId {
    match id {
        PointId::NumId(n) => PlanPointId::Number(*n),
        PointId::Uuid(u) => PlanPointId::String(u.to_string()),
    }
}

fn from_edge_payload(payload: qdrant_edge::Payload) -> HashMap<String, Value> {
    payload.0.into_iter().collect()
}

/// Typed search hit from a scored query point.
///
/// `text` stays `None` (the payload still carries any text field).
pub(crate) fn from_edge_scored_point_to_hit(point: qdrant_edge::ScoredPoint) -> SearchHit {
    SearchHit {
        id: from_edge_plan_id(&point.id),
        score: point.score,
        text: None,
        payload: point.payload.map(from_edge_payload),
        collection: None,
        vector: point.vector.map(edge_vector_to_json),
    }
}

/// Typed search hit from a scroll/retrieve record. Records carry no similarity
/// score, so `score` is `0.0`.
pub(crate) fn from_edge_record_to_hit(record: Record) -> SearchHit {
    SearchHit {
        id: from_edge_plan_id(&record.id),
        score: 0.0,
        text: None,
        payload: record.payload.map(from_edge_payload),
        collection: None,
        vector: record.vector.map(edge_vector_to_json),
    }
}

/// Typed facet entry from a `qdrant-edge` facet hit.
pub(crate) fn from_edge_facet_hit(hit: qdrant_edge::FacetValueHit) -> FacetHit {
    FacetHit {
        value: qdrant_edge::ValueVariants::from(hit.value).to_value(),
        count: hit.count as u64,
    }
}

fn edge_vector_to_json(vector: qdrant_edge::VectorStructInternal) -> Value {
    match vector {
        qdrant_edge::VectorStructInternal::Single(values) => serde_json::json!(values),
        qdrant_edge::VectorStructInternal::MultiDense(values) => {
            serde_json::to_value(values).unwrap_or(Value::Null)
        }
        qdrant_edge::VectorStructInternal::Named(values) => Value::Object(
            values
                .into_iter()
                .map(|(name, value)| {
                    let value = serde_json::to_value(value).unwrap_or(Value::Null);
                    (name, value)
                })
                .collect(),
        ),
    }
}

/// Wrap a `qdrant-edge` library error. These are low-level failures from the
/// in-process HNSW engine (I/O, index corruption, lock poisoning, etc.).
pub(crate) fn edge_err(e: impl std::fmt::Display) -> QqlError {
    QqlError::execution("QQL-EDGE-LIB", format!("qdrant-edge: {e}"), None)
}
