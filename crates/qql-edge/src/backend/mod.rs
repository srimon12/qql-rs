//! Local Qdrant backend via qdrant-edge — in-process HNSW search, zero network.

pub mod config_builder;
pub mod conversions;
pub mod query_converter;
pub mod vector_parser;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use qdrant_edge::{
    EdgeConfigBuilder, EdgeShard, Filter as EdgeFilter, PointInsertOperations, PointOperations,
    UpdateOperation, VectorInternal, VectorOperations, VectorStructInternal, VectorStructPersisted,
    WithPayloadInterface, WithVector,
};
use serde_json::Value;
use tokio::sync::RwLock;

use config_builder::build_edge_config;
use conversions::{edge_err, from_edge_id, from_edge_record, from_edge_scored, to_edge_id};
use query_converter::convert_query_request;
use vector_parser::ToEdgeVector;

use qql::backend::{CollectionInfo, CollectionSchema, PointGroup};
use qql::client::{
    CountPointsReq, CreateCollectionReq, CreateFieldIndexReq, DeletePointsReq, GetPointsReq,
    QdrantAdminOps, QdrantCoreOps, RetrievedPoint, ScoredPoint, ScrollPointsReq, SetPayloadReq,
    UpdateVectorsReq, UpsertPointsReq,
};
use qql::pipeline::{PointId as QqlPointId, QueryPointsGroupsRequest, QueryPointsRequest};
use qql_core::error::QqlError;

pub struct EdgeQdrant {
    base_path: PathBuf,
    on_disk_payload: bool,
    shards: RwLock<HashMap<String, Arc<EdgeShard>>>,
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
        }
    }

    fn collection_path(&self, name: &str) -> PathBuf {
        self.base_path.join(name)
    }

    /// Open (or create) the shard for `collection`.
    async fn open_shard(&self, name: &str) -> Result<Arc<EdgeShard>, QqlError> {
        self.open_shard_with_req(name, None).await
    }

    async fn open_shard_with_req(
        &self,
        name: &str,
        req: Option<&CreateCollectionReq>,
    ) -> Result<Arc<EdgeShard>, QqlError> {
        // Fast path: already cached
        {
            let shards = self.shards.read().await;
            if let Some(shard) = shards.get(name) {
                return Ok(Arc::clone(shard));
            }
        }

        // Slow path: load or create on blocking thread
        let path = self.collection_path(name);
        let on_disk = self.on_disk_payload;
        let config_res = req.map(|r| build_edge_config(r, on_disk));
        let shard = tokio::task::spawn_blocking(move || -> Result<EdgeShard, QqlError> {
            if path.join("segments").exists() {
                EdgeShard::load(&path, None).map_err(edge_err)
            } else {
                std::fs::create_dir_all(&path)
                    .map_err(|e| QqlError::runtime(format!("create collection dir: {e}")))?;

                let config = match config_res {
                    Some(c) => c?,
                    None => EdgeConfigBuilder::new().on_disk_payload(on_disk).build(),
                };

                EdgeShard::new(&path, config).map_err(edge_err)
            }
        })
        .await
        .map_err(|e| QqlError::runtime(format!("spawn_blocking: {e}")))??;

        let shard = Arc::new(shard);
        self.shards
            .write()
            .await
            .insert(name.to_string(), Arc::clone(&shard));
        Ok(shard)
    }
}

#[async_trait]
impl QdrantCoreOps for EdgeQdrant {
    // ── collections ─────────────────────────────────────────────────

