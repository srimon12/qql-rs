//! Remote shard-snapshot access for edge bootstrap (REST only).
//!
//! Deliberately outside [`QdrantOps`](crate::client::QdrantOps): snapshots are
//! opaque tar streams served by a file endpoint, not JSON route responses, and
//! only the REST transport can serve them. This client streams a shard snapshot
//! to a local file so the caller can hand it to the engine's snapshot API
//! (`qdrant_edge::EdgeShard::unpack_snapshot`, wrapped by `qql-edge`) — the
//! archive is never unpacked or merged by hand.
//!
//! Official seed flow (Qdrant Edge sync guide): stream
//! `GET /collections/{c}/shards/{shard_id}/snapshot`, unpack it into the edge
//! data directory, then use the shard. The snapshot carries the source
//! collection's configuration, built HNSW indexes and quantized data, so the
//! device does not have to re-index.

use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;

use qql_core::error::QqlError;
use reqwest::Client;
use serde::Deserialize;
use tokio::io::AsyncWriteExt;

use crate::rest::classify_backend_error_code;

/// Summary of a remote collection's shard distribution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteShardListing {
    /// Shard ids hosted on the node that answered, sorted and deduplicated.
    pub local_shard_ids: Vec<u32>,
    /// Shard ids hosted on other nodes, sorted and deduplicated.
    pub remote_shard_ids: Vec<u32>,
    /// Total shard count reported by the cluster info endpoint.
    pub shard_count: u64,
}

#[derive(Deserialize)]
struct ClusterInfoEnvelope {
    result: ClusterInfoBody,
}

#[derive(Deserialize)]
struct ClusterInfoBody {
    #[serde(default)]
    local_shards: Vec<LocalShard>,
    #[serde(default)]
    remote_shards: Vec<RemoteShard>,
    #[serde(default)]
    shard_count: u64,
}

#[derive(Deserialize)]
struct LocalShard {
    shard_id: u32,
}

#[derive(Deserialize)]
struct RemoteShard {
    shard_id: u32,
}

/// REST client for the snapshot endpoints of a remote Qdrant node.
///
/// One instance targets one node. Timeouts are generous: a shard snapshot is
/// streamed in chunks and can be large.
#[derive(Debug, Clone)]
pub struct RemoteSnapshotClient {
    client: Client,
    base_url: String,
    api_key: Option<String>,
}

impl RemoteSnapshotClient {
    /// Build a client for `base_url` (trailing slash optional).
    ///
    /// Fails closed when the URL is empty. `api_key` is sent as the `api-key`
    /// header on every request.
    pub fn new(base_url: impl Into<String>, api_key: Option<String>) -> Result<Self, QqlError> {
        let base_url = base_url.into().trim().trim_end_matches('/').to_string();
        if base_url.is_empty() {
            return Err(QqlError::execution(
                "QQL-SNAPSHOT-URL",
                "remote URL for snapshot bootstrap is empty; pass --from <url> or set --url",
                None,
            ));
        }
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(600))
            .build()
            .map_err(|error| {
                QqlError::transport(
                    "QQL-TRANSPORT",
                    format!("failed to build snapshot HTTP client: {error}"),
                    None,
                )
            })?;
        Ok(Self {
            client,
            base_url,
            api_key,
        })
    }

    fn get(&self, url: &str) -> reqwest::RequestBuilder {
        let mut request = self.client.get(url);
        if let Some(key) = self.api_key.as_deref()
            && !key.is_empty()
        {
            request = request.header("api-key", key);
        }
        request
    }

    /// Fetch the collection's shard distribution (`GET /collections/{c}/cluster`).
    pub async fn list_shards(&self, collection: &str) -> Result<RemoteShardListing, QqlError> {
        let url = format!("{}/collections/{collection}/cluster", self.base_url);
        let response = self.get(&url).send().await.map_err(|error| {
            QqlError::transport(
                "QQL-TRANSPORT",
                format!("shard discovery request failed: {error}"),
                None,
            )
            .with_url(url.clone())
        })?;
        let status = response.status();
        let body = response.text().await.map_err(|error| {
            QqlError::backend(
                "QQL-BACKEND",
                format!("failed to read shard discovery response: {error}"),
                None,
            )
            .with_url(url.clone())
        })?;
        if !status.is_success() {
            return Err(http_error(status.as_u16(), &body, collection, &url));
        }
        let envelope: ClusterInfoEnvelope = serde_json::from_str(&body).map_err(|error| {
            QqlError::execution(
                "QQL-BACKEND-JSON",
                format!("shard discovery response is not valid JSON: {error}"),
                None,
            )
            .with_url(url.clone())
        })?;
        let local_shard_ids: Vec<u32> = envelope
            .result
            .local_shards
            .iter()
            .map(|shard| shard.shard_id)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let remote_shard_ids: Vec<u32> = envelope
            .result
            .remote_shards
            .iter()
            .map(|shard| shard.shard_id)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        Ok(RemoteShardListing {
            local_shard_ids,
            remote_shard_ids,
            shard_count: envelope.result.shard_count,
        })
    }

    /// Stream `GET /collections/{c}/shards/{shard_id}/snapshot` into `dest`.
    ///
    /// The file is written chunk by chunk; the returned count is the number of
    /// bytes written. `dest`'s parent directory must exist.
    pub async fn download_shard_snapshot(
        &self,
        collection: &str,
        shard_id: u32,
        dest: &Path,
    ) -> Result<u64, QqlError> {
        let url = format!(
            "{}/collections/{collection}/shards/{shard_id}/snapshot",
            self.base_url
        );
        let mut response = self.get(&url).send().await.map_err(|error| {
            QqlError::transport(
                "QQL-TRANSPORT",
                format!("snapshot download failed: {error}"),
                None,
            )
            .with_url(url.clone())
        })?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(http_error(status.as_u16(), &body, collection, &url));
        }

        let mut file = tokio::fs::File::create(dest).await.map_err(|error| {
            QqlError::execution(
                "QQL-SNAPSHOT-IO",
                format!("cannot create snapshot file {}: {error}", dest.display()),
                None,
            )
        })?;
        let mut written: u64 = 0;
        while let Some(chunk) = response.chunk().await.map_err(|error| {
            QqlError::transport(
                "QQL-TRANSPORT",
                format!("snapshot download interrupted: {error}"),
                None,
            )
            .with_url(url.clone())
        })? {
            file.write_all(&chunk).await.map_err(|error| {
                QqlError::execution(
                    "QQL-SNAPSHOT-IO",
                    format!("cannot write snapshot file {}: {error}", dest.display()),
                    None,
                )
            })?;
            written += chunk.len() as u64;
        }
        file.flush().await.map_err(|error| {
            QqlError::execution(
                "QQL-SNAPSHOT-IO",
                format!("cannot flush snapshot file {}: {error}", dest.display()),
                None,
            )
        })?;
        Ok(written)
    }
}

