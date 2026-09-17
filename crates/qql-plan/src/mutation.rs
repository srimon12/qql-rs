use crate::filter::{point_id_req_typed, top_level_filter, value_to_json};
use crate::semantic::PlanShardKey;
use crate::types::*;
use alloc::format;
use qql_core::ast::{
    ClearPayloadStmt, DeletePayloadStmt, DeleteStmt, DeleteVectorStmt, PointEntry, PointSelector,
    UpdatePayloadStmt, UpdateVectorStmt, UpsertPoint, UpsertStmt,
};
use qql_core::error::QqlError;

/// Lower `UPSERT INTO` to the `PUT /collections/{c}/points` request body.
///
/// Whole-point placeholders (`VALUES :p` / `VALUES ?`) are skipped: they
/// splice in at execution time. Template planning therefore yields the inline
/// points only; executors must route point-param templates through the
/// point-splice path, never dispatch a template plan directly.
pub fn lower_upsert_request(stmt: &UpsertStmt) -> Result<UpsertRequest, QqlError> {
    Ok(UpsertRequest {
        points: stmt
            .points
            .iter()
            .filter_map(|point| match point {
                PointEntry::Inline(inline) => Some(lower_upsert_point(inline)),
                PointEntry::Param(..) | PointEntry::PositionalParam(..) => None,
            })
            .collect(),
        update_filter: stmt
            .update_filter
            .as_ref()
            .map(top_level_filter)
            .transpose()?,
        update_mode: stmt.update_mode.map(|mode| match mode {
            qql_core::ast::UpsertUpdateMode::InsertOnly => UpdateMode::InsertOnly,
            qql_core::ast::UpsertUpdateMode::UpdateOnly => UpdateMode::UpdateOnly,
            qql_core::ast::UpsertUpdateMode::Upsert => UpdateMode::Upsert,
        }),
        shard_key: stmt.shard_key.as_ref().map(PlanShardKey::from),
    })
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
pub fn lower_delete_request(stmt: &DeleteStmt) -> Result<DeleteRequest, QqlError> {
    Ok(match &stmt.selector {
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
            filter: Some(top_level_filter(filter)?),
            shard_key: stmt.shard_key.as_ref().map(PlanShardKey::from),
        },
    })
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
pub fn lower_update_payload_request(
    stmt: &UpdatePayloadStmt,
) -> Result<UpdatePayloadRequest, QqlError> {
    let mut payload = serde_json::Map::with_capacity(stmt.payload.len());
    for (key, value) in &stmt.payload {
        payload.insert(key.clone(), value_to_json(value));
    }
    Ok(match &stmt.selector {
        PointSelector::Id(id) => UpdatePayloadRequest {
            points: Some(vec![point_id_req_typed(id)]),
            filter: None,
            payload,
            key: stmt.key.clone(),
            shard_key: stmt.shard_key.as_ref().map(PlanShardKey::from),
        },
        PointSelector::Ids(ids) => UpdatePayloadRequest {
            points: Some(ids.iter().map(point_id_req_typed).collect()),
            filter: None,
            payload,
            key: stmt.key.clone(),
            shard_key: stmt.shard_key.as_ref().map(PlanShardKey::from),
        },
        PointSelector::Filter(filter) => UpdatePayloadRequest {
            points: None,
            filter: Some(top_level_filter(filter)?),
            payload,
            key: stmt.key.clone(),
            shard_key: stmt.shard_key.as_ref().map(PlanShardKey::from),
        },
    })
}

/// Lower `CLEAR PAYLOAD` to the `POST /points/payload/clear` request body.
pub fn lower_clear_payload_request(
    stmt: &ClearPayloadStmt,
) -> Result<ClearPayloadRequest, QqlError> {
    Ok(match &stmt.selector {
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
            filter: Some(top_level_filter(filter)?),
            shard_key: stmt.shard_key.as_ref().map(PlanShardKey::from),
        },
    })
}

/// Lower `DELETE PAYLOAD <keys>` to the `POST /points/payload/delete` body.
pub fn lower_delete_payload_request(
    stmt: &DeletePayloadStmt,
) -> Result<DeletePayloadRequest, QqlError> {
    Ok(match &stmt.selector {
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
            filter: Some(top_level_filter(filter)?),
            shard_key: stmt.shard_key.as_ref().map(PlanShardKey::from),
        },
    })
}

