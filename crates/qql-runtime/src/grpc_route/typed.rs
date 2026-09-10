//! Proto responses → typed executor IR, skipping the REST-shaped JSON envelope.
//!
//! Every operation the gRPC adapter executes converts its protobuf response
//! straight into [`BackendResponse`] data here — point IDs, vectors, group
//! ids, facet values, and collection metadata are typed end to end, with no
//! JSON detour. A proto response that violates its contract (a missing point
//! id, vector, group id, or facet value) fails with `QQL-BACKEND-ENVELOPE`
//! instead of fabricating a placeholder.
//!
//! Two wire fields have no typed IR representation and are dropped with a
//! documented rationale: `next_page_offset` (callers derive the cursor from
//! the last hit id) and grouped `lookup` points.

use std::collections::{BTreeMap, HashMap};

use qql_core::error::QqlError;
use qql_plan::{
    PlanFacetValue, PlanGroupId, PlanPointId, PlanShardKey, PlanVectorStruct, PlanVectorValue,
};

use crate::executor::response::{
    BackendResponse, ExecData, FacetHit, GroupedSearchResult, SearchHit,
};
use crate::executor::telemetry::{
    HardwareUsage, InferenceUsage, ModelUsage, ServerTelemetry, ServerUsage,
};
use crate::qdrant_grpc::qdrant;

use super::values::qdrant_value_to_json;

fn envelope_err(message: impl Into<String>) -> QqlError {
    QqlError::backend("QQL-BACKEND-ENVELOPE", message.into(), None)
}

/// Proto `PointId` → typed plan id. A missing id or an optionless `PointId`
/// is a backend contract violation, not a fabricated `<missing-id>` string.
pub(crate) fn point_id_to_plan(id: Option<&qdrant::PointId>) -> Result<PlanPointId, QqlError> {
    use qdrant::point_id::PointIdOptions;
    let point_id = id.ok_or_else(|| envelope_err("point is missing its id"))?;
    match &point_id.point_id_options {
        Some(PointIdOptions::Num(n)) => Ok(PlanPointId::Number(*n)),
        Some(PointIdOptions::Uuid(s)) => Ok(PlanPointId::String(s.clone())),
        None => Err(envelope_err("point id carries neither num nor uuid")),
    }
}

/// Proto `ShardKey` → typed plan shard key.
pub(crate) fn shard_key_to_plan(key: &qdrant::ShardKey) -> Result<PlanShardKey, QqlError> {
    match &key.key {
        Some(qdrant::shard_key::Key::Keyword(keyword)) => {
            Ok(PlanShardKey::Keyword(keyword.clone()))
        }
        Some(qdrant::shard_key::Key::Number(number)) => Ok(PlanShardKey::Number(*number)),
        None => Err(envelope_err("shard key carries no value")),
    }
}

/// Payload map → typed payload: empty becomes `None`, matching the REST
/// strict parser (absent/`null` payload reads as `None`).
fn payload_to_typed(
    payload: HashMap<String, qdrant::Value>,
) -> Option<HashMap<String, serde_json::Value>> {
    if payload.is_empty() {
        None
    } else {
        Some(
            payload
                .into_iter()
                .map(|(k, v)| (k, qdrant_value_to_json(&v)))
                .collect(),
        )
    }
}

/// One proto `VectorOutput` → typed vector value.
fn vector_output_to_plan(vo: &qdrant::VectorOutput) -> Result<PlanVectorValue, QqlError> {
    use qdrant::vector_output;
    match &vo.vector {
        Some(vector_output::Vector::Dense(dense)) => Ok(PlanVectorValue::Dense(dense.data.clone())),
        Some(vector_output::Vector::Sparse(sparse)) => Ok(PlanVectorValue::Sparse {
            indices: sparse.indices.clone(),
            values: sparse.values.clone(),
        }),
        Some(vector_output::Vector::MultiDense(multi)) => Ok(PlanVectorValue::MultiDense(
            multi.vectors.iter().map(|d| d.data.clone()).collect(),
        )),
        None => Err(envelope_err("vector output carries no vector")),
    }
}

