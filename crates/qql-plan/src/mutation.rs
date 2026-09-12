use crate::filter::{point_id_req_typed, top_level_filter, value_to_json};
use crate::semantic::PlanShardKey;
use crate::types::*;
use qql_core::ast::{
    ClearPayloadStmt, DeletePayloadStmt, DeleteStmt, DeleteVectorStmt, PointEntry, PointSelector,
    UpdatePayloadStmt, UpdateVectorStmt, UpsertPoint, UpsertStmt,
};

/// Lower `UPSERT INTO` to the `PUT /collections/{c}/points` request body.
///
/// Whole-point placeholders (`VALUES :p` / `VALUES ?`) are skipped: they
/// splice in at execution time. Template planning therefore yields the inline
/// points only; executors must route point-param templates through the
/// point-splice path, never dispatch a template plan directly.
pub fn lower_upsert_request(stmt: &UpsertStmt) -> UpsertRequest {
    UpsertRequest {
        points: stmt
            .points
            .iter()
            .filter_map(|point| match point {
                PointEntry::Inline(inline) => Some(lower_upsert_point(inline)),
                PointEntry::Param(..) | PointEntry::PositionalParam(..) => None,
            })
            .collect(),
        shard_key: stmt.shard_key.as_ref().map(PlanShardKey::from),
    }
}

fn lower_upsert_point(point: &UpsertPoint) -> UpsertPointRequest {
    let mut req = UpsertPointRequest {
        id: PlanPointId::from(&point.id),
        vector: None,
        payload: None,
    };
    if let Some(ref vectors) = point.vectors {
        req.vector = Some(PlanPointVectors::from(vectors));
    }
    if !point.payload.is_empty() {
        let mut payload = serde_json::Map::with_capacity(point.payload.len());
        for (key, value) in &point.payload {
            payload.insert(key.clone(), value_to_json(value));
        }
        req.payload = Some(payload);
    }
    req
}

/// Lower `DELETE` to the `POST /points/delete` body, by IDs or filter.
pub fn lower_delete_request(stmt: &DeleteStmt) -> DeleteRequest {
    match &stmt.selector {
        PointSelector::Id(id) => DeleteRequest {
            points: Some(vec![point_id_req_typed(id)]),
            filter: None,
            shard_key: stmt.shard_key.as_ref().map(PlanShardKey::from),
        },
        PointSelector::Ids(ids) => DeleteRequest {
            points: Some(ids.iter().map(point_id_req_typed).collect()),
            filter: None,
            shard_key: stmt.shard_key.as_ref().map(PlanShardKey::from),
        },
        PointSelector::Filter(filter) => DeleteRequest {
            points: None,
            filter: Some(top_level_filter(filter)),
            shard_key: stmt.shard_key.as_ref().map(PlanShardKey::from),
        },
    }
}

/// Lower `UPDATE … SET VECTOR` to the `PUT /points/vectors` request body.
///
/// One QQL statement maps to one wire request with `points` filled from the
/// AST list — compact `WHERE id =` and `VALUES` both land here.
pub fn lower_update_vector_request(stmt: &UpdateVectorStmt) -> UpdateVectorRequest {
    UpdateVectorRequest {
        points: stmt
            .points
            .iter()
            .map(|point| UpdateVectorPoint {
                id: PlanPointId::from(&point.id),
                vector: PlanPointVectors::from(&point.vectors),
            })
            .collect(),
        shard_key: stmt.shard_key.as_ref().map(PlanShardKey::from),
    }
}

