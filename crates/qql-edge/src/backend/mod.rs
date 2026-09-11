//! Local Qdrant backend via qdrant-edge — in-process HNSW search, zero network.
//!
//! # Execution model
//!
//! `qdrant_edge` is a synchronous engine. Typed operations call it directly
//! inside the async fn instead of hopping through `spawn_blocking`. The
//! trade-off: on a shared multi-thread runtime one worker thread blocks for
//! the duration of the engine call. That is accepted here because this backend
//! exists for in-process use (callers, e.g. the Python bindings, already detach
//! the GIL around the runtime), and the alternative costs a thread handoff per
//! read. Shard open/load, shard close/drop, the collections-directory scan,
//! and `EdgeQdrant::optimize_collection` stay on the blocking pool: those are
//! the genuinely heavy, CPU/IO-bound phases. No backend lock is held across an
//! engine call, so a panic inside `qdrant_edge` unwinds without poisoning this
//! backend's state.

pub mod config_builder;
pub mod conversions;
pub mod error_map;
pub mod filter_converter;
pub mod index_schema;
pub mod query_converter;
pub mod unsupported;
pub mod vector_parser;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use qdrant_edge::{
    CreateIndex, EdgeConfig, EdgeConfigBuilder, EdgeShard, EdgeShardRead, FieldIndexOperations,
    PointInsertOperations, PointOperations, UpdateOperation, VectorOperations,
    VectorStructPersisted, WalOptions, WithPayloadInterface, WithVector,
};
use serde_json::Value;
use tokio::sync::{Mutex, RwLock};

use config_builder::{build_edge_config, edge_hnsw_config_over, overlay_optimizers};
use conversions::{
    from_edge_facet_hit, from_edge_group_to_typed, from_edge_record_to_hit,
    from_edge_scored_point_to_hit, hydrate_edge_groups, to_edge_id, to_edge_ids,
};
use error_map::{EdgeOp, edge_err, edge_input_err};
use filter_converter::convert_edge_filter;
use index_schema::edge_payload_field_schema;
use query_converter::{
    convert_group_output, convert_order_by_interface, convert_query_groups_request,
    convert_query_request, convert_with_payload, convert_with_vector, parse_json_path,
};
use unsupported::{
    EdgeUnsupported, reject_collection_params, reject_collection_sharding, reject_shard_key,
    vector_hnsw_diffs,
};
use vector_parser::ToEdgeVector;

use qql::backend::{CollectionInfo, CollectionSchema};
use qql::client::QdrantOps;
use qql::executor::{BackendResponse, ExecData, FacetHit, GroupedSearchResult, SearchHit};
use qql_core::error::QqlError;
use qql_plan::UpdateOperation as PlanUpdateOperation;
use qql_plan::{
    CreateCollectionRequest, CreateIndexRequest, QueryBatchRequest, UpdateBatchRequest,
    UpdateCollectionRequest,
};

/// In-process backend: one `qdrant_edge` shard per collection under `base_path`,
/// zero network. Batch methods fan out to individual routes (no native batch RPC).
pub struct EdgeQdrant {
    base_path: PathBuf,
    on_disk_payload: bool,
    /// Optional WAL segment capacity (bytes) seeding every shard this backend
    /// opens or creates; a value already persisted in the shard's
    /// `edge_config.json` wins. `None` keeps each shard's persisted/default
    /// value (32 MiB for new shards).
    wal_segment_capacity: Option<usize>,
    shards: RwLock<HashMap<String, Arc<EdgeShard>>>,
    opening: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

/// How `open_shard_inner` treats a missing vs existing collection directory.
#[derive(Clone, Copy)]
enum OpenMode {
    /// Reads and mutations: missing collection is `QQL-EDGE-COLLECTION-NOT-FOUND`.
    MustExist,
    /// `CREATE COLLECTION`: existing `segments/` is `QQL-EDGE-COLLECTION-EXISTS`.
    CreateExclusive,
}

/// Helper: create a spawn_blocking error with the operation name for context.
fn spawn_error(operation: &str, error: impl std::fmt::Display) -> QqlError {
    QqlError::execution("QQL-EDGE-SPAWN", format!("{operation}: {error}"), None)
        .with_field("operation", operation.to_string())
}

/// Persisted WAL segment capacity (bytes) from the shard's `edge_config.json`.
///
/// `None` covers both "no config file" and "config without WAL options": the
/// shard has never recorded a capacity, so an env/CLI value may seed one. A
/// config that exists but cannot be parsed is also `None` here — `EdgeShard::load`
/// surfaces that error with full context.
fn persisted_wal_segment_capacity(collection_path: &Path) -> Option<usize> {
    match EdgeConfig::load(collection_path) {
        Some(Ok(config)) => config.wal_options.map(|wal| wal.segment_capacity),
        Some(Err(_)) | None => None,
    }
}

impl std::fmt::Debug for EdgeQdrant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EdgeQdrant")
            .field("base_path", &self.base_path)
            .field("on_disk_payload", &self.on_disk_payload)
            .field("wal_segment_capacity", &self.wal_segment_capacity)
            .finish()
    }
}

impl EdgeQdrant {
    /// Create an edge backend rooted at `base_path`; payloads persist on disk
    /// unless `on_disk_payload` is false.
    pub fn new(base_path: impl Into<PathBuf>, on_disk_payload: bool) -> Self {
        Self {
            base_path: base_path.into(),
            on_disk_payload,
            wal_segment_capacity: None,
            shards: RwLock::new(HashMap::new()),
            opening: Mutex::new(HashMap::new()),
        }
    }

    /// Override the write-ahead-log segment capacity (bytes) for every shard
    /// this backend creates or opens. qdrant-edge pre-allocates each WAL
    /// segment to this size, so embedded targets with small disks set it lower
    /// than the 32 MiB default.
    ///
    /// Seed-once: the value is applied when creating a shard or when an
    /// existing shard has no persisted capacity; a capacity already recorded
    /// in `edge_config.json` wins, so later opens never ratchet the shard's
    /// config. `None` (the default) keeps each shard's persisted value, or the
    /// engine default when creating a new shard.
    pub fn with_wal_segment_capacity(mut self, capacity: Option<usize>) -> Self {
        self.wal_segment_capacity = capacity;
        self
    }

    /// Release all open shards. Call before deleting `base_path` so qdrant-edge
    /// can flush while the directory still exists. Idempotent.
    pub async fn close(&self) -> Result<(), QqlError> {
        let shards: Vec<Arc<EdgeShard>> = {
            let mut open = self.shards.write().await;
            open.drain().map(|(_, shard)| shard).collect()
        };
        tokio::task::spawn_blocking(move || drop(shards))
            .await
            .map_err(|error| {
                QqlError::execution(
                    "QQL-EDGE-CLOSE",
                    format!("failed to close edge shards: {error}"),
                    None,
                )
            })?;
        Ok(())
    }

    fn collection_path(&self, name: &str) -> PathBuf {
        self.base_path.join(name)
    }

    /// Open an existing shard for `collection`. Never creates the collection:
    /// reads and mutations against a missing collection return
    /// `QQL-EDGE-COLLECTION-NOT-FOUND` instead of materialising a ghost
    /// collection, matching the remote backends' 404 semantics.
    async fn open_shard(&self, name: &str) -> Result<Arc<EdgeShard>, QqlError> {
        self.open_shard_inner(name, None, OpenMode::MustExist).await
    }

    async fn open_shard_inner(
        &self,
        name: &str,
        req: Option<&CreateCollectionRequest>,
        mode: OpenMode,
    ) -> Result<Arc<EdgeShard>, QqlError> {
        {
            let shards = self.shards.read().await;
            if let Some(shard) = shards.get(name) {
                return Ok(Arc::clone(shard));
            }
        }

        let opening = {
            let mut opening = self.opening.lock().await;
            Arc::clone(
                opening
                    .entry(name.to_string())
                    .or_insert_with(|| Arc::new(Mutex::new(()))),
            )
        };
        let _opening_guard = opening.lock().await;
        {
            let shards = self.shards.read().await;
            if let Some(shard) = shards.get(name) {
                return Ok(Arc::clone(shard));
            }
        }

        let path = self.collection_path(name);
        let on_disk = self.on_disk_payload;
        let wal_segment_capacity = self.wal_segment_capacity;
        let collection = name.to_string();
        let config_res = req.map(|r| build_edge_config(r, on_disk));
        let shard = tokio::task::spawn_blocking(move || -> Result<EdgeShard, QqlError> {
            if path.join("segments").exists() {
                if matches!(mode, OpenMode::CreateExclusive) {
                    return Err(QqlError::execution(
                        "QQL-EDGE-COLLECTION-EXISTS",
                        format!("collection '{collection}' already exists"),
                        None,
                    )
                    .with_collection(collection));
                }
                // Persisted value wins: the env/CLI capacity only seeds a
                // shard that never recorded one. Passing an override equal to
                // the persisted value would still be rewritten by the engine
                // on every open, and a different value would ratchet the
                // shard away from its persisted config.
                let load_config = wal_segment_capacity
                    .filter(|_| persisted_wal_segment_capacity(&path).is_none())
                    .map(|capacity| EdgeConfig {
                        wal_options: Some(WalOptions {
                            segment_capacity: capacity,
                            ..Default::default()
                        }),
                        ..Default::default()
                    });
                EdgeShard::load(&path, load_config)
                    .map_err(|e| edge_err(EdgeOp::Load, Some(&collection), e))
            } else if matches!(mode, OpenMode::CreateExclusive) {
                std::fs::create_dir_all(&path).map_err(|e| {
                    QqlError::execution(
                        "QQL-EDGE-CREATE-DIR",
                        format!("create collection directory: {e}"),
                        None,
                    )
                })?;

                let mut config = match config_res {
                    Some(c) => c?,
                    None => EdgeConfigBuilder::new().on_disk_payload(on_disk).build(),
                };
                if let Some(capacity) = wal_segment_capacity {
                    config.wal_options = Some(WalOptions {
                        segment_capacity: capacity,
                        ..Default::default()
                    });
                }

                EdgeShard::new(&path, config)
                    .map_err(|e| edge_err(EdgeOp::Create, Some(&collection), e))
            } else {
                Err(QqlError::execution(
                    "QQL-EDGE-COLLECTION-NOT-FOUND",
                    format!(
                        "collection '{collection}' does not exist; create it first with CREATE COLLECTION (or an embedding UPSERT auto-creates it)"
                    ),
                    None,
                )
                .with_collection(collection))
            }
        })
        .await
        .map_err(|e| spawn_error("open_shard", e))??;

        let shard = Arc::new(shard);
        self.shards
            .write()
            .await
            .insert(name.to_string(), Arc::clone(&shard));
        {
            let mut opening = self.opening.lock().await;
            opening.remove(name);
        }
        Ok(shard)
    }

    /// Run a planned query: `qdrant_edge` scored points map straight to
    /// [`SearchHit`], with no JSON envelope in between.
    async fn execute_edge_query(
        &self,
        collection: &str,
        req: &qql_plan::types::QueryRequest,
    ) -> Result<Vec<SearchHit>, QqlError> {
        let shard = self.open_shard(collection).await?;
        let edge_req = convert_query_request(req)?;
        let results = shard
            .query(edge_req)
            .map_err(|e| edge_err(EdgeOp::Query, Some(collection), e))?;
        Ok(results
            .into_iter()
            .map(from_edge_scored_point_to_hit)
            .collect())
    }

