use async_trait::async_trait;
use tonic::transport::Channel;

use crate::client::{CollectionInfo, CreateCollectionReq, CreateFieldIndexReq, QdrantOps};
use crate::qdrant_grpc::qdrant;
use qql_core::error::QqlError;

pub struct GrpcQdrant {
    channel: Channel,
}

impl GrpcQdrant {
    pub fn from_channel(channel: Channel) -> Self {
        Self { channel }
    }

    pub fn from_url(url: &str, _api_key: Option<String>) -> Result<Self, QqlError> {
        let endpoint = url
            .trim_start_matches("grpc://")
            .trim_start_matches("http://")
            .trim_start_matches("https://");
        let endpoint = format!("https://{endpoint}");

        let channel = tonic::transport::Endpoint::from_shared(endpoint)
            .map_err(|e| {
                QqlError::transport("QQL-TRANSPORT", format!("invalid gRPC url: {e}"), None)
            })?
            .connect_lazy();

        Ok(Self { channel })
    }

    pub fn channel(&self) -> Channel {
        self.channel.clone()
    }

    // ── Thin typed wrappers — same API shape as qdrant-client's Qdrant ──

    pub async fn query(&self, req: qdrant::QueryPoints) -> Result<qdrant::QueryResponse, QqlError> {
        let mut cl = qdrant::points_client::PointsClient::new(self.channel.clone());
        cl.query(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("query: {e}"), None))
    }

    pub async fn query_groups(
        &self,
        req: qdrant::QueryPointGroups,
    ) -> Result<qdrant::QueryGroupsResponse, QqlError> {
        let mut cl = qdrant::points_client::PointsClient::new(self.channel.clone());
        cl.query_groups(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("query_groups: {e}"), None))
    }

    pub async fn get_points(
        &self,
        req: qdrant::GetPoints,
    ) -> Result<qdrant::GetResponse, QqlError> {
        let mut cl = qdrant::points_client::PointsClient::new(self.channel.clone());
        cl.get(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("get_points: {e}"), None))
    }

    pub async fn scroll(
        &self,
        req: qdrant::ScrollPoints,
    ) -> Result<qdrant::ScrollResponse, QqlError> {
        let mut cl = qdrant::points_client::PointsClient::new(self.channel.clone());
        cl.scroll(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("scroll: {e}"), None))
    }

    pub async fn upsert_points(
        &self,
        req: qdrant::UpsertPoints,
    ) -> Result<qdrant::PointsOperationResponse, QqlError> {
        let mut cl = qdrant::points_client::PointsClient::new(self.channel.clone());
        cl.upsert(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("upsert: {e}"), None))
    }

    pub async fn delete_points(
        &self,
        req: qdrant::DeletePoints,
    ) -> Result<qdrant::PointsOperationResponse, QqlError> {
        let mut cl = qdrant::points_client::PointsClient::new(self.channel.clone());
        cl.delete(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("delete: {e}"), None))
    }

    pub async fn update_vectors(
        &self,
        req: qdrant::UpdatePointVectors,
    ) -> Result<qdrant::PointsOperationResponse, QqlError> {
        let mut cl = qdrant::points_client::PointsClient::new(self.channel.clone());
        cl.update_vectors(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("update_vectors: {e}"), None))
    }

    pub async fn set_payload(
        &self,
        req: qdrant::SetPayloadPoints,
    ) -> Result<qdrant::PointsOperationResponse, QqlError> {
        let mut cl = qdrant::points_client::PointsClient::new(self.channel.clone());
        cl.set_payload(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("set_payload: {e}"), None))
    }

    pub async fn create_collection_raw(
        &self,
        req: qdrant::CreateCollection,
    ) -> Result<qdrant::CollectionOperationResponse, QqlError> {
        let mut cl = qdrant::collections_client::CollectionsClient::new(self.channel.clone());
        cl.create(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("create_collection: {e}"), None))
    }

    pub async fn delete_collection_raw(
        &self,
        req: qdrant::DeleteCollection,
    ) -> Result<qdrant::CollectionOperationResponse, QqlError> {
        let mut cl = qdrant::collections_client::CollectionsClient::new(self.channel.clone());
        cl.delete(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("delete_collection: {e}"), None))
    }

    pub async fn create_field_index(
        &self,
        req: qdrant::CreateFieldIndexCollection,
    ) -> Result<qdrant::PointsOperationResponse, QqlError> {
        let mut cl = qdrant::points_client::PointsClient::new(self.channel.clone());
        cl.create_field_index(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("create_field_index: {e}"), None))
    }

    pub async fn list_collections_raw(&self) -> Result<qdrant::ListCollectionsResponse, QqlError> {
        let mut cl = qdrant::collections_client::CollectionsClient::new(self.channel.clone());
        cl.list(tonic::Request::new(qdrant::ListCollectionsRequest {}))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("list_collections: {e}"), None))
    }

    pub async fn collection_info_raw(
        &self,
        collection: String,
    ) -> Result<qdrant::GetCollectionInfoResponse, QqlError> {
        let mut cl = qdrant::collections_client::CollectionsClient::new(self.channel.clone());
        cl.get(tonic::Request::new(qdrant::GetCollectionInfoRequest {
            collection_name: collection,
        }))
        .await
        .map(|r| r.into_inner())
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("collection_info: {e}"), None))
    }
}

