//! Read execution: query / groups / get / scroll / count (+ query batch).

use qql_core::error::QqlError;

use crate::grpc::GrpcQdrant;
use crate::qdrant_grpc::qdrant;

use super::common::{shard_key_selector, to_point_id};
use super::filter::to_filter_opt;
use super::query::{
    to_payload_selector, to_query_groups, to_query_points, to_scroll_points, to_vectors_selector,
};
use super::responses::{
    batch_result_to_json, get_points_envelope, groups_result_to_json, point_id_to_json,
    retrieved_point_to_json, scored_point_to_json,
};

/// Run a single query request via `Points.Query`.
pub(crate) async fn execute_query(
    client: &GrpcQdrant,
    collection: &str,
    request: &qql_plan::types::QueryRequest,
) -> Result<serde_json::Value, QqlError> {
    let grpc_req = to_query_points(request, collection)?;
    let resp = client
        .query(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("query: {e}"), None))?;
    Ok(serde_json::json!({
        "result": {
            "points": resp.result.into_iter().map(scored_point_to_json).collect::<Vec<_>>()
        },
        "status": "ok",
        "time": resp.time,
    }))
}

/// Run a grouped query via `Points.QueryGroups`.
pub(crate) async fn execute_query_groups(
    client: &GrpcQdrant,
    collection: &str,
    request: &qql_plan::types::QueryGroupsRequest,
) -> Result<serde_json::Value, QqlError> {
    let grpc_req = to_query_groups(request, collection)?;
    let resp = client
        .query_groups(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("query_groups: {e}"), None))?;
    Ok(serde_json::json!({
        "result": groups_result_to_json(resp.result.ok_or_else(|| QqlError::backend(
            "QQL-GRPC", "missing groups result", None,
        ))?),
        "status": "ok",
        "time": resp.time,
    }))
}

/// Fetch points by ID via `Points.Get`.
pub(crate) async fn execute_get_points(
    client: &GrpcQdrant,
    collection: &str,
    request: &qql_plan::types::PointsRequest,
) -> Result<serde_json::Value, QqlError> {
    let grpc_req = qdrant::GetPoints {
        collection_name: collection.to_owned(),
        ids: request.ids.iter().map(to_point_id).collect(),
        with_payload: request.with_payload.as_ref().map(to_payload_selector),
        with_vectors: request.with_vector.as_ref().map(to_vectors_selector),
        shard_key_selector: shard_key_selector(&request.shard_key),
        ..Default::default()
    };
    let resp = client
        .get_points(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("get_points: {e}"), None))?;
    Ok(get_points_envelope(
        resp.result
            .into_iter()
            .map(retrieved_point_to_json)
            .collect(),
        resp.time,
    ))
}

/// Paginate points via `Points.Scroll`.
pub(crate) async fn execute_scroll(
    client: &GrpcQdrant,
    collection: &str,
    request: &qql_plan::types::ScrollRequest,
) -> Result<serde_json::Value, QqlError> {
    let grpc_req = to_scroll_points(request, collection)?;
    let resp = client
        .scroll(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("scroll: {e}"), None))?;
    let mut obj = serde_json::Map::new();
    obj.insert("status".into(), serde_json::json!("ok"));
    obj.insert("time".into(), serde_json::json!(resp.time));
    obj.insert(
        "result".into(),
        serde_json::json!({
            "points": resp.result.into_iter().map(retrieved_point_to_json).collect::<Vec<_>>()
        }),
    );
    if let Some(offset) = resp.next_page_offset {
        obj.insert("next_page_offset".into(), point_id_to_json(&offset));
    }
    Ok(serde_json::Value::Object(obj))
}

/// Count points matching a filter via `Points.Count`.
pub(crate) async fn execute_count(
    client: &GrpcQdrant,
    collection: &str,
    request: &qql_plan::types::CountRequest,
) -> Result<serde_json::Value, QqlError> {
    let grpc_req = qdrant::CountPoints {
        collection_name: collection.to_owned(),
        filter: to_filter_opt(request.filter.as_ref())?,
        exact: Some(true),
        shard_key_selector: shard_key_selector(&request.shard_key),
        ..Default::default()
    };
    let resp = client
        .count_points(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("count: {e}"), None))?;
    Ok(serde_json::json!({
        "result": { "count": resp.result.unwrap_or_default().count },
        "status": "ok",
        "time": resp.time,
    }))
}

/// Convert a batch of QueryRequests and send them via gRPC `QueryBatch`.
pub async fn execute_query_batch_grpc(
    client: &GrpcQdrant,
    collection: &str,
    batch: &qql_plan::QueryBatchRequest,
) -> Result<Vec<serde_json::Value>, QqlError> {
    let query_points: Result<Vec<_>, _> = batch
        .searches
        .iter()
        .map(|req| to_query_points(req, collection))
        .collect();
    let query_points = query_points?;

    let grpc_req = qdrant::QueryBatchPoints {
        collection_name: collection.to_string(),
        query_points,
        ..Default::default()
    };

    let resp = client
        .query_batch(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("query_batch: {e}"), None))?;

    Ok(resp.result.into_iter().map(batch_result_to_json).collect())
}
