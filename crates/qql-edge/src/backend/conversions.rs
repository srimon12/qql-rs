//! Point ID, vector, and record conversion utilities between QQL and qdrant-edge.

use qdrant_edge::{
    PointId, Record, ScoredPoint as EdgeScoredPoint, VectorInternal, VectorStructInternal,
};
use serde_json::Value;

use qql::client::{RetrievedPoint, ScoredPoint};
use qql::pipeline::PointId as QqlPointId;
use qql_core::error::QqlError;

pub(crate) fn to_edge_id(id: QqlPointId) -> Result<PointId, QqlError> {
    match id {
        QqlPointId::Num(n) => Ok(PointId::NumId(n)),
        QqlPointId::Uuid(s) => uuid::Uuid::parse_str(&s)
            .map(PointId::Uuid)
            .map_err(|e| QqlError::runtime(format!("invalid UUID point ID '{s}': {e}"))),
    }
}

pub(crate) fn from_edge_id(id: PointId) -> QqlPointId {
    match id {
        PointId::NumId(n) => QqlPointId::Num(n),
        PointId::Uuid(u) => QqlPointId::Uuid(u.to_string()),
    }
}

pub(crate) fn convert_edge_vector(v: VectorStructInternal) -> Value {
    match v {
        VectorStructInternal::Single(vec) => {
            Value::Array(vec.into_iter().map(Value::from).collect())
        }
        VectorStructInternal::MultiDense(multi) => Value::Array(
            multi
                .into_multi_vectors()
                .into_iter()
                .map(|m| Value::Array(m.into_iter().map(Value::from).collect()))
                .collect(),
        ),
        VectorStructInternal::Named(map) => {
            let mut obj = serde_json::Map::new();
            for (k, val) in map {
                let json_val = match val {
                    VectorInternal::Dense(d) => {
                        Value::Array(d.into_iter().map(Value::from).collect())
                    }
                    VectorInternal::Sparse(s) => serde_json::json!({
                        "indices": s.indices,
                        "values": s.values,
                    }),
                    VectorInternal::MultiDense(m) => Value::Array(
                        m.into_multi_vectors()
                            .into_iter()
                            .map(|sub| Value::Array(sub.into_iter().map(Value::from).collect()))
                            .collect(),
                    ),
                };
                obj.insert(k, json_val);
            }
            Value::Object(obj)
        }
    }
}

pub(crate) fn from_edge_scored(sp: EdgeScoredPoint) -> ScoredPoint {
    let payload = sp.payload.map(|p| p.0.into_iter().collect());
    let vector = sp.vector.map(convert_edge_vector);
    ScoredPoint {
        id: from_edge_id(sp.id),
        score: sp.score,
        payload,
        vector,
    }
}

pub(crate) fn from_edge_record(rec: Record) -> RetrievedPoint {
    let payload = rec.payload.map(|p| p.0.into_iter().collect());
    let vector = rec.vector.map(convert_edge_vector);
    RetrievedPoint {
        id: from_edge_id(rec.id),
        payload,
        vector,
    }
}

pub(crate) fn edge_err(e: impl std::fmt::Display) -> QqlError {
    QqlError::runtime(format!("qdrant-edge: {e}"))
}