/// Lower `UPDATE … SET PAYLOAD` to the `POST /points/payload` request body.
pub fn lower_update_payload_request(stmt: &UpdatePayloadStmt) -> UpdatePayloadRequest {
    let mut payload = serde_json::Map::with_capacity(stmt.payload.len());
    for (key, value) in &stmt.payload {
        payload.insert(key.clone(), value_to_json(value));
    }
    match &stmt.selector {
        PointSelector::Id(id) => UpdatePayloadRequest {
            points: Some(vec![point_id_req_typed(id)]),
            filter: None,
            payload,
            shard_key: stmt.shard_key.as_ref().map(PlanShardKey::from),
        },
        PointSelector::Ids(ids) => UpdatePayloadRequest {
            points: Some(ids.iter().map(point_id_req_typed).collect()),
            filter: None,
            payload,
            shard_key: stmt.shard_key.as_ref().map(PlanShardKey::from),
        },
        PointSelector::Filter(filter) => UpdatePayloadRequest {
            points: None,
            filter: Some(top_level_filter(filter)),
            payload,
            shard_key: stmt.shard_key.as_ref().map(PlanShardKey::from),
        },
    }
}

/// Lower `CLEAR PAYLOAD` to the `POST /points/payload/clear` request body.
pub fn lower_clear_payload_request(stmt: &ClearPayloadStmt) -> ClearPayloadRequest {
    match &stmt.selector {
        PointSelector::Id(id) => ClearPayloadRequest {
            points: Some(vec![point_id_req_typed(id)]),
            filter: None,
            shard_key: stmt.shard_key.as_ref().map(PlanShardKey::from),
        },
        PointSelector::Ids(ids) => ClearPayloadRequest {
            points: Some(ids.iter().map(point_id_req_typed).collect()),
            filter: None,
            shard_key: stmt.shard_key.as_ref().map(PlanShardKey::from),
        },
        PointSelector::Filter(filter) => ClearPayloadRequest {
            points: None,
            filter: Some(top_level_filter(filter)),
            shard_key: stmt.shard_key.as_ref().map(PlanShardKey::from),
        },
    }
}

/// Lower `DELETE PAYLOAD <keys>` to the `POST /points/payload/delete` body.
pub fn lower_delete_payload_request(stmt: &DeletePayloadStmt) -> DeletePayloadRequest {
    match &stmt.selector {
        PointSelector::Id(id) => DeletePayloadRequest {
            keys: stmt.keys.clone(),
            points: Some(vec![point_id_req_typed(id)]),
            filter: None,
            shard_key: stmt.shard_key.as_ref().map(PlanShardKey::from),
        },
        PointSelector::Ids(ids) => DeletePayloadRequest {
            keys: stmt.keys.clone(),
            points: Some(ids.iter().map(point_id_req_typed).collect()),
            filter: None,
            shard_key: stmt.shard_key.as_ref().map(PlanShardKey::from),
        },
        PointSelector::Filter(filter) => DeletePayloadRequest {
            keys: stmt.keys.clone(),
            points: None,
            filter: Some(top_level_filter(filter)),
            shard_key: stmt.shard_key.as_ref().map(PlanShardKey::from),
        },
    }
}

/// Lower `DELETE VECTOR <names>` to the `POST /points/vectors/delete` body.
pub fn lower_delete_vector_request(stmt: &DeleteVectorStmt) -> DeleteVectorRequest {
    match &stmt.selector {
        PointSelector::Id(id) => DeleteVectorRequest {
            points: Some(vec![point_id_req_typed(id)]),
            filter: None,
            vector: stmt.vector_names.clone(),
            shard_key: stmt.shard_key.as_ref().map(PlanShardKey::from),
        },
        PointSelector::Ids(ids) => DeleteVectorRequest {
            points: Some(ids.iter().map(point_id_req_typed).collect()),
            filter: None,
            vector: stmt.vector_names.clone(),
            shard_key: stmt.shard_key.as_ref().map(PlanShardKey::from),
        },
        PointSelector::Filter(filter) => DeleteVectorRequest {
            points: None,
            filter: Some(top_level_filter(filter)),
            vector: stmt.vector_names.clone(),
            shard_key: stmt.shard_key.as_ref().map(PlanShardKey::from),
        },
    }
}