    /// Run a planned grouped query through qdrant-edge's grouping driver.
    ///
    /// The driver only requested the `group_by` field for its candidates, so
    /// the distilled hits are hydrated from a `retrieve` carrying the plan's
    /// output selectors — the same payload/vector a remote Qdrant returns.
    /// `group_offset` trimming stays with the executor (`normalize_planned`).
    async fn execute_edge_query_groups(
        &self,
        collection: &str,
        req: &qql_plan::types::QueryGroupsRequest,
    ) -> Result<Vec<GroupedSearchResult>, QqlError> {
        reject_shard_key(req.shard_key.as_ref())?;
        let shard = self.open_shard(collection).await?;
        let edge_req = convert_query_groups_request(req)?;
        let (with_payload, with_vector) = convert_group_output(req)?;

        let mut groups = shard
            .query_groups(edge_req)
            .map_err(|e| edge_err(EdgeOp::Query, Some(collection), e))?;

        // The grouping driver always fetches the group_by field. When the
        // caller asked for neither payload nor vectors, strip that leftover
        // locally instead of a retrieve that would return empty records.
        let skip_hydrate = matches!(
            (&with_payload, &with_vector),
            (WithPayloadInterface::Bool(false), WithVector::Bool(false))
        );
        if skip_hydrate {
            for group in &mut groups {
                for hit in &mut group.hits {
                    hit.payload = None;
                    hit.vector = None;
                }
            }
        } else {
            let ids: Vec<qdrant_edge::PointId> = groups
                .iter()
                .flat_map(|group| group.hits.iter().map(|hit| hit.id))
                .collect();
            if !ids.is_empty() {
                let records = shard
                    .retrieve(qdrant_edge::RetrieveRequest {
                        point_ids: ids,
                        with_payload: Some(with_payload),
                        with_vector: Some(with_vector),
                    })
                    .map_err(|e| edge_err(EdgeOp::Retrieve, Some(collection), e))?;
                hydrate_edge_groups(&mut groups, records);
            }
        }

        groups
            .into_iter()
            .map(from_edge_group_to_typed)
            .collect::<Result<Vec<_>, _>>()
    }

    /// Run a planned points-by-id retrieve: records map straight to
    /// [`SearchHit`] with `score` 0.0 (no similarity search).
    async fn execute_edge_points(
        &self,
        collection: &str,
        req: &qql_plan::types::PointsRequest,
    ) -> Result<Vec<SearchHit>, QqlError> {
        reject_shard_key(req.shard_key.as_ref())?;
        let shard = self.open_shard(collection).await?;
        let ids = to_edge_ids(req.ids.iter())?;
        let with_payload = req
            .with_payload
            .as_ref()
            .map(convert_with_payload)
            .transpose()?
            .unwrap_or(WithPayloadInterface::Bool(true));
        let with_vector = req
            .with_vector
            .as_ref()
            .map(convert_with_vector)
            .unwrap_or(WithVector::Bool(false));

        let records = shard
            .retrieve(qdrant_edge::RetrieveRequest {
                point_ids: ids,
                with_payload: Some(with_payload),
                with_vector: Some(with_vector),
            })
            .map_err(|e| edge_err(EdgeOp::Retrieve, Some(collection), e))?;
        Ok(records.into_iter().map(from_edge_record_to_hit).collect())
    }

    /// Run a planned scroll: records map straight to [`SearchHit`]. The edge
    /// cursor (`next_page_offset`) is dropped — [`ExecData::Hits`] has no slot
    /// for it, so callers paginate by re-issuing the scroll with an explicit
    /// offset; the REST/gRPC envelopes carry the cursor.
    async fn execute_edge_scroll(
        &self,
        collection: &str,
        req: &qql_plan::types::ScrollRequest,
    ) -> Result<Vec<SearchHit>, QqlError> {
        reject_shard_key(req.shard_key.as_ref())?;
        let shard = self.open_shard(collection).await?;

        let offset = match req.offset.as_ref() {
            Some(o) => Some(to_edge_id(o.clone())?),
            None => None,
        };
        let filter = convert_edge_filter(req.filter.as_ref())?;
        let scroll_req = qdrant_edge::ScrollRequest {
            offset,
            limit: Some(req.limit.unwrap_or(10) as usize),
            filter,
            with_payload: Some(
                req.with_payload
                    .as_ref()
                    .map(convert_with_payload)
                    .transpose()?
                    .unwrap_or(WithPayloadInterface::Bool(true)),
            ),
            with_vector: req
                .with_vector
                .as_ref()
                .map(convert_with_vector)
                .unwrap_or(WithVector::Bool(false)),
            order_by: req
                .order_by
                .as_ref()
                .map(convert_order_by_interface)
                .transpose()?,
        };

        let (records, _next) = shard
            .scroll(scroll_req)
            .map_err(|e| edge_err(EdgeOp::Scroll, Some(collection), e))?;
        Ok(records.into_iter().map(from_edge_record_to_hit).collect())
    }

    /// Upsert points into the collection. Writes only report success here —
    /// the executor owns the upsert count and message.
    async fn execute_edge_upsert(
        &self,
        collection: &str,
        req: &qql_plan::types::UpsertRequest,
    ) -> Result<(), QqlError> {
        reject_shard_key(req.shard_key.as_ref())?;
        let shard = self.open_shard(collection).await?;
        let collection_name = collection.to_string();

        let mut parsed_points = Vec::with_capacity(req.points.len());
        for p in &req.points {
            let id = to_edge_id(p.id.clone())?;
            let vector_struct = p
                .vector
                .as_ref()
                .ok_or_else(|| {
                    QqlError::execution(
                        "QQL-EDGE-MISSING-VECTOR",
                        "upsert point missing vector",
                        None,
                    )
                    .with_collection(collection_name.clone())
                })?
                .clone()
                .to_edge_vector()?;
            let payload_val = Value::Object(p.payload.clone().unwrap_or_default());
            let ps = qdrant_edge::PointStruct::new(id, vector_struct, payload_val);
            let psp: qdrant_edge::PointStructPersisted = ps.into();
            parsed_points.push(psp);
        }

        let op = UpdateOperation::PointOperation(PointOperations::UpsertPoints(
            PointInsertOperations::PointsList(parsed_points),
        ));

        shard
            .update(op)
            .map_err(|e| edge_err(EdgeOp::Upsert, Some(collection), e))
    }

    async fn execute_edge_delete(
        &self,
        collection: &str,
        req: &qql_plan::types::DeleteRequest,
    ) -> Result<(), QqlError> {
        reject_shard_key(req.shard_key.as_ref())?;
        let shard = self.open_shard(collection).await?;

        let operation = if let Some(points) = &req.points {
            let ids = to_edge_ids(points.iter())?;
            UpdateOperation::PointOperation(PointOperations::DeletePoints { ids })
        } else if let Some(filter) = &req.filter {
            let edge_filter = convert_edge_filter(Some(filter))?.ok_or_else(|| {
                QqlError::execution(
                    "QQL-EDGE-FILTER-CONVERT",
                    "delete filter converted to empty",
                    None,
                )
                .with_collection(collection.to_string())
            })?;
            UpdateOperation::PointOperation(PointOperations::DeletePointsByFilter(edge_filter))
        } else {
            return Err(QqlError::execution(
                "QQL-EDGE-DELETE-REQUIRES-TARGET",
                "delete requires point ids or a filter",
                None,
            )
            .with_collection(collection.to_string()));
        };

        shard
            .update(operation)
            .map_err(|e| edge_err(EdgeOp::Delete, Some(collection), e))
    }

    async fn execute_edge_clear_payload(
        &self,
        collection: &str,
        req: &qql_plan::types::ClearPayloadRequest,
    ) -> Result<(), QqlError> {
        reject_shard_key(req.shard_key.as_ref())?;
        let shard = self.open_shard(collection).await?;

        let operation = if let Some(points) = &req.points {
            let ids = to_edge_ids(points.iter())?;
            UpdateOperation::PayloadOperation(qdrant_edge::PayloadOps::ClearPayload { points: ids })
        } else if let Some(filter) = &req.filter {
            let edge_filter = convert_edge_filter(Some(filter))?.ok_or_else(|| {
                QqlError::execution(
                    "QQL-EDGE-FILTER-CONVERT",
                    "clear_payload filter converted to empty",
                    None,
                )
                .with_collection(collection.to_string())
            })?;
            UpdateOperation::PayloadOperation(qdrant_edge::PayloadOps::ClearPayloadByFilter(
                edge_filter,
            ))
        } else {
            return Err(QqlError::execution(
                "QQL-EDGE-CLEAR-PAYLOAD-REQUIRES-TARGET",
                "clear_payload requires point ids or a filter",
                None,
            )
            .with_collection(collection.to_string()));
        };

        shard
            .update(operation)
            .map_err(|e| edge_err(EdgeOp::ClearPayload, Some(collection), e))
    }

    async fn execute_edge_delete_payload(
        &self,
        collection: &str,
        req: &qql_plan::types::DeletePayloadRequest,
    ) -> Result<(), QqlError> {
        reject_shard_key(req.shard_key.as_ref())?;
        let shard = self.open_shard(collection).await?;

        let points = req
            .points
            .as_ref()
            .map(|pts| to_edge_ids(pts.iter()))
            .transpose()?;
        let filter = convert_edge_filter(req.filter.as_ref())?;

        if points.is_none() && filter.is_none() {
            return Err(QqlError::execution(
                "QQL-EDGE-DELETE-PAYLOAD-REQUIRES-TARGET",
                "delete_payload requires point ids or a filter",
                None,
            )
            .with_collection(collection.to_string()));
        }

        let keys: Vec<qdrant_edge::JsonPath> = req
            .keys
            .iter()
            .map(|k| parse_json_path(k))
            .collect::<Result<Vec<_>, _>>()?;

        let operation = UpdateOperation::PayloadOperation(qdrant_edge::PayloadOps::DeletePayload(
            qdrant_edge::DeletePayloadOp {
                keys,
                points,
                filter,
            },
        ));

        shard
            .update(operation)
            .map_err(|e| edge_err(EdgeOp::DeletePayload, Some(collection), e))
    }

    async fn execute_edge_delete_vectors(
        &self,
        collection: &str,
        req: &qql_plan::types::DeleteVectorRequest,
    ) -> Result<(), QqlError> {
        reject_shard_key(req.shard_key.as_ref())?;
        let shard = self.open_shard(collection).await?;
        let vector_names: Vec<String> = req.vector.clone();

        let operation = if let Some(points) = &req.points {
            let ids = to_edge_ids(points.iter())?;
            UpdateOperation::VectorOperation(VectorOperations::DeleteVectors(
                qdrant_edge::PointIdsList { points: ids },
                vector_names,
            ))
        } else if let Some(filter) = &req.filter {
            let edge_filter = convert_edge_filter(Some(filter))?.ok_or_else(|| {
                QqlError::execution(
                    "QQL-EDGE-FILTER-CONVERT",
                    "delete_vectors filter converted to empty",
                    None,
                )
                .with_collection(collection.to_string())
            })?;
            UpdateOperation::VectorOperation(VectorOperations::DeleteVectorsByFilter(
                edge_filter,
                vector_names,
            ))
        } else {
            return Err(QqlError::execution(
                "QQL-EDGE-DELETE-VECTORS-REQUIRES-TARGET",
                "delete_vectors requires point ids or a filter",
                None,
            )
            .with_collection(collection.to_string()));
        };

        shard
            .update(operation)
            .map_err(|e| edge_err(EdgeOp::DeleteVectors, Some(collection), e))
    }

