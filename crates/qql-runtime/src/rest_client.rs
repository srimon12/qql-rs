//! REST client construction for Qdrant (`RestQdrant`).
//!
//! Split from `rest.rs` (size hygiene): the `RestQdrant` struct, its header
//! pre-parsing, and every constructor. Request execution (`call_*`,
//! `execute_typed`, `get_stream`), the `QdrantOps` implementation, and the
//! envelope helpers stay in `rest`. Behavior is unchanged.

use std::time::Duration;

use reqwest::{Client, header::HeaderValue};

use qql_core::error::QqlError;

/// HTTP header for Qdrant 1.19 read affinity (`X-Qdrant-Route-Affinity`).
pub const ROUTE_AFFINITY_HEADER: &str = "X-Qdrant-Route-Affinity";

/// REST transport adapter implementing the `QdrantOps` backend contract over
/// the Qdrant HTTP API.
#[derive(Clone)]
pub struct RestQdrant {
    pub(crate) base_url: String,
    pub(crate) api_key: Option<String>,
    /// Stable value hashed by Qdrant to pin reads to one replica (session /
    /// user id). Sent as [`ROUTE_AFFINITY_HEADER`] on every request.
    pub(crate) route_affinity: Option<String>,
    /// Pre-parsed header values for the per-request headers above: parsing
    /// once at construction instead of on every request. `None` when the
    /// value is unset (or failed to parse, in which case sending falls back
    /// to the runtime `&str` path with identical behavior).
    pub(crate) api_key_header: Option<HeaderValue>,
    pub(crate) affinity_header: Option<HeaderValue>,
    pub(crate) client: Client,
}

/// Parse a header value once at construction; `None` keeps the per-request
/// `&str` fallback (identical behavior, just parses per call).
fn preparse_header(value: Option<&str>) -> Option<HeaderValue> {
    value.and_then(|v| HeaderValue::from_str(v).ok())
}

/// Redacts the API key: adapters surface in logs and error contexts.
impl std::fmt::Debug for RestQdrant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RestQdrant")
            .field("base_url", &self.base_url)
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field("route_affinity", &self.route_affinity)
            .field(
                "api_key_header",
                &self.api_key_header.as_ref().map(|_| "<redacted>"),
            )
            .field(
                "affinity_header",
                &self.affinity_header.as_ref().map(|_| "<redacted>"),
            )
            .field("client", &self.client)
            .finish()
    }
}

impl RestQdrant {
    /// Construct with a 30s request timeout.
    ///
    /// Never panics (RUN-015 / RUN-010). If the timed client builder fails
    /// (effectively unreachable on stock reqwest), falls back to
    /// [`Client::new`]. Prefer [`Self::with_timeout`] when client-build
    /// failures must surface as errors.
    pub fn new(base_url: impl Into<String>, api_key: Option<String>) -> Self {
        let base_url = base_url.into();
        Self::with_timeout(base_url.clone(), api_key.clone(), Duration::from_secs(30))
            .unwrap_or_else(|_| Self::with_client(base_url, api_key, Client::new()))
    }

    /// Construct with an explicit request timeout. Fallible so library
    /// callers can surface client-build failures without panicking.
    pub fn with_timeout(
        base_url: impl Into<String>,
        api_key: Option<String>,
        timeout: Duration,
    ) -> Result<Self, QqlError> {
        let base_url = base_url.into();
        let client = Client::builder()
            .timeout(timeout)
            .connect_timeout(Duration::from_secs(10).min(timeout))
            .build()
            .map_err(|e| {
                QqlError::transport(
                    "QQL-TRANSPORT-BUILD",
                    format!("failed to build HTTP client: {e}"),
                    None,
                )
            })?;
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key_header: preparse_header(api_key.as_deref()),
            affinity_header: None,
            api_key,
            route_affinity: None,
            client,
        })
    }

    /// Construct from a pre-built [`Client`] (the panic-free fallback used by
    /// [`Self::new`]).
    pub fn with_client(base_url: String, api_key: Option<String>, client: Client) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key_header: preparse_header(api_key.as_deref()),
            affinity_header: None,
            api_key,
            route_affinity: None,
            client,
        }
    }

    /// Construct with a per-read (stall) timeout and **no total request cap**.
    ///
    /// For streaming opaque bodies (shard snapshots) the 30 s default total
    /// timeout would abort a multi-GB transfer mid-stream, while dropping the
    /// timeout entirely would hang forever on a dead peer. `read_timeout`
    /// bounds each read operation instead: the transfer may take as long as it
    /// makes progress and fails once no bytes arrive for `read_timeout`.
    pub fn with_read_timeout(
        base_url: impl Into<String>,
        api_key: Option<String>,
        read_timeout: Duration,
    ) -> Result<Self, QqlError> {
        let base_url = base_url.into();
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(read_timeout)
            .build()
            .map_err(|e| {
                QqlError::transport(
                    "QQL-TRANSPORT-BUILD",
                    format!("failed to build HTTP client: {e}"),
                    None,
                )
            })?;
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key_header: preparse_header(api_key.as_deref()),
            affinity_header: None,
            api_key,
            route_affinity: None,
            client,
        })
    }

    /// Pin subsequent reads to a stable replica via `X-Qdrant-Route-Affinity`.
    ///
    /// See Qdrant docs: read affinity / consistency guarantees (v1.19+).
    /// Empty strings are treated as unset.
    pub fn with_route_affinity(mut self, affinity: impl Into<String>) -> Self {
        let value = affinity.into();
        self.route_affinity = if value.is_empty() { None } else { Some(value) };
        self.affinity_header = preparse_header(self.route_affinity.as_deref());
        self
    }

    /// Current read-affinity value, if one is set.
    pub fn route_affinity(&self) -> Option<&str> {
        self.route_affinity.as_deref()
    }
}
