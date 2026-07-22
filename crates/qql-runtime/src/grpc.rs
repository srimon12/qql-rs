use async_trait::async_trait;
use tonic::transport::Channel;

use crate::backend::CollectionSchema;
use crate::client::{CollectionInfo, CreateCollectionReq, CreateFieldIndexReq, QdrantOps};
use crate::qdrant_grpc::qdrant;
use qql_core::error::QqlError;
use qql_plan::{QueryBatchRequest, UpdateBatchRequest};

pub struct GrpcQdrant {
    channel: Channel,
    api_key: Option<String>,
}

/// Interceptor that attaches the Qdrant API key metadata header (RUN-009).
#[derive(Clone)]
struct ApiKeyInterceptor {
    api_key: Option<String>,
}

impl tonic::service::Interceptor for ApiKeyInterceptor {
    fn call(
        &mut self,
        mut request: tonic::Request<()>,
    ) -> Result<tonic::Request<()>, tonic::Status> {
        if let Some(ref key) = self.api_key {
            let value = tonic::metadata::MetadataValue::try_from(key.as_str())
                .map_err(|e| tonic::Status::invalid_argument(format!("invalid api key: {e}")))?;
            request.metadata_mut().insert("api-key", value);
        }
        Ok(request)
    }
}

impl GrpcQdrant {
    pub fn from_channel(channel: Channel) -> Self {
        Self {
            channel,
            api_key: None,
        }
    }

    pub fn from_url(url: &str, api_key: Option<String>) -> Result<Self, QqlError> {
        Self::from_url_with_timeout(url, api_key, None)
    }

    pub fn from_url_with_timeout(
        url: &str,
        api_key: Option<String>,
        timeout: Option<std::time::Duration>,
    ) -> Result<Self, QqlError> {
        let scheme = if url.starts_with("https://") {
            "https://"
        } else {
            "http://"
        };
        let raw = url
            .trim_start_matches("grpc://")
            .trim_start_matches("https://")
            .trim_start_matches("http://");
        let endpoint = format!("{scheme}{raw}");

        let mut ep = tonic::transport::Endpoint::from_shared(endpoint).map_err(|e| {
            QqlError::transport("QQL-TRANSPORT", format!("invalid gRPC url: {e}"), None)
        })?;
        if let Some(t) = timeout {
            ep = ep.timeout(t);
        }
        let channel = ep.connect_lazy();

        Ok(Self { channel, api_key })
    }

    pub fn channel(&self) -> Channel {
        self.channel.clone()
    }

    fn points_client(
        &self,
    ) -> qdrant::points_client::PointsClient<
        tonic::service::interceptor::InterceptedService<Channel, ApiKeyInterceptor>,
    > {
        qdrant::points_client::PointsClient::with_interceptor(
            self.channel.clone(),
            ApiKeyInterceptor {
                api_key: self.api_key.clone(),
            },
        )
    }

    fn collections_client(
        &self,
    ) -> qdrant::collections_client::CollectionsClient<
        tonic::service::interceptor::InterceptedService<Channel, ApiKeyInterceptor>,
    > {
        qdrant::collections_client::CollectionsClient::with_interceptor(
            self.channel.clone(),
            ApiKeyInterceptor {
                api_key: self.api_key.clone(),
            },
        )
    }

    // ── Thin typed wrappers — same API shape as qdrant-client's Qdrant ──