    async fn execute_edge_update_vectors(
        &self,
        collection: &str,
        req: &qql_plan::types::UpdateVectorRequest,
    ) -> Result<(), QqlError> {
        reject_shard_key(req.shard_key.as_ref())?;
        let shard = self.open_shard(collection).await?;

        let mut pvps = Vec::with_capacity(req.points.len());
        for pt in &req.points {
            let id = to_edge_id(pt.id.clone())?;
            let vector_struct = pt.vector.clone().to_edge_vector()?;
            pvps.push(qdrant_edge::PointVectorsPersisted {
                id,
                vector: VectorStructPersisted::from(vector_struct),
            });
        }

        let op = UpdateOperation::VectorOperation(VectorOperations::UpdateVectors(
            qdrant_edge::UpdateVectorsOp {
                points: pvps,
                update_filter: None,
            },
        ));

        shard
            .update(op)
            .map_err(|e| edge_err(EdgeOp::UpdateVectors, Some(collection), e))
    }

    async fn execute_edge_update_payload(
        &self,
        collection: &str,
        req: &qql_plan::types::UpdatePayloadRequest,
    ) -> Result<(), QqlError> {
        reject_shard_key(req.shard_key.as_ref())?;
        let shard = self.open_shard(collection).await?;
        let payload = qdrant_edge::Payload(req.payload.clone().into_iter().collect());

        let op = if let Some(points) = &req.points {
            let ids = to_edge_ids(points.iter())?;
            qdrant_edge::PayloadOps::SetPayload(qdrant_edge::SetPayloadOp {
                payload,
                points: Some(ids),
                filter: None,
                key: None,
            })
        } else if let Some(filter) = &req.filter {
            qdrant_edge::PayloadOps::SetPayload(qdrant_edge::SetPayloadOp {
                payload,
                points: None,
                filter: Some(convert_edge_filter(Some(filter))?.ok_or_else(|| {
                    QqlError::execution(
                        "QQL-EDGE-FILTER-CONVERT",
                        "set_payload filter converted to empty",
                        None,
                    )
                    .with_collection(collection.to_string())
                })?),
                key: None,
            })
        } else {
            return Err(QqlError::execution(
                "QQL-EDGE-SET-PAYLOAD-REQUIRES-TARGET",
                "set_payload requires point ids or a filter",
                None,
            )
            .with_collection(collection.to_string()));
        };

        shard
            .update(UpdateOperation::PayloadOperation(op))
            .map_err(|e| edge_err(EdgeOp::UpdatePayload, Some(collection), e))
    }

    /// Run a planned count, returning the point count.
    async fn execute_edge_count(
        &self,
        collection: &str,
        req: &qql_plan::types::CountRequest,
    ) -> Result<u64, QqlError> {
        reject_shard_key(req.shard_key.as_ref())?;
        let shard = self.open_shard(collection).await?;
        let filter = convert_edge_filter(req.filter.as_ref())?;
        let count_req = qdrant_edge::CountRequest {
            filter,
            exact: req.exact.unwrap_or(true),
        };
        let count = shard
            .count(count_req)
            .map_err(|e| edge_err(EdgeOp::Count, Some(collection), e))?;
        Ok(count as u64)
    }

    /// Run a planned facet aggregation: `qdrant_edge` facet hits map straight
    /// to [`FacetHit`].
    async fn execute_edge_facet(
        &self,
        collection: &str,
        req: &qql_plan::types::FacetRequest,
    ) -> Result<Vec<FacetHit>, QqlError> {
        reject_shard_key(req.shard_key.as_ref())?;
        let shard = self.open_shard(collection).await?;
        let key = parse_json_path(&req.key)?;
        let mut edge_req = qdrant_edge::FacetRequest::new(key);
        if let Some(limit) = req.limit {
            edge_req.limit = usize::try_from(limit).map_err(|_| {
                edge_input_err(
                    EdgeOp::Facet,
                    Some(collection),
                    format!("limit {limit} exceeds platform usize"),
                )
            })?;
        }
        edge_req.filter = convert_edge_filter(req.filter.as_ref())?;
        edge_req.exact = req.exact.unwrap_or(false);

        let response = shard
            .facet(edge_req)
            .map_err(|e| edge_err(EdgeOp::Facet, Some(collection), e))?;
        Ok(response.hits.into_iter().map(from_edge_facet_hit).collect())
    }
}

#[async_trait]
impl QdrantOps for EdgeQdrant {
    async fn close(&self) -> Result<(), QqlError> {
        EdgeQdrant::close(self).await
    }

