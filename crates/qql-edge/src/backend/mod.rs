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
pub mod filter_converter;
pub mod query_converter;
pub mod unsupported;
pub mod vector_parser;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use qdrant_edge::{
    CreateIndex, EdgeConfigBuilder, EdgeShard, FieldIndexOperations, PayloadFieldSchema,
    PayloadSchemaType, PointInsertOperations, PointOperations, UpdateOperation, VectorOperations,
    VectorStructPersisted, WithPayloadInterface, WithVector,
};
use serde_json::Value;
use tokio::sync::{Mutex, RwLock};

use config_builder::build_edge_config;
use conversions::{
    edge_err, from_edge_facet_hit, from_edge_record_to_hit, from_edge_scored_point_to_hit,
    to_edge_id, to_edge_ids,
};
use filter_converter::convert_edge_filter;
use query_converter::{
    convert_order_by_interface, convert_query_request, convert_with_payload, convert_with_vector,
    parse_json_path,
};
use unsupported::{EdgeUnsupported, reject_collection_sharding, reject_shard_key};
use vector_parser::ToEdgeVector;

use qql::backend::{CollectionInfo, CollectionSchema};
use qql::client::QdrantOps;
use qql::executor::{BackendResponse, ExecData, FacetHit, SearchHit};
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
    shards: RwLock<HashMap<String, Arc<EdgeShard>>>,
    opening: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

/// Helper: create a spawn_blocking error with the operation name for context.
fn spawn_error(operation: &str, error: impl std::fmt::Display) -> QqlError {
    QqlError::execution("QQL-EDGE-SPAWN", format!("{operation}: {error}"), None)
        .with_field("operation", operation.to_string())
}