#[async_trait]
impl QdrantOps for GrpcQdrant {
    async fn list_collections(&self) -> Result<Vec<String>, QqlError> {
        let resp = self.list_collections_raw().await?;
        Ok(resp.collections.into_iter().map(|c| c.name).collect())
    }

    async fn collection_exists(&self, name: &str) -> Result<bool, QqlError> {
        let mut cl = qdrant::collections_client::CollectionsClient::new(self.channel.clone());
        let resp = cl
            .collection_exists(tonic::Request::new(qdrant::CollectionExistsRequest {
                collection_name: name.to_string(),
            }))
            .await
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("collection_exists: {e}"), None))?
            .into_inner();
        Ok(resp.result.map(|r| r.exists).unwrap_or(false))
    }

    async fn get_collection_info(&self, name: &str) -> Result<CollectionInfo, QqlError> {
        let resp = self.collection_info_raw(name.to_string()).await?;
        let info = resp
            .result
            .ok_or_else(|| QqlError::backend("QQL-GRPC", "collection_info: no result", None))?;
        Ok(CollectionInfo {
            status: info.status.to_string(),
            points_count: info.points_count.unwrap_or(0),
            segments_count: 0,
            schema: Default::default(),
            raw_json: None,
        })
    }

    async fn create_collection(&self, req: CreateCollectionReq) -> Result<(), QqlError> {
        let vectors_config = req.vectors_config.and_then(|v| {
            let obj = v.as_object()?;
            let mut map = std::collections::HashMap::new();
            for (name, cfg) in obj {
                let size = cfg.get("size").and_then(|s| s.as_u64()).unwrap_or(384);
                let dist = cfg
                    .get("distance")
                    .and_then(|d| d.as_str())
                    .map(|d| match d {
                        "Cosine" => qdrant::Distance::Cosine as i32,
                        "Euclid" => qdrant::Distance::Euclid as i32,
                        "Dot" => qdrant::Distance::Dot as i32,
                        "Manhattan" => qdrant::Distance::Manhattan as i32,
                        _ => qdrant::Distance::Cosine as i32,
                    })
                    .unwrap_or(qdrant::Distance::Cosine as i32);
                map.insert(
                    name.clone(),
                    qdrant::VectorParams {
                        size,
                        distance: dist,
                        ..Default::default()
                    },
                );
            }
            Some(qdrant::VectorsConfig {
                config: Some(qdrant::vectors_config::Config::ParamsMap(
                    qdrant::VectorParamsMap { map },
                )),
            })
        });

        self.create_collection_raw(qdrant::CreateCollection {
            collection_name: req.collection_name,
            vectors_config,
            ..Default::default()
        })
        .await
        .map(|_| ())
    }

    async fn update_collection(&self, _req: serde_json::Value) -> Result<(), QqlError> {
        Err(QqlError::execution(
            "QQL-EXECUTION",
            "update_collection: use execute_route for gRPC",
            None,
        ))
    }

    async fn delete_collection(&self, name: &str) -> Result<(), QqlError> {
        self.delete_collection_raw(qdrant::DeleteCollection {
            collection_name: name.to_string(),
            ..Default::default()
        })
        .await
        .map(|_| ())
    }

    async fn create_field_index(&self, _req: CreateFieldIndexReq) -> Result<(), QqlError> {
        Err(QqlError::execution(
            "QQL-EXECUTION",
            "create_field_index: use execute_route for gRPC",
            None,
        ))
    }

    async fn execute_route(
        &self,
        route: qql_plan::routing::Route,
    ) -> Result<serde_json::Value, QqlError> {
        crate::grpc_route::execute_grpc_route(self, route).await
    }
}
