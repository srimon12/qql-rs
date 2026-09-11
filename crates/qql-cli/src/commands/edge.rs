//! Local qdrant-edge utilities: config, optimize, bootstrap.

pub fn handle_configure_edge(
    patch: crate::config::EdgeConfigPatch,
) -> Result<(), Box<dyn std::error::Error>> {
    // Validate the final merged state, not just the patch: a persisted
    // `embedder = http` keeps its persisted `embed_url` when a later update
    // only touches `wal_segment_mb`.
    let (config, merged) = crate::config::EdgeConfig::merged_with(&patch)?;
    if config.embedder != "fastembed" && config.embedder != "http" {
        return Err("edge embedder must be 'fastembed' or 'http'".into());
    }
    if config.embedder == "http" && config.embed_url.is_none() {
        return Err("--embed-url is required when --embedder http is selected".into());
    }
    if config.embed_dimension == 0 {
        return Err("--embed-dim must be greater than zero".into());
    }
    if config.wal_segment_mb == Some(0) {
        return Err(
            "--wal-segment-mb must be greater than zero; omit it for the qdrant-edge 32 MiB default"
                .into(),
        );
    }
    // Fail closed at save time: k1 > 0, b in [0, 1], avg_len > 0, all finite.
    qql::embedder::Bm25Params::resolve(config.bm25_k1, config.bm25_b, config.bm25_avg_len)?;
    let path = crate::config::EdgeConfig::write_object(&merged)?;
    println!("Saved edge configuration to {}", path.display());
    println!("Use it with: qql --edge exec \"SHOW COLLECTIONS\"");
    Ok(())
}

/// Per-collection indexing counters for the doctor / check readout.
#[derive(serde::Serialize)]
pub(crate) struct IndexingState {
    /// Collection name.
    pub(crate) collection: String,
    /// Point count from collection info.
    pub(crate) points_count: u64,
    /// Indexed vector count when the backend reports one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) indexed_vectors_count: Option<u64>,
    /// Segment count from collection info.
    pub(crate) segments_count: u64,
    /// Operator hint when indexed vectors lag points.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) nudge: Option<String>,
}

/// Read indexing counters for every collection and build the
/// "run `qql edge optimize`" nudge when indexed vectors lag points.
pub(crate) async fn collect_indexing_states(
    client: &dyn qql::client::QdrantOps,
) -> Result<Vec<IndexingState>, qql_core::error::QqlError> {
    let names = client.list_collections().await?;
    let mut states = Vec::new();
    for name in names {
        // A collection that disappears mid-scan is skipped, not fatal.
        let Ok(info) = client.get_collection_info(&name).await else {
            continue;
        };
        let nudge = match info.indexed_vectors_count {
            Some(indexed) if indexed < info.points_count => Some(format!(
                "collection '{name}': {} points, {} indexed — qdrant-edge only indexes during optimize; run `qql edge optimize {name}` after bulk writes (segments below indexing_threshold stay brute-force)",
                info.points_count, indexed
            )),
            _ => None,
        };
        states.push(IndexingState {
            collection: name,
            points_count: info.points_count,
            indexed_vectors_count: info.indexed_vectors_count,
            segments_count: info.segments_count,
            nudge,
        });
    }
    Ok(states)
}

/// Format an optional count for doctor/check output (`?` when unknown).
pub(crate) fn count_label(count: Option<u64>) -> String {
    count.map_or_else(|| "?".to_string(), |value| value.to_string())
}

#[cfg(feature = "edge")]
fn info_counts(info: &qql::client::CollectionInfo) -> serde_json::Value {
    serde_json::json!({
        "points": info.points_count,
        "indexed": info.indexed_vectors_count,
        "segments": info.segments_count,
    })
}

/// Run the qdrant-edge optimizers on a local collection.
///
/// Builds `EdgeQdrant` directly instead of going through the embedding-aware
/// executor, so no model is initialized or downloaded. qdrant-edge has no
/// background optimizer: this is what merges segments and builds HNSW/sparse
/// indexes.
#[cfg(feature = "edge")]
pub async fn handle_edge_optimize(
    collection: &str,
    json: bool,
    quiet: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    match edge_optimize_inner(collection, json, quiet).await {
        Ok(()) => Ok(()),
        Err(error) => {
            if json {
                print_edge_json_error("edge-optimize", collection, error.as_ref());
            }
            Err(error)
        }
    }
}

