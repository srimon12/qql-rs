//! Thin typed points wrappers — same API shape as qdrant-client's Qdrant.

use qql_core::error::QqlError;

use crate::qdrant_grpc::qdrant;

use super::client::GrpcQdrant;
use super::error::grpc_error;

impl GrpcQdrant {
    // ── Thin typed wrappers — same API shape as qdrant-client's Qdrant ──

    /// gRPC `Query`: run a single query request.
    pub async fn query(&self, req: qdrant::QueryPoints) -> Result<qdrant::QueryResponse, QqlError> {
        let mut cl = self.points_client();
        cl.query(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| grpc_error("query", e))
    }

    /// gRPC `QueryGroups`: run a grouped query request.
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

    /// gRPC `QueryBatch`: run multiple queries in one round trip.
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

    /// gRPC `UpdateBatch`: apply multiple mutations in one round trip.
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

    /// gRPC `Get`: retrieve points by ID.
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

    /// gRPC `Scroll`: page through points with an optional filter.
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

    /// gRPC `Upsert`: write or replace points.
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

    /// gRPC `Delete`: remove points by ID or filter.
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

    /// gRPC `UpdateVectors`: replace vectors on existing points.
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

    /// gRPC `SetPayload`: merge payload values onto points.
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

    /// gRPC `Collections/Create`: create a collection from a raw request.
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

    /// gRPC `Collections/Update`: update collection parameters.
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

    /// gRPC `Collections/Delete`: drop a collection.
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

    /// gRPC `CreateFieldIndex`: index a payload field.
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

    /// gRPC `DeleteFieldIndex`: drop a payload field index.
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

    /// gRPC `Count`: count points matching an optional filter.
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

    /// gRPC `ClearPayload`: remove all payload from points.
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

    /// gRPC `DeletePayload`: remove specific payload keys from points.
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

    /// gRPC `DeleteVectors`: remove vectors from points.
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
