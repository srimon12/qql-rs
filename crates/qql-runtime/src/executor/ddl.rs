use super::helpers::build_quantization_config;
use crate::executor::{CreateCollectionReq, ExecResponse, Executor};
use qql_core::ast;
use qql_core::error::QqlError;

impl Executor {
    pub(crate) async fn do_show_collections(&self) -> Result<ExecResponse, QqlError> {
        let collections = self.client.list_collections().await?;
        Ok(ExecResponse {
            ok: true,
            operation: "show_collections".to_string(),
            message: format!("Found {} collections", collections.len()),
            data: Some(serde_json::json!({"collections": collections})),
        })
    }

    pub(crate) async fn do_show_collection(
        &self,
        collection: &str,
    ) -> Result<ExecResponse, QqlError> {
        let exists = self.client.collection_exists(collection).await?;
        if !exists {
            return Err(QqlError::execution(
                "QQL-EXECUTION",
                format!("collection '{}' does not exist", collection),
                None,
            ));
        }

        let info = self.client.get_collection_info(collection).await?;
        let data = if let Some(ref raw) = info.raw_json {
            extract_collection_diagnostics(collection, raw)
        } else {
            serde_json::json!({
                "name": collection,
                "status": info.status,
                "points_count": info.points_count,
                "segments_count": info.segments_count,
            })
        };

        Ok(ExecResponse {
            ok: true,
            operation: "show_collection".to_string(),
            message: format!("Collection: {}", collection),
            data: Some(data),
        })
    }

