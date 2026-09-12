//! Write execution: upserts / deletes / payload / vectors (+ update batch).

use qql_core::error::QqlError;

use crate::executor::response::{BackendResponse, ExecData};
use crate::grpc::GrpcQdrant;
use crate::qdrant_grpc::qdrant;

use super::common::{shard_key_selector, to_point_id};
use super::filter::to_filter_opt;
use super::query::{points_and_filter_selector, to_vectors};
use super::typed::mutation_response_to_typed;
use super::values::to_qdrant_value;

/// Map a plan [`qql_plan::types::UpdateMode`] to the proto `UpdateMode` value.
fn to_update_mode(mode: qql_plan::types::UpdateMode) -> i32 {
    match mode {
        qql_plan::types::UpdateMode::Upsert => qdrant::UpdateMode::Upsert as i32,
        qql_plan::types::UpdateMode::InsertOnly => qdrant::UpdateMode::InsertOnly as i32,
        qql_plan::types::UpdateMode::UpdateOnly => qdrant::UpdateMode::UpdateOnly as i32,
    }
}

/// Upsert points via `Points.Upsert`.
pub(crate) async fn execute_upsert(
    client: &GrpcQdrant,
    collection: &str,
    request: &qql_plan::types::UpsertRequest,
    wait: bool,
) -> Result<BackendResponse, QqlError> {
    let points: Vec<qdrant::PointStruct> = request
        .points
        .iter()
        .map(|p| {
            let id = to_point_id(&p.id);
            let vectors = p.vector.as_ref().map(to_vectors).transpose()?.flatten();
            let payload = p
                .payload
                .as_ref()
                .map(|pl| {
                    pl.iter()
                        .map(|(k, v)| (k.clone(), to_qdrant_value(v.clone())))
                        .collect()
                })
                .unwrap_or_default();
            Ok(qdrant::PointStruct {
                id: Some(id),
                vectors,
                payload,
            })
        })
        .collect::<Result<Vec<_>, QqlError>>()?;
    let grpc_req = qdrant::UpsertPoints {
        collection_name: collection.to_owned(),
        wait: Some(wait),
        points,
        shard_key_selector: shard_key_selector(&request.shard_key),
        update_filter: to_filter_opt(request.update_filter.as_ref())?,
        update_mode: request.update_mode.map(to_update_mode),
        ..Default::default()
    };
    let resp = client
        .upsert_points(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("upsert: {e}"), None))?;
    Ok(mutation_response_to_typed(resp))
}

/// Delete points by IDs or filter via `Points.Delete`.
pub(crate) async fn execute_delete(
    client: &GrpcQdrant,
    collection: &str,
    request: &qql_plan::types::DeleteRequest,
    wait: bool,
) -> Result<BackendResponse, QqlError> {
    let selector = points_and_filter_selector(request.points.as_ref(), request.filter.as_ref())?;
    let grpc_req = qdrant::DeletePoints {
        collection_name: collection.to_owned(),
        wait: Some(wait),
        points: selector,
        shard_key_selector: shard_key_selector(&request.shard_key),
        ..Default::default()
    };
    let resp = client
        .delete_points(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("delete: {e}"), None))?;
    Ok(mutation_response_to_typed(resp))
}

/// Clear full payload via `Points.ClearPayload`.
pub(crate) async fn execute_clear_payload(
    client: &GrpcQdrant,
    collection: &str,
    request: &qql_plan::types::ClearPayloadRequest,
    wait: bool,
) -> Result<BackendResponse, QqlError> {
    let selector = points_and_filter_selector(request.points.as_ref(), request.filter.as_ref())?;
    let grpc_req = qdrant::ClearPayloadPoints {
        collection_name: collection.to_owned(),
        wait: Some(wait),
        points: selector,
        shard_key_selector: shard_key_selector(&request.shard_key),
        ..Default::default()
    };
    let resp = client
        .clear_payload(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("clear_payload: {e}"), None))?;
    Ok(mutation_response_to_typed(resp))
}

/// Delete payload keys via `Points.DeletePayload`.
pub(crate) async fn execute_delete_payload(
    client: &GrpcQdrant,
    collection: &str,
    request: &qql_plan::types::DeletePayloadRequest,
    wait: bool,
) -> Result<BackendResponse, QqlError> {
    let selector = points_and_filter_selector(request.points.as_ref(), request.filter.as_ref())?;
    let grpc_req = qdrant::DeletePayloadPoints {
        collection_name: collection.to_owned(),
        wait: Some(wait),
        keys: request.keys.clone(),
        points_selector: selector,
        shard_key_selector: shard_key_selector(&request.shard_key),
        ..Default::default()
    };
    let resp = client
        .delete_payload(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("delete_payload: {e}"), None))?;
    Ok(mutation_response_to_typed(resp))
}

