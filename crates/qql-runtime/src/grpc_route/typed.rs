//! Proto responses → typed executor IR, skipping the REST-shaped JSON envelope.
//!
//! Every operation the gRPC adapter executes converts its protobuf response
//! straight into [`BackendResponse`] data here — the REST envelope parser is
//! never involved. Field mapping mirrors the (now REST-only) envelope parser
//! (`crate::envelope::parse_backend_response`) exactly, with two documented
//! exceptions:
//!
//! - `SearchHit::text` stays `None` (the typed IR keeps payload text inside
//!   `payload`; the envelope parser additionally mirrors a payload `text` key
//!   onto the hit).
//! - grouped `lookup` points are dropped (the typed IR does not model them,
//!   and the envelope parser's serde decode drops them too).
//!
//! Parity is pinned by the tests in `tests.rs`.

use std::collections::HashMap;

use qql_plan::PlanPointId;

use crate::executor::response::{
    BackendResponse, ExecData, FacetHit, GroupedSearchResult, SearchHit,
};
use crate::executor::telemetry::{
    HardwareUsage, InferenceUsage, ModelUsage, ServerTelemetry, ServerUsage,
};
use crate::qdrant_grpc::qdrant;

use super::responses::vectors_output_to_json;
use super::values::qdrant_value_to_json;

/// Proto `PointId` → typed plan id, mirroring the REST envelope parser's
/// JSON decode (`extract_search_hits`): a missing id becomes `<missing-id>`,
/// an optionless `PointId` serializes as JSON null.
pub(crate) fn point_id_to_plan(id: Option<&qdrant::PointId>) -> PlanPointId {
    use qdrant::point_id::PointIdOptions;
    let Some(point_id) = id else {
        return PlanPointId::String("<missing-id>".to_string());
    };
    match &point_id.point_id_options {
        Some(PointIdOptions::Num(n)) => PlanPointId::Number(*n),
        Some(PointIdOptions::Uuid(s)) => PlanPointId::String(s.clone()),
        None => PlanPointId::String("null".to_string()),
    }
}

/// Payload map → typed payload: empty becomes `None`, matching the REST
/// envelope parser (absent/`null` payload reads as `None`).
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

fn vector_to_typed(vectors: &Option<qdrant::VectorsOutput>) -> Option<serde_json::Value> {
    vectors.as_ref().map(vectors_output_to_json)
}

/// Proto `ScoredPoint` → [`SearchHit`]. `version`, `shard_key` and
/// `order_value` are absent from the hit IR (envelope extraction drops them
/// too). `text` stays `None`; see the module docs.
pub(crate) fn scored_point_to_hit(p: qdrant::ScoredPoint) -> SearchHit {
    SearchHit {
        id: point_id_to_plan(p.id.as_ref()),
        score: p.score,
        text: None,
        payload: payload_to_typed(p.payload),
        collection: None,
        vector: vector_to_typed(&p.vectors),
    }
}

/// Proto `RetrievedPoint` → [`SearchHit`] with an unscored `0.0` score,
/// matching envelope extraction for get/scroll points.
pub(crate) fn retrieved_point_to_hit(p: qdrant::RetrievedPoint) -> SearchHit {
    SearchHit {
        id: point_id_to_plan(p.id.as_ref()),
        score: 0.0,
        text: None,
        payload: payload_to_typed(p.payload),
        collection: None,
        vector: vector_to_typed(&p.vectors),
    }
}

/// Proto `GroupId` → JSON group key, mirroring the REST group `id` shape:
/// unsigned and signed integers both as JSON numbers, strings as strings, and
/// a missing kind as `null`.
fn group_id_to_value(id: &qdrant::GroupId) -> serde_json::Value {
    match &id.kind {
        Some(qdrant::group_id::Kind::UnsignedValue(n)) => serde_json::json!(*n),
        Some(qdrant::group_id::Kind::IntegerValue(i)) => serde_json::json!(*i),
        Some(qdrant::group_id::Kind::StringValue(s)) => serde_json::json!(s),
        None => serde_json::Value::Null,
    }
}

/// Proto `PointGroup` → typed [`GroupedSearchResult`]. Hits reuse
/// [`scored_point_to_hit`]; `lookup` has no typed IR field and is dropped.
pub(crate) fn point_group_to_typed(g: qdrant::PointGroup) -> GroupedSearchResult {
    GroupedSearchResult {
        group_id: g
            .id
            .as_ref()
            .map_or(serde_json::Value::Null, group_id_to_value),
        hits: g.hits.into_iter().map(scored_point_to_hit).collect(),
    }
}

/// Proto `FacetHit` → typed [`FacetHit`], mirroring `facet_hit_to_json`.
pub(crate) fn facet_hit_to_typed(hit: qdrant::FacetHit) -> FacetHit {
    use qdrant::facet_value::Variant;
    let value = match hit.value.and_then(|v| v.variant) {
        Some(Variant::StringValue(s)) => serde_json::Value::String(s),
        Some(Variant::IntegerValue(i)) => serde_json::json!(i),
        Some(Variant::BoolValue(b)) => serde_json::Value::Bool(b),
        None => serde_json::Value::Null,
    };
    FacetHit {
        value,
        count: hit.count,
    }
}

/// Typed `usage` from the proto message, mirroring `usage_to_json` +
/// `ServerUsage::from_json` semantics exactly: `None` when the report is
/// absent or both sections are absent; sections are independent otherwise.
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

/// Build `ServerTelemetry` from proto `time` + `usage`, mirroring
/// `ServerTelemetry::from_envelope_opt` over the JSON envelope the retired
/// fallback path built. Proto `time` is non-optional and every response type
/// on this path carries it, so the result is always `Some`.
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
