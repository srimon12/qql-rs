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
use serde::Deserialize;
use tokio::io::AsyncWriteExt;

use crate::rest::RestQdrant;

/// Per-read stall timeout for snapshot streams.
///
/// A shard snapshot is streamed with **no total request cap** — multi-GB
/// archives on slow links may legitimately take many minutes. The transfer
/// only fails once no bytes arrive for this long, which catches dead peers and
/// half-open connections without capping the total download time.
pub const SNAPSHOT_READ_TIMEOUT: Duration = Duration::from_secs(120);

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
/// One instance targets one node. The HTTP transport is the shared
/// [`RestQdrant`] adapter — headers, request ids, and backend error
/// classification are identical to every other REST route — configured for
/// streaming: no total request cap, only the
/// [`SNAPSHOT_READ_TIMEOUT`] stall timeout.
#[derive(Debug, Clone)]
pub struct RemoteSnapshotClient {
    rest: RestQdrant,
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
        let rest = RestQdrant::with_read_timeout(base_url, api_key, SNAPSHOT_READ_TIMEOUT)?;
        Ok(Self { rest })
    }

    /// Fetch the collection's shard distribution (`GET /collections/{c}/cluster`).
    pub async fn list_shards(&self, collection: &str) -> Result<RemoteShardListing, QqlError> {
        let value = self
            .rest
            .get_value(&format!("/collections/{collection}/cluster"))
            .await
            .map_err(|error| error.with_collection(collection.to_string()))?;
        let body: ClusterInfoBody = serde_json::from_value(
            value
                .get("result")
                .cloned()
                .ok_or_else(|| envelope_error("shard discovery response is missing result"))?,
        )
        .map_err(|error| {
            envelope_error(format!(
                "shard discovery response is not a valid cluster info body: {error}"
            ))
        })?;
        let local_shard_ids: Vec<u32> = body
            .local_shards
            .iter()
            .map(|shard| shard.shard_id)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let remote_shard_ids: Vec<u32> = body
            .remote_shards
            .iter()
            .map(|shard| shard.shard_id)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        Ok(RemoteShardListing {
            local_shard_ids,
            remote_shard_ids,
            shard_count: body.shard_count,
        })
    }

    /// Stream `GET /collections/{c}/shards/{shard_id}/snapshot` into `dest`.
    ///
    /// The file is written chunk by chunk; the returned count is the number of
    /// bytes written. `dest`'s parent directory must exist.
    ///
    /// When the server sends `Content-Length`, the written byte count is
    /// verified against it and a short stream fails closed with
    /// `QQL-SNAPSHOT-IO` (the partial file is removed). The body is never
    /// parsed: a snapshot is an opaque archive.
    pub async fn download_shard_snapshot(
        &self,
        collection: &str,
        shard_id: u32,
        dest: &Path,
    ) -> Result<u64, QqlError> {
        let path = format!("/collections/{collection}/shards/{shard_id}/snapshot");
        let mut response = self
            .rest
            .get_stream(&path)
            .await
            .map_err(|error| error.with_collection(collection.to_string()))?;
        let expected = response.content_length();

        let mut file = tokio::fs::File::create(dest).await.map_err(|error| {
            QqlError::execution(
                "QQL-SNAPSHOT-IO",
                format!("cannot create snapshot file {}: {error}", dest.display()),
                None,
            )
        })?;
        let mut written: u64 = 0;
        loop {
            let chunk = match response.chunk().await {
                Ok(chunk) => chunk,
                Err(error) => {
                    // A truncated body can surface as a decode error rather
                    // than a clean EOF; prefer the declared-length verdict so
                    // incomplete snapshots always fail as QQL-SNAPSHOT-IO.
                    if let Some(expected) = expected
                        && written != expected
                    {
                        let _ = tokio::fs::remove_file(dest).await;
                        return Err(truncated_error(dest, expected, written, &error.to_string()));
                    }
                    return Err(QqlError::transport(
                        "QQL-TRANSPORT",
                        format!("snapshot download interrupted: {error}"),
                        None,
                    ));
                }
            };
            let Some(chunk) = chunk else {
                break;
            };
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
        if let Some(expected) = expected
            && written != expected
        {
            let _ = tokio::fs::remove_file(dest).await;
            return Err(truncated_error(
                dest,
                expected,
                written,
                "stream ended early",
            ));
        }
        Ok(written)
    }
}

fn truncated_error(dest: &Path, expected: u64, written: u64, detail: &str) -> QqlError {
    QqlError::execution(
        "QQL-SNAPSHOT-IO",
        format!(
            "snapshot download truncated: received {written} of {expected} declared bytes \
             ({detail}); removed the partial file at {}",
            dest.display()
        ),
        None,
    )
}