/// Delete named vectors via `Points.DeleteVectors`.
pub(crate) async fn execute_delete_vectors(
    client: &GrpcQdrant,
    collection: &str,
    request: &qql_plan::types::DeleteVectorRequest,
    wait: bool,
) -> Result<BackendResponse, QqlError> {
    let selector = points_and_filter_selector(request.points.as_ref(), request.filter.as_ref())?;
    let grpc_req = qdrant::DeletePointVectors {
        collection_name: collection.to_owned(),
        wait: Some(wait),
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
    Ok(mutation_response_to_typed(resp))
}

/// Overwrite point vectors via `Points.UpdateVectors`.
pub(crate) async fn execute_update_vectors(
    client: &GrpcQdrant,
    collection: &str,
    request: &qql_plan::types::UpdateVectorRequest,
    wait: bool,
) -> Result<BackendResponse, QqlError> {
    let points: Vec<qdrant::PointVectors> = request
        .points
        .iter()
        .map(|p| {
            Ok(qdrant::PointVectors {
                id: Some(to_point_id(&p.id)),
                vectors: to_vectors(&p.vector)?,
            })
        })
        .collect::<Result<Vec<_>, QqlError>>()?;
    let grpc_req = qdrant::UpdatePointVectors {
        collection_name: collection.to_owned(),
        wait: Some(wait),
        points,
        shard_key_selector: shard_key_selector(&request.shard_key),
        ..Default::default()
    };
    let resp = client
        .update_vectors(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("update_vectors: {e}"), None))?;
    Ok(mutation_response_to_typed(resp))
}

/// Set payload fields via `Points.SetPayload`.
pub(crate) async fn execute_update_payload(
    client: &GrpcQdrant,
    collection: &str,
    request: &qql_plan::types::UpdatePayloadRequest,
    wait: bool,
) -> Result<BackendResponse, QqlError> {
    let selector = points_and_filter_selector(request.points.as_ref(), request.filter.as_ref())?;
    let payload_map: std::collections::HashMap<String, qdrant::Value> = request
        .payload
        .iter()
        .map(|(k, v)| (k.clone(), to_qdrant_value(v.clone())))
        .collect();
    let grpc_req = qdrant::SetPayloadPoints {
        collection_name: collection.to_owned(),
        wait: Some(wait),
        payload: payload_map,
        points_selector: selector,
        shard_key_selector: shard_key_selector(&request.shard_key),
        key: request.key.clone(),
        ..Default::default()
    };
    let resp = client
        .set_payload(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("set_payload: {e}"), None))?;
    Ok(mutation_response_to_typed(resp))
}

/// Replace the full payload via `Points.OverwritePayload`.
pub(crate) async fn execute_overwrite_payload(
    client: &GrpcQdrant,
    collection: &str,
    request: &qql_plan::types::UpdatePayloadRequest,
    wait: bool,
) -> Result<BackendResponse, QqlError> {
    let selector = points_and_filter_selector(request.points.as_ref(), request.filter.as_ref())?;
    let payload_map: std::collections::HashMap<String, qdrant::Value> = request
        .payload
        .iter()
        .map(|(k, v)| (k.clone(), to_qdrant_value(v.clone())))
        .collect();
    let grpc_req = qdrant::SetPayloadPoints {
        collection_name: collection.to_owned(),
        wait: Some(wait),
        payload: payload_map,
        points_selector: selector,
        shard_key_selector: shard_key_selector(&request.shard_key),
        key: request.key.clone(),
        ..Default::default()
    };
    let resp = client
        .overwrite_payload(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("overwrite_payload: {e}"), None))?;
    Ok(mutation_response_to_typed(resp))
}