    pub(crate) async fn do_create_collection(
        &self,
        stmt: ast::CreateCollectionStmt,
    ) -> Result<ExecResponse, QqlError> {
        let exists = self.client.collection_exists(&stmt.collection).await?;
        let is_hybrid = matches!(stmt.mode, ast::CollectionMode::Hybrid { .. });
        let is_rerank = matches!(stmt.mode, ast::CollectionMode::Rerank);
        let model = match &stmt.mode {
            ast::CollectionMode::Dense { model } => model.clone(),
            _ => None,
        };
        let dense_vector_name = match &stmt.mode {
            ast::CollectionMode::Hybrid { dense_vector, .. } => dense_vector.clone(),
            _ => None,
        };
        let sparse_vector_name = match &stmt.mode {
            ast::CollectionMode::Hybrid { sparse_vector, .. } => sparse_vector.clone(),
            _ => None,
        };

        if exists {
            return Ok(ExecResponse {
                ok: true,
                operation: "create_collection".to_string(),
                message: format!("Collection '{}' already exists", stmt.collection),
                data: Some(serde_json::json!({
                    "collection": stmt.collection,
                    "exists": true,
                    "hybrid": is_hybrid,
                    "rerank": is_rerank,
                })),
            });
        }

        let mut create_req = CreateCollectionReq::new(stmt.collection.to_string());

        if !stmt.vectors.is_empty() {
            let mut params_map = serde_json::Map::new();
            for v in &stmt.vectors {
                let distance_str = match v.distance {
                    ast::VectorDistance::Cosine => "Cosine",
                    ast::VectorDistance::Dot => "Dot",
                    ast::VectorDistance::Euclid => "Euclid",
                    ast::VectorDistance::Manhattan => "Manhattan",
                };
                let mut vp = serde_json::json!({
                    "size": v.size,
                    "distance": distance_str,
                });
                let vp_obj = vp.as_object_mut().unwrap();
                if v.multivector.is_some() {
                    vp_obj.insert(
                        "multivector_config".to_string(),
                        serde_json::json!({"comparator": "max_sim"}),
                    );
                }
                if let Some(ref hnsw) = v.hnsw {
                    vp_obj.insert("hnsw_config".to_string(), build_hnsw_json(hnsw));
                }
                if let Some(ref quant) = v.quantization {
                    let q_val = build_quantization_config(quant)?;
                    vp_obj.insert("quantization_config".to_string(), q_val);
                }
                params_map.insert(v.name.to_string(), vp);
            }
            create_req.vectors_config = Some(serde_json::Value::Object(params_map));
        } else {
            let dense_size = self.resolve_dense_vector_size(model.as_deref()).await? as u64;
            let dense_name =
                dense_vector_name.unwrap_or_else(|| super::DENSE_VECTOR_NAME.to_string());
            create_req.vectors_config = Some(serde_json::json!({
                dense_name: {
                    "size": dense_size,
                    "distance": "Cosine"
                }
            }));
        }

        if !stmt.sparse_vectors.is_empty() {
            let mut sparse_map = serde_json::Map::new();
            for sv in &stmt.sparse_vectors {
                sparse_map.insert(sv.name.to_string(), serde_json::json!({"modifier": "idf"}));
            }
            create_req.sparse_vectors_config = Some(serde_json::Value::Object(sparse_map));
        } else if is_hybrid || is_rerank {
            let sparse_name =
                sparse_vector_name.unwrap_or_else(|| super::SPARSE_VECTOR_NAME.to_string());
            create_req.sparse_vectors_config = Some(serde_json::json!({
                sparse_name: {"modifier": "idf"}
            }));
        }

        if let Some(ref config) = stmt.config {
            if let Some(ref hnsw) = config.hnsw {
                create_req.hnsw_config = Some(build_hnsw_json(hnsw));
            }
            if let Some(ref opt) = config.optimizers {
                create_req.optimizers_config = Some(build_optimizers_json(opt));
            }
            if let Some(ref params) = config.params {
                let mut params_map = serde_json::Map::new();
                if let Some(rf) = params.replication_factor {
                    params_map.insert("replication_factor".to_string(), serde_json::json!(rf));
                }
                if let Some(wc) = params.write_consistency_factor {
                    params_map.insert(
                        "write_consistency_factor".to_string(),
                        serde_json::json!(wc),
                    );
                }
                if let Some(od) = params.on_disk_payload {
                    params_map.insert("on_disk_payload".to_string(), serde_json::json!(od));
                }
                if let Some(rf_out) = params.read_fan_out_factor {
                    params_map.insert("read_fan_out_factor".to_string(), serde_json::json!(rf_out));
                }
                if let Some(rf_delay) = params.read_fan_out_delay_ms {
                    params_map.insert(
                        "read_fan_out_delay_ms".to_string(),
                        serde_json::json!(rf_delay),
                    );
                }
                if !params_map.is_empty() {
                    create_req.params = Some(serde_json::Value::Object(params_map));
                }
                // Shard configuration — passed directly as top-level fields
                create_req.shard_number = params.shard_number;
                create_req.sharding_method = params.sharding_method.clone();
                create_req.shard_keys = params.shard_keys.clone();
            }
            if let Some(ref quant) = config.quantization {
                let q_val = build_quantization_config(quant)?;
                create_req.quantization_config = Some(q_val);
            }
            if let Some(ref vectors) = config.vectors {
                if let Some(on_disk) = vectors.on_disk {
                    if let Some(ref mut vec_val) = create_req.vectors_config {
                        if let Some(obj) = vec_val.as_object_mut() {
                            for (_, val) in obj.iter_mut() {
                                if let Some(param) = val.as_object_mut() {
                                    param.insert("on_disk".to_string(), serde_json::json!(on_disk));
                                }
                            }
                        }
                    }
                }
            }
        }

        self.client.create_collection(create_req).await?;

        let mut message = format!("Collection '{}' created", stmt.collection);
        if stmt.vectors.is_empty() {
            if is_rerank {
                message = format!(
                    "Collection '{}' created (hybrid: dense + sparse + ColBERT)",
                    stmt.collection
                );
            } else if is_hybrid {
                message = format!(
                    "Collection '{}' created (hybrid: dense + sparse)",
                    stmt.collection
                );
            } else {
                message.push_str(" (dense)");
            }
        } else {
            message.push_str(" (multi-vector schema)");
        }

        Ok(ExecResponse {
            ok: true,
            operation: "create_collection".to_string(),
            message,
            data: Some(serde_json::json!({
                "collection": stmt.collection,
                "exists": false,
                "hybrid": is_hybrid,
                "rerank": is_rerank,
            })),
        })
    }

