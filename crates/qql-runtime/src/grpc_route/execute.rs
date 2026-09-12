//! Fast-path gRPC dispatch:
//! [`qql_plan::PlannedOperation`] → typed [`BackendResponse`].
//!
//! Dispatch a [`qql_plan::PlannedOperation`] directly to gRPC — **no Route, no
//! JSON envelope, no envelope parser**. Each variant delegates to a focused
//! helper in [`super::execute_read`], [`super::execute_write`] or
//! [`super::execute_ddl`] that builds the tonic request from the already-typed
//! fields and converts the protobuf response straight into the executor's
//! typed IR. Formula expressions convert straight from the typed
//! [`qql_plan::PlanFormula`] tree into proto expressions.

use qql_core::error::QqlError;

use crate::executor::response::BackendResponse;
use crate::grpc::GrpcQdrant;

/// Dispatch a [`qql_plan::PlannedOperation`] directly to gRPC, building typed
/// data straight from the protobuf response.
///
/// This is the single fast path for gRPC backends: there is no intermediate
/// REST `Route` projection, no JSON serialisation/deserialisation, and no
/// response envelope parsing — every response shape has a typed `ExecData`
/// variant built straight from the protobuf message.
pub async fn execute_planned_grpc(
    client: &GrpcQdrant,
    op: &qql_plan::PlannedOperation,
) -> Result<BackendResponse, QqlError> {
    use qql_plan::PlannedOperation;
    match op {
        PlannedOperation::Query {
            collection,
            request,
        } => super::execute_read::execute_query(client, collection, request).await,
        PlannedOperation::QueryGroups {
            collection,
            request,
        } => super::execute_read::execute_query_groups(client, collection, request).await,
        PlannedOperation::GetPoints {
            collection,
            request,
        } => super::execute_read::execute_get_points(client, collection, request).await,
        PlannedOperation::Scroll {
            collection,
            request,
        } => super::execute_read::execute_scroll(client, collection, request).await,
        PlannedOperation::Count {
            collection,
            request,
        } => super::execute_read::execute_count(client, collection, request).await,
        PlannedOperation::Facet {
            collection,
            request,
        } => super::execute_read::execute_facet(client, collection, request).await,
        PlannedOperation::Upsert {
            collection,
            request,
            wait,
            ..
        } => super::execute_write::execute_upsert(client, collection, request, *wait).await,
        PlannedOperation::Delete {
            collection,
            request,
            wait,
        } => super::execute_write::execute_delete(client, collection, request, *wait).await,
        PlannedOperation::ClearPayload {
            collection,
            request,
            wait,
        } => super::execute_write::execute_clear_payload(client, collection, request, *wait).await,
        PlannedOperation::DeletePayload {
            collection,
            request,
            wait,
        } => super::execute_write::execute_delete_payload(client, collection, request, *wait).await,
        PlannedOperation::DeleteVectors {
            collection,
            request,
            wait,
        } => super::execute_write::execute_delete_vectors(client, collection, request, *wait).await,
        PlannedOperation::UpdateVectors {
            collection,
            request,
            wait,
        } => super::execute_write::execute_update_vectors(client, collection, request, *wait).await,
        PlannedOperation::UpdatePayload {
            collection,
            request,
            wait,
        } => super::execute_write::execute_update_payload(client, collection, request, *wait).await,
        PlannedOperation::OverwritePayload {
            collection,
            request,
            wait,
        } => {
            super::execute_write::execute_overwrite_payload(client, collection, request, *wait)
                .await
        }
        PlannedOperation::CreateCollection {
            collection,
            request,
        } => super::execute_ddl::execute_create_collection(client, collection, request).await,
        PlannedOperation::UpdateCollection {
            collection,
            request,
        } => super::execute_ddl::execute_update_collection(client, collection, request).await,
        PlannedOperation::DropCollection { collection } => {
            super::execute_ddl::execute_drop_collection(client, collection).await
        }
        PlannedOperation::CreateIndex {
            collection,
            request,
            wait,
        } => super::execute_ddl::execute_create_index(client, collection, request, *wait).await,
        PlannedOperation::DropIndex { collection, field } => {
            super::execute_ddl::execute_drop_index(client, collection, field).await
        }
        PlannedOperation::CreateShardKey {
            collection,
            request,
        } => super::execute_ddl::execute_create_shard_key(client, collection, request).await,
        PlannedOperation::DropShardKey {
            collection,
            request,
        } => super::execute_ddl::execute_drop_shard_key(client, collection, request).await,
        PlannedOperation::ListCollections => {
            super::execute_ddl::execute_list_collections(client).await
        }
        PlannedOperation::GetCollection { collection } => {
            super::execute_ddl::execute_get_collection(client, collection).await
        }
        PlannedOperation::ListShardKeys { collection } => {
            super::execute_ddl::execute_list_shard_keys(client, collection).await
        }
        PlannedOperation::CrossRerank { .. } => Err(QqlError::execution(
            "QQL-RERANK-CROSS",
            "CROSS RERANK is executed client-side by the Executor, not as a single gRPC route",
            None,
        )),
        PlannedOperation::Batch { .. } => Err(QqlError::execution(
            "QQL-GRPC-BATCH",
            "BATCH is dispatched by the Executor through the batch RPCs, not as a single gRPC route",
            None,
        )),
        PlannedOperation::GetQuotas | PlannedOperation::SetQuotas { .. } => {
            Err(QqlError::execution(
                "QQL-GRPC-QUOTA",
                "global quotas are only exposed through Qdrant's REST API (/quotas); \
                 the public gRPC surface has no quota service. Use the REST backend",
                None,
            ))
        }
    }
}
