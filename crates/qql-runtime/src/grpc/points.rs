//! Thin typed points wrappers — same API shape as qdrant-client's Qdrant.

use qql_core::error::QqlError;

use crate::qdrant_grpc::qdrant;

use super::client::GrpcQdrant;
use super::error::grpc_error;

impl GrpcQdrant {
    // ── Thin typed wrappers — same API shape as qdrant-client's Qdrant ──

    pub async fn query(&self, req: qdrant::QueryPoints) -> Result<qdrant::QueryResponse, QqlError> {
        let mut cl = self.points_client();
        cl.query(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| grpc_error("query", e))
    }

    pub async fn query_groups(
        &self,
        req: qdrant::QueryPointGroups,
    ) -> Result<qdrant::QueryGroupsResponse, QqlError> {
        let mut cl = self.points_client();
        cl.query_groups(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| grpc_error("query_groups", e))
    }

    pub async fn query_batch(
        &self,
        req: qdrant::QueryBatchPoints,
    ) -> Result<qdrant::QueryBatchResponse, QqlError> {
        let mut cl = self.points_client();
        cl.query_batch(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| grpc_error("query_batch", e))
    }

    pub async fn update_batch(
        &self,
        req: qdrant::UpdateBatchPoints,
    ) -> Result<qdrant::UpdateBatchResponse, QqlError> {
        let mut cl = self.points_client();
        cl.update_batch(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| grpc_error("update_batch", e))
    }

    pub async fn get_points(
        &self,
        req: qdrant::GetPoints,
    ) -> Result<qdrant::GetResponse, QqlError> {
        let mut cl = self.points_client();
        cl.get(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| grpc_error("get_points", e))
    }

    pub async fn scroll(
        &self,
        req: qdrant::ScrollPoints,
    ) -> Result<qdrant::ScrollResponse, QqlError> {
        let mut cl = self.points_client();
        cl.scroll(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| grpc_error("scroll", e))
    }

    pub async fn upsert_points(
        &self,
        req: qdrant::UpsertPoints,
    ) -> Result<qdrant::PointsOperationResponse, QqlError> {
        let mut cl = self.points_client();
        cl.upsert(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| grpc_error("upsert", e))
    }

    pub async fn delete_points(
        &self,
        req: qdrant::DeletePoints,
    ) -> Result<qdrant::PointsOperationResponse, QqlError> {
        let mut cl = self.points_client();
        cl.delete(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| grpc_error("delete", e))
    }

    pub async fn update_vectors(
        &self,
        req: qdrant::UpdatePointVectors,
    ) -> Result<qdrant::PointsOperationResponse, QqlError> {
        let mut cl = self.points_client();
        cl.update_vectors(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| grpc_error("update_vectors", e))
    }

    pub async fn set_payload(
        &self,
        req: qdrant::SetPayloadPoints,
    ) -> Result<qdrant::PointsOperationResponse, QqlError> {
        let mut cl = self.points_client();
        cl.set_payload(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| grpc_error("set_payload", e))
    }

    pub async fn create_collection_raw(
        &self,
        req: qdrant::CreateCollection,
    ) -> Result<qdrant::CollectionOperationResponse, QqlError> {
        let mut cl = self.collections_client();
        cl.create(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| grpc_error("create_collection", e))
    }

    pub async fn update_collection_raw(
        &self,
        req: qdrant::UpdateCollection,
    ) -> Result<qdrant::CollectionOperationResponse, QqlError> {
        let mut cl = self.collections_client();
        cl.update(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| grpc_error("update_collection", e))
    }

    pub async fn delete_collection_raw(
        &self,
        req: qdrant::DeleteCollection,
    ) -> Result<qdrant::CollectionOperationResponse, QqlError> {
        let mut cl = self.collections_client();
        cl.delete(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| grpc_error("delete_collection", e))
    }

    pub async fn create_field_index(
        &self,
        req: qdrant::CreateFieldIndexCollection,
    ) -> Result<qdrant::PointsOperationResponse, QqlError> {
        let mut cl = self.points_client();
        cl.create_field_index(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| grpc_error("create_field_index", e))
    }

    pub async fn delete_field_index(
        &self,
        req: qdrant::DeleteFieldIndexCollection,
    ) -> Result<qdrant::PointsOperationResponse, QqlError> {
        let mut cl = self.points_client();
        cl.delete_field_index(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| grpc_error("delete_field_index", e))
    }

    pub async fn count_points(
        &self,
        req: qdrant::CountPoints,
    ) -> Result<qdrant::CountResponse, QqlError> {
        let mut cl = self.points_client();
        cl.count(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| grpc_error("count", e))
    }

    pub async fn clear_payload(
        &self,
        req: qdrant::ClearPayloadPoints,
    ) -> Result<qdrant::PointsOperationResponse, QqlError> {
        let mut cl = self.points_client();
        cl.clear_payload(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| grpc_error("clear_payload", e))
    }

    pub async fn delete_payload(
        &self,
        req: qdrant::DeletePayloadPoints,
    ) -> Result<qdrant::PointsOperationResponse, QqlError> {
        let mut cl = self.points_client();
        cl.delete_payload(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| grpc_error("delete_payload", e))
    }

    pub async fn delete_vectors(
        &self,
        req: qdrant::DeletePointVectors,
    ) -> Result<qdrant::PointsOperationResponse, QqlError> {
        let mut cl = self.points_client();
        cl.delete_vectors(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| grpc_error("delete_vectors", e))
    }
}
