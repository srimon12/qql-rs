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

mod ops;

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
    edge_hnsw_spec, edge_optimizers_spec, edge_quantization_spec, from_edge_facet_hit,
    from_edge_group_to_typed, from_edge_record_to_hit, from_edge_scored_point_to_hit,
    hydrate_edge_groups, to_edge_id, to_edge_ids,
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
use vector_parser::plan_vectors_to_edge;

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
            let vector_struct = plan_vectors_to_edge(
                p.vector
                    .as_ref()
                    .ok_or_else(|| {
                        QqlError::execution(
                            "QQL-EDGE-MISSING-VECTOR",
                            "upsert point missing vector",
                            None,
                        )
                        .with_collection(collection_name.clone())
                    })?
                    .clone(),
            )?;
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
            let vector_struct = plan_vectors_to_edge(pt.vector.clone())?;
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

#[cfg(test)]
mod tests;