/// Choose the remote shard to stream.
///
/// A qdrant-edge collection is a single shard, so an unqualified bootstrap only
/// succeeds for a single-shard collection: with an explicit `shard_id` the shard
/// must be hosted by the target node, otherwise a single local shard is chosen
/// (zero remote shards). Anything else fails closed with the shard ids to pick
/// from.
pub fn select_shard_id(
    listing: &RemoteShardListing,
    explicit: Option<u32>,
) -> Result<u32, QqlError> {
    if let Some(shard_id) = explicit {
        if listing.local_shard_ids.contains(&shard_id) {
            return Ok(shard_id);
        }
        return Err(QqlError::execution(
            "QQL-SNAPSHOT-SHARD",
            format!(
                "shard {shard_id} is not hosted by the target node (local shard ids: {:?}); \
                 point --from at the node that hosts it",
                listing.local_shard_ids
            ),
            None,
        ));
    }
    let mut distinct: BTreeSet<u32> = listing.local_shard_ids.iter().copied().collect();
    distinct.extend(listing.remote_shard_ids.iter().copied());
    if distinct.len() == 1
        && let Some(&shard_id) = listing.local_shard_ids.first()
    {
        return Ok(shard_id);
    }
    Err(QqlError::execution(
        "QQL-SNAPSHOT-SHARD",
        format!(
            "collection has {} shard(s): local {:?}, remote {:?}; a qdrant-edge collection is a \
             single shard — pass --shard-id to seed one of them",
            listing.shard_count, listing.local_shard_ids, listing.remote_shard_ids
        ),
        None,
    ))
}

fn http_error(status: u16, body: &str, collection: &str, url: &str) -> QqlError {
    let limit = body.floor_char_boundary(4096);
    let detail = &body[..limit];
    let code = classify_backend_error_code(status, detail);
    QqlError::execution(
        code,
        format!("remote Qdrant returned {status}: {detail}"),
        None,
    )
    .with_url(url.to_string())
    .with_collection(collection.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listing(local: &[u32], remote: &[u32], total: u64) -> RemoteShardListing {
        RemoteShardListing {
            local_shard_ids: local.to_vec(),
            remote_shard_ids: remote.to_vec(),
            shard_count: total,
        }
    }

    #[test]
    fn single_shard_is_auto_selected() {
        assert_eq!(select_shard_id(&listing(&[0], &[], 1), None).unwrap(), 0);
        assert_eq!(select_shard_id(&listing(&[7], &[], 1), None).unwrap(), 7);
    }

    #[test]
    fn replicated_single_shard_is_auto_selected() {
        assert_eq!(select_shard_id(&listing(&[0], &[0], 1), None).unwrap(), 0);
    }

    #[test]
    fn multi_shard_without_override_fails_closed() {
        let error = select_shard_id(&listing(&[0, 1], &[], 2), None).expect_err("multi shard");
        assert_eq!(error.code, "QQL-SNAPSHOT-SHARD");
        assert!(error.message.contains("--shard-id"), "{}", error.message);

        let error = select_shard_id(&listing(&[0], &[1], 2), None).expect_err("remote shards");
        assert_eq!(error.code, "QQL-SNAPSHOT-SHARD");
    }

    #[test]
    fn explicit_shard_must_be_local() {
        assert_eq!(
            select_shard_id(&listing(&[0, 1], &[], 2), Some(1)).unwrap(),
            1
        );
        let error = select_shard_id(&listing(&[0], &[], 1), Some(9)).expect_err("not local");
        assert_eq!(error.code, "QQL-SNAPSHOT-SHARD");
        assert!(error.message.contains("shard 9"), "{}", error.message);
    }

    #[test]
    fn empty_url_fails_closed() {
        let error = RemoteSnapshotClient::new("  ", None).expect_err("empty url");
        assert_eq!(error.code, "QQL-SNAPSHOT-URL");
    }
}
