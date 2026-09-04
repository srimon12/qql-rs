//! Write execution: upserts / deletes / payload / vectors (+ update batch).

use qql_core::error::QqlError;

use crate::grpc::GrpcQdrant;
use crate::qdrant_grpc::qdrant;

use super::common::{shard_key_selector, to_point_id};
use super::query::{points_and_filter_selector, to_vectors};
use super::responses::{mutation_response_from, update_result_to_json};
use super::values::to_qdrant_value;

/// Upsert points via `Points.Upsert`.
pub(crate) async fn execute_upsert(
    client: &GrpcQdrant,
    collection: &str,
    request: &qql_plan::types::UpsertRequest,
) -> Result<serde_json::Value, QqlError> {
    let points: Vec<qdrant::PointStruct> = request
        .points
        .iter()
        .map(|p| {
            let id = to_point_id(&p.id);
            let vectors = p.vector.as_ref().and_then(to_vectors);
            let payload = p
                .payload
                .as_ref()
                .map(|pl| {
                    pl.iter()
                        .map(|(k, v)| (k.clone(), to_qdrant_value(v.clone())))
                        .collect()
                })
                .unwrap_or_default();
            qdrant::PointStruct {
                id: Some(id),
                vectors,
                payload,
            }
        })
        .collect();
    let grpc_req = qdrant::UpsertPoints {
        collection_name: collection.to_owned(),
        wait: Some(true),
        points,
        shard_key_selector: shard_key_selector(&request.shard_key),
        ..Default::default()
    };
    let resp = client
        .upsert_points(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("upsert: {e}"), None))?;
    Ok(mutation_response_from(resp))
}

/// Delete points by IDs or filter via `Points.Delete`.
pub(crate) async fn execute_delete(
    client: &GrpcQdrant,
    collection: &str,
    request: &qql_plan::types::DeleteRequest,
) -> Result<serde_json::Value, QqlError> {
    let selector = points_and_filter_selector(request.points.as_ref(), request.filter.as_ref())?;
    let grpc_req = qdrant::DeletePoints {
        collection_name: collection.to_owned(),
        wait: Some(true),
        points: selector,
        shard_key_selector: shard_key_selector(&request.shard_key),
        ..Default::default()
    };
    let resp = client
        .delete_points(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("delete: {e}"), None))?;
    Ok(mutation_response_from(resp))
}

/// Clear full payload via `Points.ClearPayload`.
pub(crate) async fn execute_clear_payload(
    client: &GrpcQdrant,
    collection: &str,
    request: &qql_plan::types::ClearPayloadRequest,
) -> Result<serde_json::Value, QqlError> {
    let selector = points_and_filter_selector(request.points.as_ref(), request.filter.as_ref())?;
    let grpc_req = qdrant::ClearPayloadPoints {
        collection_name: collection.to_owned(),
        wait: Some(true),
        points: selector,
        shard_key_selector: shard_key_selector(&request.shard_key),
        ..Default::default()
    };
    let resp = client
        .clear_payload(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("clear_payload: {e}"), None))?;
    Ok(mutation_response_from(resp))
}

/// Delete payload keys via `Points.DeletePayload`.
pub(crate) async fn execute_delete_payload(
    client: &GrpcQdrant,
    collection: &str,
    request: &qql_plan::types::DeletePayloadRequest,
) -> Result<serde_json::Value, QqlError> {
    let selector = points_and_filter_selector(request.points.as_ref(), request.filter.as_ref())?;
    let grpc_req = qdrant::DeletePayloadPoints {
        collection_name: collection.to_owned(),
        wait: Some(true),
        keys: request.keys.clone(),
        points_selector: selector,
        shard_key_selector: shard_key_selector(&request.shard_key),
        ..Default::default()
    };
    let resp = client
        .delete_payload(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("delete_payload: {e}"), None))?;
    Ok(mutation_response_from(resp))
}

/// Delete named vectors via `Points.DeleteVectors`.
pub(crate) async fn execute_delete_vectors(
    client: &GrpcQdrant,
    collection: &str,
    request: &qql_plan::types::DeleteVectorRequest,
) -> Result<serde_json::Value, QqlError> {
    let selector = points_and_filter_selector(request.points.as_ref(), request.filter.as_ref())?;
    let grpc_req = qdrant::DeletePointVectors {
        collection_name: collection.to_owned(),
        wait: Some(true),
        points_selector: selector,
        vectors: Some(qdrant::VectorsSelector {
            names: request.vector.clone(),
        }),
        shard_key_selector: shard_key_selector(&request.shard_key),
        ..Default::default()
    };
    let resp = client
        .delete_vectors(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("delete_vectors: {e}"), None))?;
    Ok(mutation_response_from(resp))
}

/// Overwrite point vectors via `Points.UpdateVectors`.
pub(crate) async fn execute_update_vectors(
    client: &GrpcQdrant,
    collection: &str,
    request: &qql_plan::types::UpdateVectorRequest,
) -> Result<serde_json::Value, QqlError> {
    let points: Vec<qdrant::PointVectors> = request
        .points
        .iter()
        .map(|p| qdrant::PointVectors {
            id: Some(to_point_id(&p.id)),
            vectors: to_vectors(&p.vector),
        })
        .collect();
    let grpc_req = qdrant::UpdatePointVectors {
        collection_name: collection.to_owned(),
        wait: Some(true),
        points,
        shard_key_selector: shard_key_selector(&request.shard_key),
        ..Default::default()
    };
    let resp = client
        .update_vectors(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("update_vectors: {e}"), None))?;
    Ok(mutation_response_from(resp))
}