/// Proto `VectorsOutput` → typed [`PlanVectorStruct`] (single or named set).
pub(crate) fn vectors_output_to_typed(
    vectors: &qdrant::VectorsOutput,
) -> Result<PlanVectorStruct, QqlError> {
    use qdrant::vectors_output::VectorsOptions;
    match &vectors.vectors_options {
        Some(VectorsOptions::Vector(vo)) => {
            Ok(PlanVectorStruct::Single(vector_output_to_plan(vo)?))
        }
        Some(VectorsOptions::Vectors(named)) => {
            let entries = named
                .vectors
                .iter()
                .map(|(name, vo)| Ok((name.clone(), vector_output_to_plan(vo)?)))
                .collect::<Result<BTreeMap<_, _>, QqlError>>()?;
            Ok(PlanVectorStruct::Named(entries))
        }
        None => Err(envelope_err("vectors output carries no vector options")),
    }
}

fn vector_to_typed(
    vectors: &Option<qdrant::VectorsOutput>,
) -> Result<Option<PlanVectorStruct>, QqlError> {
    vectors.as_ref().map(vectors_output_to_typed).transpose()
}

/// Proto `ScoredPoint` → [`SearchHit`]. `version`, `shard_key` and
/// `order_value` are absent from the hit IR.
pub(crate) fn scored_point_to_hit(p: qdrant::ScoredPoint) -> Result<SearchHit, QqlError> {
    Ok(SearchHit {
        id: point_id_to_plan(p.id.as_ref())?,
        score: p.score,
        payload: payload_to_typed(p.payload),
        collection: None,
        vector: vector_to_typed(&p.vectors)?,
    })
}

/// Proto `RetrievedPoint` → [`SearchHit`] with an unscored `0.0` score.
pub(crate) fn retrieved_point_to_hit(p: qdrant::RetrievedPoint) -> Result<SearchHit, QqlError> {
    Ok(SearchHit {
        id: point_id_to_plan(p.id.as_ref())?,
        score: 0.0,
        payload: payload_to_typed(p.payload),
        collection: None,
        vector: vector_to_typed(&p.vectors)?,
    })
}

/// Proto `GroupId` → typed group key.
fn group_id_to_plan(id: &qdrant::GroupId) -> Result<PlanGroupId, QqlError> {
    match &id.kind {
        Some(qdrant::group_id::Kind::UnsignedValue(n)) => Ok(PlanGroupId::Unsigned(*n)),
        Some(qdrant::group_id::Kind::IntegerValue(i)) => Ok(PlanGroupId::Signed(*i)),
        Some(qdrant::group_id::Kind::StringValue(s)) => Ok(PlanGroupId::Keyword(s.clone())),
        None => Err(envelope_err("group id carries no value")),
    }
}

/// Proto `PointGroup` → typed [`GroupedSearchResult`]. `lookup` has no typed
/// IR field and is dropped.
pub(crate) fn point_group_to_typed(g: qdrant::PointGroup) -> Result<GroupedSearchResult, QqlError> {
    let id =
        g.id.as_ref()
            .ok_or_else(|| envelope_err("group is missing its id"))?;
    Ok(GroupedSearchResult {
        group_id: group_id_to_plan(id)?,
        hits: g
            .hits
            .into_iter()
            .map(scored_point_to_hit)
            .collect::<Result<_, _>>()?,
    })
}

/// Proto `FacetHit` → typed [`FacetHit`].
pub(crate) fn facet_hit_to_typed(hit: qdrant::FacetHit) -> Result<FacetHit, QqlError> {
    use qdrant::facet_value::Variant;
    let value = match hit.value.and_then(|v| v.variant) {
        Some(Variant::StringValue(s)) => PlanFacetValue::Keyword(s),
        Some(Variant::IntegerValue(i)) => PlanFacetValue::Integer(i),
        Some(Variant::BoolValue(b)) => PlanFacetValue::Bool(b),
        None => return Err(envelope_err("facet hit is missing its value")),
    };
    Ok(FacetHit {
        value,
        count: hit.count,
    })
}

