//! Local Qdrant backend via qdrant-edge — in-process HNSW search, zero network.

pub mod config_builder;
pub mod conversions;
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
    edge_err, from_edge_id, from_edge_record, from_edge_scored_point, to_edge_id, to_edge_ids,
};
use query_converter::{
    convert_order_by_interface, convert_query_request, convert_with_payload, convert_with_vector,
    parse_json_path,
};
use unsupported::{reject_collection_sharding, reject_shard_key, EdgeUnsupported};
use vector_parser::ToEdgeVector;

use qql::backend::{CollectionInfo, CollectionSchema};
use qql::client::QdrantOps;
use qql_core::error::QqlError;
use qql_plan::UpdateOperation as PlanUpdateOperation;
use qql_plan::{
    CreateCollectionRequest, CreateIndexRequest, QueryBatchRequest, UpdateBatchRequest,
    UpdateCollectionRequest,
};

pub struct EdgeQdrant {
    base_path: PathBuf,
    on_disk_payload: bool,
    shards: RwLock<HashMap<String, Arc<EdgeShard>>>,
    opening: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

fn mutation_response() -> Value {
    serde_json::json!({
        "result": { "status": "completed" },
        "status": "ok",
        "time": 0.0_f64,
    })
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

    async fn execute_edge_query(
        &self,
        collection: &str,
        req: &qql_plan::types::QueryRequest,
    ) -> Result<Value, QqlError> {
        let shard = self.open_shard(collection).await?;
        let req_clone = req.clone();
        let results = tokio::task::spawn_blocking(
            move || -> Result<Vec<qdrant_edge::ScoredPoint>, QqlError> {
                let edge_req = convert_query_request(&req_clone)?;
                shard.query(edge_req).map_err(edge_err)
            },
        )
        .await
        .map_err(|e| spawn_error("query", e))??;

        Ok(serde_json::json!({
            "result": {
                "points": results.into_iter().map(from_edge_scored_point).collect::<Vec<_>>()
            },
            "status": "ok",
            "time": 0.0,
        }))
    }

    async fn execute_edge_points(
        &self,
        collection: &str,
        req: &qql_plan::types::PointsRequest,
    ) -> Result<Value, QqlError> {
        reject_shard_key(req.shard_key.as_deref())?;
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

        let records = tokio::task::spawn_blocking(move || {
            shard
                .retrieve(qdrant_edge::RetrieveRequest {
                    point_ids: ids,
                    with_payload: Some(with_payload),
                    with_vector: Some(with_vector),
                })
                .map_err(edge_err)
        })
        .await
        .map_err(|e| spawn_error("get_points", e))??;

        Ok(serde_json::json!({
            "result": {
                "points": records.into_iter().map(from_edge_record).collect::<Vec<_>>()
            },
            "status": "ok",
            "time": 0.0,
        }))
    }

    async fn execute_edge_scroll(
        &self,
        collection: &str,
        req: &qql_plan::types::ScrollRequest,
    ) -> Result<Value, QqlError> {
        reject_shard_key(req.shard_key.as_deref())?;
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

        let (records, next) =
            tokio::task::spawn_blocking(move || shard.scroll(scroll_req).map_err(edge_err))
                .await
                .map_err(|e| spawn_error("scroll", e))??;

        let retrieved: Vec<Value> = records.into_iter().map(from_edge_record).collect();
        let next_offset = next.map(|id| from_edge_id(&id));
        let mut obj = serde_json::Map::new();
        obj.insert("status".into(), serde_json::json!("ok"));
        obj.insert("time".into(), serde_json::json!(0.0));
        obj.insert(
            "result".into(),
            serde_json::json!({
                "points": retrieved,
            }),
        );
        if let Some(no) = next_offset {
            obj.insert("next_page_offset".into(), no);
        }
        Ok(Value::Object(obj))
    }

    async fn execute_edge_upsert(
        &self,
        collection: &str,
        req: &qql_plan::types::UpsertRequest,
    ) -> Result<Value, QqlError> {
        reject_shard_key(req.shard_key.as_deref())?;
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

        tokio::task::spawn_blocking(move || shard.update(op).map_err(edge_err))
            .await
            .map_err(|e| spawn_error("upsert", e))??;

        Ok(mutation_response())
    }

    async fn execute_edge_delete(
        &self,
        collection: &str,
        req: &qql_plan::types::DeleteRequest,
    ) -> Result<Value, QqlError> {
        reject_shard_key(req.shard_key.as_deref())?;
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

        tokio::task::spawn_blocking(move || shard.update(operation).map_err(edge_err))
            .await
            .map_err(|e| spawn_error("delete", e))??;

        Ok(mutation_response())
    }

    async fn execute_edge_clear_payload(
        &self,
        collection: &str,
        req: &qql_plan::types::ClearPayloadRequest,
    ) -> Result<Value, QqlError> {
        reject_shard_key(req.shard_key.as_deref())?;
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

        tokio::task::spawn_blocking(move || shard.update(operation).map_err(edge_err))
            .await
            .map_err(|e| spawn_error("clear_payload", e))??;
        Ok(mutation_response())
    }

    async fn execute_edge_delete_payload(
        &self,
        collection: &str,
        req: &qql_plan::types::DeletePayloadRequest,
    ) -> Result<Value, QqlError> {
        reject_shard_key(req.shard_key.as_deref())?;
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

        tokio::task::spawn_blocking(move || shard.update(operation).map_err(edge_err))
            .await
            .map_err(|e| spawn_error("delete_payload", e))??;
        Ok(mutation_response())
    }

    async fn execute_edge_delete_vectors(
        &self,
        collection: &str,
        req: &qql_plan::types::DeleteVectorRequest,
    ) -> Result<Value, QqlError> {
        reject_shard_key(req.shard_key.as_deref())?;
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

        tokio::task::spawn_blocking(move || shard.update(operation).map_err(edge_err))
            .await
            .map_err(|e| spawn_error("delete_vectors", e))??;
        Ok(mutation_response())
    }

    async fn execute_edge_update_vectors(
        &self,
        collection: &str,
        req: &qql_plan::types::UpdateVectorRequest,
    ) -> Result<Value, QqlError> {
        reject_shard_key(req.shard_key.as_deref())?;
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

        tokio::task::spawn_blocking(move || shard.update(op).map_err(edge_err))
            .await
            .map_err(|e| spawn_error("update_vectors", e))??;

        Ok(mutation_response())
    }

    async fn execute_edge_update_payload(
        &self,
        collection: &str,
        req: &qql_plan::types::UpdatePayloadRequest,
    ) -> Result<Value, QqlError> {
        reject_shard_key(req.shard_key.as_deref())?;
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

        tokio::task::spawn_blocking(move || {
            shard
                .update(UpdateOperation::PayloadOperation(op))
                .map_err(edge_err)
        })
        .await
        .map_err(|e| spawn_error("set_payload", e))??;

        Ok(mutation_response())
    }

    async fn execute_edge_count(
        &self,
        collection: &str,
        req: &qql_plan::types::CountRequest,
    ) -> Result<Value, QqlError> {
        reject_shard_key(req.shard_key.as_deref())?;
        let shard = self.open_shard(collection).await?;
        let filter = convert_edge_filter(req.filter.as_ref())?;
        let count_req = qdrant_edge::CountRequest {
            filter,
            exact: req.exact.unwrap_or(true),
        };
        let count = tokio::task::spawn_blocking(move || shard.count(count_req).map_err(edge_err))
            .await
            .map_err(|e| spawn_error("count", e))??;
        Ok(serde_json::json!({
            "result": {
                "count": count,
            },
            "status": "ok",
            "time": 0.0,
        }))
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
                {
                    if let Some(name) = entry.file_name().to_str() {
                        if !name.starts_with('.') {
                            cols.push(name.to_string());
                        }
                    }
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
        let (info, dense_vectors, sparse_vectors, vectors) =
            tokio::task::spawn_blocking(move || -> Result<_, qql_core::error::QqlError> {
                let info = shard.info().map_err(edge_err)?;
                let cfg = shard.config();
                let dense = cfg
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
                let sparse = cfg
                    .sparse_vectors
                    .keys()
                    .map(|k| qql::backend::SparseVectorSpec {
                        name: k.clone(),
                        index: None,
                        modifier: None,
                    })
                    .collect();
                Ok((info, dense, sparse, vectors))
            })
            .await
            .map_err(|e| spawn_error("get_collection_info", e))??;

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
                .with_field_name(req.field_name.clone()))
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

        tokio::task::spawn_blocking(move || shard.update(op).map_err(edge_err))
            .await
            .map_err(|e| spawn_error("create_field_index", e))?
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
        tokio::task::spawn_blocking(move || shard.update(op).map_err(edge_err))
            .await
            .map_err(|e| spawn_error("delete_field_index", e))?
    }

    async fn execute_planned(&self, op: &qql_plan::PlannedOperation) -> Result<Value, QqlError> {
        reject_shard_key(op.shard_key())?;
        use qql_plan::PlannedOperation::*;
        match op {
            Query {
                collection,
                request,
            } => self.execute_edge_query(collection, request).await,
            QueryGroups { .. } => Err(EdgeUnsupported::GroupBy.error()),
            GetPoints {
                collection,
                request,
            } => self.execute_edge_points(collection, request).await,
            Scroll {
                collection,
                request,
            } => self.execute_edge_scroll(collection, request).await,
            Count {
                collection,
                request,
            } => self.execute_edge_count(collection, request).await,
            Upsert {
                collection,
                request,
                ..
            } => self.execute_edge_upsert(collection, request).await,
            Delete {
                collection,
                request,
            } => self.execute_edge_delete(collection, request).await,
            UpdatePayload {
                collection,
                request,
            } => self.execute_edge_update_payload(collection, request).await,
            ClearPayload {
                collection,
                request,
            } => self.execute_edge_clear_payload(collection, request).await,
            DeletePayload {
                collection,
                request,
            } => self.execute_edge_delete_payload(collection, request).await,
            UpdateVectors {
                collection,
                request,
            } => self.execute_edge_update_vectors(collection, request).await,
            DeleteVectors {
                collection,
                request,
            } => self.execute_edge_delete_vectors(collection, request).await,
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
                Ok(mutation_response())
            }
            UpdateCollection { .. } => Err(EdgeUnsupported::AlterCollection.error()),
            DropCollection { collection } => {
                self.delete_collection(collection).await?;
                Ok(mutation_response())
            }
            CreateIndex {
                collection,
                request,
            } => {
                self.create_field_index(collection, request).await?;
                Ok(mutation_response())
            }
            DropIndex { collection, field } => {
                self.delete_field_index(collection, field).await?;
                Ok(mutation_response())
            }
            CreateShardKey { .. } | DropShardKey { .. } | ListShardKeys { .. } => {
                Err(EdgeUnsupported::ShardKeyDdl.error())
            }
            ListCollections => {
                let cols = self.list_collections().await?;
                Ok(serde_json::json!({
                    "result": {
                        "collections": cols.into_iter().map(|c| serde_json::json!({"name": c})).collect::<Vec<_>>(),
                    },
                    "status": "ok",
                    "time": 0.0,
                }))
            }
            GetCollection { collection } => {
                let info = self.get_collection_info(collection).await?;
                Ok(serde_json::json!({
                    "result": {
                        "status": info.status,
                        "points_count": info.points_count,
                        "segments_count": info.segments_count,
                        "config": {
                            "params": {
                                "vectors": edge_vectors_json(&info.schema.vectors)?,
                                "sparse_vectors": edge_sparse_vectors_json(&info.schema.sparse_vectors),
                            }
                        },
                        "payload_schema": {},
                    },
                    "status": "ok",
                    "time": 0.0,
                }))
            }
            CrossRerank { .. } => Err(EdgeUnsupported::Route {
                path_hint: "CROSS RERANK",
            }
            .error()),
            GetQuotas | SetQuotas { .. } => Err(EdgeUnsupported::Quota.error()),
        }
    }

