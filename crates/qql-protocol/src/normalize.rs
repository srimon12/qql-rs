//! Pure response normalization: typed [`BackendResponse`] → [`ExecResponse`].
//!
//! Shared by the runtime executor and the WASM host so both report the same
//! operation label, message, and closed [`ExecData`] payload for a planned
//! operation. No I/O: server telemetry travels on the response, never on the
//! error path.

use qql_core::error::QqlError;

use qql_plan::PlannedOperation;

use crate::response::{BackendResponse, ExecData, ExecResponse, GroupedSearchResult};

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
///
/// Pure (no I/O): server telemetry travels on the [`BackendResponse`], never
/// on the error path. The runtime's `dispatch_planned`/`explain_analyze` and
/// the WASM execute path share this single normalization, so labels, messages,
/// and payload shapes cannot drift between hosts.
pub fn normalize_planned(
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
        // Status-only operations: no typed payload to surface. Listing every
        // remaining variant (instead of a wildcard) keeps `ExecData` closed
        // end to end — a new data-returning variant becomes a compile error
        // here instead of silently dropping its payload.
        PlannedOperation::CreateCollection { .. }
        | PlannedOperation::UpdateCollection { .. }
        | PlannedOperation::DropCollection { .. }
        | PlannedOperation::CreateIndex { .. }
        | PlannedOperation::DropIndex { .. }
        | PlannedOperation::CreateShardKey { .. }
        | PlannedOperation::DropShardKey { .. }
        | PlannedOperation::Batch { .. } => (format!("{label} ok"), None),
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
pub fn normalize_query_item(mut response: BackendResponse) -> Result<ExecResponse, QqlError> {
    let telemetry = response.telemetry.take();
    let count = response.data.hits().map_or(0, |hits| hits.len());
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
/// [`qql_plan::types::UpdateOperation::operation_name`] matches [`PlannedOperation::operation_label`]
/// for every mutation variant, and the upsert point count rides on the wire
/// `UpsertRequest` — so this reproduces [`normalize_planned`] exactly without
/// borrowing the original operation vector.
pub fn normalize_update_item(
    op: &qql_plan::types::UpdateOperation,
    mut response: BackendResponse,
) -> Result<ExecResponse, QqlError> {
    let telemetry = response.telemetry.take();
    let (message, data) = match op {
        qql_plan::types::UpdateOperation::Upsert { upsert } => {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::response::{BackendResponse, ExecData, SearchHit};
    use crate::telemetry::ServerTelemetry;
    use qql_plan::{PlanPointId, PlannedOperation};

    fn hit(id: u64, score: f64) -> SearchHit {
        SearchHit {
            id: PlanPointId::Number(id),
            score,
            payload: None,
            collection: None,
            vector: None,
        }
    }

    fn planned(sql: &str) -> PlannedOperation {
        qql_plan::plan::plan(&qql_core::parser::Parser::parse(sql).expect("parse")).expect("plan")
    }

    #[test]
    fn normalize_query_item_matches_normalize_planned() {
        let op = planned("QUERY [0.1, 0.2] FROM docs LIMIT 10;");
        let telemetry = Some(ServerTelemetry {
            time_s: Some(0.012),
            usage: None,
        });
        let response = BackendResponse {
            data: ExecData::Hits(vec![hit(1, 0.95), hit(2, 0.85)]),
            telemetry: telemetry.clone(),
        };

        let planned_res = normalize_planned(&op, response.clone()).expect("normalize_planned");
        let item_res = normalize_query_item(response).expect("normalize_query_item");

        assert_eq!(item_res, planned_res);
        assert!(item_res.ok);
        assert_eq!(item_res.operation, "QUERY");
        assert_eq!(item_res.message, "Found 2 hits");
        assert_eq!(item_res.telemetry, telemetry);
    }

    #[test]
    fn normalize_update_item_matches_normalize_planned_for_all_mutations() {
        let cases = [
            "UPSERT INTO docs VALUES {id: 1, vector: [0.1, 0.2]};",
            "DELETE FROM docs WHERE id = 1;",
            "UPDATE docs SET PAYLOAD = {a: 1} WHERE id = 1;",
            "UPDATE docs SET PAYLOAD = {a: 1} OVERWRITE WHERE id = 1;",
            "CLEAR PAYLOAD FROM docs WHERE id = 1;",
            "DELETE PAYLOAD a FROM docs WHERE id = 1;",
            "UPDATE docs SET VECTOR dense = [0.1, 0.2] WHERE id = 1;",
            "DELETE VECTOR default FROM docs WHERE id = 1;",
        ];

        let telemetry = Some(ServerTelemetry {
            time_s: Some(0.005),
            usage: None,
        });

        for sql in cases {
            let op = planned(sql);
            let (_, wire_op) = qql_plan::mutation::planned_to_update_operation(&op)
                .unwrap_or_else(|| panic!("failed to convert {sql} to UpdateOperation"));

            let resp = BackendResponse {
                data: ExecData::Mutation { affected: None },
                telemetry: telemetry.clone(),
            };

            let from_planned =
                normalize_planned(&op, resp.clone()).unwrap_or_else(|e| panic!("{sql}: {e}"));
            let from_item =
                normalize_update_item(&wire_op, resp).unwrap_or_else(|e| panic!("{sql} item: {e}"));

            assert_eq!(from_planned, from_item, "mismatch for statement: {sql}");
        }
    }

    #[test]
    fn normalize_update_item_upsert_multi_point_count() {
        let op = planned(
            "UPSERT INTO docs VALUES {id: 1, vector: [0.1, 0.2]}, \
                                    {id: 2, vector: [0.3, 0.4]}, \
                                    {id: 3, vector: [0.5, 0.6]};",
        );
        let (_, wire_op) = qql_plan::mutation::planned_to_update_operation(&op).unwrap();
        let resp = BackendResponse {
            data: ExecData::Mutation { affected: None },
            telemetry: None,
        };

        let from_planned = normalize_planned(&op, resp.clone()).unwrap();
        let from_item = normalize_update_item(&wire_op, resp).unwrap();

        assert_eq!(from_planned, from_item);
        assert_eq!(from_item.message, "Upserted 3 point(s)");
        assert_eq!(
            from_item.data,
            Some(ExecData::Mutation { affected: Some(3) })
        );
    }

    #[test]
    fn trims_group_offset_exactly_once() {
        let op = planned(
            "QUERY TEXT 'x' MODEL 'm' FROM docs USING dense GROUP BY category LIMIT 3 OFFSET 1;",
        );
        let groups = vec![
            GroupedSearchResult {
                group_id: qql_plan::PlanGroupId::Unsigned(1),
                hits: vec![hit(1, 0.9)],
            },
            GroupedSearchResult {
                group_id: qql_plan::PlanGroupId::Unsigned(2),
                hits: vec![hit(2, 0.8)],
            },
        ];
        let normalized = normalize_planned(
            &op,
            BackendResponse {
                data: ExecData::Groups(groups),
                telemetry: None,
            },
        )
        .expect("normalize_planned");
        assert_eq!(normalized.message, "Found 1 group(s)");
        assert_eq!(normalized.data.unwrap().groups().unwrap().len(), 1);
    }

    #[test]
    fn upsert_reports_request_point_count() {
        let op = planned("UPSERT INTO docs VALUES {id: 1, vector: [0.1]}, {id: 2, vector: [0.2]};");
        let normalized = normalize_planned(
            &op,
            BackendResponse {
                data: ExecData::Mutation { affected: None },
                telemetry: None,
            },
        )
        .expect("normalize_planned");
        assert_eq!(normalized.message, "Upserted 2 point(s)");
        assert_eq!(
            normalized.data,
            Some(ExecData::Mutation { affected: Some(2) })
        );
    }
}
