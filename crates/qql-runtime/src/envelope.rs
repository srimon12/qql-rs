//! REST-shaped envelope → typed IR parsing.
//!
//! REST is the transport that speaks JSON envelopes, so
//! `RestQdrant::execute_planned` is the canonical caller. The gRPC fallback
//! arms for operations that are not typed yet (marked `TODO(R2)`) and the
//! executor's test doubles temporarily reuse this parser; `TODO(R2/R3)`
//! removes those callers.

use qql_core::error::QqlError;
use qql_plan::{PlanPointId, PlannedOperation};

use crate::executor::response::{
    BackendResponse, ExecData, FacetHit, GroupedSearchResult, SearchHit,
};
use crate::executor::telemetry::ServerTelemetry;

/// Parse a legacy REST-shaped envelope for `op` into typed IR.
///
/// Extraction matches the executor's historical normalization semantics per
/// operation: query-like ops read `result.points` (or bare arrays / `points`),
/// COUNT reads `result.count` (or `count`, default 0), FACET reads
/// `result.hits` (or `hits`), grouped queries read `result.groups` (or
/// `groups`), and everything else passes the envelope through untouched.
/// Server telemetry is extracted leniently; absent telemetry yields `None`,
/// never an error.
pub(crate) fn parse_backend_response(
    op: &PlannedOperation,
    envelope: serde_json::Value,
) -> Result<BackendResponse, QqlError> {
    let telemetry = ServerTelemetry::from_envelope_opt(&envelope);
    let data = match op {
        PlannedOperation::Query { .. }
        | PlannedOperation::Scroll { .. }
        | PlannedOperation::GetPoints { .. } => ExecData::Hits(extract_search_hits(&envelope)),
        PlannedOperation::QueryGroups { .. } => ExecData::Groups(extract_groups(&envelope)?),
        PlannedOperation::Count { .. } => {
            let count = envelope
                .get("result")
                .and_then(|result| result.get("count"))
                .and_then(serde_json::Value::as_u64)
                .or_else(|| envelope.get("count").and_then(serde_json::Value::as_u64))
                .unwrap_or(0);
            ExecData::Count(count)
        }
        PlannedOperation::Facet { .. } => {
            let hits = envelope
                .get("result")
                .and_then(|result| result.get("hits"))
                .cloned()
                .or_else(|| envelope.get("hits").cloned())
                .unwrap_or_else(|| serde_json::json!([]));
            let entries = hits.as_array().cloned().unwrap_or_default();
            let facet = entries
                .into_iter()
                .map(|entry| FacetHit {
                    value: entry
                        .get("value")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null),
                    count: entry
                        .get("count")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(0),
                })
                .collect();
            ExecData::Facet(facet)
        }
        _ => ExecData::Raw(envelope),
    };
    Ok(BackendResponse { data, telemetry })
}

/// Parse one `/points/query/batch` item (bare `{"points": […]}` per the
/// OpenAPI `QueryResponse` shape) into a typed hits response.
#[cfg(feature = "rest")]
pub(crate) fn parse_query_batch_item(item: &serde_json::Value) -> BackendResponse {
    BackendResponse {
        data: ExecData::Hits(extract_search_hits(item)),
        telemetry: ServerTelemetry::from_envelope_opt(item),
    }
}

/// Extract `result.groups` (or bare `groups`) into typed group results.
fn extract_groups(envelope: &serde_json::Value) -> Result<Vec<GroupedSearchResult>, QqlError> {
    let groups = envelope
        .get("result")
        .and_then(|result| result.get("groups"))
        .or_else(|| envelope.get("groups"))
        .cloned()
        .unwrap_or_else(|| serde_json::json!([]));
    serde_json::from_value(groups).map_err(|error| {
        QqlError::backend(
            "QQL-BACKEND-JSON",
            format!("failed to parse grouped query response: {error}"),
            None,
        )
    })
}

/// Extract search hits from a REST-shaped query/scroll/get-points response.
pub(crate) fn extract_search_hits(result: &serde_json::Value) -> Vec<SearchHit> {
    let points = result
        .get("result")
        .and_then(|r| r.get("points"))
        .and_then(serde_json::Value::as_array)
        // `/points/query/batch` answers one QueryResponse per search, and per
        // the OpenAPI `QueryResponse` schema each item carries the points at
        // its TOP LEVEL: `{"points": [...]}` — no `result` wrapper. Without
        // this branch every same-collection QUERY batch silently reports
        // 0 hits.
        .or_else(|| result.get("points").and_then(serde_json::Value::as_array))
        // `POST /collections/{c}/points` (get points by ID) returns `result`
        // as a bare array of point records.
        .or_else(|| result.get("result").and_then(serde_json::Value::as_array));

    match points {
        Some(pts) => pts
            .iter()
            .map(|hit| SearchHit {
                id: hit
                    .get("id")
                    .map(|id| match id {
                        serde_json::Value::Number(n) => {
                            if let Some(u) = n.as_u64() {
                                PlanPointId::Number(u)
                            } else {
                                PlanPointId::String(n.to_string())
                            }
                        }
                        serde_json::Value::String(s) => PlanPointId::String(s.clone()),
                        other => PlanPointId::String(other.to_string()),
                    })
                    .unwrap_or_else(|| PlanPointId::String("<missing-id>".to_string())),
                score: hit
                    .get("score")
                    .and_then(|v| match v {
                        serde_json::Value::Number(n) => n.as_f64(),
                        serde_json::Value::String(s) => s.parse::<f64>().ok(),
                        _ => None,
                    })
                    .unwrap_or(0.0) as f32,
                text: hit
                    .get("payload")
                    .and_then(|p| p.get("text"))
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned),
                payload: hit.get("payload").and_then(|p| {
                    p.as_object()
                        .map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                }),
                collection: None,
                vector: hit
                    .get("vector")
                    .cloned()
                    .or_else(|| hit.get("vectors").cloned()),
            })
            .collect(),
        None => Vec::new(),
    }
}