    async fn execute_query_batch(
        &self,
        collection: &str,
        batch: &QueryBatchRequest,
    ) -> Result<Vec<serde_json::Value>, QqlError> {
        for request in &batch.searches {
            reject_shard_key(request.shard_key.as_deref())?;
        }
        let mut results = Vec::with_capacity(batch.searches.len());
        for req in &batch.searches {
            results.push(self.execute_edge_query(collection, req).await?);
        }
        Ok(results)
    }

    async fn execute_update_batch(
        &self,
        collection: &str,
        batch: &UpdateBatchRequest,
    ) -> Result<Vec<serde_json::Value>, QqlError> {
        let mut results = Vec::with_capacity(batch.operations.len());
        for op in &batch.operations {
            match op {
                PlanUpdateOperation::Upsert { upsert } => {
                    results.push(self.execute_edge_upsert(collection, upsert).await?);
                }
                PlanUpdateOperation::Delete { delete } => {
                    results.push(self.execute_edge_delete(collection, delete).await?);
                }
                PlanUpdateOperation::SetPayload { set_payload } => {
                    results.push(
                        self.execute_edge_update_payload(collection, set_payload)
                            .await?,
                    );
                }
                PlanUpdateOperation::ClearPayload { clear_payload } => {
                    results.push(
                        self.execute_edge_clear_payload(collection, clear_payload)
                            .await?,
                    );
                }
                PlanUpdateOperation::UpdateVectors { update_vectors } => {
                    results.push(
                        self.execute_edge_update_vectors(collection, update_vectors)
                            .await?,
                    );
                }
                PlanUpdateOperation::DeleteVectors { delete_vectors } => {
                    results.push(
                        self.execute_edge_delete_vectors(collection, delete_vectors)
                            .await?,
                    );
                }
            }
        }
        Ok(results)
    }
}

