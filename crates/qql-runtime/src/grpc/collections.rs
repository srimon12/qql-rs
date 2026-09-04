//! Thin typed collections wrappers — same API shape as qdrant-client's Qdrant.

use qql_core::error::QqlError;

use crate::qdrant_grpc::qdrant;

use super::client::GrpcQdrant;
use super::error::grpc_error;

impl GrpcQdrant {
    /// `CreateShardKey` RPC: add a custom shard key to a collection.
    pub async fn create_shard_key(
        &self,
        req: qdrant::CreateShardKeyRequest,
    ) -> Result<qdrant::CreateShardKeyResponse, QqlError> {
        let mut cl = self.collections_client();
        cl.create_shard_key(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| grpc_error("create_shard_key", e))
    }

    /// `DeleteShardKey` RPC: remove a custom shard key from a collection.
    pub async fn delete_shard_key(
        &self,
        req: qdrant::DeleteShardKeyRequest,
    ) -> Result<qdrant::DeleteShardKeyResponse, QqlError> {
        let mut cl = self.collections_client();
        cl.delete_shard_key(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| grpc_error("delete_shard_key", e))
    }

    /// `ListShardKeys` RPC: enumerate a collection's custom shard keys.
    pub async fn list_shard_keys(
        &self,
        req: qdrant::ListShardKeysRequest,
    ) -> Result<qdrant::ListShardKeysResponse, QqlError> {
        let mut cl = self.collections_client();
        cl.list_shard_keys(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| grpc_error("list_shard_keys", e))
    }

    /// `List` RPC returning the raw collections response.
    pub async fn list_collections_raw(&self) -> Result<qdrant::ListCollectionsResponse, QqlError> {
        let mut cl = self.collections_client();
        cl.list(tonic::Request::new(qdrant::ListCollectionsRequest {}))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| grpc_error("list_collections", e))
    }

    /// `Get` RPC returning the raw collection-info response.
    pub async fn collection_info_raw(
        &self,
        collection: String,
    ) -> Result<qdrant::GetCollectionInfoResponse, QqlError> {
        let mut cl = self.collections_client();
        cl.get(tonic::Request::new(qdrant::GetCollectionInfoRequest {
            collection_name: collection,
        }))
        .await
        .map(|r| r.into_inner())
        .map_err(|e| grpc_error("collection_info", e))
    }
}