impl std::fmt::Debug for EdgeQdrant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EdgeQdrant")
            .field("base_path", &self.base_path)
            .field("on_disk_payload", &self.on_disk_payload)
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
            shards: RwLock::new(HashMap::new()),
            opening: Mutex::new(HashMap::new()),
        }
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
        self.open_shard_inner(name, None, false).await
    }

    async fn open_shard_with_req(
        &self,
        name: &str,
        req: Option<&CreateCollectionRequest>,
    ) -> Result<Arc<EdgeShard>, QqlError> {
        self.open_shard_inner(name, req, true).await
    }

    async fn open_shard_inner(
        &self,
        name: &str,
        req: Option<&CreateCollectionRequest>,
        create: bool,
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
        let collection = name.to_string();
        let config_res = req.map(|r| build_edge_config(r, on_disk));
        let shard = tokio::task::spawn_blocking(move || -> Result<EdgeShard, QqlError> {
            if path.join("segments").exists() {
                EdgeShard::load(&path, None).map_err(edge_err)
            } else if create {
                std::fs::create_dir_all(&path).map_err(|e| {
                    QqlError::execution(
                        "QQL-EDGE-CREATE-DIR",
                        format!("create collection directory: {e}"),
                        None,
                    )
                })?;

                let config = match config_res {
                    Some(c) => c?,
                    None => EdgeConfigBuilder::new().on_disk_payload(on_disk).build(),
                };

                EdgeShard::new(&path, config).map_err(edge_err)
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
        let results = shard.query(edge_req).map_err(edge_err)?;
        Ok(results
            .into_iter()
            .map(from_edge_scored_point_to_hit)
            .collect())
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
            .map_err(edge_err)?;
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

        let (records, _next) = shard.scroll(scroll_req).map_err(edge_err)?;
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

        shard.update(op).map_err(edge_err)
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

        shard.update(operation).map_err(edge_err)
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

        shard.update(operation).map_err(edge_err)
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

        shard.update(operation).map_err(edge_err)
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

        shard.update(operation).map_err(edge_err)
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

        shard.update(op).map_err(edge_err)
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
            .map_err(edge_err)
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
        let count = shard.count(count_req).map_err(edge_err)?;
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
                QqlError::execution(
                    "QQL-EDGE-FACET",
                    format!("facet limit {limit} exceeds platform usize"),
                    None,
                )
                .with_collection(collection.to_string())
            })?;
        }
        edge_req.filter = convert_edge_filter(req.filter.as_ref())?;
        edge_req.exact = req.exact.unwrap_or(false);

        let response = shard.facet(edge_req).map_err(edge_err)?;
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
        let info = shard.info().map_err(edge_err)?;
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
                hnsw: None,
                quantization: None,
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
                // qdrant-edge 0.8 still surfaces `on_disk` rather than
                // the full memory placement enum on its public config.
                memory: None,
            })
            .collect();
        let sparse_vectors = cfg
            .sparse_vectors
            .keys()
            .map(|k| qql::backend::SparseVectorSpec {
                name: k.clone(),
                index: None,
                modifier: None,
            })
            .collect();

        Ok(CollectionInfo {
            status: "green".to_string(),
            points_count: info.points_count as u64,
            segments_count: info.segments_count as u64,
            schema: CollectionSchema {
                dense_vectors,
                sparse_vectors,
                vectors,
                ..Default::default()
            },
        })
    }

    async fn create_collection(
        &self,
        collection_name: &str,
        req: &CreateCollectionRequest,
    ) -> Result<(), QqlError> {
        if self.collection_exists(collection_name).await? {
            return Err(QqlError::execution(
                "QQL-EDGE-COLLECTION-EXISTS",
                format!("collection '{collection_name}' already exists"),
                None,
            )
            .with_collection(collection_name.to_string()));
        }
        self.open_shard_with_req(collection_name, Some(req)).await?;
        Ok(())
    }

    async fn update_collection(
        &self,
        _collection_name: &str,
        _req: &UpdateCollectionRequest,
    ) -> Result<(), QqlError> {
        Err(EdgeUnsupported::AlterCollection.error())
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

        let schema_type = match req.field_schema.to_lowercase().as_str() {
            "keyword" => PayloadSchemaType::Keyword,
            "uuid" => PayloadSchemaType::Uuid,
            "integer" | "int" => PayloadSchemaType::Integer,
            "float" => PayloadSchemaType::Float,
            "bool" | "boolean" => PayloadSchemaType::Bool,
            "geo" => PayloadSchemaType::Geo,
            "text" => PayloadSchemaType::Text,
            "datetime" => PayloadSchemaType::Datetime,
            other => {
                return Err(QqlError::execution(
                    "QQL-EDGE-UNSUPPORTED-FIELD-TYPE",
                    format!("unsupported field index type: '{other}'"),
                    None,
                )
                .with_collection(collection_name.to_string())
                .with_field_name(req.field_name.clone()));
            }
        };

        let field_schema = Some(PayloadFieldSchema::FieldType(schema_type));
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

        shard.update(op).map_err(edge_err)
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
        shard.update(op).map_err(edge_err)
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
            QueryGroups { .. } => return Err(EdgeUnsupported::GroupBy.error()),
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
            // DDL: status-only. Collection params/sharding stay fail-closed.
            CreateCollection {
                collection,
                request,
            } => {
                reject_collection_sharding(
                    request.shard_number,
                    request.sharding_method.as_deref(),
                    request.shard_keys.as_deref(),
                )?;
                if request.params.is_some() {
                    return Err(EdgeUnsupported::CollectionParams.error());
                }
                self.create_collection(collection, request).await?;
                ExecData::Mutation { affected: None }
            }
            UpdateCollection { .. } => return Err(EdgeUnsupported::AlterCollection.error()),
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
        tokio::task::spawn_blocking(move || shard.optimize().map_err(edge_err))
            .await
            .map_err(|e| spawn_error("optimize", e))?
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
        PlanFacetValue, PlanPointId, PlanVectorStruct, PlanVectorValue, PlannedOperation, plan,
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

            // Facet → Facet (previously rejected with QQL-EDGE-FACET).
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

    /// FACET used to fail offline with QQL-EDGE-FACET ("currently only
    /// supported via REST"). Driving the full executor proves dispatch now
    /// selects the typed edge path and FACET succeeds end to end.
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
}
