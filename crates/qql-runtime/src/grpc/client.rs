//! [`GrpcQdrant`] handle: connection, auth metadata, client factories.

use tonic::transport::Channel;

use qql_core::error::QqlError;

use crate::qdrant_grpc::qdrant;

/// gRPC metadata key for Qdrant 1.19 read affinity (same string as the HTTP header).
pub const ROUTE_AFFINITY_METADATA: &str = "x-qdrant-route-affinity";

pub struct GrpcQdrant {
    channel: Channel,
    api_key: Option<String>,
    /// Stable value hashed by Qdrant to pin reads to one replica.
    route_affinity: Option<String>,
}

/// Interceptor that attaches API-key and optional route-affinity metadata.
#[derive(Clone)]
pub(crate) struct MetadataInterceptor {
    api_key: Option<String>,
    route_affinity: Option<String>,
}

impl tonic::service::Interceptor for MetadataInterceptor {
    fn call(
        &mut self,
        mut request: tonic::Request<()>,
    ) -> Result<tonic::Request<()>, tonic::Status> {
        if let Some(ref key) = self.api_key {
            let value = tonic::metadata::MetadataValue::try_from(key.as_str())
                .map_err(|e| tonic::Status::invalid_argument(format!("invalid api key: {e}")))?;
            request.metadata_mut().insert("api-key", value);
        }
        if let Some(ref affinity) = self.route_affinity {
            let value =
                tonic::metadata::MetadataValue::try_from(affinity.as_str()).map_err(|e| {
                    tonic::Status::invalid_argument(format!("invalid route affinity: {e}"))
                })?;
            request
                .metadata_mut()
                .insert(ROUTE_AFFINITY_METADATA, value);
        }
        Ok(request)
    }
}

impl GrpcQdrant {
    pub fn from_channel(channel: Channel) -> Self {
        Self {
            channel,
            api_key: None,
            route_affinity: None,
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

        Ok(Self {
            channel,
            api_key,
            route_affinity: None,
        })
    }

    /// Pin subsequent reads to a stable replica via gRPC metadata
    /// `x-qdrant-route-affinity` (Qdrant 1.19+). Empty → unset.
    pub fn with_route_affinity(mut self, affinity: impl Into<String>) -> Self {
        let value = affinity.into();
        self.route_affinity = if value.is_empty() { None } else { Some(value) };
        self
    }

    pub fn route_affinity(&self) -> Option<&str> {
        self.route_affinity.as_deref()
    }

    pub fn channel(&self) -> Channel {
        self.channel.clone()
    }

    pub(crate) fn interceptor(&self) -> MetadataInterceptor {
        MetadataInterceptor {
            api_key: self.api_key.clone(),
            route_affinity: self.route_affinity.clone(),
        }
    }

    pub(crate) fn points_client(
        &self,
    ) -> qdrant::points_client::PointsClient<
        tonic::service::interceptor::InterceptedService<Channel, MetadataInterceptor>,
    > {
        qdrant::points_client::PointsClient::with_interceptor(
            self.channel.clone(),
            self.interceptor(),
        )
    }

    pub(crate) fn collections_client(
        &self,
    ) -> qdrant::collections_client::CollectionsClient<
        tonic::service::interceptor::InterceptedService<Channel, MetadataInterceptor>,
    > {
        qdrant::collections_client::CollectionsClient::with_interceptor(
            self.channel.clone(),
            self.interceptor(),
        )
    }
}
