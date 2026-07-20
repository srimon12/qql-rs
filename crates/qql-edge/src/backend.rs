//! Local Qdrant backend via qdrant-edge — in-process HNSW search, zero network.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use qdrant_edge::{
    EdgeConfigBuilder, EdgeShard, Filter as EdgeFilter, NamedQuery, PointId, PointInsertOperations,
    PointOperations, PointStruct, Record, ScoredPoint as EdgeScoredPoint, SearchParams,
    SearchRequest, UpdateOperation, VectorInternal, VectorOperations, VectorStructInternal,
    VectorStructPersisted, WithPayloadInterface, WithVector,
};
use serde_json::Value;
use tokio::sync::RwLock;

use qql::backend::{CollectionInfo, CollectionSchema, PointGroup};
use qql::client::{
    CountPointsReq, CreateCollectionReq, CreateFieldIndexReq, DeletePointsReq, GetPointsReq,
    QdrantOps, RetrievedPoint, ScoredPoint, ScrollPointsReq, SetPayloadReq, UpdateVectorsReq,
    UpsertPointsReq,
};
use qql::pipeline::{PointId as QqlPointId, QueryPointsGroupsRequest, QueryPointsRequest};
use qql_core::error::QqlError;

// ── PointId conversion ──────────────────────────────────────────────

fn to_edge_id(id: QqlPointId) -> PointId {
    match id {
        QqlPointId::Num(n) => PointId::NumId(n),
        QqlPointId::Uuid(s) => {
            let uuid: uuid::Uuid = s.parse().unwrap_or_else(|_| uuid::Uuid::nil());
            PointId::Uuid(uuid)
        }
    }
}

fn from_edge_id(id: PointId) -> QqlPointId {
    match id {
        PointId::NumId(n) => QqlPointId::Num(n),
        PointId::Uuid(u) => QqlPointId::Uuid(u.to_string()),
    }
}

// ── ScoredPoint conversion ─────────────────────────────────────────

fn from_edge_scored(sp: EdgeScoredPoint) -> ScoredPoint {
    let payload = sp.payload.map(|p| p.0.into_iter().collect());
    ScoredPoint {
        id: from_edge_id(sp.id),
        score: sp.score,
        payload,
        vector: None,
    }
}

fn from_edge_record(rec: Record) -> RetrievedPoint {
    let payload = rec.payload.map(|p| p.0.into_iter().collect());
    RetrievedPoint {
        id: from_edge_id(rec.id),
        payload,
        vector: None,
    }
}

// ── Error conversion ───────────────────────────────────────────────

fn edge_err(e: impl std::fmt::Display) -> QqlError {
    QqlError::runtime(format!("qdrant-edge: {e}"))
}

