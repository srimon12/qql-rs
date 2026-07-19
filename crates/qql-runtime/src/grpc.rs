//! gRPC transport integration backed by the official `qdrant-client` crate.

use qdrant_client::Qdrant;

use qql_core::error::QqlError;

/// Owns, or reuses, an official Qdrant gRPC client.
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
            QqlError::runtime(format!("failed to build Qdrant gRPC client: {error}"))
        })?;
        Ok(Self { client })
    }

    pub fn client(&self) -> &Qdrant {
        &self.client
    }
}