#[cfg(feature = "edge")]
async fn edge_optimize_inner(
    collection: &str,
    json: bool,
    quiet: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    use qql::client::QdrantOps;

    let config = crate::config::EdgeConfig::load()?.apply_environment();
    let wal_segment_capacity = qql_edge::wal_segment_capacity_bytes(config.wal_segment_mb)?;
    let backend = qql_edge::EdgeQdrant::new(config.data_dir, config.on_disk_payload)
        .with_wal_segment_capacity(wal_segment_capacity);

    let outcome = async {
        let before = backend.get_collection_info(collection).await?;
        let optimized = backend.optimize_collection(collection).await?;
        let after = backend.get_collection_info(collection).await?;
        Ok::<_, qql_core::error::QqlError>((before, optimized, after))
    }
    .await;
    let closed = backend.close().await;
    let (before, optimized, after) = outcome?;
    closed?;

    // The optimizer loop can return having merged segments without building
    // HNSW: the indexing optimizer skips segments below the collection's
    // `indexing_threshold`. Report that lag instead of claiming success.
    let indexing_lag = after
        .indexed_vectors_count
        .is_some_and(|indexed| indexed < after.points_count);
    let progress = if optimized {
        format!(
            "{} → {} segments, {} points, indexed {} → {}",
            before.segments_count,
            after.segments_count,
            after.points_count,
            count_label(before.indexed_vectors_count),
            count_label(after.indexed_vectors_count)
        )
    } else {
        format!(
            "{} segments, {} points, indexed {}",
            after.segments_count,
            after.points_count,
            count_label(after.indexed_vectors_count)
        )
    };
    let summary = if optimized {
        format!("Optimized '{collection}': {progress}")
    } else {
        format!("'{collection}' is already optimal: {progress}")
    };
    let message = if indexing_lag {
        format!(
            "{summary}; indexing still lags: {} of {} vectors indexed — segments below the \
             collection's indexing_threshold stay brute-force; lower it with \
             `ALTER COLLECTION {collection} WITH OPTIMIZERS (indexing_threshold = …)` \
             and re-run `qql edge optimize {collection}`",
            count_label(after.indexed_vectors_count),
            after.points_count
        )
    } else {
        summary
    };

    if json {
        println!(
            "{}",
            serde_json::json!({
                "ok": true,
                "operation": "edge-optimize",
                "collection": collection,
                "optimized": optimized,
                "status": if indexing_lag { "warn" } else { "ok" },
                "indexing_lag": indexing_lag,
                "before": info_counts(&before),
                "after": info_counts(&after),
                "message": message,
            })
        );
    } else if !quiet {
        println!("{message}");
    }
    Ok(())
}