fn edge_vectors_json(vectors: &[qql::backend::VectorSpec]) -> Result<Value, QqlError> {
    if let [vector] = vectors {
        if vector.name.is_none() {
            return Ok(serde_json::json!({
                "size": vector.size,
                "distance": vector.distance,
            }));
        }
    }
    if vectors.iter().any(|vector| vector.name.is_none()) {
        return Err(QqlError::execution(
            "QQL-EDGE-MULTI-VECTOR-NAMES",
            "multiple dense vectors must have explicit names in edge mode",
            None,
        ));
    }
    let entries = vectors
        .iter()
        .map(|vector| {
            let name = vector.name.clone().ok_or_else(|| {
                QqlError::execution(
                    "QQL-EDGE-VECTOR-NAME-MISSING",
                    "dense vector is missing a name in edge mode",
                    None,
                )
            })?;
            Ok((
                name,
                serde_json::json!({
                    "size": vector.size,
                    "distance": vector.distance,
                }),
            ))
        })
        .collect::<Result<serde_json::Map<String, Value>, QqlError>>()?;
    Ok(Value::Object(entries))
}

fn edge_sparse_vectors_json(vectors: &[qql::backend::SparseVectorSpec]) -> Value {
    Value::Object(
        vectors
            .iter()
            .map(|vector| {
                (
                    vector.name.clone(),
                    serde_json::json!({
                        "modifier": vector.modifier,
                    }),
                )
            })
            .collect(),
    )
}