/// Typed `usage` from the proto message, mirroring `ServerUsage::from_json`
/// semantics exactly: `None` when the report is absent or both sections are
/// absent; sections are independent otherwise.
pub(crate) fn usage_to_telemetry(usage: Option<&qdrant::Usage>) -> Option<ServerUsage> {
    let report = usage?;
    let hardware = report.hardware.map(|h| HardwareUsage {
        cpu: h.cpu,
        payload_io_read: h.payload_io_read,
        payload_io_write: h.payload_io_write,
        payload_index_io_read: h.payload_index_io_read,
        payload_index_io_write: h.payload_index_io_write,
        vector_io_read: h.vector_io_read,
        vector_io_write: h.vector_io_write,
    });
    let inference = report.inference.as_ref().map(|inf| InferenceUsage {
        models: inf
            .models
            .iter()
            .map(|(name, m)| (name.clone(), ModelUsage { tokens: m.tokens }))
            .collect(),
    });
    if hardware.is_none() && inference.is_none() {
        return None;
    }
    Some(ServerUsage {
        hardware,
        inference,
    })
}

/// Build `ServerTelemetry` from proto `time` + `usage`. Proto `time` is
/// non-optional and every response type on this path carries it, so the
/// result is always `Some`.
pub(crate) fn telemetry_from_proto(
    time: f64,
    usage: Option<&qdrant::Usage>,
) -> Option<ServerTelemetry> {
    Some(ServerTelemetry {
        time_s: Some(time),
        usage: usage_to_telemetry(usage),
    })
}

/// Proto `PointsOperationResponse` (write ops, payload indexes) → status-only
/// [`ExecData::Mutation`] + typed telemetry.
///
/// The proto `UpdateResult` carries only operation id + status — no affected
/// count — so `affected` is always `None`; the executor derives upsert counts
/// from the request during normalization.
pub(crate) fn mutation_response_to_typed(resp: qdrant::PointsOperationResponse) -> BackendResponse {
    BackendResponse {
        data: ExecData::Mutation { affected: None },
        telemetry: telemetry_from_proto(resp.time, resp.usage.as_ref()),
    }
}

/// Proto collection / shard-key mutation response (bool status + time) →
/// status-only [`ExecData::Mutation`] + typed telemetry. These response types
/// carry no `usage` field.
pub(crate) fn collection_mutation_to_typed(time: f64) -> BackendResponse {
    BackendResponse {
        data: ExecData::Mutation { affected: None },
        telemetry: telemetry_from_proto(time, None),
    }
}

/// Test-only parity oracle: convert a gRPC `Usage` into the REST `usage` JSON
/// shape consumed by [`ServerUsage::from_json`]. Production code never builds
/// response JSON — REST telemetry stays a lenient JSON parse, and gRPC
/// telemetry converts proto → [`ServerUsage`] directly via
/// [`usage_to_telemetry`].
#[cfg(test)]
pub(crate) fn usage_to_json(usage: Option<&qdrant::Usage>) -> serde_json::Value {
    let Some(report) = usage else {
        return serde_json::Value::Null;
    };
    let hardware = report.hardware.as_ref().map(|h| {
        serde_json::json!({
            "cpu": h.cpu,
            "payload_io_read": h.payload_io_read,
            "payload_io_write": h.payload_io_write,
            "payload_index_io_read": h.payload_index_io_read,
            "payload_index_io_write": h.payload_index_io_write,
            "vector_io_read": h.vector_io_read,
            "vector_io_write": h.vector_io_write,
        })
    });
    let inference = report.inference.as_ref().map(|inf| {
        let models: serde_json::Map<String, serde_json::Value> = inf
            .models
            .iter()
            .map(|(name, m)| (name.clone(), serde_json::json!({ "tokens": m.tokens })))
            .collect();
        serde_json::json!({ "models": models })
    });
    serde_json::json!({
        "hardware": hardware,
        "inference": inference,
    })
}
