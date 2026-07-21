use async_trait::async_trait;
use qdrant_client::Qdrant;

use crate::client::{CollectionInfo, CreateCollectionReq, CreateFieldIndexReq, QdrantOps};
use qql_core::error::QqlError;

pub struct GrpcQdrant {
    client: Qdrant,
}

impl GrpcQdrant {
    pub fn from_client(client: Qdrant) -> Self {
        Self { client }
    }

    pub fn from_url(url: &str, api_key: Option<String>) -> Result<Self, QqlError> {
        let mut builder = Qdrant::from_url(url);
        if let Some(api_key) = api_key {
            builder = builder.api_key(api_key);
        }
        let client = builder.build().map_err(|error| {
            QqlError::transport(
                "QQL-TRANSPORT",
                format!("failed to build Qdrant gRPC client: {error}"),
                None,
            )
        })?;
        Ok(Self { client })
    }

    pub fn client(&self) -> &Qdrant {
        &self.client
    }
}

#[async_trait]
impl QdrantOps for GrpcQdrant {
    async fn list_collections(&self) -> Result<Vec<String>, QqlError> {
        let resp =
            self.client.list_collections().await.map_err(|e| {
                QqlError::backend("QQL-GRPC", format!("list_collections: {e}"), None)
            })?;
        Ok(resp.collections.into_iter().map(|c| c.name).collect())
    }

    async fn collection_exists(&self, name: &str) -> Result<bool, QqlError> {
        self.client
            .collection_exists(name)
            .await
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("collection_exists: {e}"), None))
    }

    async fn get_collection_info(&self, name: &str) -> Result<CollectionInfo, QqlError> {
        let resp = self
            .client
            .collection_info(qdrant_client::qdrant::GetCollectionInfoRequest {
                collection_name: name.to_string(),
            })
            .await
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("collection_info: {e}"), None))?;
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
                        "Cosine" => qdrant_client::qdrant::Distance::Cosine as i32,
                        "Euclid" => qdrant_client::qdrant::Distance::Euclid as i32,
                        "Dot" => qdrant_client::qdrant::Distance::Dot as i32,
                        "Manhattan" => qdrant_client::qdrant::Distance::Manhattan as i32,
                        _ => qdrant_client::qdrant::Distance::Cosine as i32,
                    })
                    .unwrap_or(qdrant_client::qdrant::Distance::Cosine as i32);
                map.insert(
                    name.clone(),
                    qdrant_client::qdrant::VectorParams {
                        size,
                        distance: dist,
                        ..Default::default()
                    },
                );
            }
            Some(qdrant_client::qdrant::VectorsConfig {
                config: Some(qdrant_client::qdrant::vectors_config::Config::ParamsMap(
                    qdrant_client::qdrant::VectorParamsMap { map },
                )),
            })
        });

        self.client
            .create_collection(qdrant_client::qdrant::CreateCollection {
                collection_name: req.collection_name,
                vectors_config,
                ..Default::default()
            })
            .await
            .map(|_| ())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("create_collection: {e}"), None))
    }

    async fn update_collection(&self, _req: serde_json::Value) -> Result<(), QqlError> {
        Err(QqlError::execution(
            "QQL-EXECUTION",
            "update_collection: use execute_route for gRPC",
            None,
        ))
    }

    async fn delete_collection(&self, name: &str) -> Result<(), QqlError> {
        self.client
            .delete_collection(qdrant_client::qdrant::DeleteCollection {
                collection_name: name.to_string(),
                ..Default::default()
            })
            .await
            .map(|_| ())
            .map_err(|e| QqlError::backend("QQL-GRPC", format!("delete_collection: {e}"), None))
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
        crate::grpc_route::execute_grpc_route(&self.client, route).await
    }
}
