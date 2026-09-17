//! Pure response normalization: typed [`BackendResponse`] → [`ExecResponse`].
//!
//! Split out of `dispatch.rs` so batch executors (`batch.rs`, WASM
//! `pipeline.rs`) can normalize batch items without depending on dispatch.
//! No I/O here: server telemetry travels on the response, never on the error
//! path. Both [`Executor::dispatch_planned`](super::Executor::dispatch_planned)
//! and `explain_analyze` share [`normalize_planned`]; batch hot paths use the
//! item normalizers below, which read only the owned wire batch — never the
//! original `PlannedOperation` vector.

use qql_core::error::QqlError;
use qql_plan::{PlannedOperation, UpdateOperation};

use super::response::{BackendResponse, ExecData, ExecResponse};
use super::{GroupedSearchResult, SearchHit};

/// Trim a grouped result set by the client-side `group_offset` (which has no
/// wire representation) exactly once.
fn trim_group_offset(groups: &mut Vec<GroupedSearchResult>, offset: Option<u64>) {
    let Some(offset) = offset else {
        return;
    };
    let offset = offset as usize;
    if offset < groups.len() {
        groups.drain(0..offset);
    } else {
        groups.clear();
    }
}

/// Decode + normalize a typed backend response into an `ExecResponse`.
/// Pure (no I/O): server telemetry travels on the [`BackendResponse`],
/// never on the error path; both `dispatch_planned` and `explain_analyze`
/// share this single normalization.
pub(crate) fn normalize_planned(
    op: &PlannedOperation,
    mut response: BackendResponse,
) -> Result<ExecResponse, QqlError> {
    let telemetry = response.telemetry.take();
    let label = op.operation_label();
    let (message, data) = match op {
        PlannedOperation::Query { .. }
        | PlannedOperation::Scroll { .. }
        | PlannedOperation::GetPoints { .. } => {
            let count = response.data.hits().map_or(0, |hits| hits.len());
            (format!("Found {count} hits"), Some(response.data))
        }
        PlannedOperation::QueryGroups { request, .. } => {
            // `group_offset` has no wire representation (it is serde-skipped
            // and absent from the gRPC proto), so the backend never applies
            // it: trim the returned groups client-side, exactly once.
            if let ExecData::Groups(groups) = &mut response.data {
                trim_group_offset(groups, request.group_offset);
            }
            let count = response.data.groups().map_or(0, |groups| groups.len());
            (format!("Found {count} group(s)"), Some(response.data))
        }
        PlannedOperation::Count { .. } => {
            let count = response.data.count().unwrap_or(0);
            (format!("Count: {count}"), Some(ExecData::Count(count)))
        }
        PlannedOperation::Facet { .. } => {
            let count = response.data.facet().map_or(0, |hits| hits.len());
            (format!("Found {count} facet hit(s)"), Some(response.data))
        }
        PlannedOperation::ListCollections => {
            let count = response.data.collections().map_or(0, <[String]>::len);
            (format!("Found {count} collection(s)"), Some(response.data))
        }
        PlannedOperation::GetCollection { .. } => (format!("{label} ok"), Some(response.data)),
        PlannedOperation::Upsert { request, .. } => {
            let n = request.points.len();
            (
                format!("Upserted {n} point(s)"),
                Some(ExecData::Mutation {
                    affected: Some(n as u64),
                }),
            )
        }
        PlannedOperation::Delete { .. }
        | PlannedOperation::UpdatePayload { .. }
        | PlannedOperation::OverwritePayload { .. }
        | PlannedOperation::ClearPayload { .. }
        | PlannedOperation::DeletePayload { .. }
        | PlannedOperation::UpdateVectors { .. }
        | PlannedOperation::DeleteVectors { .. } => (
            format!("{label} ok"),
            Some(ExecData::Mutation { affected: None }),
        ),
        PlannedOperation::ListShardKeys { .. } => ("Shard keys listed".into(), Some(response.data)),
        PlannedOperation::GetQuotas => ("Quota configuration shown".into(), Some(response.data)),
        PlannedOperation::SetQuotas { .. } => {
            ("Quota configuration updated".into(), Some(response.data))
        }
        PlannedOperation::CrossRerank { .. } => {
            return Err(QqlError::execution(
                "QQL-CROSS-RERANK",
                "CROSS RERANK must be executed client-side, not via a Qdrant route",
                None,
            ));
        }
        _ => (format!("{label} ok"), None),
    };
    Ok(ExecResponse {
        ok: true,
        operation: label.into(),
        message,
        data,
        telemetry,
    })
}

/// Normalize one query-batch member response.
///
/// Ambient query batches contain only plain `Query` operations (grouped
/// search is never batched), whose normalization reads nothing from the
/// operation — hit count comes from the response, the label is static.
/// Batch hot paths use this instead of [`normalize_planned`] so the owned
/// `Vec<PlannedOperation>` can move into the wire batch with zero clones.
pub(crate) fn normalize_query_item(
    mut response: BackendResponse,
) -> Result<ExecResponse, QqlError> {
    let telemetry = response.telemetry.take();
    let count = response
        .data
        .hits()
        .map_or(0, |hits: &[SearchHit]| hits.len());
    Ok(ExecResponse {
        ok: true,
        operation: "QUERY".to_string(),
        message: format!("Found {count} hits"),
        data: Some(response.data),
        telemetry,
    })
}

/// Normalize one mutation-batch member response from its owned wire operation.
///
/// [`UpdateOperation::operation_name`] matches [`PlannedOperation::operation_label`]
/// for every mutation variant, and the upsert point count rides on the wire
/// `UpsertRequest` — so this reproduces [`normalize_planned`] exactly without
/// borrowing the original operation vector.
pub(crate) fn normalize_update_item(
    op: &UpdateOperation,
    mut response: BackendResponse,
) -> Result<ExecResponse, QqlError> {
    let telemetry = response.telemetry.take();
    let (message, data) = match op {
        UpdateOperation::Upsert { upsert } => {
            let n = upsert.points.len();
            (
                format!("Upserted {n} point(s)"),
                Some(ExecData::Mutation {
                    affected: Some(n as u64),
                }),
            )
        }
        _ => (
            format!("{} ok", op.operation_name()),
            Some(ExecData::Mutation { affected: None }),
        ),
    };
    Ok(ExecResponse {
        ok: true,
        operation: op.operation_name().to_string(),
        message,
        data,
        telemetry,
    })
}
