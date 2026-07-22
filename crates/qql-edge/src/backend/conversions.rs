use qdrant_edge::{PointId, Record};
use serde_json::Value;

use qql_core::error::QqlError;
use qql_plan::PlanPointId;

pub(crate) fn to_edge_id(id: impl IntoPlanPointId) -> Result<PointId, QqlError> {
    match id.into_plan_point_id() {
        PlanPointId::Number(n) => Ok(PointId::NumId(n)),
        PlanPointId::String(s) => uuid::Uuid::parse_str(&s).map(PointId::Uuid).map_err(|e| {
            QqlError::execution("QQL-EDGE", format!("invalid UUID point id: {e}"), None)
        }),
    }
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
            Value::Number(n) => PlanPointId::Number(n.as_u64().unwrap_or(0)),
            Value::String(s) => PlanPointId::String(s),
            _ => PlanPointId::Number(0),
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
    Value::Object(obj)
}

pub(crate) fn edge_err(e: impl std::fmt::Display) -> QqlError {
    QqlError::execution("QQL-EDGE", format!("qdrant-edge: {e}"), None)
}