/// Convert a mutation batch and send via gRPC `UpdateBatch`. Per-item update
/// results carry only status, so each item is typed as a status-only
/// [`ExecData::Mutation`]; the executor derives upsert counts from the
/// request. The proto batch response carries one `time`/`usage` pair for the
/// whole round trip, not per item, so per-item telemetry stays `None` (adding
/// the shared total to every item would multiply it in report aggregation).
pub async fn execute_update_batch_grpc(
    client: &GrpcQdrant,
    collection: &str,
    batch: &qql_plan::UpdateBatchRequest,
    wait: bool,
) -> Result<Vec<BackendResponse>, QqlError> {
    let operations: Vec<qdrant::PointsUpdateOperation> = batch
        .operations
        .iter()
        .map(to_points_update_operation)
        .collect::<Result<Vec<_>, QqlError>>()?;

    let grpc_req = qdrant::UpdateBatchPoints {
        collection_name: collection.to_string(),
        wait: Some(wait),
        operations,
        ..Default::default()
    };

    let resp = client
        .update_batch(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("update_batch: {e}"), None))?;

    Ok(resp
        .result
        .into_iter()
        .map(|_| BackendResponse {
            data: ExecData::Mutation { affected: None },
            telemetry: None,
        })
        .collect())
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
                    Ok(qdrant::PointStruct {
                        id: Some(to_point_id(&p.id)),
                        vectors: p.vector.as_ref().map(to_vectors).transpose()?.flatten(),
                        payload,
                    })
                })
                .collect::<Result<Vec<_>, QqlError>>()?;
            let shard_key_selector = shard_key_selector(&upsert.shard_key);
            Operation::Upsert(points_update_operation::PointStructList {
                points,
                shard_key_selector,
                update_filter: super::filter::to_filter_opt(upsert.update_filter.as_ref())?,
                update_mode: upsert.update_mode.map(to_update_mode),
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
                key: set_payload.key.clone(),
            })
        }
        UpdateOperation::Overwrite { overwrite_payload } => {
            let payload_map: std::collections::HashMap<String, qdrant::Value> = overwrite_payload
                .payload
                .iter()
                .map(|(k, v)| (k.clone(), to_qdrant_value(v.clone())))
                .collect();
            Operation::OverwritePayload(points_update_operation::OverwritePayload {
                payload: payload_map,
                points_selector: points_and_filter_selector(
                    overwrite_payload.points.as_ref(),
                    overwrite_payload.filter.as_ref(),
                )?,
                shard_key_selector: shard_key_selector(&overwrite_payload.shard_key),
                key: overwrite_payload.key.clone(),
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
        UpdateOperation::DeletePayload { delete_payload } => {
            Operation::DeletePayload(points_update_operation::DeletePayload {
                keys: delete_payload.keys.clone(),
                points_selector: points_and_filter_selector(
                    delete_payload.points.as_ref(),
                    delete_payload.filter.as_ref(),
                )?,
                shard_key_selector: shard_key_selector(&delete_payload.shard_key),
            })
        }
        UpdateOperation::UpdateVectors { update_vectors } => {
            let points: Vec<qdrant::PointVectors> = update_vectors
                .points
                .iter()
                .map(|p| {
                    Ok(qdrant::PointVectors {
                        id: Some(to_point_id(&p.id)),
                        vectors: to_vectors(&p.vector)?,
                    })
                })
                .collect::<Result<Vec<_>, QqlError>>()?;
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

#[cfg(test)]
mod mutation_gap_tests {
    use super::to_points_update_operation;
    use qql_core::parser::Parser;
    use qql_plan::{UpdateOperation, plan};

    fn planned_update_op(source: &str) -> (String, UpdateOperation) {
        let stmt = Parser::parse(source).expect("parse");
        let op = plan(&stmt).expect("plan");
        qql_plan::mutation::planned_to_update_operation(&op).expect("update operation")
    }

    #[test]
    fn upsert_guards_map_to_proto() {
        let (collection, update) = planned_update_op(
            "UPSERT INTO docs VALUES {id: 1, vector: [0.1]} UPDATE FILTER status = 'active' UPDATE MODE insert_only;",
        );
        assert_eq!(collection, "docs");
        let proto = to_points_update_operation(&update).expect("proto");
        match proto.operation.expect("operation") {
            crate::qdrant_grpc::qdrant::points_update_operation::Operation::Upsert(list) => {
                assert!(list.update_filter.is_some(), "update_filter must map");
                assert_eq!(
                    list.update_mode,
                    Some(crate::qdrant_grpc::qdrant::UpdateMode::InsertOnly as i32)
                );
            }
            other => panic!("expected Upsert, got {other:?}"),
        }
    }

    #[test]
    fn upsert_defaults_leave_proto_guards_empty() {
        let (_, update) = planned_update_op("UPSERT INTO docs VALUES {id: 1, vector: [0.1]};");
        let proto = to_points_update_operation(&update).expect("proto");
        match proto.operation.expect("operation") {
            crate::qdrant_grpc::qdrant::points_update_operation::Operation::Upsert(list) => {
                assert!(list.update_filter.is_none());
                assert!(list.update_mode.is_none());
            }
            other => panic!("expected Upsert, got {other:?}"),
        }
    }

    #[test]
    fn set_payload_key_maps_to_proto() {
        let (_, update) =
            planned_update_op("UPDATE docs SET PAYLOAD = {a: 1} KEY 'a.b' WHERE id = 1;");
        let proto = to_points_update_operation(&update).expect("proto");
        match proto.operation.expect("operation") {
            crate::qdrant_grpc::qdrant::points_update_operation::Operation::SetPayload(set) => {
                assert_eq!(set.key.as_deref(), Some("a.b"));
            }
            other => panic!("expected SetPayload, got {other:?}"),
        }
    }

    #[test]
    fn overwrite_maps_to_proto_overwrite_variant() {
        let (_, update) =
            planned_update_op("UPDATE docs SET PAYLOAD = {a: 1} KEY 'a.b' OVERWRITE WHERE id = 1;");
        // Single-route REST projection stays fail-closed for overwrite.
        let stmt =
            Parser::parse("UPDATE docs SET PAYLOAD = {a: 1} OVERWRITE WHERE id = 1;").unwrap();
        let op = plan(&stmt).unwrap();
        assert!(qql_plan::to_rest_route(&op).is_err());
        // The batch op maps to the proto overwrite variant (not set_payload).
        let proto = to_points_update_operation(&update).expect("proto");
        match proto.operation.expect("operation") {
            crate::qdrant_grpc::qdrant::points_update_operation::Operation::OverwritePayload(
                overwrite,
            ) => {
                assert_eq!(overwrite.key.as_deref(), Some("a.b"));
            }
            other => panic!("expected OverwritePayload, got {other:?}"),
        }
    }
}