/// Emit a structured `--json` failure for a one-shot edge command.
///
/// Mirrors the `ok` / `operation` / `message` triple of the `ExecResponse`
/// results every `qql exec --json` run reports (and the `check` / `doctor`
/// JSON shapes), plus the command's `collection`. The caller still returns
/// `Err`, so the exit code is unchanged and stdout stays machine-parseable.
#[cfg(feature = "edge")]
fn print_edge_json_error(
    operation: &str,
    collection: &str,
    error: &(dyn std::error::Error + 'static),
) {
    println!(
        "{}",
        serde_json::json!({
            "ok": false,
            "operation": operation,
            "collection": collection,
            "message": error.to_string(),
        })
    );
}

/// Seed a local edge collection from a remote Qdrant shard snapshot.
///
/// Streams `GET /collections/{c}/shards/{id}/snapshot`, unpacks it with the
/// engine's snapshot API into a staging directory, verifies it by loading the
/// shard, and only then swaps it into the edge data directory. An existing
/// local collection is replaced only with `--force`; until the swap, it is
/// untouched. The snapshot carries config, built HNSW indexes and quantized
/// data — this is the documented "offload indexing" seed flow, not a
/// statement-based copy.
#[cfg(feature = "edge")]
pub async fn handle_edge_bootstrap(
    from: &str,
    api_key: Option<String>,
    collection: &str,
    shard_id: Option<u32>,
    force: bool,
    json: bool,
    quiet: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    match edge_bootstrap_inner(from, api_key, collection, shard_id, force, json, quiet).await {
        Ok(()) => Ok(()),
        Err(error) => {
            if json {
                print_edge_json_error("edge-bootstrap", collection, error.as_ref());
            }
            Err(error)
        }
    }
}

#[cfg(feature = "edge")]
async fn edge_bootstrap_inner(
    from: &str,
    api_key: Option<String>,
    collection: &str,
    shard_id: Option<u32>,
    force: bool,
    json: bool,
    quiet: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = crate::config::EdgeConfig::load()?.apply_environment();
    if collection == ".qql-bootstrap" {
        return Err("collection name '.qql-bootstrap' is reserved for snapshot staging".into());
    }
    let target = config.data_dir.join(collection);
    let replaced = target.exists();
    if replaced && !force {
        return Err(format!(
            "local collection '{collection}' already exists at {}; pass --force to replace it \
             (the remote snapshot is downloaded and verified before anything is moved)",
            target.display()
        )
        .into());
    }
    std::fs::create_dir_all(&config.data_dir)?;

    // Staging lives beside collections, never inside one. A leftover
    // `previous/` from a half-finished --force swap is restored first so a
    // retry cannot delete the only copy of the old collection.
    let workspace = config.data_dir.join(".qql-bootstrap").join(collection);
    let leftover = workspace.join("previous");
    if leftover.exists() && !target.exists() {
        std::fs::rename(&leftover, &target)?;
    }
    if workspace.exists() {
        std::fs::remove_dir_all(&workspace)?;
    }
    std::fs::create_dir_all(&workspace)?;
    let snapshot_path = workspace.join("shard.snapshot");
    let stage = workspace.join("stage");

    let run = async {
        let api_key = api_key.filter(|key| !key.is_empty()).or_else(|| {
            std::env::var("QDRANT_API_KEY")
                .ok()
                .filter(|key| !key.is_empty())
        });
        let client = qql::snapshots::RemoteSnapshotClient::new(from, api_key)?;
        let listing = client.list_shards(collection).await?;
        let chosen = qql::snapshots::select_shard_id(&listing, shard_id)?;
        let bytes = client
            .download_shard_snapshot(collection, chosen, &snapshot_path)
            .await?;
        qql_edge::unpack_snapshot(&snapshot_path, &stage)?;
        let verified = qql_edge::inspect_shard(&stage)?;
        Ok::<_, qql_core::error::QqlError>((listing, chosen, bytes, verified))
    }
    .await;

    // On any download/unpack/verify failure the existing collection is untouched.
    let (listing, chosen, bytes, verified) = match run {
        Ok(value) => value,
        Err(error) => {
            let _ = std::fs::remove_dir_all(&workspace);
            return Err(error.into());
        }
    };

    // Commit. Only now is the existing collection touched.
    let previous = workspace.join("previous");
    if target.exists() {
        std::fs::rename(&target, &previous)?;
    }
    if let Err(error) = std::fs::rename(&stage, &target) {
        if previous.exists() {
            std::fs::rename(&previous, &target).map_err(|restore| {
                format!(
                    "failed to move the verified snapshot into {} ({error}); \
                     previous collection is still at {} (restore failed: {restore})",
                    target.display(),
                    previous.display()
                )
            })?;
        }
        let _ = std::fs::remove_dir_all(&workspace);
        return Err(format!(
            "failed to move the verified snapshot into {}: {error}",
            target.display()
        )
        .into());
    }

    // Verify the shard loads from its final path before reporting success.
    // A failed inspect after a successful rename restores the previous
    // collection when one existed; the workspace is not deleted if restore
    // fails, so the previous copy stays on disk.
    let summary = match qql_edge::inspect_shard(&target) {
        Ok(summary) => summary,
        Err(error) => {
            if previous.exists() {
                let _ = std::fs::remove_dir_all(&target);
                if let Err(restore) = std::fs::rename(&previous, &target) {
                    return Err(format!(
                        "snapshot failed to load ({error}); previous collection is still at {} \
                         (restore failed: {restore})",
                        previous.display()
                    )
                    .into());
                }
            }
            let _ = std::fs::remove_dir_all(&workspace);
            return Err(error.into());
        }
    };
    if previous.exists() {
        let _ = std::fs::remove_dir_all(&previous);
    }
    let _ = std::fs::remove_dir_all(&workspace);

    let message = format!(
        "Bootstrapped '{collection}' from {from} [shard {chosen}]: {} points, {} indexed, {} segments ({bytes} bytes) → {}",
        summary.points_count,
        summary.indexed_vectors_count,
        summary.segments_count,
        target.display()
    );
    if json {
        println!(
            "{}",
            serde_json::json!({
                "ok": true,
                "operation": "edge-bootstrap",
                "collection": collection,
                "from": from,
                "shard_id": chosen,
                "shard_count": listing.shard_count,
                "local_shard_ids": listing.local_shard_ids,
                "remote_shards": listing.remote_shard_ids.len(),
                "snapshot_bytes": bytes,
                "points_count": summary.points_count,
                "indexed_vectors_count": summary.indexed_vectors_count,
                "segments_count": summary.segments_count,
                "path": target.display().to_string(),
                "replaced": replaced,
                "staged": info_counts_from_summary(&verified),
                "message": message,
            })
        );
    } else if !quiet {
        println!("{message}");
        if summary.indexed_vectors_count < summary.points_count {
            println!(
                "hint: run `qql edge optimize {collection}` to merge segments and build indexes (segments below the collection's indexing_threshold stay brute-force)"
            );
        }
    }
    Ok(())
}

#[cfg(feature = "edge")]
fn info_counts_from_summary(summary: &qql_edge::ShardSummary) -> serde_json::Value {
    serde_json::json!({
        "points": summary.points_count,
        "indexed": summary.indexed_vectors_count,
        "segments": summary.segments_count,
    })
}