    pub(crate) async fn do_alter_collection(
        &self,
        stmt: ast::AlterCollectionStmt,
    ) -> Result<ExecResponse, QqlError> {
        let exists = self.client.collection_exists(&stmt.collection).await?;
        if !exists {
            return Err(QqlError::execution(
                "QQL-EXECUTION",
                format!("collection '{}' does not exist", stmt.collection),
                None,
            ));
        }

        let mut req_map = serde_json::Map::new();
        req_map.insert(
            "collection_name".to_string(),
            serde_json::json!(stmt.collection),
        );

        if let Some(ref config) = stmt.config {
            if let Some(ref hnsw) = config.hnsw {
                req_map.insert("hnsw_config".to_string(), build_hnsw_json(hnsw));
            }
            if let Some(ref opt) = config.optimizers {
                req_map.insert("optimizers_config".to_string(), build_optimizers_json(opt));
            }
            if let Some(ref params) = config.params {
                let mut params_map = serde_json::Map::new();
                if let Some(rf) = params.replication_factor {
                    params_map.insert("replication_factor".to_string(), serde_json::json!(rf));
                }
                if let Some(wc) = params.write_consistency_factor {
                    params_map.insert(
                        "write_consistency_factor".to_string(),
                        serde_json::json!(wc),
                    );
                }
                if let Some(od) = params.on_disk_payload {
                    params_map.insert("on_disk_payload".to_string(), serde_json::json!(od));
                }
                if let Some(rf_out) = params.read_fan_out_factor {
                    params_map.insert("read_fan_out_factor".to_string(), serde_json::json!(rf_out));
                }
                if let Some(rf_delay) = params.read_fan_out_delay_ms {
                    params_map.insert(
                        "read_fan_out_delay_ms".to_string(),
                        serde_json::json!(rf_delay),
                    );
                }
                req_map.insert("params".to_string(), serde_json::Value::Object(params_map));
            }
            if let Some(ref quant_update) = config.quantization_update {
                if quant_update.disabled {
                    req_map.insert(
                        "quantization_config".to_string(),
                        serde_json::json!({ "disabled": true }),
                    );
                } else if let Some(ref quant) = quant_update.config {
                    let q_val = build_quantization_config(quant)?;
                    req_map.insert("quantization_config".to_string(), q_val);
                }
            }
            if let Some(ref vectors) = config.vectors {
                if let Some(on_disk) = vectors.on_disk {
                    req_map.insert(
                        "vectors_config".to_string(),
                        serde_json::json!({ "on_disk": on_disk }),
                    );
                }
            }
        }

        self.client
            .update_collection(serde_json::Value::Object(req_map))
            .await?;

        Ok(ExecResponse {
            ok: true,
            operation: "alter_collection".to_string(),
            message: format!("Collection '{}' altered", stmt.collection),
            data: Some(serde_json::json!({"collection": stmt.collection})),
        })
    }

    pub(crate) async fn do_drop_collection(
        &self,
        collection: &str,
    ) -> Result<ExecResponse, QqlError> {
        let exists = self.client.collection_exists(collection).await?;
        if !exists {
            return Err(QqlError::execution(
                "QQL-EXECUTION",
                format!("collection '{}' does not exist", collection),
                None,
            ));
        }

        self.client.delete_collection(collection).await?;
        Ok(ExecResponse {
            ok: true,
            operation: "drop_collection".to_string(),
            message: format!("Collection '{}' dropped", collection),
            data: Some(serde_json::json!({"collection": collection})),
        })
    }

    pub(crate) async fn do_create_shard_key(
        &self,
        stmt: ast::CreateShardKeyStmt,
    ) -> Result<ExecResponse, QqlError> {
        let r = qql_plan::routing::route(&ast::Stmt::CreateShardKey(Box::new(stmt)));
        self.client.execute_route(r).await?;
        Ok(ExecResponse {
            ok: true,
            operation: "create_shard_key".to_string(),
            message: format!("Shard key created"),
            data: None,
        })
    }

    pub(crate) async fn do_create_index(
        &self,
        stmt: ast::CreateIndexStmt,
    ) -> Result<ExecResponse, QqlError> {
        let req = crate::executor::CreateFieldIndexReq {
            collection_name: stmt.collection.to_string(),
            field: stmt.field.to_string(),
            field_type: stmt.field_type.to_string(),
            options: stmt
                .options
                .iter()
                .map(|(k, v)| (k.to_string(), super::helpers::clone_value(v)))
                .collect(),
        };

        self.client.create_field_index(req).await?;

        Ok(ExecResponse {
            ok: true,
            operation: "create_index".to_string(),
            message: format!("Index created on field '{}'", stmt.field),
            data: None,
        })
    }

    pub(crate) async fn do_drop_index(
        &self,
        stmt: ast::DropIndexStmt,
    ) -> Result<ExecResponse, QqlError> {
        self.client
            .delete_field_index(&stmt.collection, &stmt.field)
            .await?;

        Ok(ExecResponse {
            ok: true,
            operation: "drop_index".to_string(),
            message: format!(
                "Index dropped on field '{}' from collection '{}'",
                stmt.field, stmt.collection
            ),
            data: None,
        })
    }
}

