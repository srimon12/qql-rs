use crate::filter::{point_id_req_typed, top_level_filter, value_to_json};
use crate::query::lower_vector_value;
use crate::types::*;
use qql_core::ast::{
    ClearPayloadStmt, DeleteStmt, DeleteVectorStmt, EmbeddingSpec, PointSelector, PointVectors,
    Stmt, UpdatePayloadStmt, UpdateVectorStmt, UpsertPoint, UpsertStmt,
};

pub fn lower_upsert_request(stmt: &UpsertStmt) -> UpsertRequest {
    UpsertRequest {
        points: stmt.points.iter().map(lower_upsert_point).collect(),
        shard_key: stmt.shard_key.clone(),
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
        let mut payload = serde_json::Map::new();
        for (key, value) in &point.payload {
            payload.insert(key.clone(), value_to_json(value));
        }
        req.payload = Some(payload);
    }
    req
}

pub fn lower_point_vectors(vectors: &PointVectors) -> PlanPointVectors {
    PlanPointVectors::from(vectors)
}

pub fn lower_delete_request(stmt: &DeleteStmt) -> DeleteRequest {
    match &stmt.selector {
        PointSelector::Id(id) => DeleteRequest {
            points: Some(vec![point_id_req_typed(id)]),
            filter: None,
            shard_key: stmt.shard_key.clone(),
        },
        PointSelector::Ids(ids) => DeleteRequest {
            points: Some(ids.iter().map(point_id_req_typed).collect()),
            filter: None,
            shard_key: stmt.shard_key.clone(),
        },
        PointSelector::Filter(filter) => DeleteRequest {
            points: None,
            filter: Some(top_level_filter(filter)),
            shard_key: stmt.shard_key.clone(),
        },
    }
}

pub fn lower_update_vector_request(stmt: &UpdateVectorStmt) -> UpdateVectorRequest {
    let vector = if let Some(ref name) = stmt.vector_name {
        PlanPointVectors::Named(vec![(name.clone(), lower_vector_value(&stmt.vector))])
    } else {
        PlanPointVectors::Unnamed(lower_vector_value(&stmt.vector))
    };
    UpdateVectorRequest {
        points: vec![UpdateVectorPoint {
            id: PlanPointId::from(&stmt.point_id),
            vector,
        }],
    }
}

pub fn lower_update_payload_request(stmt: &UpdatePayloadStmt) -> UpdatePayloadRequest {
    let mut payload = serde_json::Map::new();
    for (key, value) in &stmt.payload {
        payload.insert(key.clone(), value_to_json(value));
    }
    match &stmt.selector {
        PointSelector::Id(id) => UpdatePayloadRequest {
            points: Some(vec![point_id_req_typed(id)]),
            filter: None,
            payload,
        },
        PointSelector::Ids(ids) => UpdatePayloadRequest {
            points: Some(ids.iter().map(point_id_req_typed).collect()),
            filter: None,
            payload,
        },
        PointSelector::Filter(filter) => UpdatePayloadRequest {
            points: None,
            filter: Some(top_level_filter(filter)),
            payload,
        },
    }
}

pub fn lower_clear_payload_request(stmt: &ClearPayloadStmt) -> ClearPayloadRequest {
    match &stmt.selector {
        PointSelector::Id(id) => ClearPayloadRequest {
            points: Some(vec![point_id_req_typed(id)]),
            filter: None,
        },
        PointSelector::Ids(ids) => ClearPayloadRequest {
            points: Some(ids.iter().map(point_id_req_typed).collect()),
            filter: None,
        },
        PointSelector::Filter(filter) => ClearPayloadRequest {
            points: None,
            filter: Some(top_level_filter(filter)),
        },
    }
}

pub fn lower_delete_vector_request(stmt: &DeleteVectorStmt) -> DeleteVectorRequest {
    match &stmt.selector {
        PointSelector::Id(id) => DeleteVectorRequest {
            points: Some(vec![point_id_req_typed(id)]),
            filter: None,
            vector: stmt.vector_names.clone(),
        },
        PointSelector::Ids(ids) => DeleteVectorRequest {
            points: Some(ids.iter().map(point_id_req_typed).collect()),
            filter: None,
            vector: stmt.vector_names.clone(),
        },
        PointSelector::Filter(filter) => DeleteVectorRequest {
            points: None,
            filter: Some(top_level_filter(filter)),
            vector: stmt.vector_names.clone(),
        },
    }
}

pub fn lower_scroll_request(
    limit: u64,
    filter: Option<&qql_core::ast::FilterExpr>,
    after: Option<&qql_core::ast::PointId>,
    shard_key: Option<String>,
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
        offset: after.map(point_id_req_typed),
        limit: Some(limit),
        with_payload: Some(PayloadSelectorReq::All(true)),
        with_vector,
        order_by: None,
        shard_key,
    }
}

pub fn embedding_has_wait(spec: &EmbeddingSpec) -> bool {
    match spec {
        EmbeddingSpec::Dense { .. } | EmbeddingSpec::Hybrid { .. } => true,
    }
}

/// Lower a mutation statement into a collection name + wire `UpdateOperation`.
/// Returns `None` for non-mutation statements (QUERY, DDL, SCROLL, COUNT, …).
pub fn lower_update_operation(stmt: &Stmt) -> Option<(String, UpdateOperation)> {
    match stmt {
        Stmt::Upsert(u) => Some((
            u.collection.clone(),
            UpdateOperation::Upsert {
                upsert: lower_upsert_request(u),
            },
        )),
        Stmt::Delete(d) => Some((
            d.collection.clone(),
            UpdateOperation::Delete {
                delete: lower_delete_request(d),
            },
        )),
        Stmt::UpdatePayload(u) => Some((
            u.collection.clone(),
            UpdateOperation::SetPayload {
                set_payload: lower_update_payload_request(u),
            },
        )),
        Stmt::ClearPayload(c) => Some((
            c.collection.clone(),
            UpdateOperation::ClearPayload {
                clear_payload: lower_clear_payload_request(c),
            },
        )),
        Stmt::UpdateVector(u) => Some((
            u.collection.clone(),
            UpdateOperation::UpdateVectors {
                update_vectors: lower_update_vector_request(u),
            },
        )),
        Stmt::DeleteVector(d) => Some((
            d.collection.clone(),
            UpdateOperation::DeleteVectors {
                delete_vectors: lower_delete_vector_request(d),
            },
        )),
        _ => None,
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
        } => Some((
            collection.clone(),
            UpdateOperation::Delete {
                delete: request.clone(),
            },
        )),
        PlannedOperation::UpdatePayload {
            collection,
            request,
        } => Some((
            collection.clone(),
            UpdateOperation::SetPayload {
                set_payload: request.clone(),
            },
        )),
        PlannedOperation::ClearPayload {
            collection,
            request,
        } => Some((
            collection.clone(),
            UpdateOperation::ClearPayload {
                clear_payload: request.clone(),
            },
        )),
        PlannedOperation::UpdateVectors {
            collection,
            request,
        } => Some((
            collection.clone(),
            UpdateOperation::UpdateVectors {
                update_vectors: request.clone(),
            },
        )),
        PlannedOperation::DeleteVectors {
            collection,
            request,
        } => Some((
            collection.clone(),
            UpdateOperation::DeleteVectors {
                delete_vectors: request.clone(),
            },
        )),
        _ => None,
    }
}