    async fn list_collections(&self) -> Result<Vec<String>, QqlError> {
        let path = self.base_path.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<String>, QqlError> {
            let mut cols = Vec::new();
            if !path.exists() {
                return Ok(cols);
            }
            let mut dir = std::fs::read_dir(&path).map_err(|e| {
                QqlError::execution(
                    "QQL-EDGE-READ-DIR",
                    format!("failed to read collections directory: {e}"),
                    None,
                )
            })?;
            while let Some(entry) = dir.next().transpose().map_err(|e| {
                QqlError::execution(
                    "QQL-EDGE-DIR-ENTRY",
                    format!("failed to read directory entry: {e}"),
                    None,
                )
            })? {
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false)
                    && entry.path().join("segments").is_dir()
                    && let Some(name) = entry.file_name().to_str()
                    && !name.starts_with('.')
                {
                    cols.push(name.to_string());
                }
            }
            cols.sort();
            Ok(cols)
        })
        .await
        .map_err(|e| spawn_error("list_collections", e))?
    }

    async fn collection_exists(&self, name: &str) -> Result<bool, QqlError> {
        Ok(self.collection_path(name).join("segments").exists())
    }

    async fn get_collection_info(&self, name: &str) -> Result<CollectionInfo, QqlError> {
        let shard = self.open_shard(name).await?;
        let info = shard
            .info()
            .map_err(|e| edge_err(EdgeOp::Info, Some(name), e))?;
        let cfg = shard.config();
        let dense_vectors = cfg
            .vectors
            .keys()
            .filter(|name| !name.is_empty())
            .cloned()
            .collect();
        let vectors = cfg
            .vectors
            .iter()
            .map(|(name, params)| qql::backend::VectorSpec {
                name: (!name.is_empty()).then(|| name.clone()),
                size: params.size as u64,
                distance: match params.distance {
                    qdrant_edge::Distance::Cosine => "Cosine",
                    qdrant_edge::Distance::Dot => "Dot",
                    qdrant_edge::Distance::Euclid => "Euclid",
                    qdrant_edge::Distance::Manhattan => "Manhattan",
                }
                .to_string(),
                hnsw: params
                    .hnsw_config
                    .and_then(|hnsw| serde_json::to_value(hnsw).ok())
                    .and_then(|value| value.as_object().cloned()),
                quantization: params
                    .quantization_config
                    .as_ref()
                    .and_then(|quant| serde_json::to_value(quant).ok()),
                multivector: params.multivector_config.as_ref().map(|mv| {
                    let mut map = serde_json::Map::new();
                    map.insert(
                        "comparator".into(),
                        serde_json::Value::String(match mv.comparator {
                            qdrant_edge::MultiVectorComparator::MaxSim => "max_sim".into(),
                        }),
                    );
                    map
                }),
                on_disk: params.on_disk,
                datatype: params.datatype.map(|dt| {
                    match dt {
                        qdrant_edge::VectorStorageDatatype::Float32 => "float32",
                        qdrant_edge::VectorStorageDatatype::Float16 => "float16",
                        qdrant_edge::VectorStorageDatatype::Uint8 => "uint8",
                        qdrant_edge::VectorStorageDatatype::Turbo4 => "turbo4",
                    }
                    .to_string()
                }),
                memory: None,
            })
            .collect();
        let sparse_vectors = cfg
            .sparse_vectors
            .iter()
            .map(|(name, params)| {
                let mut index = serde_json::Map::new();
                if let Some(threshold) = params.full_scan_threshold {
                    index.insert(
                        "full_scan_threshold".into(),
                        serde_json::json!(threshold as u64),
                    );
                }
                if let Some(on_disk) = params.on_disk {
                    index.insert("on_disk".into(), serde_json::Value::Bool(on_disk));
                }
                if let Some(datatype) = params.datatype {
                    index.insert(
                        "datatype".into(),
                        serde_json::Value::String(
                            match datatype {
                                qdrant_edge::VectorStorageDatatype::Float32 => "float32",
                                qdrant_edge::VectorStorageDatatype::Float16 => "float16",
                                qdrant_edge::VectorStorageDatatype::Uint8 => "uint8",
                                qdrant_edge::VectorStorageDatatype::Turbo4 => "turbo4",
                            }
                            .into(),
                        ),
                    );
                }
                qql::backend::SparseVectorSpec {
                    name: name.clone(),
                    index: (!index.is_empty()).then_some(index),
                    modifier: params.modifier.map(|modifier| match modifier {
                        qdrant_edge::Modifier::Idf => "idf".into(),
                        qdrant_edge::Modifier::None => "none".into(),
                    }),
                }
            })
            .collect();

        Ok(CollectionInfo {
            status: "green".to_string(),
            points_count: info.points_count as u64,
            indexed_vectors_count: Some(info.indexed_vectors_count as u64),
            segments_count: info.segments_count as u64,
            schema: CollectionSchema {
                dense_vectors,
                sparse_vectors,
                vectors,
                hnsw: serde_json::to_value(cfg.hnsw_config())
                    .ok()
                    .and_then(|value| value.as_object().cloned()),
                optimizers: serde_json::to_value(cfg.optimizers())
                    .ok()
                    .and_then(|value| value.as_object().cloned()),
                quantization: cfg
                    .quantization_config
                    .as_ref()
                    .and_then(|quant| serde_json::to_value(quant).ok()),
                params: qql::backend::CollectionParamsSpec {
                    on_disk_payload: cfg.on_disk_payload,
                    ..Default::default()
                },
                ..Default::default()
            },
        })
    }

    async fn create_collection(
        &self,
        collection_name: &str,
        req: &CreateCollectionRequest,
    ) -> Result<(), QqlError> {
        self.open_shard_inner(collection_name, Some(req), OpenMode::CreateExclusive)
            .await?;
        Ok(())
    }

    /// Apply the `ALTER COLLECTION` fields qdrant-edge can persist.
    ///
    /// qdrant-edge exposes `set_hnsw_config`, `set_vector_hnsw_config`, and
    /// `set_optimizers_config` (all persist to `edge_config.json`); per-vector
    /// fields beyond HNSW, sparse diffs, collection params, and a collection
    /// quantization diff have no setter, so they are rejected per field — only
    /// when the request actually carries them. Validation runs before any
    /// setter, so a mixed request never half-applies.
    async fn update_collection(
        &self,
        collection_name: &str,
        req: &UpdateCollectionRequest,
    ) -> Result<(), QqlError> {
        if req.params.is_some() {
            return Err(EdgeUnsupported::AlterCollectionParams.error());
        }
        if req.quantization_config.is_some() {
            return Err(EdgeUnsupported::AlterCollectionQuantization.error());
        }
        let vector_hnsw = vector_hnsw_diffs(req)?;
        let shard = self.open_shard(collection_name).await?;
        if let Some(hnsw) = req.hnsw_config.as_ref() {
            let hnsw = edge_hnsw_config_over(hnsw, &shard.config().hnsw_config())?;
            shard
                .set_hnsw_config(hnsw)
                .map_err(|e| edge_err(EdgeOp::AlterCollection, Some(collection_name), e))?;
        }
        if let Some(optimizers) = req.optimizers_config.as_ref() {
            let optimizers = overlay_optimizers(optimizers, &shard.config().optimizers())?;
            shard
                .set_optimizers_config(optimizers)
                .map_err(|e| edge_err(EdgeOp::AlterCollection, Some(collection_name), e))?;
        }
        for (name, diff) in &vector_hnsw {
            // `set_vector_hnsw_config` replaces the whole per-vector block, so
            // merge the diff over the vector's effective config (its own, else
            // the collection-wide default), mirroring the server's field-wise
            // diff semantics. The engine stores a full per-vector config or
            // none, so the merged block pins every field: a later collection-
            // wide HNSW change no longer reaches this vector.
            let base = {
                let config = shard.config();
                config
                    .vectors
                    .get(name.as_str())
                    .and_then(|params| params.hnsw_config)
                    .unwrap_or_else(|| config.hnsw_config())
            };
            let merged = edge_hnsw_config_over(diff, &base)?;
            shard
                .set_vector_hnsw_config(name, merged)
                .map_err(|e| edge_err(EdgeOp::AlterCollection, Some(collection_name), e))?;
        }
        Ok(())
    }

    async fn delete_collection(&self, name: &str) -> Result<(), QqlError> {
        let path = self.collection_path(name);
        let shard = {
            let mut shards = self.shards.write().await;
            shards.remove(name)
        };
        {
            let mut opening = self.opening.lock().await;
            opening.remove(name);
        }
        if let Some(shard) = shard {
            tokio::task::spawn_blocking(move || drop(shard))
                .await
                .map_err(|error| {
                    QqlError::execution(
                        "QQL-EDGE-DELETE-COLLECTION-CLOSE",
                        format!("failed to close collection '{name}' before delete: {error}"),
                        None,
                    )
                    .with_collection(name.to_string())
                })?;
        }
        tokio::task::spawn_blocking(move || {
            if path.exists() {
                std::fs::remove_dir_all(&path).map_err(|e| {
                    QqlError::execution(
                        "QQL-EDGE-DELETE-COLLECTION",
                        format!("failed to delete collection directory: {e}"),
                        None,
                    )
                })
            } else {
                Ok(())
            }
        })
        .await
        .map_err(|e| spawn_error("delete_collection", e))?
    }

    async fn create_field_index(
        &self,
        collection_name: &str,
        req: &CreateIndexRequest,
    ) -> Result<(), QqlError> {
        let shard = self.open_shard(collection_name).await?;

        let field_schema = Some(edge_payload_field_schema(req)?);
        let field_name: qdrant_edge::JsonPath = serde_json::from_value(serde_json::Value::String(
            req.field_name.clone(),
        ))
        .map_err(|e| {
            QqlError::execution(
                "QQL-EDGE-FIELD-NAME",
                format!("invalid field name: {e}"),
                None,
            )
            .with_collection(collection_name.to_string())
            .with_field_name(req.field_name.clone())
        })?;

        let create_index = CreateIndex {
            field_name,
            field_schema,
        };

        let op =
            UpdateOperation::FieldIndexOperation(FieldIndexOperations::CreateIndex(create_index));

        shard
            .update(op)
            .map_err(|e| edge_err(EdgeOp::CreateIndex, Some(collection_name), e))
    }

    async fn delete_field_index(
        &self,
        collection_name: &str,
        field_name: &str,
    ) -> Result<(), QqlError> {
        let shard = self.open_shard(collection_name).await?;
        let field_name_json: qdrant_edge::JsonPath = serde_json::from_value(
            serde_json::Value::String(field_name.to_string()),
        )
        .map_err(|e| {
            QqlError::execution(
                "QQL-EDGE-FIELD-NAME",
                format!("invalid field name: {e}"),
                None,
            )
            .with_collection(collection_name.to_string())
            .with_field_name(field_name.to_string())
        })?;
        let op = UpdateOperation::FieldIndexOperation(FieldIndexOperations::DeleteIndex(
            field_name_json,
        ));
        shard
            .update(op)
            .map_err(|e| edge_err(EdgeOp::DropIndex, Some(collection_name), e))
    }

    async fn execute_planned(
        &self,
        op: &qql_plan::PlannedOperation,
    ) -> Result<BackendResponse, QqlError> {
        reject_shard_key(op.shard_key())?;
        use qql_plan::PlannedOperation::*;
        let data = match op {
            // Reads: qdrant-edge values map straight to `ExecData`.
            Query {
                collection,
                request,
            } => ExecData::Hits(self.execute_edge_query(collection, request).await?),
            QueryGroups {
                collection,
                request,
            } => ExecData::Groups(self.execute_edge_query_groups(collection, request).await?),
            GetPoints {
                collection,
                request,
            } => ExecData::Hits(self.execute_edge_points(collection, request).await?),
            Scroll {
                collection,
                request,
            } => ExecData::Hits(self.execute_edge_scroll(collection, request).await?),
            Count {
                collection,
                request,
            } => ExecData::Count(self.execute_edge_count(collection, request).await?),
            Facet {
                collection,
                request,
            } => ExecData::Facet(self.execute_edge_facet(collection, request).await?),
            // Writes: status-only. The executor owns the upsert count/message.
            Upsert {
                collection,
                request,
                ..
            } => {
                self.execute_edge_upsert(collection, request).await?;
                ExecData::Mutation { affected: None }
            }
            Delete {
                collection,
                request,
                ..
            } => {
                self.execute_edge_delete(collection, request).await?;
                ExecData::Mutation { affected: None }
            }
            UpdatePayload {
                collection,
                request,
                ..
            } => {
                self.execute_edge_update_payload(collection, request)
                    .await?;
                ExecData::Mutation { affected: None }
            }
            ClearPayload {
                collection,
                request,
                ..
            } => {
                self.execute_edge_clear_payload(collection, request).await?;
                ExecData::Mutation { affected: None }
            }
            DeletePayload {
                collection,
                request,
                ..
            } => {
                self.execute_edge_delete_payload(collection, request)
                    .await?;
                ExecData::Mutation { affected: None }
            }
            UpdateVectors {
                collection,
                request,
                ..
            } => {
                self.execute_edge_update_vectors(collection, request)
                    .await?;
                ExecData::Mutation { affected: None }
            }
            DeleteVectors {
                collection,
                request,
                ..
            } => {
                self.execute_edge_delete_vectors(collection, request)
                    .await?;
                ExecData::Mutation { affected: None }
            }
            // DDL: status-only. Sharding stays fail-closed; create-time params
            // are narrowed to the keys the engine can persist.
            CreateCollection {
                collection,
                request,
            } => {
                reject_collection_sharding(
                    request.shard_number,
                    request.sharding_method,
                    request.shard_keys.as_deref(),
                )?;
                reject_collection_params(request.params.as_ref())?;
                self.create_collection(collection, request).await?;
                ExecData::Mutation { affected: None }
            }
            UpdateCollection {
                collection,
                request,
            } => {
                self.update_collection(collection, request).await?;
                ExecData::Mutation { affected: None }
            }
            DropCollection { collection } => {
                self.delete_collection(collection).await?;
                ExecData::Mutation { affected: None }
            }
            CreateIndex {
                collection,
                request,
                ..
            } => {
                self.create_field_index(collection, request).await?;
                ExecData::Mutation { affected: None }
            }
            DropIndex { collection, field } => {
                self.delete_field_index(collection, field).await?;
                ExecData::Mutation { affected: None }
            }
            CreateShardKey { .. } | DropShardKey { .. } | ListShardKeys { .. } => {
                return Err(EdgeUnsupported::ShardKeyDdl.error());
            }
            // Collection metadata maps straight onto the typed variants.
            ListCollections => ExecData::Collections(self.list_collections().await?),
            GetCollection { collection } => {
                ExecData::Collection(self.get_collection_info(collection).await?)
            }
            CrossRerank { .. } => {
                return Err(EdgeUnsupported::Route {
                    path_hint: "CROSS RERANK",
                }
                .error());
            }
            GetQuotas | SetQuotas { .. } => return Err(EdgeUnsupported::Quota.error()),
        };
        Ok(BackendResponse {
            data,
            telemetry: None,
        })
    }

    async fn optimize_collection(&self, collection: &str) -> Result<bool, QqlError> {
        let shard = self.open_shard(collection).await?;
        tokio::task::spawn_blocking(move || shard.optimize())
            .await
            .map_err(|e| spawn_error("optimize", e))?
            .map_err(|e| edge_err(EdgeOp::Optimize, Some(collection), e))
    }

    async fn execute_query_batch(
        &self,
        collection: &str,
        batch: &QueryBatchRequest,
    ) -> Result<Vec<BackendResponse>, QqlError> {
        for request in &batch.searches {
            reject_shard_key(request.shard_key.as_ref())?;
        }
        let mut results = Vec::with_capacity(batch.searches.len());
        for req in &batch.searches {
            results.push(BackendResponse {
                data: ExecData::Hits(self.execute_edge_query(collection, req).await?),
                telemetry: None,
            });
        }
        Ok(results)
    }

    async fn execute_update_batch(
        &self,
        collection: &str,
        batch: &UpdateBatchRequest,
    ) -> Result<Vec<BackendResponse>, QqlError> {
        let mut results = Vec::with_capacity(batch.operations.len());
        for op in &batch.operations {
            match op {
                PlanUpdateOperation::Upsert { upsert } => {
                    self.execute_edge_upsert(collection, upsert).await?;
                }
                PlanUpdateOperation::Delete { delete } => {
                    self.execute_edge_delete(collection, delete).await?;
                }
                PlanUpdateOperation::SetPayload { set_payload } => {
                    self.execute_edge_update_payload(collection, set_payload)
                        .await?;
                }
                PlanUpdateOperation::ClearPayload { clear_payload } => {
                    self.execute_edge_clear_payload(collection, clear_payload)
                        .await?;
                }
                PlanUpdateOperation::DeletePayload { delete_payload } => {
                    self.execute_edge_delete_payload(collection, delete_payload)
                        .await?;
                }
                PlanUpdateOperation::UpdateVectors { update_vectors } => {
                    self.execute_edge_update_vectors(collection, update_vectors)
                        .await?;
                }
                PlanUpdateOperation::DeleteVectors { delete_vectors } => {
                    self.execute_edge_delete_vectors(collection, delete_vectors)
                        .await?;
                }
            }
            results.push(BackendResponse {
                data: ExecData::Mutation { affected: None },
                telemetry: None,
            });
        }
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qql::executor::{Executor, OnError};
    use qql_core::parser::Parser;
    use qql_plan::{
        PlanFacetValue, PlanGroupId, PlanPointId, PlanVectorStruct, PlanVectorValue,
        PlannedOperation, plan,
    };
    use serde_json::json;

    fn plan_one(qql: &str) -> PlannedOperation {
        let statement = Parser::parse(qql).unwrap_or_else(|error| panic!("parse '{qql}': {error}"));
        plan(&statement).unwrap_or_else(|error| panic!("plan '{qql}': {error}"))
    }

    fn temp_dir(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("qql-edge-{tag}-{}", std::process::id()))
    }

    async fn seed_docs(backend: &EdgeQdrant) {
        backend
            .execute_planned(&plan_one(
                "CREATE COLLECTION docs (dense VECTOR(3, COSINE))",
            ))
            .await
            .expect("create collection");
        backend
            .execute_planned(&plan_one(
                "CREATE INDEX ON COLLECTION docs FOR city TYPE keyword",
            ))
            .await
            .expect("create facet index");
        for qql in [
            "UPSERT INTO docs VALUES {id: 1, vector: {dense: [1.0, 0.0, 0.0]}, city: 'NYC', price: 10}",
            "UPSERT INTO docs VALUES {id: 2, vector: {dense: [0.0, 1.0, 0.0]}, city: 'NYC', price: 30}",
            "UPSERT INTO docs VALUES {id: 3, vector: {dense: [0.0, 0.0, 1.0]}, city: 'SF', price: 20}",
        ] {
            backend
                .execute_planned(&plan_one(qql))
                .await
                .unwrap_or_else(|error| panic!("seed '{qql}': {error}"));
        }
    }

    /// `execute_planned` answers every read with typed `ExecData` built
    /// straight from qdrant-edge results (no JSON envelope, no telemetry).
    #[test]
    fn typed_reads_and_facet_return_exec_data() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        runtime.block_on(async {
            let dir = temp_dir("typed-reads");
            let _ = std::fs::remove_dir_all(&dir);
            let backend = EdgeQdrant::new(&dir, false);
            seed_docs(&backend).await;

            // Query → Hits with score, payload, and vector mapped directly.
            let query = plan_one(
                "QUERY [1.0, 0.0, 0.0] FROM docs USING dense WITH PAYLOAD true WITH VECTOR true LIMIT 5",
            );
            let response = backend.execute_planned(&query).await.expect("typed query");
            assert!(
                response.telemetry.is_none(),
                "in-process edge has no telemetry"
            );
            let ExecData::Hits(hits) = &response.data else {
                panic!("expected typed hits, got {:?}", response.data);
            };
            assert_eq!(hits.len(), 3);
            assert_eq!(hits[0].id, PlanPointId::Number(1));
            assert!((hits[0].score - 1.0).abs() < 1e-6);
            assert_eq!(
                hits[0].payload.as_ref().and_then(|p| p.get("city")),
                Some(&json!("NYC"))
            );
            assert_eq!(
                hits[0].vector,
                Some(PlanVectorStruct::Named(std::collections::BTreeMap::from(
                    [(
                        "dense".to_string(),
                        PlanVectorValue::Dense(vec![1.0, 0.0, 0.0])
                    )]
                ))),
                "WITH VECTOR true must map the vector"
            );
            assert!(hits[0].collection.is_none());

            // GetPoints → Hits with score 0.0 (no similarity search).
            let points = plan_one("QUERY POINTS (1, 2) FROM docs WITH PAYLOAD true");
            let response = backend
                .execute_planned(&points)
                .await
                .expect("typed points");
            let ExecData::Hits(hits) = &response.data else {
                panic!("expected typed hits, got {:?}", response.data);
            };
            assert_eq!(hits.len(), 2);
            assert!(hits.iter().all(|hit| hit.score == 0.0));

            // Scroll → Hits; `next_page_offset` is intentionally not carried.
            let scroll = plan_one("SCROLL FROM docs LIMIT 2");
            let response = backend
                .execute_planned(&scroll)
                .await
                .expect("typed scroll");
            let ExecData::Hits(hits) = &response.data else {
                panic!("expected typed hits, got {:?}", response.data);
            };
            assert_eq!(hits.len(), 2);
            assert!(hits.iter().all(|hit| hit.score == 0.0));

            // Count → Count.
            let count = plan_one("COUNT FROM docs WHERE city = 'NYC'");
            let response = backend
                .execute_planned(&count)
                .await
                .expect("typed count");
            assert_eq!(response.data, ExecData::Count(2));

            // Facet → Facet (previously rejected as unsupported offline).
            let facet = plan_one("FACET city FROM docs");
            let response = backend
                .execute_planned(&facet)
                .await
                .expect("typed facet");
            assert_eq!(
                response.data,
                ExecData::Facet(vec![
                    FacetHit {
                        value: PlanFacetValue::Keyword("NYC".into()),
                        count: 2,
                    },
                    FacetHit {
                        value: PlanFacetValue::Keyword("SF".into()),
                        count: 1,
                    },
                ])
            );

            // Mutations answer typed, status-only data too.
            let upsert = plan_one(
                "UPSERT INTO docs VALUES {id: 4, vector: {dense: [0.5, 0.5, 0.5]}}",
            );
            let response = backend
                .execute_planned(&upsert)
                .await
                .expect("typed upsert");
            assert_eq!(response.data, ExecData::Mutation { affected: None });

            backend.close().await.expect("close edge backend");
            let _ = std::fs::remove_dir_all(dir);
        });
    }

    /// Every write and DDL op answers `execute_planned` with a status-only
    /// typed `ExecData::Mutation` — no JSON envelope, no `Raw`.
    #[test]
    fn typed_mutations_and_ddl_return_exec_data() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        runtime.block_on(async {
            let dir = temp_dir("typed-mutations");
            let _ = std::fs::remove_dir_all(&dir);
            let backend = EdgeQdrant::new(&dir, false);
            backend
                .execute_planned(&plan_one(
                    "CREATE COLLECTION docs (dense VECTOR(3, COSINE))",
                ))
                .await
                .expect("create collection");

            let cases = [
                "UPSERT INTO docs VALUES {id: 1, vector: {dense: [1.0, 0.0, 0.0]}, city: 'NYC'}",
                "UPDATE docs SET PAYLOAD = {price: 10} WHERE id = 1",
                "UPDATE docs SET VECTOR dense = [0.0, 1.0, 0.0] WHERE id = 1",
                "DELETE VECTOR dense FROM docs WHERE id = 1",
                "DELETE PAYLOAD city FROM docs WHERE id = 1",
                "CLEAR PAYLOAD FROM docs WHERE id = 1",
                "DELETE FROM docs WHERE id = 1",
                "CREATE INDEX ON COLLECTION docs FOR city TYPE keyword",
                "DROP INDEX ON COLLECTION docs FOR city",
            ];
            for qql in cases {
                let response = backend
                    .execute_planned(&plan_one(qql))
                    .await
                    .unwrap_or_else(|error| panic!("{qql}: {error}"));
                assert_eq!(
                    response.data,
                    ExecData::Mutation { affected: None },
                    "{qql} must answer status-only typed mutation data"
                );
                assert!(response.telemetry.is_none());
            }

            let response = backend
                .execute_planned(&plan_one("DROP COLLECTION docs"))
                .await
                .expect("drop collection");
            assert_eq!(response.data, ExecData::Mutation { affected: None });

            backend.close().await.expect("close edge backend");
            let _ = std::fs::remove_dir_all(dir);
        });
    }

    /// `execute_update_batch` answers typed status-only mutations directly —
    /// no JSON, no envelope parsing — one response per operation, in order.
    #[test]
    fn typed_update_batch_returns_mutations() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        runtime.block_on(async {
            let dir = temp_dir("typed-update-batch");
            let _ = std::fs::remove_dir_all(&dir);
            let backend = EdgeQdrant::new(&dir, false);

            let ops = vec![
                plan_one("CREATE COLLECTION docs (dense VECTOR(3, COSINE))"),
                plan_one("UPSERT INTO docs VALUES {id: 1, vector: {dense: [1.0, 0.0, 0.0]}}"),
            ];
            for op in &ops {
                backend.execute_planned(op).await.expect("setup op");
            }

            let mutations = vec![
                plan_one("UPSERT INTO docs VALUES {id: 2, vector: {dense: [0.0, 1.0, 0.0]}}"),
                plan_one("UPDATE docs SET PAYLOAD = {city: 'NYC'} WHERE id = 2"),
                plan_one("DELETE FROM docs WHERE id = 2"),
            ];
            let (collection, _labels, batch) =
                qql_plan::build_update_batch(&mutations).expect("update batch");
            assert_eq!(collection, "docs");

            let responses = backend
                .execute_update_batch(&collection, &batch)
                .await
                .expect("typed update batch");
            assert_eq!(responses.len(), mutations.len());
            for response in &responses {
                assert_eq!(response.data, ExecData::Mutation { affected: None });
                assert!(response.telemetry.is_none());
                // The executor owns upsert counts; the backend never reports one.
                assert!(response.data.count().is_none());
            }

            backend.close().await.expect("close edge backend");
            let _ = std::fs::remove_dir_all(dir);
        });
    }

    /// `SHOW` metadata answers through the typed variants: collection lists
    /// via `Collections`, collection metadata via `Collection`.
    #[test]
    fn show_metadata_returns_typed_variants() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        runtime.block_on(async {
            let dir = temp_dir("typed-metadata");
            let _ = std::fs::remove_dir_all(&dir);
            let backend = EdgeQdrant::new(&dir, false);
            backend
                .execute_planned(&plan_one(
                    "CREATE COLLECTION docs (dense VECTOR(3, COSINE))",
                ))
                .await
                .expect("create collection");

            for qql in [
                "UPSERT INTO docs VALUES {id: 1, vector: {dense: [1.0, 0.0, 0.0]}}",
                "QUERY POINTS (1) FROM docs",
                "COUNT FROM docs",
                "DELETE FROM docs WHERE id = 1",
            ] {
                let response = backend
                    .execute_planned(&plan_one(qql))
                    .await
                    .unwrap_or_else(|error| panic!("{qql}: {error}"));
                assert!(
                    matches!(
                        response.data,
                        ExecData::Hits(_) | ExecData::Count(_) | ExecData::Mutation { .. }
                    ),
                    "{qql} must answer a typed variant: {:?}",
                    response.data
                );
            }

            let collections = backend
                .execute_planned(&plan_one("SHOW COLLECTIONS"))
                .await
                .expect("show collections");
            assert_eq!(
                collections.data.collections(),
                Some(["docs".to_string()].as_slice())
            );

            let collection = backend
                .execute_planned(&plan_one("SHOW COLLECTION docs"))
                .await
                .expect("show collection");
            // The typed loop above deleted the only point.
            let info = collection.data.collection().expect("typed collection info");
            assert_eq!(info.points_count, 0);
            assert_eq!(info.schema.vectors.len(), 1);

            backend.close().await.expect("close edge backend");
            let _ = std::fs::remove_dir_all(dir);
        });
    }

    /// `optimize_collection` runs the qdrant-edge optimizers. With 1000 points
    /// and 25% deleted the vacuum optimizer fires; a second call is a no-op,
    /// proving the returned flag and idempotence.
    #[test]
    fn optimize_collection_runs_optimizers_and_is_idempotent() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        runtime.block_on(async {
            let dir = temp_dir("optimize");
            let _ = std::fs::remove_dir_all(&dir);
            let backend = EdgeQdrant::new(&dir, false);
            backend
                .execute_planned(&plan_one("CREATE COLLECTION docs (dense VECTOR(1, DOT))"))
                .await
                .expect("create collection");

            // qdrant-edge only vacuums shards with at least 1000 vectors.
            let points = (1..=1000)
                .map(|id| format!("{{id: {id}, vector: {{dense: [{id}.0]}}}}"))
                .collect::<Vec<_>>()
                .join(", ");
            backend
                .execute_planned(&plan_one(&format!("UPSERT INTO docs VALUES {points}")))
                .await
                .expect("bulk upsert");

            // Delete 250/1000 = 25%, above the default 20% vacuum threshold.
            let deleted = (1..=250).map(PlanPointId::Number).collect();
            backend
                .execute_planned(&PlannedOperation::Delete {
                    collection: "docs".to_string(),
                    request: qql_plan::types::DeleteRequest {
                        points: Some(deleted),
                        filter: None,
                        shard_key: None,
                    },
                    wait: false,
                })
                .await
                .expect("bulk delete");

            let optimized = backend.optimize_collection("docs").await.expect("optimize");
            assert!(
                optimized,
                "25% deletion on 1000 points must trigger the vacuum optimizer"
            );
            let optimized_again = backend
                .optimize_collection("docs")
                .await
                .expect("second optimize");
            assert!(
                !optimized_again,
                "a second optimize call must be a no-op (idempotent)"
            );

            let count = backend
                .execute_planned(&plan_one("COUNT FROM docs"))
                .await
                .expect("count after optimize");
            assert_eq!(count.data, ExecData::Count(750));

            backend.close().await.expect("close edge backend");
            let _ = std::fs::remove_dir_all(dir);
        });
    }

    /// `indexed_vectors_count` reaches the typed `CollectionInfo` and lags
    /// `points_count` until `optimize()` builds the index — the contract the
    /// CLI doctor/check readout and optimize command rely on.
    #[test]
    fn info_reports_indexed_vectors_until_optimized() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        runtime.block_on(async {
            let dir = temp_dir("indexed-count");
            let _ = std::fs::remove_dir_all(&dir);
            let backend = EdgeQdrant::new(&dir, false);
            // 1 KB threshold so 300 four-dimensional points force indexing.
            backend
                .execute_planned(&plan_one(
                    "CREATE COLLECTION docs (dense VECTOR(4, COSINE)) \
                     WITH OPTIMIZERS (indexing_threshold = 1)",
                ))
                .await
                .expect("create collection");

            let points = (1..=300)
                .map(|id| format!("{{id: {id}, vector: {{dense: [{id}.0, 0.0, 0.0, 0.0]}}}}"))
                .collect::<Vec<_>>()
                .join(", ");
            backend
                .execute_planned(&plan_one(&format!("UPSERT INTO docs VALUES {points}")))
                .await
                .expect("bulk upsert");

            let before = backend
                .get_collection_info("docs")
                .await
                .expect("collection info");
            assert_eq!(before.points_count, 300);
            assert_eq!(
                before.indexed_vectors_count,
                Some(0),
                "an unoptimized appendable segment has no vector index yet"
            );

            assert!(
                backend.optimize_collection("docs").await.expect("optimize"),
                "the indexing optimizer must fire above the threshold"
            );

            let after = backend
                .get_collection_info("docs")
                .await
                .expect("collection info after optimize");
            assert_eq!(after.points_count, 300);
            assert!(
                after.indexed_vectors_count.unwrap_or(0) > 0,
                "optimize must index vectors: {after:?}"
            );
            assert_eq!(
                after.indexed_vectors_count,
                Some(300),
                "a 1 KB threshold must index every vector of the segment: {after:?}"
            );

            backend.close().await.expect("close edge backend");
            let _ = std::fs::remove_dir_all(dir);
        });
    }

    /// Filter lowering is shared: a filtered COUNT answers through the typed
    /// path with the direct (non-serde) lowering, proving the typed filter
    /// converter is wired into query execution.
    #[test]
    fn typed_count_uses_direct_filter_lowering() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        runtime.block_on(async {
            let dir = temp_dir("typed-filter");
            let _ = std::fs::remove_dir_all(&dir);
            let backend = EdgeQdrant::new(&dir, false);
            seed_docs(&backend).await;

            let count = plan_one("COUNT FROM docs WHERE price >= 20");
            let response = backend.execute_planned(&count).await.expect("typed count");
            assert_eq!(response.data, ExecData::Count(2));

            backend.close().await.expect("close edge backend");
            let _ = std::fs::remove_dir_all(dir);
        });
    }

    /// FACET used to be rejected offline. Driving the full executor proves
    /// dispatch selects the typed edge path and FACET succeeds end to end.
    #[test]
    fn executor_facet_succeeds_end_to_end() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        runtime.block_on(async {
            let dir = temp_dir("executor-facet");
            let _ = std::fs::remove_dir_all(&dir);
            let executor = Executor::new(Box::new(EdgeQdrant::new(&dir, false)), None);

            let report = executor
                .execute(
                    "CREATE COLLECTION docs (dense VECTOR(3, COSINE))",
                    OnError::Stop,
                )
                .await
                .expect("create collection");
            assert!(report.ok, "create failed: {report:?}");

            for qql in [
                "UPSERT INTO docs VALUES {id: 1, vector: {dense: [1.0, 0.0, 0.0]}, city: 'NYC'}",
                "UPSERT INTO docs VALUES {id: 2, vector: {dense: [0.0, 1.0, 0.0]}, city: 'NYC'}",
                "UPSERT INTO docs VALUES {id: 3, vector: {dense: [0.0, 0.0, 1.0]}, city: 'SF'}",
            ] {
                let report = executor.execute(qql, OnError::Stop).await.expect("upsert");
                assert!(report.ok, "upsert failed: {report:?}");
            }

            let report = executor
                .execute(
                    "CREATE INDEX ON COLLECTION docs FOR city TYPE keyword",
                    OnError::Stop,
                )
                .await
                .expect("create index");
            assert!(report.ok, "create index failed: {report:?}");

            let report = executor
                .execute("FACET city FROM docs LIMIT 10", OnError::Stop)
                .await
                .expect("facet run");
            assert!(report.ok, "FACET must succeed offline now: {report:?}");
            let response = report.first().expect("facet response");
            assert!(
                matches!(response.data.as_ref(), Some(ExecData::Facet(_))),
                "expected typed facet data, got {:?}",
                response.data
            );
            let entries = response.facet().expect("facet entries");
            assert_eq!(entries.len(), 2, "facet entries: {entries:?}");
            assert!(entries.contains(&(PlanFacetValue::Keyword("NYC".into()), 2)));
            assert!(entries.contains(&(PlanFacetValue::Keyword("SF".into()), 1)));

            executor.close().await.expect("close edge executor");
            let _ = std::fs::remove_dir_all(dir);
        });
    }

    /// A read against a nonexistent collection keeps the typed not-found code
    /// (no ghost collection, no generic engine error).
    #[test]
    fn missing_collection_reports_typed_not_found() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        runtime.block_on(async {
            let dir = temp_dir("missing-collection");
            let _ = std::fs::remove_dir_all(&dir);
            let backend = EdgeQdrant::new(&dir, false);

            let error = backend
                .execute_planned(&plan_one("SHOW COLLECTION never_created"))
                .await
                .expect_err("missing collection must fail");
            assert_eq!(error.code, "QQL-EDGE-COLLECTION-NOT-FOUND");
            assert_eq!(error.field("collection"), Some("never_created"));

            let _ = std::fs::remove_dir_all(dir);
        });
    }

    /// A wrong-dimension upsert is rejected by the engine and keeps its precise
    /// category instead of collapsing into a generic library error.
    #[test]
    fn engine_dimension_mismatch_reports_dimension_code() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        runtime.block_on(async {
            let dir = temp_dir("engine-dimension");
            let _ = std::fs::remove_dir_all(&dir);
            let backend = EdgeQdrant::new(&dir, false);
            backend
                .execute_planned(&plan_one(
                    "CREATE COLLECTION docs (dense VECTOR(3, COSINE))",
                ))
                .await
                .expect("create collection");

            let error = backend
                .execute_planned(&plan_one(
                    "UPSERT INTO docs VALUES {id: 1, vector: {dense: [1.0, 2.0]}}",
                ))
                .await
                .expect_err("dimension mismatch must fail");
            assert_eq!(error.code, "QQL-EDGE-DIMENSION");
            assert_eq!(error.field("operation"), Some("upsert"));
            assert_eq!(error.field("collection"), Some("docs"));
            assert!(
                error.message.contains("Vector dimension error"),
                "engine cause lost: {}",
                error.message
            );

            backend.close().await.expect("close edge backend");
            let _ = std::fs::remove_dir_all(dir);
        });
    }

    /// Non-UUID string point IDs are rejected before the engine call, with the
    /// dedicated id-conversion code.
    #[test]
    fn invalid_string_point_id_reports_code() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        runtime.block_on(async {
            let dir = temp_dir("invalid-point-id");
            let _ = std::fs::remove_dir_all(&dir);
            let backend = EdgeQdrant::new(&dir, false);
            backend
                .execute_planned(&plan_one(
                    "CREATE COLLECTION docs (dense VECTOR(3, COSINE))",
                ))
                .await
                .expect("create collection");

            let error = backend
                .execute_planned(&plan_one("QUERY POINTS ('doc-1') FROM docs"))
                .await
                .expect_err("non-UUID string id must fail");
            assert_eq!(error.code, "QQL-EDGE-INVALID-POINT-ID");
            assert!(
                error.message.contains("doc-1"),
                "id lost from message: {}",
                error.message
            );

            backend.close().await.expect("close edge backend");
            let _ = std::fs::remove_dir_all(dir);
        });
    }

    /// A corrupted shard config makes `EdgeShard::load` fail; the failure must
    /// surface as the engine-storage code with the crate's own cause text.
    #[test]
    fn corrupted_shard_load_reports_storage_code() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        runtime.block_on(async {
            let dir = temp_dir("corrupt-shard");
            let _ = std::fs::remove_dir_all(&dir);
            let backend = EdgeQdrant::new(&dir, false);
            backend
                .execute_planned(&plan_one(
                    "CREATE COLLECTION docs (dense VECTOR(3, COSINE))",
                ))
                .await
                .expect("create collection");
            backend.close().await.expect("close edge backend");

            // Corrupt the persisted shard config so the next load fails inside
            // qdrant-edge (`ServiceError` from the JSON read).
            std::fs::write(dir.join("docs").join("edge_config.json"), b"{ not json")
                .expect("corrupt config");

            let error = backend
                .execute_planned(&plan_one("SHOW COLLECTION docs"))
                .await
                .expect_err("corrupt shard must fail to load");
            assert_eq!(error.code, "QQL-EDGE-STORAGE");
            assert_eq!(error.field("operation"), Some("load"));
            assert_eq!(error.field("collection"), Some("docs"));
            assert!(
                error.message.contains("qdrant-edge load failed:"),
                "operation context lost: {}",
                error.message
            );

            let _ = std::fs::remove_dir_all(dir);
        });
    }

    /// An I/O failure creating the collection directory (base path is a file)
    /// reports the dedicated storage-path code.
    #[test]
    fn create_directory_io_failure_reports_code() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        runtime.block_on(async {
            let base = temp_dir("create-dir-io");
            let _ = std::fs::remove_dir_all(&base);
            std::fs::write(&base, b"not a directory").expect("seed file");
            let backend = EdgeQdrant::new(&base, false);

            let error = backend
                .execute_planned(&plan_one(
                    "CREATE COLLECTION docs (dense VECTOR(1, COSINE))",
                ))
                .await
                .expect_err("create_dir_all over a file must fail");
            assert_eq!(error.code, "QQL-EDGE-CREATE-DIR");

            let _ = std::fs::remove_file(&base);
        });
    }

    /// `QUERY … GROUP BY` runs through qdrant-edge's grouping driver: typed
    /// group ids, hydrated payloads, and the executor's client-side
    /// `group_offset` trimming.
    #[test]
    fn executor_group_by_returns_typed_groups() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        runtime.block_on(async {
            let dir = temp_dir("group-by");
            let _ = std::fs::remove_dir_all(&dir);
            let executor = Executor::new(Box::new(EdgeQdrant::new(&dir, false)), None);

            let report = executor
                .execute(
                    "CREATE COLLECTION docs (dense VECTOR(3, DOT))",
                    OnError::Stop,
                )
                .await
                .expect("create collection");
            assert!(report.ok, "create failed: {report:?}");
            let report = executor
                .execute(
                    "CREATE INDEX ON COLLECTION docs FOR district TYPE keyword",
                    OnError::Stop,
                )
                .await
                .expect("create index");
            assert!(report.ok, "index failed: {report:?}");
            for qql in [
                "UPSERT INTO docs VALUES {id: 1, vector: {dense: [3.0, 0.0, 0.0]}, district: 'NYC', price: 10}",
                "UPSERT INTO docs VALUES {id: 2, vector: {dense: [2.0, 0.0, 0.0]}, district: 'SF', price: 20}",
                "UPSERT INTO docs VALUES {id: 3, vector: {dense: [1.0, 0.0, 0.0]}, district: 'LA', price: 30}",
                "UPSERT INTO docs VALUES {id: 4, vector: {dense: [0.5, 0.0, 0.0]}, district: 'NYC', price: 40}",
            ] {
                let report = executor.execute(qql, OnError::Stop).await.expect("upsert");
                assert!(report.ok, "{qql} failed: {report:?}");
            }

            let report = executor
                .execute(
                    "QUERY [3.0, 0.0, 0.0] FROM docs USING dense GROUP BY district LIMIT 10",
                    OnError::Stop,
                )
                .await
                .expect("group run");
            assert!(report.ok, "GROUP BY must succeed offline: {report:?}");
            let response = report.first().expect("group response");
            let Some(ExecData::Groups(groups)) = response.data.as_ref() else {
                panic!("expected typed groups, got {:?}", response.data);
            };
            assert_eq!(groups.len(), 3, "groups: {groups:?}");
            assert_eq!(groups[0].group_id, PlanGroupId::Keyword("NYC".into()));
            assert_eq!(groups[1].group_id, PlanGroupId::Keyword("SF".into()));
            assert_eq!(groups[2].group_id, PlanGroupId::Keyword("LA".into()));
            assert_eq!(groups[0].hits.len(), 2, "NYC has two points");
            assert_eq!(groups[1].hits.len(), 1);
            // Hydration: the grouping driver only fetched the `district` field.
            let payload = groups[0].hits[0]
                .payload
                .as_ref()
                .expect("hydrated payload");
            assert_eq!(payload.get("price"), Some(&json!(10)));

            // `group_offset` has no wire field: the backend returns
            // LIMIT+OFFSET groups and the executor trims OFFSET client-side.
            let report = executor
                .execute(
                    "QUERY [3.0, 0.0, 0.0] FROM docs USING dense GROUP BY district LIMIT 1 OFFSET 1",
                    OnError::Stop,
                )
                .await
                .expect("group offset run");
            assert!(report.ok, "group offset failed: {report:?}");
            let response = report.first().expect("group response");
            let Some(ExecData::Groups(groups)) = response.data.as_ref() else {
                panic!("expected typed groups, got {:?}", response.data);
            };
            assert_eq!(groups.len(), 1, "offset must trim one group: {groups:?}");
            assert_eq!(groups[0].group_id, PlanGroupId::Keyword("SF".into()));

            // `LOOKUP FROM` has no edge equivalent; only that sub-feature fails.
            let report = executor
                .execute(
                    "QUERY [3.0, 0.0, 0.0] FROM docs USING dense \
                     GROUP BY district LOOKUP FROM districts LIMIT 5",
                    OnError::Continue,
                )
                .await
                .expect("lookup run");
            assert!(!report.ok, "LOOKUP FROM must fail");
            assert!(
                report.results[0]
                    .message
                    .contains("QQL-EDGE-UNSUPPORTED-GROUP-LOOKUP"),
                "expected GROUP-LOOKUP code, got {:?}",
                report.results[0].message
            );

            executor.close().await.expect("close edge executor");
            let _ = std::fs::remove_dir_all(dir);
        });
    }

    /// `PARAMS (acorn = …)` executes on edge: qdrant-edge 0.8 models ACORN as
    /// a first-class `SearchParams` field, so the query must reach the engine.
    #[test]
    fn acorn_params_execute_on_edge() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        runtime.block_on(async {
            let dir = temp_dir("acorn");
            let _ = std::fs::remove_dir_all(&dir);
            let backend = EdgeQdrant::new(&dir, false);
            seed_docs(&backend).await;

            let query = plan_one(
                "QUERY [1.0, 0.0, 0.0] FROM docs USING dense WHERE city = 'NYC' \
                 PARAMS (acorn = true, max_selectivity = 0.4) LIMIT 5",
            );
            let response = backend
                .execute_planned(&query)
                .await
                .expect("ACORN query must execute offline");
            let ExecData::Hits(hits) = &response.data else {
                panic!("expected typed hits, got {:?}", response.data);
            };
            assert_eq!(hits.len(), 2);

            backend.close().await.expect("close edge backend");
            let _ = std::fs::remove_dir_all(dir);
        });
    }

    /// `ALTER COLLECTION` applies the qdrant-edge config setters and persists
    /// them; params / quantization / excluded optimizer keys stay fail-closed
    /// per field.
    #[test]
    fn alter_collection_applies_engine_config() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        runtime.block_on(async {
            let dir = temp_dir("alter-collection");
            let _ = std::fs::remove_dir_all(&dir);
            let backend = EdgeQdrant::new(&dir, false);
            backend
                .execute_planned(&plan_one(
                    "CREATE COLLECTION docs (dense VECTOR(3, COSINE))",
                ))
                .await
                .expect("create collection");

            let response = backend
                .execute_planned(&plan_one(
                    "ALTER COLLECTION docs WITH HNSW (m = 32) \
                     WITH OPTIMIZERS (indexing_threshold = 500)",
                ))
                .await
                .expect("ALTER must apply");
            assert_eq!(response.data, ExecData::Mutation { affected: None });

            {
                let shard = backend.open_shard("docs").await.expect("open shard");
                let config = shard.config();
                assert_eq!(config.hnsw_config().m, 32);
                assert_eq!(config.optimizers().indexing_threshold, Some(500));
            }

            backend
                .execute_planned(&plan_one(
                    "ALTER COLLECTION docs WITH HNSW (ef_construct = 200) \
                     WITH OPTIMIZERS (deleted_threshold = 0.3)",
                ))
                .await
                .expect("partial ALTER must merge");
            {
                let shard = backend.open_shard("docs").await.expect("open shard");
                let config = shard.config();
                assert_eq!(config.hnsw_config().m, 32, "partial ALTER must keep m");
                assert_eq!(config.hnsw_config().ef_construct, 200);
                assert_eq!(config.optimizers().indexing_threshold, Some(500));
                assert_eq!(config.optimizers().deleted_threshold, Some(0.3));
            }

            // Persisted: a fresh backend sees the updated engine config.
            backend.close().await.expect("close edge backend");
            let reopened = EdgeQdrant::new(&dir, false);
            {
                let shard = reopened.open_shard("docs").await.expect("reopen shard");
                let config = shard.config();
                assert_eq!(config.hnsw_config().m, 32);
                assert_eq!(config.hnsw_config().ef_construct, 200);
                assert_eq!(config.optimizers().indexing_threshold, Some(500));
                assert_eq!(config.optimizers().deleted_threshold, Some(0.3));
            }

            let error = reopened
                .execute_planned(&plan_one(
                    "ALTER COLLECTION docs WITH PARAMS (replication_factor = 3)",
                ))
                .await
                .expect_err("params have no edge setter");
            assert_eq!(error.code, "QQL-EDGE-UNSUPPORTED-ALTER-PARAMS");

            let error = reopened
                .execute_planned(&plan_one(
                    "ALTER COLLECTION docs WITH QUANTIZATION (disabled = true)",
                ))
                .await
                .expect_err("quantization has no edge setter");
            assert_eq!(error.code, "QQL-EDGE-UNSUPPORTED-ALTER-QUANTIZATION");

            let error = reopened
                .execute_planned(&plan_one(
                    "ALTER COLLECTION docs WITH OPTIMIZERS (flush_interval_sec = 5)",
                ))
                .await
                .expect_err("excluded optimizer key");
            assert_eq!(error.code, "QQL-EDGE-UNSUPPORTED-OPTIMIZER-KEY");

            reopened.close().await.expect("close edge backend");
            let _ = std::fs::remove_dir_all(dir);
        });
    }

    /// Per-vector `ALTER COLLECTION`: HNSW patches field-merge over the
    /// vector's effective config and persist across reopen; every other
    /// per-vector field and all sparse diffs reject per field, only when
    /// present, and a mixed request never half-applies.
    #[test]
    fn alter_collection_applies_per_vector_hnsw_diff() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        runtime.block_on(async {
            let dir = temp_dir("alter-vector-diff");
            let _ = std::fs::remove_dir_all(&dir);
            let backend = EdgeQdrant::new(&dir, false);
            backend
                .execute_planned(&plan_one(
                    "CREATE COLLECTION docs (dense VECTOR(3, COSINE), other VECTOR(3, COSINE)) \
                     WITH HNSW (m = 8, ef_construct = 64)",
                ))
                .await
                .expect("create collection");

            backend
                .execute_planned(&plan_one(
                    "ALTER COLLECTION docs WITH VECTOR dense (HNSW (m = 32))",
                ))
                .await
                .expect("per-vector HNSW must apply");
            {
                let shard = backend.open_shard("docs").await.expect("open shard");
                let config = shard.config();
                let dense = config.vectors.get("dense").expect("dense vector");
                let hnsw = dense.hnsw_config.expect("per-vector HNSW");
                assert_eq!(hnsw.m, 32);
                // Unset diff fields inherit the collection-wide value.
                assert_eq!(hnsw.ef_construct, 64);
                assert!(
                    config
                        .vectors
                        .get("other")
                        .expect("other vector")
                        .hnsw_config
                        .is_none(),
                    "untouched vectors keep no per-vector override"
                );
            }

            // A partial diff merges over the vector's own config.
            backend
                .execute_planned(&plan_one(
                    "ALTER COLLECTION docs WITH VECTOR dense (HNSW (ef_construct = 200))",
                ))
                .await
                .expect("partial per-vector HNSW must merge");
            {
                let shard = backend.open_shard("docs").await.expect("open shard");
                let hnsw = shard
                    .config()
                    .vectors
                    .get("dense")
                    .and_then(|params| params.hnsw_config)
                    .expect("per-vector HNSW");
                assert_eq!(hnsw.m, 32, "partial diff keeps m");
                assert_eq!(hnsw.ef_construct, 200);
            }

            backend.close().await.expect("close edge backend");
            let reopened = EdgeQdrant::new(&dir, false);
            {
                let shard = reopened.open_shard("docs").await.expect("reopen shard");
                let hnsw = shard
                    .config()
                    .vectors
                    .get("dense")
                    .and_then(|params| params.hnsw_config)
                    .expect("persisted per-vector HNSW");
                assert_eq!(hnsw.m, 32);
                assert_eq!(hnsw.ef_construct, 200);
            }

            for (query, key) in [
                (
                    "ALTER COLLECTION docs WITH VECTOR dense (VECTOR (memory = 'cold'))",
                    "memory",
                ),
                (
                    "ALTER COLLECTION docs WITH VECTOR dense (QUANTIZATION (type = 'scalar'))",
                    "quantization_config",
                ),
            ] {
                let error = reopened
                    .execute_planned(&plan_one(query))
                    .await
                    .expect_err("per-vector field has no edge setter");
                assert_eq!(error.code, "QQL-EDGE-UNSUPPORTED-VECTOR-DIFF");
                assert_eq!(error.field("vector_name"), Some("dense"));
                assert_eq!(error.field("config_key"), Some(key));
            }

            let error = reopened
                .execute_planned(&plan_one(
                    "ALTER COLLECTION docs WITH SPARSE bm25 (SPARSE (modifier = 'idf'))",
                ))
                .await
                .expect_err("sparse diff has no edge setter");
            assert_eq!(error.code, "QQL-EDGE-UNSUPPORTED-SPARSE-DIFF");
            assert_eq!(error.field("vector_name"), Some("bm25"));
            assert_eq!(error.field("config_key"), Some("modifier"));

            // A mixed request is validated before any setter runs.
            let error = reopened
                .execute_planned(&plan_one(
                    "ALTER COLLECTION docs WITH VECTOR dense (HNSW (m = 48), VECTOR (on_disk = true))",
                ))
                .await
                .expect_err("mixed request must reject");
            assert_eq!(error.code, "QQL-EDGE-UNSUPPORTED-VECTOR-DIFF");
            {
                let shard = reopened.open_shard("docs").await.expect("open shard");
                let hnsw = shard
                    .config()
                    .vectors
                    .get("dense")
                    .and_then(|params| params.hnsw_config)
                    .expect("per-vector HNSW");
                assert_eq!(hnsw.m, 32, "rejected request must not half-apply HNSW");
            }

            reopened.close().await.expect("close edge backend");
            let _ = std::fs::remove_dir_all(dir);
        });
    }

    /// Create-time `WITH PARAMS (on_disk_payload = …)` maps onto the engine
    /// config; other param keys still fail closed.
    #[test]
    fn create_collection_params_honor_on_disk_payload() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        runtime.block_on(async {
            let dir = temp_dir("create-params");
            let _ = std::fs::remove_dir_all(&dir);
            // Executor-level default is on-disk; the statement overrides it.
            let backend = EdgeQdrant::new(&dir, true);
            backend
                .execute_planned(&plan_one(
                    "CREATE COLLECTION docs (dense VECTOR(1, COSINE)) \
                     WITH PARAMS (on_disk_payload = false)",
                ))
                .await
                .expect("on_disk_payload must be accepted");

            let raw = std::fs::read(dir.join("docs").join("edge_config.json"))
                .expect("read persisted config");
            let config: serde_json::Value = serde_json::from_slice(&raw).expect("parse config");
            assert_eq!(config["on_disk_payload"], json!(false));

            let error = backend
                .execute_planned(&plan_one(
                    "CREATE COLLECTION other (dense VECTOR(1, COSINE)) \
                     WITH PARAMS (replication_factor = 3)",
                ))
                .await
                .expect_err("replication params have no edge equivalent");
            assert_eq!(error.code, "QQL-EDGE-UNSUPPORTED-COLLECTION-PARAMS");

            backend.close().await.expect("close edge backend");
            let _ = std::fs::remove_dir_all(dir);
        });
    }

    /// Per-vector storage / datatype / quantization / HNSW and sparse index
    /// options from `CREATE COLLECTION` land in the engine config instead of
    /// being dropped silently.
    #[test]
    fn create_collection_passes_per_vector_engine_config() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        runtime.block_on(async {
            let dir = temp_dir("create-per-vector");
            let _ = std::fs::remove_dir_all(&dir);
            let backend = EdgeQdrant::new(&dir, false);
            backend
                .execute_planned(&plan_one(
                    "CREATE COLLECTION docs ( \
                       dense VECTOR(4, COSINE) \
                         WITH HNSW (m = 24) \
                         WITH QUANTIZATION (type = 'scalar', quantile = 0.99) \
                         WITH VECTOR (memory = 'cold', datatype = 'float16'), \
                       sparse SPARSE \
                         WITH SPARSE (full_scan_threshold = 5000, memory = 'pinned', \
                                      datatype = 'float16', modifier = 'idf') \
                     )",
                ))
                .await
                .expect("create collection with per-vector engine config");

            {
                let shard = backend.open_shard("docs").await.expect("open shard");
                let config = shard.config();

                let dense = config.vectors.get("dense").expect("dense vector");
                assert_eq!(
                    dense.on_disk,
                    Some(true),
                    "memory = 'cold' maps to the engine's on-disk flag"
                );
                assert_eq!(
                    dense.datatype,
                    Some(qdrant_edge::VectorStorageDatatype::Float16)
                );
                assert!(
                    dense.quantization_config.is_some(),
                    "per-vector quantization must reach the engine"
                );
                assert_eq!(
                    dense.hnsw_config.expect("per-vector HNSW").m,
                    24,
                    "per-vector HNSW must reach the engine"
                );

                let sparse = config.sparse_vectors.get("sparse").expect("sparse vector");
                assert_eq!(sparse.full_scan_threshold, Some(5_000));
                assert_eq!(
                    sparse.on_disk,
                    Some(false),
                    "memory = 'pinned' keeps the sparse index in RAM"
                );
                assert_eq!(
                    sparse.datatype,
                    Some(qdrant_edge::VectorStorageDatatype::Float16)
                );
                assert_eq!(sparse.modifier, Some(qdrant_edge::Modifier::Idf));
            }

            // Config persists and reloads with the same values.
            backend.close().await.expect("close edge backend");
            let reopened = EdgeQdrant::new(&dir, false);
            {
                let shard = reopened.open_shard("docs").await.expect("reopen shard");
                let config = shard.config();
                assert_eq!(
                    config
                        .vectors
                        .get("dense")
                        .and_then(|v| v.hnsw_config)
                        .map(|h| h.m),
                    Some(24)
                );
            }

            reopened.close().await.expect("close edge backend");
            let _ = std::fs::remove_dir_all(dir);
        });
    }

    /// The WAL segment capacity is seeded once: a persisted value wins over
    /// later env/CLI values, and reopening with the knob still set (or unset)
    /// does not rewrite the persisted config when the value is unchanged.
    #[test]
    fn wal_segment_capacity_seeds_once_then_persisted_wins() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        runtime.block_on(async {
            let dir = temp_dir("wal-capacity");
            let _ = std::fs::remove_dir_all(&dir);
            let config_path = dir.join("docs").join("edge_config.json");
            let capacity = 4 * 1024 * 1024;
            let larger = 8 * 1024 * 1024;

            // Seed: the knob applies while the shard has no persisted value.
            let backend = EdgeQdrant::new(&dir, false).with_wal_segment_capacity(Some(capacity));
            backend
                .execute_planned(&plan_one(
                    "CREATE COLLECTION docs (dense VECTOR(1, COSINE))",
                ))
                .await
                .expect("create collection");
            {
                let shard = backend.open_shard("docs").await.expect("open shard");
                assert_eq!(
                    shard
                        .config()
                        .wal_options
                        .as_ref()
                        .map(|w| w.segment_capacity),
                    Some(capacity),
                    "create must persist the configured WAL capacity"
                );
            }
            backend.close().await.expect("close edge backend");
            let seeded_bytes = std::fs::read(&config_path).expect("read seeded config");

            // Persisted wins: a different knob value must not ratchet the
            // shard config on load.
            let reopened = EdgeQdrant::new(&dir, false).with_wal_segment_capacity(Some(larger));
            {
                let shard = reopened.open_shard("docs").await.expect("reopen shard");
                assert_eq!(
                    shard
                        .config()
                        .wal_options
                        .as_ref()
                        .map(|w| w.segment_capacity),
                    Some(capacity),
                    "persisted capacity must win over a later env/CLI value"
                );
            }
            reopened.close().await.expect("close edge backend");
            assert_eq!(
                std::fs::read(&config_path).expect("read after override"),
                seeded_bytes,
                "persisted config must not change when a different knob value is supplied"
            );

            // Env still set at the seeded value, then unset: the file must stay
            // byte-identical. (qdrant-edge 0.8 rewrites edge_config.json on
            // every load via `SaveOnDisk::new`, so mtime is not a stable
            // signal; content equality is the observable invariant.)
            for knob in [Some(capacity), None] {
                let backend = EdgeQdrant::new(&dir, false).with_wal_segment_capacity(knob);
                {
                    let shard = backend.open_shard("docs").await.expect("reopen shard");
                    assert_eq!(
                        shard
                            .config()
                            .wal_options
                            .as_ref()
                            .map(|w| w.segment_capacity),
                        Some(capacity),
                        "unset/equal knob must keep the persisted capacity"
                    );
                }
                backend.close().await.expect("close edge backend");
                assert_eq!(
                    std::fs::read(&config_path).expect("read after reopen"),
                    seeded_bytes,
                    "edge_config.json must stay identical across reopens ({knob:?})"
                );
            }

            // qdrant-edge 0.8's load path calls `SaveOnDisk::new`, which
            // rewrites `edge_config.json` unconditionally (verified: mtime
            // bumps while the content is identical). Content byte-equality —
            // asserted above — is the observable no-ratchet invariant without
            // patching the dependency.
            let _ = std::fs::remove_dir_all(dir);
        });
    }
}
