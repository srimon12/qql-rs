use std::collections::{BTreeMap, HashMap};

use qdrant_edge::{PointId, Record};

use qql::executor::{FacetHit, SearchHit};
use qql_core::error::QqlError;
use qql_plan::{PlanFacetValue, PlanPointId, PlanVectorStruct, PlanVectorValue};

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

fn from_edge_payload(payload: qdrant_edge::Payload) -> HashMap<String, serde_json::Value> {
    payload.0.into_iter().collect()
}

/// Typed search hit from a scored query point.
pub(crate) fn from_edge_scored_point_to_hit(point: qdrant_edge::ScoredPoint) -> SearchHit {
    SearchHit {
        id: from_edge_plan_id(&point.id),
        score: point.score,
        payload: point.payload.map(from_edge_payload),
        collection: None,
        vector: point.vector.map(edge_vector_to_typed),
    }
}

/// Typed search hit from a scroll/retrieve record. Records carry no similarity
/// score, so `score` is `0.0`.
pub(crate) fn from_edge_record_to_hit(record: Record) -> SearchHit {
    SearchHit {
        id: from_edge_plan_id(&record.id),
        score: 0.0,
        payload: record.payload.map(from_edge_payload),
        collection: None,
        vector: record.vector.map(edge_vector_to_typed),
    }
}

/// Typed facet entry from a `qdrant-edge` facet hit. Edge UUID facet values
/// serialize as strings on the REST surface, so they become keywords here.
pub(crate) fn from_edge_facet_hit(hit: qdrant_edge::FacetValueHit) -> FacetHit {
    FacetHit {
        value: match hit.value {
            qdrant_edge::FacetValue::Keyword(keyword) => PlanFacetValue::Keyword(keyword),
            qdrant_edge::FacetValue::Int(integer) => PlanFacetValue::Integer(integer),
            qdrant_edge::FacetValue::Uuid(uuid) => {
                PlanFacetValue::Keyword(uuid::Uuid::from_u128(uuid).to_string())
            }
            qdrant_edge::FacetValue::Bool(flag) => PlanFacetValue::Bool(flag),
        },
        count: hit.count as u64,
    }
}

/// One `qdrant-edge` vector value → typed plan vector value.
fn edge_vector_value_to_typed(vector: qdrant_edge::VectorInternal) -> PlanVectorValue {
    match vector {
        qdrant_edge::VectorInternal::Dense(values) => PlanVectorValue::Dense(values),
        qdrant_edge::VectorInternal::Sparse(sparse) => PlanVectorValue::Sparse {
            indices: sparse.indices,
            values: sparse.values,
        },
        qdrant_edge::VectorInternal::MultiDense(multi) => {
            PlanVectorValue::MultiDense(multi.into_multi_vectors())
        }
    }
}

/// `qdrant-edge` vector struct → typed [`PlanVectorStruct`] (single or named).
fn edge_vector_to_typed(vector: qdrant_edge::VectorStructInternal) -> PlanVectorStruct {
    match vector {
        qdrant_edge::VectorStructInternal::Single(values) => {
            PlanVectorStruct::Single(PlanVectorValue::Dense(values))
        }
        qdrant_edge::VectorStructInternal::MultiDense(multi) => {
            PlanVectorStruct::Single(PlanVectorValue::MultiDense(multi.into_multi_vectors()))
        }
        qdrant_edge::VectorStructInternal::Named(vectors) => PlanVectorStruct::Named(
            vectors
                .into_iter()
                .map(|(name, vector)| (name.to_string(), edge_vector_value_to_typed(vector)))
                .collect::<BTreeMap<_, _>>(),
        ),
    }
}