fn envelope_error(message: impl Into<String>) -> QqlError {
    QqlError::backend("QQL-BACKEND-ENVELOPE", message.into(), None)
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

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::path::PathBuf;
    use std::thread;

    use super::*;

    /// Serve exactly one HTTP/1.1 response, then close the connection.
    fn serve_once(reply: impl FnOnce() -> Vec<u8> + Send + 'static) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test listener");
        let addr = listener.local_addr().expect("listener addr");
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept test connection");
            let mut request = [0u8; 4096];
            let _ = stream.read(&mut request);
            let _ = stream.write_all(&reply());
            let _ = stream.flush();
        });
        format!("http://{addr}")
    }

    fn raw_http(status: &str, headers: &str, body: &[u8]) -> Vec<u8> {
        let mut out =
            format!("HTTP/1.1 {status}\r\n{headers}Connection: close\r\n\r\n").into_bytes();
        out.extend_from_slice(body);
        out
    }

    fn temp_snapshot_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("qql-snapshot-test-{}-{name}", std::process::id()))
    }

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

    #[tokio::test]
    async fn downloads_snapshot_and_returns_byte_count() {
        let body = b"snapshot archive bytes".to_vec();
        let expected = body.clone();
        let url = serve_once(move || {
            raw_http(
                "200 OK",
                &format!("Content-Length: {}\r\n", body.len()),
                &body,
            )
        });
        let client = RemoteSnapshotClient::new(url, None).expect("client");
        let dest = temp_snapshot_path("download.snapshot");
        let written = client
            .download_shard_snapshot("docs", 0, &dest)
            .await
            .expect("download");
        assert_eq!(written, expected.len() as u64);
        assert_eq!(std::fs::read(&dest).expect("read file"), expected);
        let _ = std::fs::remove_file(&dest);
    }

    #[tokio::test]
    async fn declared_length_mismatch_fails_closed() {
        // Content-Length promises 64 bytes but only 7 arrive before the
        // connection closes: the partial file must not survive.
        let url = serve_once(|| raw_http("200 OK", "Content-Length: 64\r\n", b"partial"));
        let client = RemoteSnapshotClient::new(url, None).expect("client");
        let dest = temp_snapshot_path("truncated.snapshot");
        let error = client
            .download_shard_snapshot("docs", 0, &dest)
            .await
            .expect_err("truncated download must fail");
        assert_eq!(error.code, "QQL-SNAPSHOT-IO", "{error}");
        assert!(error.message.contains("truncated"), "{}", error.message);
        assert!(!dest.exists(), "partial snapshot file must be removed");
    }

    #[tokio::test]
    async fn list_shards_parses_cluster_envelope() {
        let url = serve_once(|| {
            let body = br#"{"result":{"local_shards":[{"shard_id":1},{"shard_id":0}],"remote_shards":[{"shard_id":2}],"shard_count":3},"status":"ok","time":0.001}"#;
            raw_http(
                "200 OK",
                &format!("Content-Length: {}\r\n", body.len()),
                body,
            )
        });
        let client = RemoteSnapshotClient::new(url, None).expect("client");
        let listing = client.list_shards("docs").await.expect("list shards");
        assert_eq!(listing.local_shard_ids, vec![0, 1]);
        assert_eq!(listing.remote_shard_ids, vec![2]);
        assert_eq!(listing.shard_count, 3);
    }

    #[tokio::test]
    async fn http_errors_reuse_backend_classification() {
        let client = RemoteSnapshotClient::new(
            serve_once(|| {
                raw_http(
                    "404 Not Found",
                    "Content-Type: application/json\r\n",
                    br#"{"status":{"error":"Not found: Collection docs not found"}}"#,
                )
            }),
            None,
        )
        .expect("client");
        let error = client.list_shards("docs").await.expect_err("404");
        assert_eq!(error.code, "QQL-BACKEND-COLLECTION-NOT-FOUND");

        let client = RemoteSnapshotClient::new(
            serve_once(|| {
                raw_http(
                    "404 Not Found",
                    "Content-Type: application/json\r\n",
                    br#"{"status":{"error":"Not found: Collection docs not found"}}"#,
                )
            }),
            None,
        )
        .expect("client");
        let error = client
            .download_shard_snapshot("docs", 0, &temp_snapshot_path("404.snapshot"))
            .await
            .expect_err("404");
        assert_eq!(error.code, "QQL-BACKEND-COLLECTION-NOT-FOUND");
    }
}