/// Lower `DELETE VECTOR <names>` to the `POST /points/vectors/delete` body.
pub fn lower_delete_vector_request(
    stmt: &DeleteVectorStmt,
) -> Result<DeleteVectorRequest, QqlError> {
    Ok(match &stmt.selector {
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
            filter: Some(top_level_filter(filter)?),
            vector: stmt.vector_names.clone(),
            shard_key: stmt.shard_key.as_ref().map(PlanShardKey::from),
        },
    })
}

/// Parse a UUID point ID string (dashes optional) into its 128-bit value.
fn uuid_to_u128(s: &str) -> Option<u128> {
    let clean: String = s.chars().filter(|c| *c != '-').collect();
    if clean.len() == 32 {
        u128::from_str_radix(&clean, 16).ok()
    } else {
        None
    }
}

/// Increment a 128-bit UUID point ID string by 1 to implement exclusive
/// cursor pagination (`id > after`), matching integer `AFTER n -> offset n+1`.
///
/// Returns `None` for non-UUID strings and for the maximum UUID (which has
/// no successor); both cases are rejected by [`validate_scroll_after`].
fn increment_uuid_point_id(s: &str) -> Option<String> {
    let val = uuid_to_u128(s)?;
    let next = val.checked_add(1)?;
    Some(format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        (next >> 96) as u32,
        ((next >> 80) & 0xffff) as u16,
        ((next >> 64) & 0xffff) as u16,
        ((next >> 48) & 0xffff) as u16,
        (next & 0xffff_ffff_ffff) as u64
    ))
}

/// Validate a `SCROLL ... AFTER` cursor before lowering.
///
/// The cursor contract is exclusive (`id > after`): integers advance by one
/// and UUIDs to their successor. Cursors with no successor fail closed
/// instead of emitting a request that silently re-scans or loops forever:
/// - `u64::MAX` / UUID-max are already the last representable ID, so there
///   is no next page (`saturating_add` would echo the anchor back forever).
///
/// Opaque (non-UUID) strings pass through: they have no computable
/// successor, so lowering resumes at the anchor unchanged (pinned by
/// `scroll_with_vector_after_string_id`). Only integer and UUID cursors get
/// exclusive advancement.
///
/// Placeholders (`:name` / `?N`) pass through: they are gated earlier by
/// `ensure_no_unbound_params` (or bound before planning), so there is no
/// concrete cursor to judge yet.
///
/// Planners must call this before [`lower_scroll_request`], which stays
/// infallible for its single wired call site.
pub fn validate_scroll_after(after: Option<&qql_core::ast::PointId>) -> Result<(), QqlError> {
    let Some(id) = after else {
        return Ok(());
    };
    match id {
        qql_core::ast::PointId::Number(n) if *n == u64::MAX => Err(QqlError::validation(
            "QQL-PLAN-SCROLL-AFTER",
            "SCROLL AFTER is already the maximum point ID; there is no next page",
            None,
        )),
        qql_core::ast::PointId::Number(_) => Ok(()),
        qql_core::ast::PointId::String(s) => match uuid_to_u128(s) {
            Some(v) if v == u128::MAX => Err(QqlError::validation(
                "QQL-PLAN-SCROLL-AFTER",
                "SCROLL AFTER is already the maximum UUID; there is no next page",
                None,
            )),
            // UUIDs advance to their successor; opaque strings have no
            // successor and lower unchanged (see `lower_scroll_request`).
            _ => Ok(()),
        },
        qql_core::ast::PointId::Param(..) | qql_core::ast::PointId::PositionalParam(..) => Ok(()),
    }
}