fn extract_collection_diagnostics(name: &str, raw: &serde_json::Value) -> serde_json::Value {
    let status = raw
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("green")
        .to_string();
    let points_count = raw
        .get("points_count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let segments_count = raw
        .get("segments_count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let indexed_vectors_count = raw
        .get("indexed_vectors_count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);

    let has_sparse = raw
        .pointer("/config/params/sparse_vectors")
        .and_then(serde_json::Value::as_object)
        .map(|m| !m.is_empty())
        .unwrap_or(false);
    let topology = if has_sparse {
        "hybrid".to_string()
    } else {
        "dense".to_string()
    };

    let binding = raw
        .pointer("/config/quantization_config")
        .or_else(|| raw.pointer("/config/params/quantization_config"))
        .and_then(|qc| qc.as_object())
        .cloned();

    let quantization = if let Some(qc) = binding {
        if qc.contains_key("scalar") {
            serde_json::Value::String("scalar".to_string())
        } else if qc.contains_key("product") {
            serde_json::Value::String("product".to_string())
        } else if qc.contains_key("binary") {
            serde_json::Value::String("binary".to_string())
        } else {
            serde_json::Value::Null
        }
    } else {
        serde_json::Value::Null
    };

    let mut payload_schema = serde_json::Map::new();
    if let Some(ps) = raw
        .get("payload_schema")
        .and_then(serde_json::Value::as_object)
    {
        for (field, info) in ps {
            let data_type = info
                .get("data_type")
                .or_else(|| info.get("type"))
                .and_then(|v| v.as_str())
                .unwrap_or("keyword")
                .to_string();
            let mut entry = serde_json::Map::new();
            entry.insert("type".to_string(), serde_json::Value::String(data_type));
            if let Some(params) = info.get("params") {
                entry.insert("params".to_string(), params.clone());
            }
            payload_schema.insert(field.clone(), serde_json::Value::Object(entry));
        }
    }

    serde_json::json!({
        "name": name,
        "status": status,
        "points_count": points_count,
        "segments_count": segments_count,
        "indexed_vectors_count": indexed_vectors_count,
        "topology": topology,
        "quantization": quantization,
        "payload_schema": payload_schema,
    })
}

fn build_hnsw_json(hnsw: &ast::HnswRuntimeConfig) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    if let Some(m) = hnsw.m {
        map.insert("m".to_string(), serde_json::json!(m));
    }
    if let Some(ef) = hnsw.ef_construct {
        map.insert("ef_construct".to_string(), serde_json::json!(ef));
    }
    if let Some(fs) = hnsw.full_scan_threshold {
        map.insert("full_scan_threshold".to_string(), serde_json::json!(fs));
    }
    if let Some(mi) = hnsw.max_indexing_threads {
        map.insert("max_indexing_threads".to_string(), serde_json::json!(mi));
    }
    if let Some(od) = hnsw.on_disk {
        map.insert("on_disk".to_string(), serde_json::json!(od));
    }
    if let Some(pm) = hnsw.payload_m {
        map.insert("payload_m".to_string(), serde_json::json!(pm));
    }
    serde_json::Value::Object(map)
}

fn build_optimizers_json(opt: &ast::OptimizersRuntimeConfig) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    if let Some(dt) = opt.deleted_threshold {
        map.insert("deleted_threshold".to_string(), serde_json::json!(dt));
    }
    if let Some(vm) = opt.vacuum_min_vector_number {
        map.insert(
            "vacuum_min_vector_number".to_string(),
            serde_json::json!(vm),
        );
    }
    if let Some(ds) = opt.default_segment_number {
        map.insert("default_segment_number".to_string(), serde_json::json!(ds));
    }
    if let Some(ms) = opt.max_segment_size {
        map.insert("max_segment_size".to_string(), serde_json::json!(ms));
    }
    if let Some(mt) = opt.memmap_threshold {
        map.insert("memmap_threshold".to_string(), serde_json::json!(mt));
    }
    if let Some(it) = opt.indexing_threshold {
        map.insert("indexing_threshold".to_string(), serde_json::json!(it));
    }
    if let Some(fi) = opt.flush_interval_sec {
        map.insert("flush_interval_sec".to_string(), serde_json::json!(fi));
    }
    if let Some(pu) = opt.prevent_unoptimized {
        map.insert("prevent_unoptimized".to_string(), serde_json::json!(pu));
    }
    if let Some(ref t) = opt.max_optimization_threads {
        if t.auto_ {
            map.insert(
                "max_optimization_threads".to_string(),
                serde_json::json!("auto"),
            );
        } else {
            map.insert(
                "max_optimization_threads".to_string(),
                serde_json::json!(t.value),
            );
        }
    }
    serde_json::Value::Object(map)
}
