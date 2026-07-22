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
    CreateIndex, EdgeConfigBuilder, EdgeShard, FieldIndexOperations, PayloadFieldSchema,
    PayloadSchemaType, PointInsertOperations, PointOperations, UpdateOperation, VectorOperations,
    VectorStructPersisted, WithPayloadInterface, WithVector,
};
use serde_json::Value;
use tokio::sync::RwLock;

use config_builder::build_edge_config;
use conversions::{edge_err, from_edge_id, from_edge_record, to_edge_id};
use query_converter::convert_query_request_with_shard;
use vector_parser::ToEdgeVector;

use qql::backend::{CollectionInfo, CollectionSchema};
use qql::client::{CreateCollectionReq, CreateFieldIndexReq, QdrantOps};
use qql_core::error::QqlError;
use qql_plan::routing::{RequestBody, Route};

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
        {
            let shards = self.shards.read().await;
            if let Some(shard) = shards.get(name) {
                return Ok(Arc::clone(shard));
            }
        }

        let path = self.collection_path(name);
        let on_disk = self.on_disk_payload;
        let config_res = req.map(|r| build_edge_config(r, on_disk));
        let shard = tokio::task::spawn_blocking(move || -> Result<EdgeShard, QqlError> {
            if path.join("segments").exists() {
                EdgeShard::load(&path, None).map_err(edge_err)
            } else {
                std::fs::create_dir_all(&path).map_err(|e| {
                    QqlError::execution("QQL-EDGE", format!("create collection dir: {e}"), None)
                })?;

                let config = match config_res {
                    Some(c) => c?,
                    None => EdgeConfigBuilder::new().on_disk_payload(on_disk).build(),
                };

                EdgeShard::new(&path, config).map_err(edge_err)
            }
        })
        .await
        .map_err(|e| QqlError::execution("QQL-EDGE", format!("spawn_blocking: {e}"), None))??;

        let shard = Arc::new(shard);
        self.shards
            .write()
            .await
            .insert(name.to_string(), Arc::clone(&shard));
        Ok(shard)
    }
}

