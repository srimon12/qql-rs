use qdrant_edge::{PointId, Record};
use serde_json::Value;

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

/// Accept typed plan IDs and legacy JSON values during the migration.
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

impl IntoPlanPointId for serde_json::Value {
    fn into_plan_point_id(self) -> PlanPointId {
        match self {
            Value::Number(n) => n
                .as_u64()
                .map(PlanPointId::Number)
                .unwrap_or_else(|| PlanPointId::String(n.to_string())),
            Value::String(s) => PlanPointId::String(s),
            // Do NOT map unknown JSON shapes to id 0 — that silently rewrites
            // deletes/retrieves. Use an unparseable string so to_edge_id errors.
            other => PlanPointId::String(format!("__invalid_point_id__:{other}")),
        }
    }
}

pub(crate) fn from_edge_id(id: &PointId) -> Value {
    match id {
        PointId::NumId(n) => serde_json::json!(*n),
        PointId::Uuid(u) => serde_json::json!(u.to_string()),
    }
}

pub(crate) fn from_edge_record(rec: Record) -> Value {
    let id = from_edge_id(&rec.id);
    let payload: Value = rec
        .payload
        .map(|p| {
            let map: serde_json::Map<String, Value> = p.0.into_iter().collect();
            Value::Object(map)
        })
        .unwrap_or(Value::Null);
    let mut obj = serde_json::Map::new();
    obj.insert("id".into(), id);
    obj.insert("payload".into(), payload);
    if let Some(vector) = rec.vector {
        obj.insert("vector".into(), edge_vector_to_json(vector));
    }
    Value::Object(obj)
}

pub(crate) fn from_edge_scored_point(point: qdrant_edge::ScoredPoint) -> Value {
    let mut object = serde_json::Map::new();
    object.insert("id".into(), from_edge_id(&point.id));
    object.insert("score".into(), serde_json::json!(point.score));
    object.insert("version".into(), serde_json::json!(point.version));
    if let Some(payload) = point.payload {
        object.insert(
            "payload".into(),
            serde_json::to_value(payload).unwrap_or(Value::Null),
        );
    }
    if let Some(vector) = point.vector {
        object.insert("vector".into(), edge_vector_to_json(vector));
    }
    Value::Object(object)
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