    async fn list_collections(&self) -> Result<Vec<String>, QqlError> {
        let path = self.base_path.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<String>, QqlError> {
            let mut cols = Vec::new();
            if !path.exists() {
                return Ok(cols);
            }
            let mut dir = std::fs::read_dir(&path)
                .map_err(|e| QqlError::runtime(format!("read_dir: {e}")))?;
            while let Some(entry) = dir
                .next()
                .transpose()
                .map_err(|e| QqlError::runtime(format!("entry: {e}")))?
            {
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
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
        .map_err(|e| QqlError::runtime(format!("spawn_blocking: {e}")))?
    }

    async fn collection_exists(&self, name: &str) -> Result<bool, QqlError> {
        Ok(self.collection_path(name).join("segments").exists())
    }

    async fn get_collection_info(&self, name: &str) -> Result<CollectionInfo, QqlError> {
        let shard = self.open_shard(name).await?;
        let (info, dense_vectors, sparse_vectors) = tokio::task::spawn_blocking(move || {
            let info = shard.info();
            let cfg = shard.config();
            let dense = cfg.vectors.keys().cloned().collect();
            let sparse = cfg.sparse_vectors.keys().cloned().collect();
            (info, dense, sparse)
        })
        .await
        .map_err(|e| QqlError::runtime(format!("spawn_blocking: {e}")))?;

        Ok(CollectionInfo {
            status: "green".to_string(),
            points_count: info.points_count as u64,
            segments_count: info.segments_count as u64,
            schema: CollectionSchema {
                dense_vectors,
                sparse_vectors,
            },
            raw_json: None,
        })
    }

    async fn create_collection(&self, req: CreateCollectionReq) -> Result<(), QqlError> {
        self.open_shard_with_req(&req.collection_name, Some(&req))
            .await?;
        Ok(())
    }

    async fn upsert(&self, req: UpsertPointsReq) -> Result<(), QqlError> {
        let shard = self.open_shard(&req.collection_name).await?;
        let mut parsed_points = Vec::with_capacity(req.points.len());
        for p in req.points {
            let id = to_edge_id(p.id)?;
            let vector_struct = p.vector.to_edge_vector()?;
            let payload_val = Value::Object(p.payload.into_iter().collect());
            let ps = qdrant_edge::PointStruct::new(id, vector_struct, payload_val);
            let psp: qdrant_edge::PointStructPersisted = ps.into();
            parsed_points.push(psp);
        }

        let op = UpdateOperation::PointOperation(PointOperations::UpsertPoints(
            PointInsertOperations::PointsList(parsed_points),
        ));

        tokio::task::spawn_blocking(move || shard.update(op).map_err(edge_err))
            .await
            .map_err(|e| QqlError::runtime(format!("spawn_blocking: {e}")))?
    }

    async fn query(&self, req: QueryPointsRequest) -> Result<Vec<ScoredPoint>, QqlError> {
        let shard = self.open_shard(&req.collection_name).await?;
        let results = tokio::task::spawn_blocking(
            move || -> Result<Vec<qdrant_edge::ScoredPoint>, QqlError> {
                let edge_req = convert_query_request(req, &shard)?;
                shard.query(edge_req).map_err(edge_err)
            },
        )
        .await
        .map_err(|e| QqlError::runtime(format!("spawn_blocking: {e}")))??;

        Ok(results.into_iter().map(from_edge_scored).collect())
    }

    async fn query_groups(
        &self,
        _req: QueryPointsGroupsRequest,
    ) -> Result<Vec<PointGroup>, QqlError> {
        Err(QqlError::runtime("query_groups not supported in edge mode"))
    }

    async fn delete(&self, req: DeletePointsReq) -> Result<(), QqlError> {
        let shard = self.open_shard(&req.collection_name).await?;
        let operation = if let Some(id) = req.point_id {
            let edge_id = to_edge_id(id)?;
            UpdateOperation::PointOperation(PointOperations::DeletePoints { ids: vec![edge_id] })
        } else if let Some(filter) = req.filter {
            let edge_filter: EdgeFilter = serde_json::from_value(filter.0)
                .map_err(|e| QqlError::runtime(format!("filter conversion: {e}")))?;
            UpdateOperation::PointOperation(PointOperations::DeletePointsByFilter(edge_filter))
        } else {
            return Err(QqlError::runtime("delete requires point_id or filter"));
        };

        tokio::task::spawn_blocking(move || shard.update(operation).map_err(edge_err))
            .await
            .map_err(|e| QqlError::runtime(format!("spawn_blocking: {e}")))?
    }

    async fn update_vectors(&self, req: UpdateVectorsReq) -> Result<(), QqlError> {
        let shard = self.open_shard(&req.collection_name).await?;
        let id = to_edge_id(req.point_id)?;

        let mut vectors = HashMap::with_capacity(1);
        let vec_name = req.vector_name.unwrap_or_default();
        vectors.insert(vec_name, VectorInternal::Dense(req.vector));

        let pvp = qdrant_edge::PointVectorsPersisted {
            id,
            vector: VectorStructPersisted::from(VectorStructInternal::Named(vectors)),
        };

        let op = UpdateOperation::VectorOperation(VectorOperations::UpdateVectors(
            qdrant_edge::UpdateVectorsOp {
                points: vec![pvp],
                update_filter: None,
            },
        ));

        tokio::task::spawn_blocking(move || shard.update(op).map_err(edge_err))
            .await
            .map_err(|e| QqlError::runtime(format!("spawn_blocking: {e}")))?
    }

    async fn set_payload(&self, req: SetPayloadReq) -> Result<(), QqlError> {
        let shard = self.open_shard(&req.collection_name).await?;
        let payload = qdrant_edge::Payload(req.payload.into_iter().collect());

        let op = if let Some(id) = req.point_id {
            let edge_id = to_edge_id(id)?;
            qdrant_edge::PayloadOps::SetPayload(qdrant_edge::SetPayloadOp {
                payload,
                points: Some(vec![edge_id]),
                filter: None,
                key: None,
            })
        } else if let Some(filter) = req.filter {
            let edge_filter: EdgeFilter = serde_json::from_value(filter.0)
                .map_err(|e| QqlError::runtime(format!("filter conversion: {e}")))?;
            qdrant_edge::PayloadOps::SetPayload(qdrant_edge::SetPayloadOp {
                payload,
                points: None,
                filter: Some(edge_filter),
                key: None,
            })
        } else {
            return Err(QqlError::runtime("set_payload requires point_id or filter"));
        };

        tokio::task::spawn_blocking(move || {
            shard
                .update(UpdateOperation::PayloadOperation(op))
                .map_err(edge_err)
        })
        .await
        .map_err(|e| QqlError::runtime(format!("spawn_blocking: {e}")))?
    }

    async fn scroll(
        &self,
        req: ScrollPointsReq,
    ) -> Result<(Vec<RetrievedPoint>, Option<QqlPointId>), QqlError> {
        let shard = self.open_shard(&req.collection_name).await?;
        let filter: Option<EdgeFilter> = req
            .filter
            .map(|f| serde_json::from_value(f.0))
            .transpose()
            .map_err(|e| QqlError::runtime(format!("filter: {e}")))?;

        let offset = req.after.map(to_edge_id).transpose()?;

        let scroll_req = qdrant_edge::ScrollRequest {
            offset,
            limit: Some(req.limit as usize),
            filter,
            with_payload: Some(WithPayloadInterface::Bool(true)),
            with_vector: WithVector::Bool(false),
            order_by: None,
        };

        let (records, next) =
            tokio::task::spawn_blocking(move || shard.scroll(scroll_req).map_err(edge_err))
                .await
                .map_err(|e| QqlError::runtime(format!("spawn_blocking: {e}")))??;

        let retrieved = records.into_iter().map(from_edge_record).collect();
        let next_offset = next.map(from_edge_id);
        Ok((retrieved, next_offset))
    }

    async fn get(&self, req: GetPointsReq) -> Result<Vec<RetrievedPoint>, QqlError> {
        let id = qql::pipeline::helpers::to_point_id(&req.point_id)
            .map_err(|e| QqlError::runtime(format!("invalid point id: {e}")))?;
        let edge_id = to_edge_id(id)?;

        let shard = self.open_shard(&req.collection_name).await?;
        let records = tokio::task::spawn_blocking(move || {
            shard
                .retrieve(
                    &[edge_id],
                    Some(WithPayloadInterface::Bool(true)),
                    Some(WithVector::Bool(false)),
                )
                .map_err(edge_err)
        })
        .await
        .map_err(|e| QqlError::runtime(format!("spawn_blocking: {e}")))??;

        Ok(records.into_iter().map(from_edge_record).collect())
    }
}

#[async_trait]
impl QdrantAdminOps for EdgeQdrant {
    async fn update_collection(&self, _req: Value) -> Result<(), QqlError> {
        Err(QqlError::runtime(
            "update_collection not supported in edge mode",
        ))
    }

    async fn delete_collection(&self, name: &str) -> Result<(), QqlError> {
        let path = self.collection_path(name);
        // Hold the cached Arc<EdgeShard> in scope through blocking removal so concurrent queries complete safely.
        let _shard = {
            let mut shards = self.shards.write().await;
            shards.remove(name)
        };
        tokio::task::spawn_blocking(move || {
            if path.exists() {
                std::fs::remove_dir_all(&path)
                    .map_err(|e| QqlError::runtime(format!("delete collection: {e}")))
            } else {
                Ok(())
            }
        })
        .await
        .map_err(|e| QqlError::runtime(format!("spawn_blocking: {e}")))?
    }

    async fn query_batch(
        &self,
        req: Vec<QueryPointsRequest>,
    ) -> Result<Vec<Vec<ScoredPoint>>, QqlError> {
        let mut results = Vec::with_capacity(req.len());
        for query in req {
            results.push(self.query(query).await?);
        }
        Ok(results)
    }

    async fn create_field_index(&self, _req: CreateFieldIndexReq) -> Result<(), QqlError> {
        Err(QqlError::runtime(
            "create_field_index not supported in edge mode",
        ))
    }

    async fn count(&self, req: CountPointsReq) -> Result<u64, QqlError> {
        let shard = self.open_shard(&req.collection_name).await?;
        let filter: Option<EdgeFilter> = req
            .filter
            .map(|f| serde_json::from_value(f.0))
            .transpose()
            .map_err(|e| QqlError::runtime(format!("filter: {e}")))?;

        let count_req = qdrant_edge::CountRequest {
            filter,
            exact: true,
        };
        let count = tokio::task::spawn_blocking(move || shard.count(count_req).map_err(edge_err))
            .await
            .map_err(|e| QqlError::runtime(format!("spawn_blocking: {e}")))??;
        Ok(count as u64)
    }
}