#[async_trait]
impl QdrantOps for EdgeQdrant {
    async fn list_collections(&self) -> Result<Vec<String>, QqlError> {
        let path = self.base_path.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<String>, QqlError> {
            let mut cols = Vec::new();
            if !path.exists() {
                return Ok(cols);
            }
            let mut dir = std::fs::read_dir(&path)
                .map_err(|e| QqlError::execution("QQL-EDGE", format!("read_dir: {e}"), None))?;
            while let Some(entry) = dir
                .next()
                .transpose()
                .map_err(|e| QqlError::execution("QQL-EDGE", format!("entry: {e}"), None))?
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
        .map_err(|e| QqlError::execution("QQL-EDGE", format!("spawn_blocking: {e}"), None))?
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
        .map_err(|e| QqlError::execution("QQL-EDGE", format!("spawn_blocking: {e}"), None))?;

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

    async fn update_collection(&self, _req: serde_json::Value) -> Result<(), QqlError> {
        Err(QqlError::execution(
            "QQL-EDGE",
            "update_collection not supported in edge mode",
            None,
        ))
    }

    async fn delete_collection(&self, name: &str) -> Result<(), QqlError> {
        let path = self.collection_path(name);
        let _shard = {
            let mut shards = self.shards.write().await;
            shards.remove(name)
        };
        tokio::task::spawn_blocking(move || {
            if path.exists() {
                std::fs::remove_dir_all(&path).map_err(|e| {
                    QqlError::execution("QQL-EDGE", format!("delete collection: {e}"), None)
                })
            } else {
                Ok(())
            }
        })
        .await
        .map_err(|e| QqlError::execution("QQL-EDGE", format!("spawn_blocking: {e}"), None))?
    }

    async fn create_field_index(&self, req: CreateFieldIndexReq) -> Result<(), QqlError> {
        let shard = self.open_shard(&req.collection_name).await?;

        let schema_type = match req.field_type.to_lowercase().as_str() {
            "keyword" => PayloadSchemaType::Keyword,
            "integer" | "int" => PayloadSchemaType::Integer,
            "float" => PayloadSchemaType::Float,
            "bool" | "boolean" => PayloadSchemaType::Bool,
            "geo" => PayloadSchemaType::Geo,
            "text" => PayloadSchemaType::Text,
            other => {
                return Err(QqlError::execution(
                    "QQL-EDGE",
                    format!("unsupported field index type: '{other}'"),
                    None,
                ))
            }
        };

        let field_schema = Some(PayloadFieldSchema::FieldType(schema_type));
        let field_name: qdrant_edge::JsonPath =
            serde_json::from_value(serde_json::Value::String(req.field.clone()))
                .map_err(|e| QqlError::execution("QQL-EDGE", format!("field name: {e}"), None))?;

        let create_index = CreateIndex {
            field_name,
            field_schema,
        };

        let op =
            UpdateOperation::FieldIndexOperation(FieldIndexOperations::CreateIndex(create_index));

        tokio::task::spawn_blocking(move || shard.update(op).map_err(edge_err))
            .await
            .map_err(|e| QqlError::execution("QQL-EDGE", format!("spawn_blocking: {e}"), None))?
    }

    async fn execute_route(&self, route: Route) -> Result<Value, QqlError> {
        match route.body {
            Some(RequestBody::Query(req)) => {
                let collection = extract_collection(&route.path)?;
                let shard = self.open_shard(&collection).await?;
                let results = tokio::task::spawn_blocking(
                    move || -> Result<Vec<qdrant_edge::ScoredPoint>, QqlError> {
                        let edge_req = convert_query_request_with_shard(&req, &shard)?;
                        shard.search(edge_req).map_err(edge_err)
                    },
                )
                .await
                .map_err(|e| {
                    QqlError::execution("QQL-EDGE", format!("spawn_blocking: {e}"), None)
                })??;

                Ok(serde_json::json!({
                    "result": results.into_iter().map(|sp| {
                        let id = from_edge_id(&sp.id);
                        let payload: Value = serde_json::to_value(&sp.payload).unwrap_or_default();
                        serde_json::json!({
                            "id": id,
                            "score": sp.score,
                            "payload": payload,
                            "version": sp.version,
                        })
                    }).collect::<Vec<_>>(),
                    "status": "ok",
                    "time": 0.0,
                }))
            }
            Some(RequestBody::QueryGroups(_)) => Err(QqlError::execution(
                "QQL-EDGE",
                "query_groups not supported in edge mode",
                None,
            )),
            Some(RequestBody::Points(req)) => {
                let collection = extract_collection(&route.path)?;
                let shard = self.open_shard(&collection).await?;
                let ids: Vec<qdrant_edge::PointId> = req
                    .ids
                    .iter()
                    .filter_map(|id| to_edge_id(id.clone()).ok())
                    .collect();

                let records = tokio::task::spawn_blocking(move || {
                    shard
                        .retrieve(
                            &ids,
                            Some(WithPayloadInterface::Bool(true)),
                            Some(WithVector::Bool(false)),
                        )
                        .map_err(edge_err)
                })
                .await
                .map_err(|e| {
                    QqlError::execution("QQL-EDGE", format!("spawn_blocking: {e}"), None)
                })??;

                Ok(serde_json::json!({
                    "result": records.into_iter().map(from_edge_record).collect::<Vec<_>>(),
                    "status": "ok",
                    "time": 0.0,
                }))
            }
            Some(RequestBody::Scroll(req)) => {
                let collection = extract_collection(&route.path)?;
                let shard = self.open_shard(&collection).await?;

                let offset = req.offset.as_ref().and_then(|o| to_edge_id(o.clone()).ok());
                let scroll_req = qdrant_edge::ScrollRequest {
                    offset,
                    limit: Some(req.limit.unwrap_or(10) as usize),
                    filter: None,
                    with_payload: Some(WithPayloadInterface::Bool(true)),
                    with_vector: WithVector::Bool(false),
                    order_by: None,
                };

                let (records, next) =
                    tokio::task::spawn_blocking(move || shard.scroll(scroll_req).map_err(edge_err))
                        .await
                        .map_err(|e| {
                            QqlError::execution("QQL-EDGE", format!("spawn_blocking: {e}"), None)
                        })??;

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
            Some(RequestBody::Upsert(req)) => {
                let collection = extract_collection(&route.path)?;
                let shard = self.open_shard(&collection).await?;

                let mut parsed_points = Vec::with_capacity(req.points.len());
                for p in req.points {
                    let id = to_edge_id(p.id)?;
                    let vector_struct = p.vector.unwrap_or_default().to_edge_vector()?;
                    let payload_val = Value::Object(p.payload.unwrap_or_default());
                    let ps = qdrant_edge::PointStruct::new(id, vector_struct, payload_val);
                    let psp: qdrant_edge::PointStructPersisted = ps.into();
                    parsed_points.push(psp);
                }

                let op = UpdateOperation::PointOperation(PointOperations::UpsertPoints(
                    PointInsertOperations::PointsList(parsed_points),
                ));

                tokio::task::spawn_blocking(move || shard.update(op).map_err(edge_err))
                    .await
                    .map_err(|e| {
                        QqlError::execution("QQL-EDGE", format!("spawn_blocking: {e}"), None)
                    })??;

                Ok(Value::Object(Default::default()))
            }
            Some(RequestBody::Delete(req)) => {
                let collection = extract_collection(&route.path)?;
                let shard = self.open_shard(&collection).await?;

                let operation = if let Some(points) = &req.points {
                    let ids: Vec<qdrant_edge::PointId> = points
                        .iter()
                        .filter_map(|id| to_edge_id(id.clone()).ok())
                        .collect();
                    UpdateOperation::PointOperation(PointOperations::DeletePoints { ids })
                } else if let Some(filter) = &req.filter {
                    let mut filter_val = serde_json::to_value(filter).map_err(|e| {
                        QqlError::execution("QQL-EDGE", format!("invalid filter: {e}"), None)
                    })?;
                    if filter_val.get("key").is_some() {
                        filter_val = serde_json::json!({ "must": [filter_val] });
                    }
                    let edge_filter: qdrant_edge::Filter = serde_json::from_value(filter_val)
                        .map_err(|e| {
                            QqlError::execution(
                                "QQL-EDGE",
                                format!("invalid filter format: {e}"),
                                None,
                            )
                        })?;
                    UpdateOperation::PointOperation(PointOperations::DeletePointsByFilter(
                        edge_filter,
                    ))
                } else {
                    return Err(QqlError::execution(
                        "QQL-EDGE",
                        "delete requires point ids or filter",
                        None,
                    ));
                };

                tokio::task::spawn_blocking(move || shard.update(operation).map_err(edge_err))
                    .await
                    .map_err(|e| {
                        QqlError::execution("QQL-EDGE", format!("spawn_blocking: {e}"), None)
                    })??;

                Ok(Value::Object(Default::default()))
            }
            Some(RequestBody::UpdateVector(req)) => {
                let collection = extract_collection(&route.path)?;
                let shard = self.open_shard(&collection).await?;

                let mut pvps = Vec::with_capacity(req.points.len());
                for pt in req.points {
                    let id = to_edge_id(pt.id)?;
                    let vector_struct = pt.vector.to_edge_vector()?;
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
                    .map_err(|e| {
                        QqlError::execution("QQL-EDGE", format!("spawn_blocking: {e}"), None)
                    })??;

                Ok(Value::Object(Default::default()))
            }
            Some(RequestBody::UpdatePayload(req)) => {
                let collection = extract_collection(&route.path)?;
                let shard = self.open_shard(&collection).await?;
                let payload = qdrant_edge::Payload(req.payload.clone().into_iter().collect());

                let op = if let Some(points) = &req.points {
                    let ids: Vec<qdrant_edge::PointId> = points
                        .iter()
                        .filter_map(|id| to_edge_id(id.clone()).ok())
                        .collect();
                    qdrant_edge::PayloadOps::SetPayload(qdrant_edge::SetPayloadOp {
                        payload,
                        points: Some(ids),
                        filter: None,
                        key: None,
                    })
                } else {
                    return Err(QqlError::execution(
                        "QQL-EDGE",
                        "set_payload by filter not yet supported in edge mode",
                        None,
                    ));
                };

                tokio::task::spawn_blocking(move || {
                    shard
                        .update(UpdateOperation::PayloadOperation(op))
                        .map_err(edge_err)
                })
                .await
                .map_err(|e| {
                    QqlError::execution("QQL-EDGE", format!("spawn_blocking: {e}"), None)
                })??;

                Ok(Value::Object(Default::default()))
            }
            Some(RequestBody::CreateCollection(req)) => {
                let create_req = CreateCollectionReq {
                    collection_name: extract_collection(&route.path)?,
                    vectors_config: Some(serde_json::to_value(&req.vectors).unwrap_or_default()),
                    sparse_vectors_config: req
                        .sparse_vectors
                        .as_ref()
                        .map(|sv| serde_json::to_value(sv).unwrap_or_default()),
                    hnsw_config: None,
                    optimizers_config: None,
                    quantization_config: None,
                    params: None,
                    shard_number: None,
                    sharding_method: None,
                    shard_keys: None,
                };
                self.create_collection(create_req).await?;
                Ok(Value::Object(Default::default()))
            }
            Some(RequestBody::CreateIndex(req)) => {
                let ft = req.field_schema.as_str();
                let create_index = CreateFieldIndexReq {
                    collection_name: extract_collection(&route.path)?,
                    field: req.field_name.clone(),
                    field_type: match ft {
                        "keyword" | "uuid" => "keyword",
                        "integer" | "int" => "integer",
                        "float" => "float",
                        "bool" | "boolean" => "bool",
                        "geo" => "geo",
                        "text" => "text",
                        "datetime" => "integer",
                        _ => "keyword",
                    }
                    .to_string(),
                    options: HashMap::new(),
                };
                self.create_field_index(create_index).await?;
                Ok(Value::Object(Default::default()))
            }
            None => match route.method {
                qql_plan::types::Method::Get if route.path == "/collections" => {
                    let cols = self.list_collections().await?;
                    Ok(serde_json::json!({
                        "result": {
                            "collections": cols.into_iter().map(|c| serde_json::json!({"name": c})).collect::<Vec<_>>(),
                        },
                        "status": "ok",
                        "time": 0.0,
                    }))
                }
                qql_plan::types::Method::Get if route.path.starts_with("/collections/") => {
                    let collection = extract_collection(&route.path)?;
                    let info = self.get_collection_info(&collection).await?;
                    Ok(serde_json::json!({
                        "result": {
                            "status": info.status,
                            "points_count": info.points_count,
                            "segments_count": info.segments_count,
                            "config": {
                                "params": {
                                    "vectors": serde_json::json!({}),
                                }
                            },
                            "payload_schema": {},
                        },
                        "status": "ok",
                        "time": 0.0,
                    }))
                }
                qql_plan::types::Method::Delete if route.path.starts_with("/collections/") => {
                    let collection = extract_collection(&route.path)?;
                    self.delete_collection(&collection).await?;
                    Ok(serde_json::json!({
                        "result": true,
                        "status": "ok",
                        "time": 0.0,
                    }))
                }
                _ => Err(QqlError::execution(
                    "QQL-EDGE",
                    format!("unsupported: {} {}", route.method.as_str(), route.path),
                    None,
                )),
            },
        }
    }
}

fn extract_collection(path: &str) -> Result<String, QqlError> {
    let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    if segments.len() >= 2
        && segments[0] == "collections"
        && segments[1] != "points"
        && !segments[1].is_empty()
    {
        Ok(segments[1].to_string())
    } else {
        Err(QqlError::execution(
            "QQL-EDGE",
            format!("cannot extract collection from path: {path}"),
            None,
        ))
    }
}