/// Lower `SCROLL` to the `/points/scroll` body: payload on, vectors off,
/// plus optional payload selection and payload-key ordering.
///
/// The `after` cursor lowers exclusively (`AFTER n` → `offset n + 1`,
/// UUIDs to their successor); opaque strings pass through unchanged.
/// Callers must run [`validate_scroll_after`] first to reject cursors with
/// no next page (`u64::MAX` / UUID-max).
pub fn lower_scroll_request(
    limit: u64,
    filter: Option<&qql_core::ast::FilterExpr>,
    after: Option<&qql_core::ast::PointId>,
    order_by: Option<&qql_core::ast::ScrollOrderBy>,
    shard_key: Option<qql_core::ast::ShardKey>,
    with_payload: Option<&qql_core::ast::PayloadSelector>,
    with_vector: Option<&qql_core::ast::VectorSelector>,
) -> Result<ScrollRequest, QqlError> {
    let with_payload = match with_payload {
        None => Some(PayloadSelectorReq::All(true)),
        Some(qql_core::ast::PayloadSelector::All) => Some(PayloadSelectorReq::All(true)),
        Some(qql_core::ast::PayloadSelector::None) => Some(PayloadSelectorReq::All(false)),
        Some(qql_core::ast::PayloadSelector::Include(fields)) => {
            Some(PayloadSelectorReq::Include {
                include: fields.clone(),
            })
        }
        Some(qql_core::ast::PayloadSelector::Exclude(fields)) => {
            Some(PayloadSelectorReq::Exclude {
                exclude: fields.clone(),
            })
        }
    };
    let with_vector = match with_vector {
        Some(qql_core::ast::VectorSelector::All) => Some(VectorSelectorReq::All(true)),
        Some(qql_core::ast::VectorSelector::None) => Some(VectorSelectorReq::All(false)),
        Some(qql_core::ast::VectorSelector::Names(names)) => {
            Some(VectorSelectorReq::Names(names.clone()))
        }
        None => Some(VectorSelectorReq::All(false)),
    };
    Ok(ScrollRequest {
        filter: filter.map(top_level_filter).transpose()?,
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
        with_payload,
        with_vector,
        order_by: order_by.map(|order| OrderByQuery {
            key: order.field.clone(),
            direction: Some(match order.direction {
                qql_core::ast::OrderDirection::Asc => "asc".into(),
                qql_core::ast::OrderDirection::Desc => "desc".into(),
            }),
            start_from: order.start_from.as_ref().map(value_to_json),
        }),
        shard_key: shard_key.as_ref().map(PlanShardKey::from),
    })
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
        PlannedOperation::OverwritePayload {
            collection,
            request,
            ..
        } => Some((
            collection.clone(),
            UpdateOperation::Overwrite {
                overwrite_payload: request.clone(),
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

/// Moving variant of [`planned_to_update_operation`]: destructures an owned
/// operation into its wire form with zero clones.
///
/// Batch builders that own `Vec<PlannedOperation>` use this so the hot
/// success path never clones request payloads (vectors, filters, payloads).
pub fn planned_to_update_operation_owned(
    op: crate::plan::PlannedOperation,
) -> Option<(String, UpdateOperation)> {
    use crate::plan::PlannedOperation;
    match op {
        PlannedOperation::Upsert {
            collection,
            request,
            ..
        } => Some((collection, UpdateOperation::Upsert { upsert: request })),
        PlannedOperation::Delete {
            collection,
            request,
            ..
        } => Some((collection, UpdateOperation::Delete { delete: request })),
        PlannedOperation::UpdatePayload {
            collection,
            request,
            ..
        } => Some((
            collection,
            UpdateOperation::SetPayload {
                set_payload: request,
            },
        )),
        PlannedOperation::OverwritePayload {
            collection,
            request,
            ..
        } => Some((
            collection,
            UpdateOperation::Overwrite {
                overwrite_payload: request,
            },
        )),
        PlannedOperation::ClearPayload {
            collection,
            request,
            ..
        } => Some((
            collection,
            UpdateOperation::ClearPayload {
                clear_payload: request,
            },
        )),
        PlannedOperation::DeletePayload {
            collection,
            request,
            ..
        } => Some((
            collection,
            UpdateOperation::DeletePayload {
                delete_payload: request,
            },
        )),
        PlannedOperation::UpdateVectors {
            collection,
            request,
            ..
        } => Some((
            collection,
            UpdateOperation::UpdateVectors {
                update_vectors: request,
            },
        )),
        PlannedOperation::DeleteVectors {
            collection,
            request,
            ..
        } => Some((
            collection,
            UpdateOperation::DeleteVectors {
                delete_vectors: request,
            },
        )),
        _ => None,
    }
}

/// Inverse of [`planned_to_update_operation_owned`]: rebuild a planned
/// mutation from a moved-out wire operation.
///
/// Cold-path only: batch executors consume `Vec<PlannedOperation>` into the
/// wire batch, and on transport/cardinality failure reconstruct per-member
/// operations for individual retry. Only the small collection `String` is
/// cloned per member; request payloads move back untouched.
///
/// Rebuilt operations carry `wait: true`, matching ambient batch semantics
/// ("ambient groups always wait"); the wire batch likewise executes with
/// `wait=true`, so retry preserves the durability the batch promised.
pub fn update_operation_into_planned(
    collection: &str,
    op: UpdateOperation,
) -> crate::plan::PlannedOperation {
    use crate::plan::PlannedOperation;
    match op {
        UpdateOperation::Upsert { upsert } => PlannedOperation::Upsert {
            collection: collection.to_owned(),
            request: upsert,
            wait: true,
        },
        UpdateOperation::Delete { delete } => PlannedOperation::Delete {
            collection: collection.to_owned(),
            request: delete,
            wait: true,
        },
        UpdateOperation::SetPayload { set_payload } => PlannedOperation::UpdatePayload {
            collection: collection.to_owned(),
            request: set_payload,
            wait: true,
        },
        UpdateOperation::Overwrite { overwrite_payload } => PlannedOperation::OverwritePayload {
            collection: collection.to_owned(),
            request: overwrite_payload,
            wait: true,
        },
        UpdateOperation::ClearPayload { clear_payload } => PlannedOperation::ClearPayload {
            collection: collection.to_owned(),
            request: clear_payload,
            wait: true,
        },
        UpdateOperation::DeletePayload { delete_payload } => PlannedOperation::DeletePayload {
            collection: collection.to_owned(),
            request: delete_payload,
            wait: true,
        },
        UpdateOperation::UpdateVectors { update_vectors } => PlannedOperation::UpdateVectors {
            collection: collection.to_owned(),
            request: update_vectors,
            wait: true,
        },
        UpdateOperation::DeleteVectors { delete_vectors } => PlannedOperation::DeleteVectors {
            collection: collection.to_owned(),
            request: delete_vectors,
            wait: true,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qql_core::ast::PointId;

    #[test]
    fn scroll_after_accepts_incrementable_cursors() {
        assert!(validate_scroll_after(None).is_ok());
        assert!(validate_scroll_after(Some(&PointId::Number(42))).is_ok());
        assert!(
            validate_scroll_after(Some(&PointId::String(
                "00000000-0000-0000-0000-00000000002a".into()
            )))
            .is_ok()
        );
        // Placeholders are gated by the binder, not the cursor check.
        assert!(validate_scroll_after(Some(&PointId::param("after"))).is_ok());
        assert!(validate_scroll_after(Some(&PointId::positional_param(0))).is_ok());
    }

    #[test]
    fn scroll_after_passes_through_opaque_strings() {
        // No computable successor exists, so lowering resumes at the anchor
        // unchanged (pinned by `scroll_with_vector_after_string_id`); only
        // cursors with no next page at all are rejected.
        assert!(validate_scroll_after(Some(&PointId::String("id-with-quote".into()))).is_ok());
    }

    #[test]
    fn scroll_after_rejects_maximum_ids() {
        // `saturating_add` would echo the anchor back: no next page exists.
        let err = validate_scroll_after(Some(&PointId::Number(u64::MAX)))
            .expect_err("u64::MAX has no successor");
        assert_eq!(err.code, "QQL-PLAN-SCROLL-AFTER");
        let err = validate_scroll_after(Some(&PointId::String(
            "ffffffff-ffff-ffff-ffff-ffffffffffff".into(),
        )))
        .expect_err("UUID-max has no successor");
        assert_eq!(err.code, "QQL-PLAN-SCROLL-AFTER");
        assert!(increment_uuid_point_id("ffffffff-ffff-ffff-ffff-ffffffffffff").is_none());
    }
}