    pub async fn query(&self, req: qdrant::QueryPoints) -> Result<qdrant::QueryResponse, QqlError> {
        let mut cl = self.points_client();
        cl.query(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("query: {e}"), None))
    }

    pub async fn query_groups(
        &self,
        req: qdrant::QueryPointGroups,
    ) -> Result<qdrant::QueryGroupsResponse, QqlError> {
        let mut cl = self.points_client();
        cl.query_groups(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("query_groups: {e}"), None))
    }

    pub async fn query_batch(
        &self,
        req: qdrant::QueryBatchPoints,
    ) -> Result<qdrant::QueryBatchResponse, QqlError> {
        let mut cl = self.points_client();
        cl.query_batch(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("query_batch: {e}"), None))
    }

    pub async fn update_batch(
        &self,
        req: qdrant::UpdateBatchPoints,
    ) -> Result<qdrant::UpdateBatchResponse, QqlError> {
        let mut cl = self.points_client();
        cl.update_batch(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("update_batch: {e}"), None))
    }

    pub async fn get_points(
        &self,
        req: qdrant::GetPoints,
    ) -> Result<qdrant::GetResponse, QqlError> {
        let mut cl = self.points_client();
        cl.get(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("get_points: {e}"), None))
    }

    pub async fn scroll(
        &self,
        req: qdrant::ScrollPoints,
    ) -> Result<qdrant::ScrollResponse, QqlError> {
        let mut cl = self.points_client();
        cl.scroll(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("scroll: {e}"), None))
    }

    pub async fn upsert_points(
        &self,
        req: qdrant::UpsertPoints,
    ) -> Result<qdrant::PointsOperationResponse, QqlError> {
        let mut cl = self.points_client();
        cl.upsert(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("upsert: {e}"), None))
    }

    pub async fn delete_points(
        &self,
        req: qdrant::DeletePoints,
    ) -> Result<qdrant::PointsOperationResponse, QqlError> {
        let mut cl = self.points_client();
        cl.delete(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("delete: {e}"), None))
    }

    pub async fn update_vectors(
        &self,
        req: qdrant::UpdatePointVectors,
    ) -> Result<qdrant::PointsOperationResponse, QqlError> {
        let mut cl = self.points_client();
        cl.update_vectors(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("update_vectors: {e}"), None))
    }

    pub async fn set_payload(
        &self,
        req: qdrant::SetPayloadPoints,
    ) -> Result<qdrant::PointsOperationResponse, QqlError> {
        let mut cl = self.points_client();
        cl.set_payload(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("set_payload: {e}"), None))
    }

    pub async fn create_collection_raw(
        &self,
        req: qdrant::CreateCollection,
    ) -> Result<qdrant::CollectionOperationResponse, QqlError> {
        let mut cl = self.collections_client();
        cl.create(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("create_collection: {e}"), None))
    }

    pub async fn update_collection_raw(
        &self,
        req: qdrant::UpdateCollection,
    ) -> Result<qdrant::CollectionOperationResponse, QqlError> {
        let mut cl = self.collections_client();
        cl.update(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("update_collection: {e}"), None))
    }

    pub async fn delete_collection_raw(
        &self,
        req: qdrant::DeleteCollection,
    ) -> Result<qdrant::CollectionOperationResponse, QqlError> {
        let mut cl = self.collections_client();
        cl.delete(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("delete_collection: {e}"), None))
    }

    pub async fn create_field_index(
        &self,
        req: qdrant::CreateFieldIndexCollection,
    ) -> Result<qdrant::PointsOperationResponse, QqlError> {
        let mut cl = self.points_client();
        cl.create_field_index(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("create_field_index: {e}"), None))
    }

    pub async fn delete_field_index(
        &self,
        req: qdrant::DeleteFieldIndexCollection,
    ) -> Result<qdrant::PointsOperationResponse, QqlError> {
        let mut cl = self.points_client();
        cl.delete_field_index(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("delete_field_index: {e}"), None))
    }

    pub async fn count_points(
        &self,
        req: qdrant::CountPoints,
    ) -> Result<qdrant::CountResponse, QqlError> {
        let mut cl = self.points_client();
        cl.count(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("count: {e}"), None))
    }

    pub async fn clear_payload(
        &self,
        req: qdrant::ClearPayloadPoints,
    ) -> Result<qdrant::PointsOperationResponse, QqlError> {
        let mut cl = self.points_client();
        cl.clear_payload(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("clear_payload: {e}"), None))
    }

    pub async fn delete_vectors(
        &self,
        req: qdrant::DeletePointVectors,
    ) -> Result<qdrant::PointsOperationResponse, QqlError> {
        let mut cl = self.points_client();
        cl.delete_vectors(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("delete_vectors: {e}"), None))
    }

    pub async fn create_shard_key(
        &self,
        req: qdrant::CreateShardKeyRequest,
    ) -> Result<qdrant::CreateShardKeyResponse, QqlError> {
        let mut cl = self.collections_client();
        cl.create_shard_key(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("create_shard_key: {e}"), None))
    }

    pub async fn delete_shard_key(
        &self,
        req: qdrant::DeleteShardKeyRequest,
    ) -> Result<qdrant::DeleteShardKeyResponse, QqlError> {
        let mut cl = self.collections_client();
        cl.delete_shard_key(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("delete_shard_key: {e}"), None))
    }

    pub async fn list_shard_keys(
        &self,
        req: qdrant::ListShardKeysRequest,
    ) -> Result<qdrant::ListShardKeysResponse, QqlError> {
        let mut cl = self.collections_client();
        cl.list_shard_keys(tonic::Request::new(req))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("list_shard_keys: {e}"), None))
    }

    pub async fn list_collections_raw(&self) -> Result<qdrant::ListCollectionsResponse, QqlError> {
        let mut cl = self.collections_client();
        cl.list(tonic::Request::new(qdrant::ListCollectionsRequest {}))
            .await
            .map(|r| r.into_inner())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("list_collections: {e}"), None))
    }

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
        let mut cl = self.collections_client();
        match cl
            .collection_exists(tonic::Request::new(qdrant::CollectionExistsRequest {
                collection_name: name.to_string(),
            }))
            .await
        {
            Ok(resp) => Ok(resp.into_inner().result.map(|r| r.exists).unwrap_or(false)),
            Err(status) if status.code() == tonic::Code::NotFound => Ok(false),
            Err(e) => Err(QqlError::backend(
                "QQL-GRPC",
                format!("collection_exists: {e}"),
                None,
            )),
        }
    }

    async fn get_collection_info(&self, name: &str) -> Result<CollectionInfo, QqlError> {
        let resp = self.collection_info_raw(name.to_string()).await?;
        let info = resp
            .result
            .ok_or_else(|| QqlError::backend("QQL-GRPC", "collection_info: no result", None))?;

        // Extract vector names from the protobuf response so the executor can
        // validate that `USING` is present for named-vector collections.
        let mut schema = CollectionSchema::default();
        if let Some(config) = &info.config {
            if let Some(params) = &config.params {
                if let Some(vc) = &params.vectors_config {
                    if let Some(vc_cfg) = &vc.config {
                        if let qdrant::vectors_config::Config::ParamsMap(map) = vc_cfg {
                            schema.dense_vectors = map.map.keys().cloned().collect();
                        }
                    }
                }
                if let Some(sparse) = &params.sparse_vectors_config {
                    schema.sparse_vectors = sparse.map.keys().cloned().collect();
                }
            }
        }

        Ok(CollectionInfo {
            status: info.status.to_string(),
            points_count: info.points_count.unwrap_or(0),
            segments_count: 0,
            schema,
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

    async fn delete_field_index(
        &self,
        _collection_name: &str,
        _field_name: &str,
    ) -> Result<(), QqlError> {
        Err(QqlError::execution(
            "QQL-EXECUTION",
            "delete_field_index: use execute_route for gRPC",
            None,
        ))
    }

    async fn execute_route(
        &self,
        route: qql_plan::routing::Route,
    ) -> Result<serde_json::Value, QqlError> {
        crate::grpc_route::execute_grpc_route(self, route).await
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