/// Set payload fields via `Points.SetPayload`.
pub(crate) async fn execute_update_payload(
    client: &GrpcQdrant,
    collection: &str,
    request: &qql_plan::types::UpdatePayloadRequest,
) -> Result<serde_json::Value, QqlError> {
    let selector = points_and_filter_selector(request.points.as_ref(), request.filter.as_ref())?;
    let payload_map: std::collections::HashMap<String, qdrant::Value> = request
        .payload
        .iter()
        .map(|(k, v)| (k.clone(), to_qdrant_value(v.clone())))
        .collect();
    let grpc_req = qdrant::SetPayloadPoints {
        collection_name: collection.to_owned(),
        wait: Some(true),
        payload: payload_map,
        points_selector: selector,
        shard_key_selector: shard_key_selector(&request.shard_key),
        ..Default::default()
    };
    let resp = client
        .set_payload(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("set_payload: {e}"), None))?;
    Ok(mutation_response_from(resp))
}

/// Convert a mutation batch and send via gRPC `UpdateBatch`.
pub async fn execute_update_batch_grpc(
    client: &GrpcQdrant,
    collection: &str,
    batch: &qql_plan::UpdateBatchRequest,
) -> Result<Vec<serde_json::Value>, QqlError> {
    let operations: Vec<qdrant::PointsUpdateOperation> = batch
        .operations
        .iter()
        .map(to_points_update_operation)
        .collect::<Result<Vec<_>, QqlError>>()?;

    let grpc_req = qdrant::UpdateBatchPoints {
        collection_name: collection.to_string(),
        wait: Some(true),
        operations,
        ..Default::default()
    };

    let resp = client
        .update_batch(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("update_batch: {e}"), None))?;

    Ok(resp.result.into_iter().map(update_result_to_json).collect())
}

pub(crate) fn to_points_update_operation(
    op: &qql_plan::UpdateOperation,
) -> Result<qdrant::PointsUpdateOperation, QqlError> {
    use qdrant::points_update_operation::{self, Operation};
    use qql_plan::UpdateOperation;

    let operation = match op {
        UpdateOperation::Upsert { upsert } => {
            let points: Vec<qdrant::PointStruct> = upsert
                .points
                .iter()
                .map(|p| {
                    let payload = p
                        .payload
                        .as_ref()
                        .map(|pl| {
                            pl.iter()
                                .map(|(k, v)| (k.clone(), to_qdrant_value(v.clone())))
                                .collect()
                        })
                        .unwrap_or_default();
                    qdrant::PointStruct {
                        id: Some(to_point_id(&p.id)),
                        vectors: p.vector.as_ref().and_then(to_vectors),
                        payload,
                    }
                })
                .collect();
            let shard_key_selector = shard_key_selector(&upsert.shard_key);
            Operation::Upsert(points_update_operation::PointStructList {
                points,
                shard_key_selector,
                update_filter: None,
                update_mode: None,
            })
        }
        UpdateOperation::Delete { delete } => {
            let points =
                points_and_filter_selector(delete.points.as_ref(), delete.filter.as_ref())?;
            let shard_key_selector = shard_key_selector(&delete.shard_key);
            Operation::DeletePoints(points_update_operation::DeletePoints {
                points,
                shard_key_selector,
            })
        }
        UpdateOperation::SetPayload { set_payload } => {
            let payload_map: std::collections::HashMap<String, qdrant::Value> = set_payload
                .payload
                .iter()
                .map(|(k, v)| (k.clone(), to_qdrant_value(v.clone())))
                .collect();
            Operation::SetPayload(points_update_operation::SetPayload {
                payload: payload_map,
                points_selector: points_and_filter_selector(
                    set_payload.points.as_ref(),
                    set_payload.filter.as_ref(),
                )?,
                shard_key_selector: shard_key_selector(&set_payload.shard_key),
                key: None,
            })
        }
        UpdateOperation::ClearPayload { clear_payload } => {
            Operation::ClearPayload(points_update_operation::ClearPayload {
                points: points_and_filter_selector(
                    clear_payload.points.as_ref(),
                    clear_payload.filter.as_ref(),
                )?,
                shard_key_selector: shard_key_selector(&clear_payload.shard_key),
            })
        }
        UpdateOperation::UpdateVectors { update_vectors } => {
            let points: Vec<qdrant::PointVectors> = update_vectors
                .points
                .iter()
                .map(|p| qdrant::PointVectors {
                    id: Some(to_point_id(&p.id)),
                    vectors: to_vectors(&p.vector),
                })
                .collect();
            Operation::UpdateVectors(points_update_operation::UpdateVectors {
                points,
                shard_key_selector: shard_key_selector(&update_vectors.shard_key),
                update_filter: None,
            })
        }
        UpdateOperation::DeleteVectors { delete_vectors } => {
            Operation::DeleteVectors(points_update_operation::DeleteVectors {
                points_selector: points_and_filter_selector(
                    delete_vectors.points.as_ref(),
                    delete_vectors.filter.as_ref(),
                )?,
                vectors: Some(qdrant::VectorsSelector {
                    names: delete_vectors.vector.clone(),
                }),
                shard_key_selector: shard_key_selector(&delete_vectors.shard_key),
            })
        }
    };

    Ok(qdrant::PointsUpdateOperation {
        operation: Some(operation),
    })
}