/// Increment a 128-bit UUID point ID string by 1 to implement exclusive
/// cursor pagination (`id > after`), matching integer `AFTER n -> offset n+1`.
fn increment_uuid_point_id(s: &str) -> Option<String> {
    let clean: String = s.chars().filter(|c| *c != '-').collect();
    if clean.len() == 32 {
        let val = u128::from_str_radix(&clean, 16).ok()?;
        let next = val.saturating_add(1);
        Some(format!(
            "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
            (next >> 96) as u32,
            ((next >> 80) & 0xffff) as u16,
            ((next >> 64) & 0xffff) as u16,
            ((next >> 48) & 0xffff) as u16,
            (next & 0xffff_ffff_ffff) as u64
        ))
    } else {
        None
    }
}

/// Lower `SCROLL` to the `/points/scroll` body: payload on, vectors off.
pub fn lower_scroll_request(
    limit: u64,
    filter: Option<&qql_core::ast::FilterExpr>,
    after: Option<&qql_core::ast::PointId>,
    shard_key: Option<qql_core::ast::ShardKey>,
    with_vector: Option<&qql_core::ast::VectorSelector>,
) -> ScrollRequest {
    let with_vector = match with_vector {
        Some(qql_core::ast::VectorSelector::All) => Some(VectorSelectorReq::All(true)),
        Some(qql_core::ast::VectorSelector::None) => Some(VectorSelectorReq::All(false)),
        Some(qql_core::ast::VectorSelector::Names(names)) => {
            Some(VectorSelectorReq::Names(names.clone()))
        }
        None => Some(VectorSelectorReq::All(false)),
    };
    ScrollRequest {
        filter: filter.map(top_level_filter),
        offset: after.map(|id| match id {
            qql_core::ast::PointId::Number(n) => PlanPointId::Number(n.saturating_add(1)),
            qql_core::ast::PointId::String(s) => {
                if let Some(next) = increment_uuid_point_id(s) {
                    PlanPointId::String(next)
                } else {
                    PlanPointId::String(s.clone())
                }
            }
            other => point_id_req_typed(other),
        }),
        limit: Some(limit),
        with_payload: Some(PayloadSelectorReq::All(true)),
        with_vector,
        order_by: None,
        shard_key: shard_key.as_ref().map(PlanShardKey::from),
    }
}

/// Lower a planned mutation into a wire `UpdateOperation` for batching.
pub fn planned_to_update_operation(
    op: &crate::plan::PlannedOperation,
) -> Option<(String, UpdateOperation)> {
    use crate::plan::PlannedOperation;
    match op {
        PlannedOperation::Upsert {
            collection,
            request,
            ..
        } => Some((
            collection.clone(),
            UpdateOperation::Upsert {
                upsert: request.clone(),
            },
        )),
        PlannedOperation::Delete {
            collection,
            request,
            ..
        } => Some((
            collection.clone(),
            UpdateOperation::Delete {
                delete: request.clone(),
            },
        )),
        PlannedOperation::UpdatePayload {
            collection,
            request,
            ..
        } => Some((
            collection.clone(),
            UpdateOperation::SetPayload {
                set_payload: request.clone(),
            },
        )),
        PlannedOperation::ClearPayload {
            collection,
            request,
            ..
        } => Some((
            collection.clone(),
            UpdateOperation::ClearPayload {
                clear_payload: request.clone(),
            },
        )),
        PlannedOperation::DeletePayload {
            collection,
            request,
            ..
        } => Some((
            collection.clone(),
            UpdateOperation::DeletePayload {
                delete_payload: request.clone(),
            },
        )),
        PlannedOperation::UpdateVectors {
            collection,
            request,
            ..
        } => Some((
            collection.clone(),
            UpdateOperation::UpdateVectors {
                update_vectors: request.clone(),
            },
        )),
        PlannedOperation::DeleteVectors {
            collection,
            request,
            ..
        } => Some((
            collection.clone(),
            UpdateOperation::DeleteVectors {
                delete_vectors: request.clone(),
            },
        )),
        _ => None,
    }
}
