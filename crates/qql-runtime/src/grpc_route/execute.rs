//! Fast-path gRPC dispatch: [`PlannedOperation`] → execution helpers.
//!
//! Dispatch a [`PlannedOperation`] directly to gRPC — **no Route, no JSON**.
//! This is the fast path for gRPC backends: each variant delegates to a
//! focused helper in [`super::execute_read`], [`super::execute_write`] or
//! [`super::execute_ddl`] that builds the tonic request from the
//! already-typed fields. There is no intermediate REST `Route` projection
//! and no JSON serialisation/deserialisation.

use qql_core::error::QqlError;

use crate::grpc::GrpcQdrant;

/// Dispatch a [`PlannedOperation`] directly to gRPC — **no Route, no JSON**.
///
/// This is the fast path for gRPC backends.  It matches each
/// `PlannedOperation` variant and builds the corresponding tonic
/// request from the already-typed fields.  There is no intermediate
/// REST `Route` projection and no JSON serialisation/deserialisation.
pub async fn execute_planned_grpc(
    client: &GrpcQdrant,
    op: &qql_plan::PlannedOperation,
) -> Result<serde_json::Value, QqlError> {
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
        PlannedOperation::Upsert {
            collection,
            request,
            ..
        } => super::execute_write::execute_upsert(client, collection, request).await,
        PlannedOperation::Delete {
            collection,
            request,
        } => super::execute_write::execute_delete(client, collection, request).await,
        PlannedOperation::ClearPayload {
            collection,
            request,
        } => super::execute_write::execute_clear_payload(client, collection, request).await,
        PlannedOperation::DeletePayload {
            collection,
            request,
        } => super::execute_write::execute_delete_payload(client, collection, request).await,
        PlannedOperation::DeleteVectors {
            collection,
            request,
        } => super::execute_write::execute_delete_vectors(client, collection, request).await,
        PlannedOperation::UpdateVectors {
            collection,
            request,
        } => super::execute_write::execute_update_vectors(client, collection, request).await,
        PlannedOperation::UpdatePayload {
            collection,
            request,
        } => super::execute_write::execute_update_payload(client, collection, request).await,
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
        } => super::execute_ddl::execute_create_index(client, collection, request).await,
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
        PlannedOperation::GetQuotas | PlannedOperation::SetQuotas { .. } => {
            Err(QqlError::execution(
                "QQL-GRPC-QUOTA",
                "global quotas are only exposed through Qdrant's REST API (/quotas); \
                 the public gRPC surface has no quota service. Use the REST backend",
                None,
            ))
        }
        PlannedOperation::Facet { .. } => Err(QqlError::execution(
            "QQL-GRPC-FACET",
            "faceting is only exposed through Qdrant's REST API (/collections/{name}/facet); \
             the public gRPC surface has no facet service. Use the REST backend",
            None,
        )),
    }
}