// ── EdgeQdrant ─────────────────────────────────────────────────────

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
        let shard = tokio::task::spawn_blocking(move || -> Result<EdgeShard, QqlError> {
            let config = EdgeConfigBuilder::new().on_disk_payload(on_disk).build();
            if path.join("segments").exists() {
                EdgeShard::load(&path, Some(config)).map_err(edge_err)
            } else {
                std::fs::create_dir_all(&path)
                    .map_err(|e| QqlError::runtime(format!("create collection dir: {e}")))?;
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

    /// Run a blocking operation on a shard by name.
    async fn with_shard<T, F>(&self, name: &str, f: F) -> Result<T, QqlError>
    where
        T: Send + 'static,
        F: FnOnce(Arc<EdgeShard>) -> Result<T, qdrant_edge::OperationError> + Send + 'static,
    {
        let shard = self.open_shard(name).await?;
        tokio::task::spawn_blocking(move || f(shard).map_err(edge_err))
            .await
            .map_err(|e| QqlError::runtime(format!("spawn_blocking: {e}")))?
    }
}

#[async_trait]
impl QdrantOps for EdgeQdrant {
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
        let info = self.with_shard(name, |shard| Ok(shard.info())).await?;
        Ok(CollectionInfo {
            status: "green".to_string(),
            points_count: info.points_count as u64,
            segments_count: info.segments_count as u64,
            schema: CollectionSchema {
                dense_vectors: vec!["".to_string()],
                sparse_vectors: vec![],
            },
            raw_json: None,
        })
    }

    async fn create_collection(&self, req: CreateCollectionReq) -> Result<(), QqlError> {
        self.open_shard(&req.collection_name).await?;
        Ok(())
    }

    async fn update_collection(&self, _req: Value) -> Result<(), QqlError> {
        Err(QqlError::runtime(
            "update_collection not supported in edge mode",
        ))
    }

    async fn delete_collection(&self, name: &str) -> Result<(), QqlError> {
        let path = self.collection_path(name);
        {
            let mut shards = self.shards.write().await;
            shards.remove(name);
        }
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

    // ── data ────────────────────────────────────────────────────────

    async fn upsert(&self, req: UpsertPointsReq) -> Result<(), QqlError> {
        let points: Vec<qdrant_edge::PointStructPersisted> = req
            .points
            .into_iter()
            .map(|p| {
                let id = to_edge_id(p.id);
                // Parse vector JSON — assume dense Vec<f32> for now
                let vector = serde_json::from_value::<Vec<f32>>(p.vector.clone())
                    .map(|v| {
                        let mut map = std::collections::HashMap::new();
                        map.insert(String::new(), VectorInternal::Dense(v));
                        VectorStructInternal::Named(map)
                    })
                    .unwrap_or_else(|_| {
                        // fallback: try to map as a single flat value
                        VectorStructInternal::Single(vec![])
                    });

                let payload = serde_json::to_value(&p.payload)
                    .unwrap_or(serde_json::Value::Object(Default::default()));

                let ps = PointStruct::new(id, vector, payload);
                ps.into() // PointStruct → PointStructPersisted
            })
            .collect();

        let op = UpdateOperation::PointOperation(PointOperations::UpsertPoints(
            PointInsertOperations::PointsList(points),
        ));

        self.with_shard(&req.collection_name, move |shard| shard.update(op))
            .await
    }

    async fn query(&self, req: QueryPointsRequest) -> Result<Vec<ScoredPoint>, QqlError> {
        let qv = req
            .query
            .ok_or_else(|| QqlError::runtime("query variant required"))?;

        let (vector, _using) = match &qv {
            qql::pipeline::QueryVariant::Nearest(v) => (v.clone(), None::<String>),
            _ => {
                return Err(QqlError::runtime(format!(
                    "edge mode does not yet support query variant: {qv:?}"
                )));
            }
        };

        // Build the search request
        let filter: Option<EdgeFilter> = req
            .filter
            .map(|f| {
                serde_json::from_value(f.0)
                    .map_err(|e| QqlError::runtime(format!("invalid filter: {e}")))
            })
            .transpose()?;

        let search_params = req.params.map(|p| SearchParams {
            hnsw_ef: p.hnsw_ef.map(|v| v as usize),
            exact: p.exact.unwrap_or(false),
            indexed_only: p.indexed_only.unwrap_or(false),
            quantization: p
                .quantization
                .map(|q| qdrant_edge::QuantizationSearchParams {
                    ignore: q.ignore.unwrap_or(false),
                    rescore: q.rescore,
                    oversampling: q.oversampling,
                }),
            ..SearchParams::default()
        });

        let search_req = SearchRequest {
            query: qdrant_edge::QueryEnum::Nearest(NamedQuery {
                query: VectorInternal::Dense(vector),
                using: None,
            }),
            filter,
            params: search_params,
            limit: req.limit as usize,
            offset: req.offset as usize,
            with_payload: Some(WithPayloadInterface::Bool(true)),
            with_vector: Some(WithVector::Bool(false)),
            score_threshold: req.score_threshold,
        };

        let collection = req.collection_name.clone();
        let results = self
            .with_shard(&collection, move |shard| shard.search(search_req))
            .await?;

        Ok(results.into_iter().map(from_edge_scored).collect())
    }

    async fn query_groups(
        &self,
        _req: QueryPointsGroupsRequest,
    ) -> Result<Vec<PointGroup>, QqlError> {
        Err(QqlError::runtime("query_groups not supported in edge mode"))
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

    async fn delete(&self, req: DeletePointsReq) -> Result<(), QqlError> {
        let operation = if let Some(id) = req.point_id {
            let edge_id = to_edge_id(id);
            UpdateOperation::PointOperation(PointOperations::DeletePoints { ids: vec![edge_id] })
        } else if let Some(filter) = req.filter {
            let edge_filter: EdgeFilter = serde_json::from_value(filter.0)
                .map_err(|e| QqlError::runtime(format!("filter conversion: {e}")))?;
            UpdateOperation::PointOperation(PointOperations::DeletePointsByFilter(edge_filter))
        } else {
            return Err(QqlError::runtime("delete requires point_id or filter"));
        };

        self.with_shard(&req.collection_name, move |shard| shard.update(operation))
            .await
    }

    async fn update_vectors(&self, req: UpdateVectorsReq) -> Result<(), QqlError> {
        let id = to_edge_id(req.point_id);

        let mut vectors = std::collections::HashMap::new();
        let vec_name = req.vector_name.unwrap_or_default();
        vectors.insert(vec_name.clone(), VectorInternal::Dense(req.vector));

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

        self.with_shard(&req.collection_name, move |shard| shard.update(op))
            .await
    }

    async fn set_payload(&self, req: SetPayloadReq) -> Result<(), QqlError> {
        let payload: qdrant_edge::Payload = serde_json::from_value(
            serde_json::to_value(&req.payload)
                .map_err(|e| QqlError::runtime(format!("payload ser: {e}")))?,
        )
        .map_err(|e| QqlError::runtime(format!("payload conversion: {e}")))?;

        let op = if let Some(id) = req.point_id {
            qdrant_edge::PayloadOps::SetPayload(qdrant_edge::SetPayloadOp {
                payload,
                points: Some(vec![to_edge_id(id)]),
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

        self.with_shard(&req.collection_name, move |shard| {
            shard.update(UpdateOperation::PayloadOperation(op))
        })
        .await
    }

    async fn create_field_index(&self, _req: CreateFieldIndexReq) -> Result<(), QqlError> {
        Err(QqlError::runtime(
            "create_field_index not supported in edge mode",
        ))
    }

    async fn scroll(
        &self,
        req: ScrollPointsReq,
    ) -> Result<(Vec<RetrievedPoint>, Option<QqlPointId>), QqlError> {
        let filter: Option<EdgeFilter> = req
            .filter
            .map(|f| serde_json::from_value(f.0))
            .transpose()
            .map_err(|e| QqlError::runtime(format!("filter: {e}")))?;

        let scroll_req = qdrant_edge::ScrollRequest {
            offset: req.after.map(to_edge_id),
            limit: Some(req.limit as usize),
            filter,
            with_payload: Some(WithPayloadInterface::Bool(true)),
            with_vector: WithVector::Bool(false),
            order_by: None,
        };

        let collection = req.collection_name.clone();
        let (records, next) = self
            .with_shard(&collection, move |shard| shard.scroll(scroll_req))
            .await?;

        let retrieved = records.into_iter().map(from_edge_record).collect();
        let next_offset = next.map(from_edge_id);
        Ok((retrieved, next_offset))
    }

    async fn count(&self, req: CountPointsReq) -> Result<u64, QqlError> {
        let filter: Option<EdgeFilter> = req
            .filter
            .map(|f| serde_json::from_value(f.0))
            .transpose()
            .map_err(|e| QqlError::runtime(format!("filter: {e}")))?;

        let count_req = qdrant_edge::CountRequest {
            filter,
            exact: true,
        };
        let collection = req.collection_name.clone();
        let count = self
            .with_shard(&collection, move |shard| shard.count(count_req))
            .await?;
        Ok(count as u64)
    }

    async fn get(&self, req: GetPointsReq) -> Result<Vec<RetrievedPoint>, QqlError> {
        let id = qql::pipeline::helpers::to_point_id(&req.point_id)
            .map_err(|e| QqlError::runtime(format!("invalid point id: {e}")))?;
        let edge_id = to_edge_id(id);

        let collection = req.collection_name.clone();
        let records = self
            .with_shard(&collection, move |shard| {
                shard.retrieve(
                    &[edge_id],
                    Some(WithPayloadInterface::Bool(true)),
                    Some(WithVector::Bool(false)),
                )
            })
            .await?;

        Ok(records.into_iter().map(from_edge_record).collect())
    }
}
