//! [`GrpcQdrant`] handle: connection, auth metadata, client factories.

use tonic::transport::Channel;

use qql_core::error::QqlError;

use crate::qdrant_grpc::qdrant;

/// gRPC metadata key for Qdrant 1.19 read affinity (same string as the HTTP header).
pub const ROUTE_AFFINITY_METADATA: &str = "x-qdrant-route-affinity";

/// gRPC metadata key for request correlation (`x-request-id`).
pub const REQUEST_ID_METADATA: &str = crate::client::REQUEST_ID_HEADER;

/// Qdrant gRPC backend handle: `tonic` channel plus API-key and route-affinity
/// metadata, with typed points/collections client factories.
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
        // Per-request correlation id — Qdrant echoes it into its log lines,
        // matching the REST adapter's `x-request-id` header.
        let request_id = crate::client::next_request_id();
        if let Ok(value) = tonic::metadata::MetadataValue::try_from(request_id.as_str()) {
            request.metadata_mut().insert(REQUEST_ID_METADATA, value);
        }
        Ok(request)
    }
}

impl GrpcQdrant {
    /// Wrap an existing `tonic` channel without credentials.
    pub fn from_channel(channel: Channel) -> Self {
        Self {
            channel,
            api_key: None,
            route_affinity: None,
        }
    }

    /// Connect lazily to a `grpc://` / `http(s)://` Qdrant endpoint with an
    /// optional API key.
    pub fn from_url(url: &str, api_key: Option<String>) -> Result<Self, QqlError> {
        Self::from_url_with_timeout(url, api_key, None)
    }

    /// Like `from_url`, with an explicit overall request timeout on the endpoint.
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

    /// Current route-affinity value, if set.
    pub fn route_affinity(&self) -> Option<&str> {
        self.route_affinity.as_deref()
    }

    /// Clone the underlying `tonic` channel for custom clients.
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

#[cfg(test)]
mod repro_tests {
    use super::*;
    use crate::client::QdrantOps;
    use qql_plan::{PlanPointId, PlannedOperation, PointsRequest};

    fn get_points_op() -> PlannedOperation {
        PlannedOperation::GetPoints {
            collection: "docs".into(),
            request: PointsRequest {
                ids: vec![PlanPointId::Number(1)],
                with_payload: None,
                with_vector: None,
                shard_key: None,
            },
        }
    }

    /// Contract pin: tonic's `connect_lazy` captures the tokio reactor at
    /// construction (hyper-util timer handle), so `GrpcQdrant::from_url` MUST
    /// run with a tokio runtime entered. Hosts that construct on a foreign
    /// thread (PyO3 / N-API constructors) enter their driving runtime first —
    /// see `crates/pyqql/src/embedder.rs` and `crates/nqql/src/lib.rs`.
    #[test]
    #[should_panic(expected = "no reactor running")]
    fn construct_outside_runtime_is_a_programming_error() {
        let _ = GrpcQdrant::from_url("http://127.0.0.1:1", None);
    }

    /// The fixed flow: the owning runtime is entered while the channel is
    /// built (what pyqql must do), so connect_lazy's tasks bind to it.
    #[test]
    fn construct_inside_entered_runtime_then_call() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let client = {
            let _enter = rt.enter();
            GrpcQdrant::from_url("http://127.0.0.1:1", None)
                .expect("construction must not panic with the runtime entered")
        };
        let result = rt.block_on(client.execute_planned(&get_points_op()));
        assert!(
            result.is_err(),
            "unreachable endpoint must error, got {result:?}"
        );
    }
}
