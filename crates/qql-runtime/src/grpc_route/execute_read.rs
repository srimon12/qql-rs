//! Read execution: query / groups / get / scroll / count / facet (+ query batch).
//!
//! Every read converts its protobuf response straight into a typed
//! [`BackendResponse`] (see [`super::typed`]) — no REST-shaped JSON envelope
//! and no envelope parser. Two wire fields have no typed IR representation and
//! are dropped with a documented rationale: `next_page_offset` (callers derive
//! the cursor from the last hit id) and grouped `lookup` points.

use qql_core::error::QqlError;

use crate::executor::response::{BackendResponse, ExecData};
use crate::grpc::GrpcQdrant;
use crate::qdrant_grpc::qdrant;

use super::query::{
    to_count_points, to_facet_counts, to_get_points, to_query_groups, to_query_points,
    to_read_consistency, to_scroll_points,
};
use super::typed::{
    facet_hit_to_typed, point_group_to_typed, retrieved_point_to_hit, scored_point_to_hit,
    telemetry_from_proto,
};

/// Run a single query request via `Points.Query`.
pub(crate) async fn execute_query(
    client: &GrpcQdrant,
    collection: &str,
    request: &qql_plan::types::QueryRequest,
) -> Result<BackendResponse, QqlError> {
    let grpc_req = to_query_points(request, collection)?;
    let resp = client
        .query(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("query: {e}"), None))?;
    let hits = resp
        .result
        .into_iter()
        .map(scored_point_to_hit)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(BackendResponse {
        data: ExecData::Hits(hits),
        telemetry: telemetry_from_proto(resp.time, resp.usage.as_ref()),
    })
}

/// Run a grouped query via `Points.QueryGroups`.
///
/// Grouped `lookup` points are not modelled by [`ExecData::Groups`] and are
/// dropped (the REST strict parser drops them too).
pub(crate) async fn execute_query_groups(
    client: &GrpcQdrant,
    collection: &str,
    request: &qql_plan::types::QueryGroupsRequest,
) -> Result<BackendResponse, QqlError> {
    let grpc_req = to_query_groups(request, collection)?;
    let resp = client
        .query_groups(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("query_groups: {e}"), None))?;
    let result = resp
        .result
        .ok_or_else(|| QqlError::backend("QQL-GRPC", "missing groups result", None))?;
    Ok(BackendResponse {
        data: ExecData::Groups(
            result
                .groups
                .into_iter()
                .map(point_group_to_typed)
                .collect::<Result<Vec<_>, _>>()?,
        ),
        telemetry: telemetry_from_proto(resp.time, resp.usage.as_ref()),
    })
}

/// Fetch points by ID via `Points.Get`.
pub(crate) async fn execute_get_points(
    client: &GrpcQdrant,
    collection: &str,
    request: &qql_plan::types::PointsRequest,
) -> Result<BackendResponse, QqlError> {
    let grpc_req = to_get_points(request, collection);
    let resp = client
        .get_points(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("get_points: {e}"), None))?;
    let hits = resp
        .result
        .into_iter()
        .map(retrieved_point_to_hit)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(BackendResponse {
        data: ExecData::Hits(hits),
        telemetry: telemetry_from_proto(resp.time, resp.usage.as_ref()),
    })
}

/// Paginate points via `Points.Scroll`.
///
/// `next_page_offset` has no representation in [`ExecData::Hits`] and is
/// dropped — callers that need it (e.g. the CLI dump cursor) derive the next
/// cursor from the last returned point id.
pub(crate) async fn execute_scroll(
    client: &GrpcQdrant,
    collection: &str,
    request: &qql_plan::types::ScrollRequest,
) -> Result<BackendResponse, QqlError> {
    let grpc_req = to_scroll_points(request, collection)?;
    let resp = client
        .scroll(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("scroll: {e}"), None))?;
    let hits = resp
        .result
        .into_iter()
        .map(retrieved_point_to_hit)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(BackendResponse {
        data: ExecData::Hits(hits),
        telemetry: telemetry_from_proto(resp.time, resp.usage.as_ref()),
    })
}

/// Count points matching a filter via `Points.Count`.
pub(crate) async fn execute_count(
    client: &GrpcQdrant,
    collection: &str,
    request: &qql_plan::types::CountRequest,
) -> Result<BackendResponse, QqlError> {
    let grpc_req = to_count_points(request, collection)?;
    let resp = client
        .count_points(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("count: {e}"), None))?;
    let count = resp
        .result
        .ok_or_else(|| {
            QqlError::backend(
                "QQL-BACKEND-ENVELOPE",
                "count response is missing its result",
                None,
            )
        })?
        .count;
    Ok(BackendResponse {
        data: ExecData::Count(count),
        telemetry: telemetry_from_proto(resp.time, resp.usage.as_ref()),
    })
}

/// Compute facet counts via `Points.Facet`.
pub(crate) async fn execute_facet(
    client: &GrpcQdrant,
    collection: &str,
    request: &qql_plan::types::FacetRequest,
) -> Result<BackendResponse, QqlError> {
    let grpc_req = to_facet_counts(request, collection)?;
    let resp = client
        .facet(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("facet: {e}"), None))?;
    let hits = resp
        .hits
        .into_iter()
        .map(facet_hit_to_typed)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(BackendResponse {
        data: ExecData::Facet(hits),
        telemetry: telemetry_from_proto(resp.time, resp.usage.as_ref()),
    })
}

/// Convert a batch of QueryRequests and send them via gRPC `QueryBatch`.
///
/// Each proto `BatchResult` converts straight into typed hits. The proto batch
/// response carries one `time`/`usage` pair for the whole round trip, not per
/// item, so per-item telemetry stays `None` — attributing the shared total to
/// every item would multiply it in report aggregation (parity with REST batch
/// items, which only report telemetry when the server sends it per item).
pub async fn execute_query_batch_grpc(
    client: &GrpcQdrant,
    collection: &str,
    batch: &qql_plan::QueryBatchRequest,
    timeout: Option<u64>,
    consistency: Option<qql_plan::types::ReadConsistencyParam>,
) -> Result<Vec<BackendResponse>, QqlError> {
    let query_points: Result<Vec<_>, _> = batch
        .searches
        .iter()
        .map(|req| to_query_points(req, collection))
        .collect();
    let query_points = query_points?;

    let grpc_req = qdrant::QueryBatchPoints {
        collection_name: collection.to_string(),
        query_points,
        read_consistency: consistency.as_ref().map(to_read_consistency),
        timeout,
    };

    let resp = client
        .query_batch(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("query_batch: {e}"), None))?;

    resp.result
        .into_iter()
        .map(|batch_result| {
            Ok(BackendResponse {
                data: ExecData::Hits(
                    batch_result
                        .result
                        .into_iter()
                        .map(scored_point_to_hit)
                        .collect::<Result<Vec<_>, _>>()?,
                ),
                telemetry: None,
            })
        })
        .collect()
}