/// Convert a plan-layer filter into a qdrant-edge `Filter`.
///
/// Bare condition objects (`{"key": ...}`) are wrapped as `{"must": [...]}`
/// so they match the Filter schema. Conversion failures return an error —
/// previously COUNT/DELETE silently dropped malformed filters via `.ok()`.
/// This is the single JSON→`qdrant_edge::Filter` converter: query, scroll,
/// count, and mutation paths all go through it.
pub(crate) fn convert_edge_filter(
    filter: Option<&impl serde::Serialize>,
) -> Result<Option<qdrant_edge::Filter>, QqlError> {
    let Some(filter) = filter else {
        return Ok(None);
    };
    let mut filter_val = serde_json::to_value(filter).map_err(|e| {
        QqlError::execution(
            "QQL-EDGE-FILTER-SERIALIZE",
            format!("failed to serialize filter: {e}"),
            None,
        )
    })?;
    if filter_val.get("key").is_some() {
        filter_val = serde_json::json!({ "must": [filter_val] });
    }
    let edge_filter: qdrant_edge::Filter = serde_json::from_value(filter_val).map_err(|e| {
        QqlError::execution(
            "QQL-EDGE-FILTER-DESERIALIZE",
            format!("failed to deserialize filter: {e}"),
            None,
        )
    })?;
    Ok(Some(edge_filter))
}
