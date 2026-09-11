//! `QdrantOps` trait implementation for the in-process edge backend.
//!
//! Split out of `backend/mod.rs`; the parent module keeps the `EdgeQdrant`
//! type, shard opening, and inherent query/mutation helpers (all visible to
//! this child module).

use super::*;

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
                hnsw: Some(edge_hnsw_spec(&cfg.hnsw_config())),
                optimizers: Some(edge_optimizers_spec(&cfg.optimizers())),
                quantization: cfg.quantization_config.as_ref().map(edge_quantization_spec),
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
