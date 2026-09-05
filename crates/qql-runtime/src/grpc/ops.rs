//! [`QdrantOps`] implementation for [`GrpcQdrant`].

use async_trait::async_trait;

use qql_core::error::QqlError;
use qql_plan::{QueryBatchRequest, UpdateBatchRequest};

use crate::client::{CollectionInfo, QdrantOps};
use crate::qdrant_grpc::qdrant;

use super::client::GrpcQdrant;
use super::error::grpc_error;
use super::schema::schema_from_grpc_collection;

#[async_trait]
impl QdrantOps for GrpcQdrant {
    async fn list_collections(&self) -> Result<Vec<String>, QqlError> {
        let resp = self.list_collections_raw().await?;
        Ok(resp.collections.into_iter().map(|c| c.name).collect())
    }

    async fn collection_exists(&self, name: &str) -> Result<bool, QqlError> {
        let mut cl = self.collections_client();
        match cl
            .collection_exists(tonic::Request::new(qdrant::CollectionExistsRequest {
                collection_name: name.to_string(),
            }))
            .await
        {
            Ok(resp) => Ok(resp.into_inner().result.map(|r| r.exists).unwrap_or(false)),
            Err(status) if status.code() == tonic::Code::NotFound => Ok(false),
            Err(e) => Err(grpc_error("collection_exists", e)),
        }
    }

    async fn get_collection_info(&self, name: &str) -> Result<CollectionInfo, QqlError> {
        let resp = self.collection_info_raw(name.to_string()).await?;
        let info = resp.result.ok_or_else(|| {
            QqlError::backend(
                "QQL-GRPC-NO-RESULT",
                "collection_info response missing result field",
                None,
            )
            .with_collection(name.to_string())
        })?;

        Ok(CollectionInfo {
            status: info.status.to_string(),
            points_count: info.points_count.unwrap_or(0),
            segments_count: info.segments_count,
            schema: schema_from_grpc_collection(&info),
        })
    }

    async fn create_collection(
        &self,
        collection_name: &str,
        req: &qql_plan::CreateCollectionRequest,
    ) -> Result<(), QqlError> {
        let op = qql_plan::PlannedOperation::CreateCollection {
            collection: collection_name.to_string(),
            request: req.clone(),
        };
        self.execute_planned(&op).await.map(|_| ())
    }

    async fn update_collection(
        &self,
        collection_name: &str,
        req: &qql_plan::UpdateCollectionRequest,
    ) -> Result<(), QqlError> {
        let op = qql_plan::PlannedOperation::UpdateCollection {
            collection: collection_name.to_string(),
            request: req.clone(),
        };
        self.execute_planned(&op).await.map(|_| ())
    }

    async fn delete_collection(&self, name: &str) -> Result<(), QqlError> {
        let op = qql_plan::PlannedOperation::DropCollection {
            collection: name.to_string(),
        };
        self.execute_planned(&op).await.map(|_| ())
    }

    async fn create_field_index(
        &self,
        collection_name: &str,
        req: &qql_plan::CreateIndexRequest,
    ) -> Result<(), QqlError> {
        let op = qql_plan::PlannedOperation::CreateIndex {
            collection: collection_name.to_string(),
            request: req.clone(),
        };
        self.execute_planned(&op).await.map(|_| ())
    }

    async fn delete_field_index(
        &self,
        collection_name: &str,
        field_name: &str,
    ) -> Result<(), QqlError> {
        let op = qql_plan::PlannedOperation::DropIndex {
            collection: collection_name.to_string(),
            field: field_name.to_string(),
        };
        self.execute_planned(&op).await.map(|_| ())
    }

    async fn execute_planned(
        &self,
        op: &qql_plan::PlannedOperation,
    ) -> Result<serde_json::Value, QqlError> {
        crate::grpc_route::execute_planned_grpc(self, op).await
    }

    async fn execute_query_batch(
        &self,
        collection: &str,
        batch: &QueryBatchRequest,
    ) -> Result<Vec<serde_json::Value>, QqlError> {
        crate::grpc_route::execute_query_batch_grpc(self, collection, batch).await
    }

    async fn execute_update_batch(
        &self,
        collection: &str,
        batch: &UpdateBatchRequest,
    ) -> Result<Vec<serde_json::Value>, QqlError> {
        crate::grpc_route::execute_update_batch_grpc(self, collection, batch).await
    }
}
