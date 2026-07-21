use qdrant_edge::{PointId, Record};
use serde_json::Value;

use qql_core::error::QqlError;

pub(crate) fn to_edge_id(id: serde_json::Value) -> Result<PointId, QqlError> {
    match id {
        Value::Number(n) => n
            .as_u64()
            .map(PointId::NumId)
            .ok_or_else(|| QqlError::execution("QQL-EDGE", "invalid point id number", None)),
        Value::String(s) => uuid::Uuid::parse_str(&s).map(PointId::Uuid).map_err(|e| {
            QqlError::execution("QQL-EDGE", format!("invalid UUID point id: {e}"), None)
        }),
        _ => Err(QqlError::execution(
            "QQL-EDGE",
            "unsupported point id type",
            None,
        )),
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
