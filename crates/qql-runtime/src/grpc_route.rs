use crate::qdrant_grpc::qdrant;
use qql_core::error::QqlError;
use qql_plan::routing::{RequestBody, Route};
use qql_plan::types::{
    FilterClause, FilterCompound, FilterExpression, MatchValue, PayloadSelectorReq,
    VectorSelectorReq, WithLookupValue,
};
use qql_plan::{PlanPointId, PlanPointVectors, PlanQueryInput, PlanVectorValue};

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
            "QQL-GRPC",
            format!("cannot extract collection from path: {path}"),
            None,
        ))
    }
}

fn shard_key_selector(key: &Option<String>) -> Option<qdrant::ShardKeySelector> {
    key.as_ref().map(|k| qdrant::ShardKeySelector {
        shard_keys: vec![qdrant::ShardKey {
            key: Some(qdrant::shard_key::Key::Keyword(k.clone())),
        }],
        ..Default::default()
    })
}

fn shard_key_from_route(route: &Route) -> Option<String> {
    route
        .query
        .iter()
        .find(|(k, _)| k == "shard_key")
        .map(|(_, v)| v.clone())
}

pub async fn execute_grpc_route(
    client: &crate::grpc::GrpcQdrant,
    route: Route,
) -> Result<serde_json::Value, QqlError> {
    let route_shard = shard_key_from_route(&route);
    match route.body {
        Some(RequestBody::Query(req)) => {
            let collection = extract_collection(&route.path)?;
            let grpc_req = to_query_points(&req, &collection)?;
            let resp = client
                .query(grpc_req)
                .await
                .map_err(|e| QqlError::backend("QQL-GRPC", format!("query: {e}"), None))?;
            Ok(serde_json::json!({
                "result": resp.result.into_iter().map(scored_point_to_json).collect::<Vec<_>>(),
                "status": "ok",
                "time": resp.time,
            }))
        }
        Some(RequestBody::QueryGroups(req)) => {
            let collection = extract_collection(&route.path)?;
            let grpc_req = to_query_groups(&req, &collection)?;
            let resp = client
                .query_groups(grpc_req)
                .await
                .map_err(|e| QqlError::backend("QQL-GRPC", format!("query_groups: {e}"), None))?;
            Ok(serde_json::json!({
                "result": groups_result_to_json(resp.result.ok_or_else(|| QqlError::backend(
                    "QQL-GRPC", "missing groups result", None,
                ))?),
                "status": "ok",
                "time": resp.time,
            }))
        }
        Some(RequestBody::Points(req)) => {
            let collection = extract_collection(&route.path)?;
            let grpc_req = qdrant::GetPoints {
                collection_name: collection,
                ids: req.ids.iter().map(to_point_id).collect(),
                with_payload: req.with_payload.as_ref().map(to_payload_selector),
                with_vectors: req.with_vector.as_ref().map(to_vectors_selector),
                ..Default::default()
            };
            let resp = client
                .get_points(grpc_req)
                .await
                .map_err(|e| QqlError::backend("QQL-GRPC", format!("get_points: {e}"), None))?;
            Ok(serde_json::json!({
                "result": resp.result.into_iter().map(retrieved_point_to_json).collect::<Vec<_>>(),
                "status": "ok",
                "time": resp.time,
            }))
        }
        Some(RequestBody::Scroll(req)) => {
            let collection = extract_collection(&route.path)?;
            let grpc_req = qdrant::ScrollPoints {
                collection_name: collection,
                filter: req.filter.as_ref().map(to_filter),
                offset: req.offset.as_ref().map(to_point_id),
                limit: req.limit.map(|l| l as u32),
                with_payload: req.with_payload.as_ref().map(to_payload_selector),
                with_vectors: req.with_vector.as_ref().map(to_vectors_selector),
                shard_key_selector: shard_key_selector(&req.shard_key)
                    .or_else(|| shard_key_selector(&route_shard)),
                ..Default::default()
            };
            let resp = client
                .scroll(grpc_req)
                .await
                .map_err(|e| QqlError::backend("QQL-GRPC", format!("scroll: {e}"), None))?;
            let mut obj = serde_json::Map::new();
            obj.insert("status".into(), serde_json::json!("ok"));
            obj.insert("time".into(), serde_json::json!(resp.time));
            obj.insert("result".into(), serde_json::json!({
                "points": resp.result.into_iter().map(retrieved_point_to_json).collect::<Vec<_>>()
            }));
            if let Some(offset) = resp.next_page_offset {
                obj.insert("next_page_offset".into(), point_id_to_json(&offset));
            }
            Ok(serde_json::Value::Object(obj))
        }
        Some(RequestBody::Upsert(req)) => {
            let collection = extract_collection(&route.path)?;
            let points: Vec<qdrant::PointStruct> = req
                .points
                .iter()
                .map(|p| {
                    let id = to_point_id(&p.id);
                    let vectors = p.vector.as_ref().and_then(|v| to_vectors(v));
                    let payload = p
                        .payload
                        .as_ref()
                        .map(|pl| {
                            pl.iter()
                                .map(|(k, v)| (k.clone(), to_qdrant_value(v.clone())))
                                .collect()
                        })
                        .unwrap_or_default();
                    qdrant::PointStruct {
                        id: Some(id),
                        vectors,
                        payload,
                    }
                })
                .collect();
            let grpc_req = qdrant::UpsertPoints {
                collection_name: collection,
                wait: Some(true),
                points,
                shard_key_selector: shard_key_selector(&req.shard_key)
                    .or_else(|| shard_key_selector(&route_shard)),
                ..Default::default()
            };
            client
                .upsert_points(grpc_req)
                .await
                .map_err(|e| QqlError::backend("QQL-GRPC", format!("upsert: {e}"), None))?;
            Ok(serde_json::Value::Object(Default::default()))
        }
        Some(RequestBody::Delete(req)) => {
            let collection = extract_collection(&route.path)?;
            let selector = if let Some(points) = &req.points {
                Some(qdrant::PointsSelector {
                    points_selector_one_of: Some(
                        qdrant::points_selector::PointsSelectorOneOf::Points(
                            qdrant::PointsIdsList {
                                ids: points.iter().map(to_point_id).collect(),
                            },
                        ),
                    ),
                })
            } else {
                req.filter.as_ref().map(|f| qdrant::PointsSelector {
                    points_selector_one_of: Some(
                        qdrant::points_selector::PointsSelectorOneOf::Filter(to_filter(f)),
                    ),
                })
            };
            let grpc_req = qdrant::DeletePoints {
                collection_name: collection,
                wait: Some(true),
                points: selector,
                shard_key_selector: shard_key_selector(&req.shard_key)
                    .or_else(|| shard_key_selector(&route_shard)),
                ..Default::default()
            };
            client
                .delete_points(grpc_req)
                .await
                .map_err(|e| QqlError::backend("QQL-GRPC", format!("delete: {e}"), None))?;
            Ok(serde_json::Value::Object(Default::default()))
        }
        Some(RequestBody::ClearPayload(req)) => {
            let collection = extract_collection(&route.path)?;
            let selector = if let Some(points) = &req.points {
                Some(qdrant::PointsSelector {
                    points_selector_one_of: Some(
                        qdrant::points_selector::PointsSelectorOneOf::Points(
                            qdrant::PointsIdsList {
                                ids: points.iter().map(to_point_id).collect(),
                            },
                        ),
                    ),
                })
            } else {
                req.filter.as_ref().map(|f| qdrant::PointsSelector {
                    points_selector_one_of: Some(
                        qdrant::points_selector::PointsSelectorOneOf::Filter(to_filter(f)),
                    ),
                })
            };
            let grpc_req = qdrant::ClearPayloadPoints {
                collection_name: collection,
                wait: Some(true),
                points: selector,
                ..Default::default()
            };
            client
                .clear_payload(grpc_req)
                .await
                .map_err(|e| QqlError::backend("QQL-GRPC", format!("clear_payload: {e}"), None))?;
            Ok(serde_json::Value::Object(Default::default()))
        }
        Some(RequestBody::DeleteVector(req)) => {
            let collection = extract_collection(&route.path)?;
            let selector = if let Some(points) = &req.points {
                Some(qdrant::PointsSelector {
                    points_selector_one_of: Some(
                        qdrant::points_selector::PointsSelectorOneOf::Points(
                            qdrant::PointsIdsList {
                                ids: points.iter().map(to_point_id).collect(),
                            },
                        ),
                    ),
                })
            } else {
                req.filter.as_ref().map(|f| qdrant::PointsSelector {
                    points_selector_one_of: Some(
                        qdrant::points_selector::PointsSelectorOneOf::Filter(to_filter(f)),
                    ),
                })
            };
            let grpc_req = qdrant::DeletePointVectors {
                collection_name: collection,
                wait: Some(true),
                points_selector: selector,
                vectors: Some(qdrant::VectorsSelector {
                    names: req.vector.clone(),
                }),
                ..Default::default()
            };
            client
                .delete_vectors(grpc_req)
                .await
                .map_err(|e| QqlError::backend("QQL-GRPC", format!("delete_vectors: {e}"), None))?;
            Ok(serde_json::Value::Object(Default::default()))
        }
        Some(RequestBody::UpdateVector(req)) => {
            let collection = extract_collection(&route.path)?;
            let points: Vec<qdrant::PointVectors> = req
                .points
                .iter()
                .map(|p| qdrant::PointVectors {
                    id: Some(to_point_id(&p.id)),
                    vectors: to_vectors(&p.vector),
                })
                .collect();
            let grpc_req = qdrant::UpdatePointVectors {
                collection_name: collection,
                wait: Some(true),
                points,
                ..Default::default()
            };
            client
                .update_vectors(grpc_req)
                .await
                .map_err(|e| QqlError::backend("QQL-GRPC", format!("update_vectors: {e}"), None))?;
            Ok(serde_json::Value::Object(Default::default()))
        }
        Some(RequestBody::UpdatePayload(req)) => {
            let collection = extract_collection(&route.path)?;
            let payload_map: std::collections::HashMap<String, qdrant::Value> = req
                .payload
                .iter()
                .map(|(k, v)| (k.clone(), to_qdrant_value(v.clone())))
                .collect();
            let selector = if let Some(points) = &req.points {
                Some(qdrant::PointsSelector {
                    points_selector_one_of: Some(
                        qdrant::points_selector::PointsSelectorOneOf::Points(
                            qdrant::PointsIdsList {
                                ids: points.iter().map(to_point_id).collect(),
                            },
                        ),
                    ),
                })
            } else {
                req.filter.as_ref().map(|f| qdrant::PointsSelector {
                    points_selector_one_of: Some(
                        qdrant::points_selector::PointsSelectorOneOf::Filter(to_filter(f)),
                    ),
                })
            };
            let grpc_req = qdrant::SetPayloadPoints {
                collection_name: collection,
                wait: Some(true),
                points_selector: selector,
                payload: payload_map,
                ..Default::default()
            };
            client
                .set_payload(grpc_req)
                .await
                .map_err(|e| QqlError::backend("QQL-GRPC", format!("set_payload: {e}"), None))?;
            Ok(serde_json::Value::Object(Default::default()))
        }
        Some(RequestBody::UpdateCollection(req)) => {
            let collection = extract_collection(&route.path)?;
            let grpc_req = qdrant::UpdateCollection {
                collection_name: collection,
                optimizers_config: req.optimizers_config.as_ref().map(|v| {
                    qdrant::OptimizersConfigDiff {
                        deleted_threshold: v.get("deleted_threshold").and_then(|x| x.as_f64()),
                        vacuum_min_vector_number: v
                            .get("vacuum_min_vector_number")
                            .and_then(|x| x.as_u64()),
                        default_segment_number: v
                            .get("default_segment_number")
                            .and_then(|x| x.as_u64()),
                        max_segment_size: v.get("max_segment_size").and_then(|x| x.as_u64()),
                        memmap_threshold: v.get("memmap_threshold").and_then(|x| x.as_u64()),
                        indexing_threshold: v.get("indexing_threshold").and_then(|x| x.as_u64()),
                        flush_interval_sec: v.get("flush_interval_sec").and_then(|x| x.as_u64()),
                        ..Default::default()
                    }
                }),
                hnsw_config: req.hnsw_config.as_ref().map(|v| qdrant::HnswConfigDiff {
                    m: v.get("m").and_then(|x| x.as_u64()),
                    ef_construct: v.get("ef_construct").and_then(|x| x.as_u64()),
                    full_scan_threshold: v.get("full_scan_threshold").and_then(|x| x.as_u64()),
                    max_indexing_threads: v.get("max_indexing_threads").and_then(|x| x.as_u64()),
                    on_disk: v.get("on_disk").and_then(|x| x.as_bool()),
                    payload_m: v.get("payload_m").and_then(|x| x.as_u64()),
                    ..Default::default()
                }),
                ..Default::default()
            };
            client.update_collection_raw(grpc_req).await.map_err(|e| {
                QqlError::backend("QQL-GRPC", format!("update_collection: {e}"), None)
            })?;
            Ok(serde_json::Value::Object(Default::default()))
        }
        Some(RequestBody::CreateCollection(req)) => {
            let collection = extract_collection(&route.path)?;
            let grpc_req = qdrant::CreateCollection {
                collection_name: collection,
                vectors_config: req.vectors.as_ref().map(|v| {
                    let map = v
                        .iter()
                        .map(|(name, cfg)| {
                            let size = cfg.get("size").and_then(|s| s.as_u64()).unwrap_or(384);
                            let dist = cfg
                                .get("distance")
                                .and_then(|d| d.as_str())
                                .map(|d| match d {
                                    "Cosine" => qdrant::Distance::Cosine as i32,
                                    "Euclid" => qdrant::Distance::Euclid as i32,
                                    "Dot" => qdrant::Distance::Dot as i32,
                                    "Manhattan" => qdrant::Distance::Manhattan as i32,
                                    _ => qdrant::Distance::Cosine as i32,
                                })
                                .unwrap_or(qdrant::Distance::Cosine as i32);
                            (
                                name.clone(),
                                qdrant::VectorParams {
                                    size,
                                    distance: dist,
                                    ..Default::default()
                                },
                            )
                        })
                        .collect();
                    qdrant::VectorsConfig {
                        config: Some(qdrant::vectors_config::Config::ParamsMap(
                            qdrant::VectorParamsMap { map },
                        )),
                    }
                }),
                sparse_vectors_config: req.sparse_vectors.as_ref().map(|sv| {
                    let map = sv
                        .iter()
                        .map(|(name, _)| {
                            (
                                name.clone(),
                                qdrant::SparseVectorParams {
                                    ..Default::default()
                                },
                            )
                        })
                        .collect();
                    qdrant::SparseVectorConfig { map }
                }),
                hnsw_config: req.hnsw_config.as_ref().map(|v| qdrant::HnswConfigDiff {
                    m: v.get("m").and_then(|x| x.as_u64()),
                    ef_construct: v.get("ef_construct").and_then(|x| x.as_u64()),
                    full_scan_threshold: v.get("full_scan_threshold").and_then(|x| x.as_u64()),
                    max_indexing_threads: v.get("max_indexing_threads").and_then(|x| x.as_u64()),
                    on_disk: v.get("on_disk").and_then(|x| x.as_bool()),
                    payload_m: v.get("payload_m").and_then(|x| x.as_u64()),
                    ..Default::default()
                }),
                optimizers_config: req.optimizers_config.as_ref().map(|v| {
                    qdrant::OptimizersConfigDiff {
                        deleted_threshold: v.get("deleted_threshold").and_then(|x| x.as_f64()),
                        vacuum_min_vector_number: v
                            .get("vacuum_min_vector_number")
                            .and_then(|x| x.as_u64()),
                        default_segment_number: v
                            .get("default_segment_number")
                            .and_then(|x| x.as_u64()),
                        max_segment_size: v.get("max_segment_size").and_then(|x| x.as_u64()),
                        memmap_threshold: v.get("memmap_threshold").and_then(|x| x.as_u64()),
                        indexing_threshold: v.get("indexing_threshold").and_then(|x| x.as_u64()),
                        flush_interval_sec: v.get("flush_interval_sec").and_then(|x| x.as_u64()),
                        ..Default::default()
                    }
                }),
                shard_number: req
                    .shard_number
                    .or_else(|| {
                        req.params
                            .as_ref()
                            .and_then(|p| p.get("shard_number"))
                            .and_then(|v| v.as_u64())
                    })
                    .map(|n| n as u32),
                replication_factor: req
                    .params
                    .as_ref()
                    .and_then(|p| p.get("replication_factor"))
                    .and_then(|v| v.as_u64())
                    .map(|n| n as u32),
                on_disk_payload: req
                    .params
                    .as_ref()
                    .and_then(|p| p.get("on_disk_payload"))
                    .and_then(|v| v.as_bool()),
                ..Default::default()
            };
            client.create_collection_raw(grpc_req).await.map_err(|e| {
                QqlError::backend("QQL-GRPC", format!("create_collection: {e}"), None)
            })?;
            Ok(serde_json::Value::Object(Default::default()))
        }
        Some(RequestBody::CreateIndex(req)) => {
            let collection = extract_collection(&route.path)?;
            let field_type = match req.field_schema.as_str() {
                "keyword" => qdrant::FieldType::Keyword as i32,
                "integer" => qdrant::FieldType::Integer as i32,
                "float" => qdrant::FieldType::Float as i32,
                "geo" => qdrant::FieldType::Geo as i32,
                "text" => qdrant::FieldType::Text as i32,
                "bool" => qdrant::FieldType::Bool as i32,
                "datetime" => qdrant::FieldType::Datetime as i32,
                "uuid" => qdrant::FieldType::Uuid as i32,
                _ => qdrant::FieldType::Keyword as i32,
            };
            let grpc_req = qdrant::CreateFieldIndexCollection {
                collection_name: collection,
                wait: Some(true),
                field_name: req.field_name.clone(),
                field_type: Some(field_type),
                ..Default::default()
            };
            client.create_field_index(grpc_req).await.map_err(|e| {
                QqlError::backend("QQL-GRPC", format!("create_field_index: {e}"), None)
            })?;
            Ok(serde_json::Value::Object(Default::default()))
        }
        Some(RequestBody::Count(req)) => {
            let collection = extract_collection(&route.path)?;
            let grpc_req = qdrant::CountPoints {
                collection_name: collection,
                filter: req.filter.as_ref().map(to_filter),
                exact: req.exact,
                shard_key_selector: shard_key_selector(&req.shard_key)
                    .or_else(|| shard_key_selector(&route_shard)),
                ..Default::default()
            };
            let resp = client
                .count_points(grpc_req)
                .await
                .map_err(|e| QqlError::backend("QQL-GRPC", format!("count: {e}"), None))?;
            Ok(serde_json::json!({
                "result": {
                    "count": resp.result.map(|r| r.count).unwrap_or(0),
                },
                "status": "ok",
                "time": 0.0_f64,
            }))
        }
        Some(RequestBody::CreateShardKey(req)) => {
            let collection = extract_collection(&route.path)?;
            let grpc_req = qdrant::CreateShardKeyRequest {
                collection_name: collection,
                request: Some(qdrant::CreateShardKey {
                    shard_key: Some(qdrant::ShardKey {
                        key: Some(qdrant::shard_key::Key::Keyword(req.shard_key.clone())),
                    }),
                    shards_number: req.shards_number.map(|n| n as u32),
                    replication_factor: req.replication_factor.map(|n| n as u32),
                    ..Default::default()
                }),
                ..Default::default()
            };
            client.create_shard_key(grpc_req).await.map_err(|e| {
                QqlError::backend("QQL-GRPC", format!("create_shard_key: {e}"), None)
            })?;
            Ok(serde_json::Value::Object(Default::default()))
        }
        Some(RequestBody::DropShardKey(req)) => {
            let collection = extract_collection(&route.path)?;
            let grpc_req = qdrant::DeleteShardKeyRequest {
                collection_name: collection,
                request: Some(qdrant::DeleteShardKey {
                    shard_key: Some(qdrant::ShardKey {
                        key: Some(qdrant::shard_key::Key::Keyword(req.shard_key.clone())),
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            };
            client.delete_shard_key(grpc_req).await.map_err(|e| {
                QqlError::backend("QQL-GRPC", format!("delete_shard_key: {e}"), None)
            })?;
            Ok(serde_json::Value::Object(Default::default()))
        }
        None => match route.method {
            qql_plan::types::Method::Get if route.path == "/collections" => {
                let resp = client
                    .list_collections_raw()
                    .await
                    .map_err(|e| QqlError::backend("QQL-GRPC", format!("list: {e}"), None))?;
                Ok(list_collections_response_to_json(resp))
            }
            qql_plan::types::Method::Get if route.path.ends_with("/shards") => {
                let collection = extract_collection(&route.path)?;
                let grpc_req = qdrant::ListShardKeysRequest {
                    collection_name: collection,
                    ..Default::default()
                };
                let resp = client.list_shard_keys(grpc_req).await.map_err(|e| {
                    QqlError::backend("QQL-GRPC", format!("list_shard_keys: {e}"), None)
                })?;
                let keys: Vec<serde_json::Value> = resp
                    .shard_keys
                    .into_iter()
                    .filter_map(|d| d.key)
                    .map(|sk| match sk.key {
                        Some(qdrant::shard_key::Key::Keyword(s)) => serde_json::Value::String(s),
                        Some(qdrant::shard_key::Key::Number(n)) => {
                            serde_json::Value::Number((n).into())
                        }
                        None => serde_json::Value::Null,
                    })
                    .collect();
                Ok(serde_json::json!({ "result": { "shard_keys": keys } }))
            }
            qql_plan::types::Method::Get if route.path.starts_with("/collections/") => {
                let collection = extract_collection(&route.path)?;
                let resp = client.collection_info_raw(collection).await.map_err(|e| {
                    QqlError::backend("QQL-GRPC", format!("get_collection: {e}"), None)
                })?;
                Ok(collection_info_to_json(resp))
            }
            qql_plan::types::Method::Delete if route.path.contains("/index/") => {
                // DROP INDEX: /collections/{collection}/index/{field_name}
                let segments: Vec<&str> = route.path.trim_start_matches('/').split('/').collect();
                let collection = segments
                    .get(1)
                    .ok_or_else(|| {
                        QqlError::execution("QQL-GRPC", "cannot extract collection from path", None)
                    })?
                    .to_string();
                let field_name = segments
                    .get(3)
                    .ok_or_else(|| {
                        QqlError::execution("QQL-GRPC", "cannot extract field_name from path", None)
                    })?
                    .to_string();
                client
                    .delete_field_index(qdrant::DeleteFieldIndexCollection {
                        collection_name: collection,
                        field_name,
                        wait: Some(true),
                        ..Default::default()
                    })
                    .await
                    .map_err(|e| {
                        QqlError::backend("QQL-GRPC", format!("delete_field_index: {e}"), None)
                    })?;
                Ok(serde_json::Value::Object(Default::default()))
            }
            qql_plan::types::Method::Delete if route.path.starts_with("/collections/") => {
                let collection = extract_collection(&route.path)?;
                client
                    .delete_collection_raw(qdrant::DeleteCollection {
                        collection_name: collection,
                        ..Default::default()
                    })
                    .await
                    .map_err(|e| {
                        QqlError::backend("QQL-GRPC", format!("delete_collection: {e}"), None)
                    })?;
                Ok(serde_json::Value::Object(Default::default()))
            }
            _ => Err(QqlError::execution(
                "QQL-GRPC",
                format!("unsupported: {} {}", route.method.as_str(), route.path),
                None,
            )),
        },
    }
}

/// Convert a batch of QueryRequests and send them via gRPC `QueryBatch`.
pub async fn execute_query_batch_grpc(
    client: &crate::grpc::GrpcQdrant,
    collection: &str,
    batch: &qql_plan::QueryBatchRequest,
) -> Result<Vec<serde_json::Value>, QqlError> {
    let query_points: Result<Vec<_>, _> = batch
        .searches
        .iter()
        .map(|req| to_query_points(req, collection))
        .collect();
    let query_points = query_points?;

    let grpc_req = qdrant::QueryBatchPoints {
        collection_name: collection.to_string(),
        query_points,
        ..Default::default()
    };

    let resp = client
        .query_batch(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("query_batch: {e}"), None))?;

    Ok(resp.result.into_iter().map(batch_result_to_json).collect())
}

/// Convert a mutation batch and send via gRPC `UpdateBatch`.
pub async fn execute_update_batch_grpc(
    client: &crate::grpc::GrpcQdrant,
    collection: &str,
    batch: &qql_plan::UpdateBatchRequest,
) -> Result<Vec<serde_json::Value>, QqlError> {
    let operations: Vec<qdrant::PointsUpdateOperation> = batch
        .operations
        .iter()
        .map(to_points_update_operation)
        .collect();

    let grpc_req = qdrant::UpdateBatchPoints {
        collection_name: collection.to_string(),
        wait: Some(true),
        operations,
        ..Default::default()
    };

    let resp = client
        .update_batch(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("update_batch: {e}"), None))?;

    Ok(resp.result.into_iter().map(update_result_to_json).collect())
}

fn to_points_update_operation(op: &qql_plan::UpdateOperation) -> qdrant::PointsUpdateOperation {
    use qdrant::points_update_operation::{self, Operation};
    use qql_plan::UpdateOperation;

    let operation = match op {
        UpdateOperation::Upsert { upsert } => {
            let points: Vec<qdrant::PointStruct> = upsert
                .points
                .iter()
                .map(|p| {
                    let payload = p
                        .payload
                        .as_ref()
                        .map(|pl| {
                            pl.iter()
                                .map(|(k, v)| (k.clone(), to_qdrant_value(v.clone())))
                                .collect()
                        })
                        .unwrap_or_default();
                    qdrant::PointStruct {
                        id: Some(to_point_id(&p.id)),
                        vectors: p.vector.as_ref().and_then(|v| to_vectors(v)),
                        payload,
                    }
                })
                .collect();
            let shard_key_selector = upsert.shard_key.as_ref().map(|k| qdrant::ShardKeySelector {
                shard_keys: vec![qdrant::ShardKey {
                    key: Some(qdrant::shard_key::Key::Keyword(k.clone())),
                }],
                ..Default::default()
            });
            Operation::Upsert(points_update_operation::PointStructList {
                points,
                shard_key_selector,
                update_filter: None,
                update_mode: None,
            })
        }
        UpdateOperation::Delete { delete } => {
            let points = points_and_filter_selector(delete.points.as_ref(), delete.filter.as_ref());
            let shard_key_selector = delete.shard_key.as_ref().map(|k| qdrant::ShardKeySelector {
                shard_keys: vec![qdrant::ShardKey {
                    key: Some(qdrant::shard_key::Key::Keyword(k.clone())),
                }],
                ..Default::default()
            });
            Operation::DeletePoints(points_update_operation::DeletePoints {
                points,
                shard_key_selector,
            })
        }
        UpdateOperation::SetPayload { set_payload } => {
            let payload_map: std::collections::HashMap<String, qdrant::Value> = set_payload
                .payload
                .iter()
                .map(|(k, v)| (k.clone(), to_qdrant_value(v.clone())))
                .collect();
            Operation::SetPayload(points_update_operation::SetPayload {
                payload: payload_map,
                points_selector: points_and_filter_selector(
                    set_payload.points.as_ref(),
                    set_payload.filter.as_ref(),
                ),
                shard_key_selector: None,
                key: None,
            })
        }
        UpdateOperation::ClearPayload { clear_payload } => {
            Operation::ClearPayload(points_update_operation::ClearPayload {
                points: points_and_filter_selector(
                    clear_payload.points.as_ref(),
                    clear_payload.filter.as_ref(),
                ),
                shard_key_selector: None,
            })
        }
        UpdateOperation::UpdateVectors { update_vectors } => {
            let points: Vec<qdrant::PointVectors> = update_vectors
                .points
                .iter()
                .map(|p| qdrant::PointVectors {
                    id: Some(to_point_id(&p.id)),
                    vectors: to_vectors(&p.vector),
                })
                .collect();
            Operation::UpdateVectors(points_update_operation::UpdateVectors {
                points,
                shard_key_selector: None,
                update_filter: None,
            })
        }
        UpdateOperation::DeleteVectors { delete_vectors } => {
            Operation::DeleteVectors(points_update_operation::DeleteVectors {
                points_selector: points_and_filter_selector(
                    delete_vectors.points.as_ref(),
                    delete_vectors.filter.as_ref(),
                ),
                vectors: Some(qdrant::VectorsSelector {
                    names: delete_vectors.vector.clone(),
                }),
                shard_key_selector: None,
            })
        }
    };

    qdrant::PointsUpdateOperation {
        operation: Some(operation),
    }
}

fn points_and_filter_selector(
    points: Option<&Vec<PlanPointId>>,
    filter: Option<&FilterExpression>,
) -> Option<qdrant::PointsSelector> {
    if let Some(points) = points {
        Some(qdrant::PointsSelector {
            points_selector_one_of: Some(qdrant::points_selector::PointsSelectorOneOf::Points(
                qdrant::PointsIdsList {
                    ids: points.iter().map(to_point_id).collect(),
                },
            )),
        })
    } else {
        filter.map(|f| qdrant::PointsSelector {
            points_selector_one_of: Some(qdrant::points_selector::PointsSelectorOneOf::Filter(
                to_filter(f),
            )),
        })
    }
}

fn update_result_to_json(r: qdrant::UpdateResult) -> serde_json::Value {
    let status = match r.status() {
        qdrant::UpdateStatus::Acknowledged => "acknowledged",
        qdrant::UpdateStatus::Completed => "completed",
        qdrant::UpdateStatus::ClockRejected => "clock_rejected",
        qdrant::UpdateStatus::WaitTimeout => "wait_timeout",
        qdrant::UpdateStatus::UnknownUpdateStatus => "unknown",
    };
    serde_json::json!({
        "operation_id": r.operation_id,
        "status": status,
    })
}

fn to_query_points(
    req: &qql_plan::types::QueryRequest,
    collection: &str,
) -> Result<qdrant::QueryPoints, QqlError> {
    Ok(qdrant::QueryPoints {
        collection_name: collection.into(),
        prefetch: req.prefetch.iter().map(to_prefetch).collect(),
        query: Some(to_query_variant(&req.query)?),
        using: req.using.clone(),
        filter: req.filter.as_ref().map(to_filter),
        params: req.params.as_ref().map(to_search_params),
        score_threshold: req.score_threshold.map(|s| s as f32),
        limit: req.limit,
        offset: req.offset,
        with_payload: req.with_payload.as_ref().map(to_payload_selector),
        with_vectors: req.with_vector.as_ref().map(to_vectors_selector),
        shard_key_selector: shard_key_selector(&req.shard_key),
        ..Default::default()
    })
}

fn to_query_groups(
    req: &qql_plan::types::QueryGroupsRequest,
    collection: &str,
) -> Result<qdrant::QueryPointGroups, QqlError> {
    Ok(qdrant::QueryPointGroups {
        collection_name: collection.into(),
        prefetch: req.prefetch.iter().map(to_prefetch).collect(),
        query: Some(to_query_variant(&req.query)?),
        using: req.using.clone(),
        filter: req.filter.as_ref().map(to_filter),
        params: req.params.as_ref().map(to_search_params),
        score_threshold: req.score_threshold.map(|s| s as f32),
        with_payload: req.with_payload.as_ref().map(to_payload_selector),
        with_vectors: req.with_vector.as_ref().map(to_vectors_selector),
        group_by: req.group_by.clone(),
        group_size: Some(req.group_size),
        limit: Some(req.limit),
        with_lookup: req.with_lookup.as_ref().map(|wv| match wv {
            WithLookupValue::Collection(c) => qdrant::WithLookup {
                collection: c.clone(),
                ..Default::default()
            },
            WithLookupValue::Full(wl) => qdrant::WithLookup {
                collection: wl.collection.clone(),
                with_payload: wl.with_payload.as_ref().map(to_payload_selector),
                with_vectors: wl.with_vectors.as_ref().map(to_vectors_selector),
            },
        }),
        shard_key_selector: shard_key_selector(&req.shard_key),
        ..Default::default()
    })
}

fn to_prefetch(pf: &qql_plan::types::PrefetchRequest) -> qdrant::PrefetchQuery {
    qdrant::PrefetchQuery {
        prefetch: pf
            .prefetch
            .as_ref()
            .map(|pfs| pfs.iter().map(to_prefetch).collect())
            .unwrap_or_default(),
        query: pf.query.as_ref().and_then(|q| to_query_variant(q).ok()),
        using: pf.using.clone(),
        filter: pf.filter.as_ref().map(to_filter),
        params: pf.params.as_ref().map(to_search_params),
        score_threshold: pf.score_threshold.map(|s| s as f32),
        limit: pf.limit,
        lookup_from: pf.lookup_from.as_ref().map(|l| qdrant::LookupLocation {
            collection_name: l.collection.clone(),
            vector_name: l.vector.clone(),
            ..Default::default()
        }),
    }
}

fn to_query_variant(qv: &qql_plan::types::QueryVariant) -> Result<qdrant::Query, QqlError> {
    use qdrant::query::Variant;
    use qql_plan::types::QueryVariant;

    let variant = match qv {
        QueryVariant::Nearest(nq) => Variant::Nearest(to_vector_input(&nq.nearest)),
        QueryVariant::Recommend { recommend } => Variant::Recommend(qdrant::RecommendInput {
            positive: recommend.positive.iter().map(to_vector_input).collect(),
            negative: recommend.negative.iter().map(to_vector_input).collect(),
            strategy: recommend.strategy.as_deref().map(|s| match s {
                "average_vector" => qdrant::RecommendStrategy::AverageVector as i32,
                "best_score" => qdrant::RecommendStrategy::BestScore as i32,
                "sum_scores" => qdrant::RecommendStrategy::SumScores as i32,
                _ => qdrant::RecommendStrategy::AverageVector as i32,
            }),
        }),
        QueryVariant::Context { context } => Variant::Context(qdrant::ContextInput {
            pairs: context
                .iter()
                .map(|p| qdrant::ContextInputPair {
                    positive: Some(to_vector_input(&p.positive)),
                    negative: Some(to_vector_input(&p.negative)),
                })
                .collect(),
        }),
        QueryVariant::Discover { discover } => Variant::Discover(qdrant::DiscoverInput {
            target: Some(to_vector_input(&discover.target)),
            context: Some(qdrant::ContextInput {
                pairs: discover
                    .context
                    .iter()
                    .map(|p| qdrant::ContextInputPair {
                        positive: Some(to_vector_input(&p.positive)),
                        negative: Some(to_vector_input(&p.negative)),
                    })
                    .collect(),
            }),
        }),
        QueryVariant::OrderBy { order_by } => {
            let dir = order_by.direction.as_deref().map(|d| match d {
                "asc" => qdrant::Direction::Asc as i32,
                "desc" => qdrant::Direction::Desc as i32,
                _ => qdrant::Direction::Asc as i32,
            });
            Variant::OrderBy(qdrant::OrderBy {
                key: order_by.key.clone(),
                direction: dir,
                ..Default::default()
            })
        }
        QueryVariant::Sample { .. } => Variant::Sample(0),
        QueryVariant::Fusion { fusion } => {
            let val = match fusion.as_str() {
                "rrf" => 1,
                "dbsf" => 2,
                _ => 0,
            };
            Variant::Fusion(val)
        }
        QueryVariant::Rrf(_rrf_q) => Variant::Fusion(1),
        QueryVariant::Formula(fq) => Variant::Formula(qdrant::Formula {
            expression: to_formula_expression(&qql_plan::query::lower_formula_expr(&fq.formula.0)),
            defaults: fq
                .defaults
                .clone()
                .unwrap_or_default()
                .into_iter()
                .map(|(k, v)| (k, to_qdrant_value(v)))
                .collect(),
        }),
        QueryVariant::RelevanceFeedback { relevance_feedback } => {
            let feedback = relevance_feedback
                .feedback
                .iter()
                .map(|item| qdrant::FeedbackItem {
                    example: Some(to_vector_input(&item.example)),
                    score: item.score as f32,
                })
                .collect();
            let strategy = Some(qdrant::FeedbackStrategy {
                variant: Some(qdrant::feedback_strategy::Variant::Naive(
                    qdrant::NaiveFeedbackStrategy {
                        a: relevance_feedback.strategy.naive.a as f32,
                        b: relevance_feedback.strategy.naive.b as f32,
                        c: relevance_feedback.strategy.naive.c as f32,
                    },
                )),
            });
            Variant::RelevanceFeedback(qdrant::RelevanceFeedbackInput {
                target: Some(to_vector_input(&relevance_feedback.target)),
                feedback,
                strategy,
            })
        }
    };
    Ok(qdrant::Query {
        variant: Some(variant),
    })
}

fn to_vector_input(input: &PlanQueryInput) -> qdrant::VectorInput {
    use qdrant::vector_input::Variant;
    match input {
        PlanQueryInput::Point(id) => qdrant::VectorInput {
            variant: Some(Variant::Id(to_point_id(id))),
        },
        PlanQueryInput::Vector(PlanVectorValue::Dense(data)) => qdrant::VectorInput {
            variant: Some(Variant::Dense(qdrant::DenseVector { data: data.clone() })),
        },
        PlanQueryInput::Vector(PlanVectorValue::Sparse { indices, values }) => {
            qdrant::VectorInput {
                variant: Some(Variant::Sparse(qdrant::SparseVector {
                    indices: indices.clone(),
                    values: values.clone(),
                })),
            }
        }
        PlanQueryInput::Vector(PlanVectorValue::MultiDense(rows)) => qdrant::VectorInput {
            variant: Some(Variant::MultiDense(qdrant::MultiDenseVector {
                vectors: rows
                    .iter()
                    .map(|row| qdrant::DenseVector { data: row.clone() })
                    .collect(),
            })),
        },
        PlanQueryInput::Document { text, model } => qdrant::VectorInput {
            variant: Some(Variant::Document(qdrant::Document {
                text: text.clone(),
                model: model.clone().unwrap_or_default(),
                ..Default::default()
            })),
        },
    }
}

fn to_filter(fe: &FilterExpression) -> qdrant::Filter {
    match fe {
        FilterExpression::Compound(fc) => compound_to_filter(fc),
        FilterExpression::Single(fc) => qdrant::Filter {
            must: vec![to_condition(fc)],
            ..Default::default()
        },
    }
}

fn compound_to_filter(fc: &FilterCompound) -> qdrant::Filter {
    qdrant::Filter {
        must: fc.must.iter().map(to_condition).collect(),
        must_not: fc.must_not.iter().map(to_condition).collect(),
        should: fc.should.iter().map(to_condition).collect(),
        ..Default::default()
    }
}

fn to_condition(clause: &FilterClause) -> qdrant::Condition {
    use qdrant::condition::ConditionOneOf;
    match clause {
        FilterClause::Field(fc) => {
            let mut field = qdrant::FieldCondition {
                key: fc.key.clone(),
                ..Default::default()
            };
            if let Some(mv) = &fc.r#match {
                field.r#match = Some(to_match(mv));
            }
            if let Some(r) = &fc.range {
                field.range = Some(qdrant::Range {
                    gt: r.gt.as_ref().and_then(|v| v.as_f64()),
                    gte: r.gte.as_ref().and_then(|v| v.as_f64()),
                    lt: r.lt.as_ref().and_then(|v| v.as_f64()),
                    lte: r.lte.as_ref().and_then(|v| v.as_f64()),
                });
            }
            if let Some(b) = &fc.geo_bounding_box {
                field.geo_bounding_box = Some(qdrant::GeoBoundingBox {
                    top_left: Some(qdrant::GeoPoint {
                        lat: b.top_left.lat,
                        lon: b.top_left.lon,
                    }),
                    bottom_right: Some(qdrant::GeoPoint {
                        lat: b.bottom_right.lat,
                        lon: b.bottom_right.lon,
                    }),
                });
            }
            if let Some(r) = &fc.geo_radius {
                field.geo_radius = Some(qdrant::GeoRadius {
                    center: Some(qdrant::GeoPoint {
                        lat: r.center.lat,
                        lon: r.center.lon,
                    }),
                    radius: r.radius as f32,
                });
            }
            if let Some(vc) = &fc.values_count {
                field.values_count = Some(qdrant::ValuesCount {
                    gt: vc.gt,
                    gte: vc.gte,
                    lt: vc.lt,
                    lte: vc.lte,
                });
            }
            qdrant::Condition {
                condition_one_of: Some(ConditionOneOf::Field(field)),
            }
        }
        FilterClause::IsNull(n) => qdrant::Condition {
            condition_one_of: Some(ConditionOneOf::IsNull(qdrant::IsNullCondition {
                key: n.is_null.key.clone(),
            })),
        },
        FilterClause::IsEmpty(e) => qdrant::Condition {
            condition_one_of: Some(ConditionOneOf::IsEmpty(qdrant::IsEmptyCondition {
                key: e.is_empty.key.clone(),
            })),
        },
        FilterClause::HasId(h) => qdrant::Condition {
            condition_one_of: Some(ConditionOneOf::HasId(qdrant::HasIdCondition {
                has_id: h.has_id.iter().map(to_point_id_json).collect(),
            })),
        },
        FilterClause::HasVector(v) => qdrant::Condition {
            condition_one_of: Some(ConditionOneOf::HasVector(qdrant::HasVectorCondition {
                has_vector: v.has_vector.clone(),
            })),
        },
        FilterClause::Nested(n) => qdrant::Condition {
            condition_one_of: Some(ConditionOneOf::Nested(qdrant::NestedCondition {
                key: n.nested.key.clone(),
                filter: Some(to_filter(&n.nested.filter)),
            })),
        },
        FilterClause::Filter(f) => qdrant::Condition {
            condition_one_of: Some(ConditionOneOf::Filter(compound_to_filter(f))),
        },
    }
}

fn exact_list_match(values: &[serde_json::Value], any: bool) -> qdrant::Match {
    use qdrant::r#match::MatchValue as Mv;
    let all_strings = values.iter().all(|v| v.is_string());
    let all_ints = values
        .iter()
        .all(|v| v.as_i64().is_some() || v.as_u64().is_some());
    if all_strings {
        let strings: Vec<String> = values
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
        return qdrant::Match {
            match_value: Some(if any {
                Mv::Keywords(qdrant::RepeatedStrings { strings })
            } else {
                Mv::ExceptKeywords(qdrant::RepeatedStrings { strings })
            }),
        };
    }
    if all_ints {
        let integers: Vec<i64> = values
            .iter()
            .filter_map(|v| v.as_i64().or_else(|| v.as_u64().map(|n| n as i64)))
            .collect();
        return qdrant::Match {
            match_value: Some(if any {
                Mv::Integers(qdrant::RepeatedIntegers { integers })
            } else {
                Mv::ExceptIntegers(qdrant::RepeatedIntegers { integers })
            }),
        };
    }
    // Mixed types: keep keywords for string-only entries (lossy but never invent ints).
    // Prefer empty over wrong semantics when types mix — still map strings.
    let strings: Vec<String> = values
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    qdrant::Match {
        match_value: Some(if any {
            Mv::Keywords(qdrant::RepeatedStrings { strings })
        } else {
            Mv::ExceptKeywords(qdrant::RepeatedStrings { strings })
        }),
    }
}

fn to_match(mv: &MatchValue) -> qdrant::Match {
    use qdrant::r#match::MatchValue as Mv;
    match mv {
        MatchValue::Value { value } => {
            if let Some(s) = value.as_str() {
                qdrant::Match {
                    match_value: Some(Mv::Keyword(s.into())),
                }
            } else if let Some(b) = value.as_bool() {
                qdrant::Match {
                    match_value: Some(Mv::Boolean(b)),
                }
            } else if let Some(n) = value.as_i64() {
                qdrant::Match {
                    match_value: Some(Mv::Integer(n)),
                }
            } else {
                qdrant::Match { match_value: None }
            }
        }
        MatchValue::Text { text } => qdrant::Match {
            match_value: Some(Mv::Text(text.clone())),
        },
        MatchValue::TextAny { text } => qdrant::Match {
            match_value: Some(Mv::TextAny(text.clone())),
        },
        MatchValue::Any { any } => exact_list_match(any, true),
        MatchValue::Except { except } => exact_list_match(except, false),
        MatchValue::Phrase { phrase } => qdrant::Match {
            match_value: Some(Mv::Phrase(phrase.clone())),
        },
    }
}

fn to_point_id(id: &PlanPointId) -> qdrant::PointId {
    match id {
        PlanPointId::Number(n) => qdrant::PointId {
            point_id_options: Some(qdrant::point_id::PointIdOptions::Num(*n)),
        },
        PlanPointId::String(s) => qdrant::PointId {
            point_id_options: Some(qdrant::point_id::PointIdOptions::Uuid(s.clone())),
        },
    }
}

fn to_point_id_json(val: &serde_json::Value) -> qdrant::PointId {
    match val {
        serde_json::Value::Number(n) => qdrant::PointId {
            point_id_options: Some(qdrant::point_id::PointIdOptions::Num(
                n.as_u64().unwrap_or(0),
            )),
        },
        serde_json::Value::String(s) => qdrant::PointId {
            point_id_options: Some(qdrant::point_id::PointIdOptions::Uuid(s.clone())),
        },
        _ => qdrant::PointId {
            point_id_options: None,
        },
    }
}

fn to_payload_selector(ps: &PayloadSelectorReq) -> qdrant::WithPayloadSelector {
    match ps {
        PayloadSelectorReq::All(b) => qdrant::WithPayloadSelector {
            selector_options: Some(qdrant::with_payload_selector::SelectorOptions::Enable(*b)),
        },
        PayloadSelectorReq::Include { include } => qdrant::WithPayloadSelector {
            selector_options: Some(qdrant::with_payload_selector::SelectorOptions::Include(
                qdrant::PayloadIncludeSelector {
                    fields: include.clone(),
                },
            )),
        },
        PayloadSelectorReq::Exclude { exclude } => qdrant::WithPayloadSelector {
            selector_options: Some(qdrant::with_payload_selector::SelectorOptions::Exclude(
                qdrant::PayloadExcludeSelector {
                    fields: exclude.clone(),
                },
            )),
        },
    }
}

fn to_vectors_selector(vs: &VectorSelectorReq) -> qdrant::WithVectorsSelector {
    match vs {
        VectorSelectorReq::All(b) => qdrant::WithVectorsSelector {
            selector_options: Some(qdrant::with_vectors_selector::SelectorOptions::Enable(*b)),
        },
        VectorSelectorReq::Names(names) => qdrant::WithVectorsSelector {
            selector_options: Some(qdrant::with_vectors_selector::SelectorOptions::Include(
                qdrant::VectorsSelector {
                    names: names.clone(),
                },
            )),
        },
    }
}

fn to_search_params(params: &qql_plan::types::SearchParamsRequest) -> qdrant::SearchParams {
    qdrant::SearchParams {
        hnsw_ef: params.hnsw_ef,
        exact: params.exact,
        indexed_only: params.indexed_only,
        quantization: params
            .quantization
            .as_ref()
            .map(|q| qdrant::QuantizationSearchParams {
                ignore: q.ignore,
                rescore: q.rescore,
                oversampling: q.oversampling,
            }),
        acorn: params.acorn.as_ref().map(|a| qdrant::AcornSearchParams {
            enable: Some(a.enable),
            max_selectivity: a.max_selectivity,
        }),
    }
}

fn plan_vector_to_proto(v: &PlanVectorValue) -> qdrant::Vector {
    match v {
        PlanVectorValue::Dense(data) => qdrant::Vector {
            vector: Some(qdrant::vector::Vector::Dense(qdrant::DenseVector {
                data: data.clone(),
            })),
            ..Default::default()
        },
        PlanVectorValue::Sparse { indices, values } => qdrant::Vector {
            vector: Some(qdrant::vector::Vector::Sparse(qdrant::SparseVector {
                indices: indices.clone(),
                values: values.clone(),
            })),
            ..Default::default()
        },
        PlanVectorValue::MultiDense(rows) => qdrant::Vector {
            vector: Some(qdrant::vector::Vector::MultiDense(
                qdrant::MultiDenseVector {
                    vectors: rows
                        .iter()
                        .map(|row| qdrant::DenseVector { data: row.clone() })
                        .collect(),
                },
            )),
            ..Default::default()
        },
    }
}

fn to_vectors(vectors: &PlanPointVectors) -> Option<qdrant::Vectors> {
    match vectors {
        PlanPointVectors::Unnamed(v) => Some(qdrant::Vectors {
            vectors_options: Some(qdrant::vectors::VectorsOptions::Vector(
                plan_vector_to_proto(v),
            )),
        }),
        PlanPointVectors::Named(entries) => {
            let mut map = std::collections::HashMap::new();
            for (name, v) in entries {
                map.insert(name.clone(), plan_vector_to_proto(v));
            }
            Some(qdrant::Vectors {
                vectors_options: Some(qdrant::vectors::VectorsOptions::Vectors(
                    qdrant::NamedVectors { vectors: map },
                )),
            })
        }
    }
}

fn to_formula_expression(val: &serde_json::Value) -> Option<qdrant::Expression> {
    use qdrant::expression::Variant;
    // OpenAPI Expression: bare number / bare string / one-key objects with snake_case keys.
    match val {
        serde_json::Value::Number(n) => n.as_f64().map(|f| qdrant::Expression {
            variant: Some(Variant::Constant(f as f32)),
        }),
        serde_json::Value::String(s) => Some(qdrant::Expression {
            variant: Some(Variant::Variable(s.clone())),
        }),
        serde_json::Value::Object(obj) if obj.len() == 1 => {
            let (key, val) = obj.iter().next()?;
            match key.as_str() {
                // REST dialect (qql-plan output) + legacy PascalCase keys
                "Constant" | "constant" => val.as_f64().map(|f| qdrant::Expression {
                    variant: Some(Variant::Constant(f as f32)),
                }),
                "Variable" | "variable" => val.as_str().map(|s| qdrant::Expression {
                    variant: Some(Variant::Variable(s.to_string())),
                }),
                "sum" | "Add" => {
                    let terms: Vec<qdrant::Expression> = val
                        .as_array()?
                        .iter()
                        .filter_map(to_formula_expression)
                        .collect();
                    Some(qdrant::Expression {
                        variant: Some(Variant::Sum(qdrant::SumExpression { sum: terms })),
                    })
                }
                "mult" | "Multiply" => {
                    let terms: Vec<qdrant::Expression> = val
                        .as_array()?
                        .iter()
                        .filter_map(to_formula_expression)
                        .collect();
                    Some(qdrant::Expression {
                        variant: Some(Variant::Mult(qdrant::MultExpression { mult: terms })),
                    })
                }
                "div" | "Divide" => {
                    let obj = val.as_object()?;
                    let left = to_formula_expression(obj.get("left")?)?;
                    let right = to_formula_expression(obj.get("right")?)?;
                    let by_zero_default = obj
                        .get("by_zero_default")
                        .and_then(|v| v.as_f64())
                        .map(|f| f as f32);
                    Some(qdrant::Expression {
                        variant: Some(Variant::Div(Box::new(qdrant::DivExpression {
                            left: Some(Box::new(left)),
                            right: Some(Box::new(right)),
                            by_zero_default,
                        }))),
                    })
                }
                "neg" | "Negate" => {
                    let inner = to_formula_expression(val)?;
                    Some(qdrant::Expression {
                        variant: Some(Variant::Neg(Box::new(inner))),
                    })
                }
                "abs" | "Abs" => {
                    let inner = to_formula_expression(val)?;
                    Some(qdrant::Expression {
                        variant: Some(Variant::Abs(Box::new(inner))),
                    })
                }
                "sqrt" | "Sqrt" => {
                    let inner = to_formula_expression(val)?;
                    Some(qdrant::Expression {
                        variant: Some(Variant::Sqrt(Box::new(inner))),
                    })
                }
                "log10" | "Log10" => {
                    let inner = to_formula_expression(val)?;
                    Some(qdrant::Expression {
                        variant: Some(Variant::Log10(Box::new(inner))),
                    })
                }
                "ln" | "NaturalLog" => {
                    let inner = to_formula_expression(val)?;
                    Some(qdrant::Expression {
                        variant: Some(Variant::Ln(Box::new(inner))),
                    })
                }
                "exp" | "Exp" => {
                    let inner = to_formula_expression(val)?;
                    Some(qdrant::Expression {
                        variant: Some(Variant::Exp(Box::new(inner))),
                    })
                }
                "pow" | "Pow" => {
                    if let Some(arr) = val.as_array() {
                        Some(qdrant::Expression {
                            variant: Some(Variant::Pow(Box::new(qdrant::PowExpression {
                                base: Some(Box::new(to_formula_expression(&arr[0])?)),
                                exponent: Some(Box::new(to_formula_expression(&arr[1])?)),
                            }))),
                        })
                    } else {
                        let obj = val.as_object()?;
                        Some(qdrant::Expression {
                            variant: Some(Variant::Pow(Box::new(qdrant::PowExpression {
                                base: Some(Box::new(to_formula_expression(obj.get("base")?)?)),
                                exponent: Some(Box::new(to_formula_expression(
                                    obj.get("exponent")?,
                                )?)),
                            }))),
                        })
                    }
                }
                "geo_distance" | "GeoDistance" => {
                    let obj = val.as_object()?;
                    let origin = obj.get("origin")?;
                    let lat = origin
                        .get("lat")
                        .or_else(|| origin.get("latitude"))
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0);
                    let lon = origin
                        .get("lon")
                        .or_else(|| origin.get("longitude"))
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0);
                    let to = obj
                        .get("to")
                        .or_else(|| obj.get("field"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    Some(qdrant::Expression {
                        variant: Some(Variant::GeoDistance(qdrant::GeoDistance {
                            origin: Some(qdrant::GeoPoint { lat, lon }),
                            to,
                        })),
                    })
                }
                "exp_decay" | "gauss_decay" | "lin_decay" => {
                    let obj = val.as_object()?;
                    let x = to_formula_expression(obj.get("x")?)?;
                    let target = obj.get("target").and_then(to_formula_expression);
                    let scale = obj.get("scale").and_then(|v| v.as_f64()).map(|f| f as f32);
                    let midpoint = obj
                        .get("midpoint")
                        .and_then(|v| v.as_f64())
                        .map(|f| f as f32);
                    let decay = Box::new(qdrant::DecayParamsExpression {
                        x: Some(Box::new(x)),
                        target: target.map(Box::new),
                        scale,
                        midpoint,
                    });
                    let variant = match key.as_str() {
                        "exp_decay" => Variant::ExpDecay(decay),
                        "lin_decay" => Variant::LinDecay(decay),
                        _ => Variant::GaussDecay(decay),
                    };
                    Some(qdrant::Expression {
                        variant: Some(variant),
                    })
                }
                "datetime" | "DateTime" => val.as_str().map(|s| qdrant::Expression {
                    variant: Some(Variant::Datetime(s.to_string())),
                }),
                "datetime_key" | "DateTimeField" => val.as_str().map(|s| qdrant::Expression {
                    variant: Some(Variant::DatetimeKey(s.to_string())),
                }),
                // Field condition used as boolean Expression (key + match/range/...)
                "key" => {
                    // Reconstruct single-key object for condition parser
                    to_condition_from_json(&serde_json::Value::Object(obj.clone())).map(|c| {
                        qdrant::Expression {
                            variant: Some(Variant::Condition(c)),
                        }
                    })
                }
                _ => {
                    // Try as a filter condition object (must/should/must_not or field clause).
                    to_condition_from_json(val).map(|c| qdrant::Expression {
                        variant: Some(Variant::Condition(c)),
                    })
                }
            }
        }
        // Multi-key objects: likely a field condition {key, match}
        serde_json::Value::Object(obj) => {
            to_condition_from_json(&serde_json::Value::Object(obj.clone())).map(|c| {
                qdrant::Expression {
                    variant: Some(Variant::Condition(c)),
                }
            })
        }
        _ => None,
    }
}

fn to_condition_from_json(val: &serde_json::Value) -> Option<qdrant::Condition> {
    match val {
        serde_json::Value::Object(obj) if obj.len() == 1 => {
            let (key, inner) = obj.iter().next()?;
            match key.as_str() {
                "And" => {
                    let conditions: Vec<qdrant::Condition> = inner
                        .as_array()?
                        .iter()
                        .filter_map(to_condition_from_json)
                        .collect();
                    Some(qdrant::Condition {
                        condition_one_of: Some(qdrant::condition::ConditionOneOf::Filter(
                            qdrant::Filter {
                                must: conditions,
                                ..Default::default()
                            },
                        )),
                    })
                }
                "Or" => {
                    let conditions: Vec<qdrant::Condition> = inner
                        .as_array()?
                        .iter()
                        .filter_map(to_condition_from_json)
                        .collect();
                    Some(qdrant::Condition {
                        condition_one_of: Some(qdrant::condition::ConditionOneOf::Filter(
                            qdrant::Filter {
                                should: conditions,
                                ..Default::default()
                            },
                        )),
                    })
                }
                "Not" => {
                    let inner_cond = to_condition_from_json(inner)?;
                    Some(qdrant::Condition {
                        condition_one_of: Some(qdrant::condition::ConditionOneOf::Filter(
                            qdrant::Filter {
                                must_not: vec![inner_cond],
                                ..Default::default()
                            },
                        )),
                    })
                }
                "Compare" => {
                    let obj = inner.as_object()?;
                    let field = obj.get("field")?.as_str()?;
                    let op = obj.get("op")?.as_str()?;
                    let value = obj.get("value")?;
                    let range = match op {
                        "Eq" => qdrant::Range {
                            gte: value.as_f64(),
                            lte: value.as_f64(),
                            ..Default::default()
                        },
                        "Gt" => qdrant::Range {
                            gt: value.as_f64(),
                            ..Default::default()
                        },
                        "Gte" => qdrant::Range {
                            gte: value.as_f64(),
                            ..Default::default()
                        },
                        "Lt" => qdrant::Range {
                            lt: value.as_f64(),
                            ..Default::default()
                        },
                        "Lte" => qdrant::Range {
                            lte: value.as_f64(),
                            ..Default::default()
                        },
                        _ => return None,
                    };
                    Some(qdrant::Condition {
                        condition_one_of: Some(qdrant::condition::ConditionOneOf::Field(
                            qdrant::FieldCondition {
                                key: field.to_string(),
                                range: Some(range),
                                ..Default::default()
                            },
                        )),
                    })
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn to_qdrant_value(val: serde_json::Value) -> qdrant::Value {
    use qdrant::value::Kind;
    match val {
        serde_json::Value::Null => qdrant::Value { kind: None },
        serde_json::Value::Bool(b) => qdrant::Value {
            kind: Some(Kind::BoolValue(b)),
        },
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                qdrant::Value {
                    kind: Some(Kind::IntegerValue(i)),
                }
            } else {
                qdrant::Value {
                    kind: Some(Kind::DoubleValue(n.as_f64().unwrap_or(0.0))),
                }
            }
        }
        serde_json::Value::String(s) => qdrant::Value {
            kind: Some(Kind::StringValue(s)),
        },
        serde_json::Value::Array(arr) => qdrant::Value {
            kind: Some(Kind::ListValue(qdrant::ListValue {
                values: arr.into_iter().map(to_qdrant_value).collect(),
            })),
        },
        serde_json::Value::Object(obj) => {
            let fields = obj
                .into_iter()
                .map(|(k, v)| (k, to_qdrant_value(v)))
                .collect();
            qdrant::Value {
                kind: Some(Kind::StructValue(qdrant::Struct { fields })),
            }
        }
    }
}

// ── Proto response → JSON conversion ─────────────────────────────

fn point_id_to_json(id: &qdrant::PointId) -> serde_json::Value {
    match &id.point_id_options {
        Some(qdrant::point_id::PointIdOptions::Num(n)) => serde_json::json!(*n),
        Some(qdrant::point_id::PointIdOptions::Uuid(s)) => serde_json::json!(s),
        None => serde_json::Value::Null,
    }
}

fn group_id_to_json(id: &qdrant::GroupId) -> serde_json::Value {
    match &id.kind {
        Some(qdrant::group_id::Kind::UnsignedValue(n)) => serde_json::json!(*n),
        Some(qdrant::group_id::Kind::IntegerValue(i)) => serde_json::json!(*i),
        Some(qdrant::group_id::Kind::StringValue(s)) => serde_json::json!(s),
        None => serde_json::Value::Null,
    }
}

fn qdrant_value_to_json(v: &qdrant::Value) -> serde_json::Value {
    use qdrant::value::Kind;
    match &v.kind {
        None | Some(Kind::NullValue(_)) => serde_json::Value::Null,
        Some(Kind::DoubleValue(d)) => serde_json::json!(*d),
        Some(Kind::IntegerValue(i)) => serde_json::json!(*i),
        Some(Kind::StringValue(s)) => serde_json::json!(s),
        Some(Kind::BoolValue(b)) => serde_json::json!(*b),
        Some(Kind::ListValue(l)) => {
            serde_json::Value::Array(l.values.iter().map(qdrant_value_to_json).collect())
        }
        Some(Kind::StructValue(s)) => serde_json::Value::Object(
            s.fields
                .iter()
                .map(|(k, v)| (k.clone(), qdrant_value_to_json(v)))
                .collect(),
        ),
    }
}

fn vector_output_to_json(vo: &qdrant::VectorOutput) -> serde_json::Value {
    use qdrant::vector_output;
    match &vo.vector {
        Some(vector_output::Vector::Dense(d)) => {
            serde_json::Value::Array(d.data.iter().map(|f| serde_json::json!(*f)).collect())
        }
        Some(vector_output::Vector::Sparse(s)) => serde_json::json!({
            "indices": s.indices,
            "values": s.values,
        }),
        Some(vector_output::Vector::MultiDense(m)) => serde_json::Value::Array(
            m.vectors
                .iter()
                .map(|d| {
                    serde_json::Value::Array(d.data.iter().map(|f| serde_json::json!(*f)).collect())
                })
                .collect(),
        ),
        None => serde_json::Value::Null,
    }
}

fn vectors_output_to_json(v: &qdrant::VectorsOutput) -> serde_json::Value {
    use qdrant::vectors_output::VectorsOptions;
    match &v.vectors_options {
        Some(VectorsOptions::Vector(vo)) => vector_output_to_json(vo),
        Some(VectorsOptions::Vectors(named)) => {
            let mut map = serde_json::Map::new();
            for (name, vec) in &named.vectors {
                map.insert(name.clone(), vector_output_to_json(vec));
            }
            serde_json::Value::Object(map)
        }
        None => serde_json::Value::Null,
    }
}

fn scored_point_to_json(p: qdrant::ScoredPoint) -> serde_json::Value {
    let id =
        p.id.as_ref()
            .map_or(serde_json::Value::Null, point_id_to_json);
    let payload = serde_json::Value::Object(
        p.payload
            .into_iter()
            .map(|(k, v)| (k, qdrant_value_to_json(&v)))
            .collect(),
    );
    let mut obj = serde_json::Map::new();
    obj.insert("id".into(), id);
    obj.insert("score".into(), serde_json::json!(p.score));
    obj.insert("payload".into(), payload);
    if p.version != 0 {
        obj.insert("version".into(), serde_json::json!(p.version));
    }
    if let Some(vectors) = &p.vectors {
        obj.insert("vector".into(), vectors_output_to_json(vectors));
    }
    serde_json::Value::Object(obj)
}

fn retrieved_point_to_json(p: qdrant::RetrievedPoint) -> serde_json::Value {
    let id =
        p.id.as_ref()
            .map_or(serde_json::Value::Null, point_id_to_json);
    let payload = serde_json::Value::Object(
        p.payload
            .into_iter()
            .map(|(k, v)| (k, qdrant_value_to_json(&v)))
            .collect(),
    );
    let mut obj = serde_json::Map::new();
    obj.insert("id".into(), id);
    obj.insert("payload".into(), payload);
    if let Some(vectors) = &p.vectors {
        obj.insert("vector".into(), vectors_output_to_json(vectors));
    }
    serde_json::Value::Object(obj)
}

fn groups_result_to_json(r: qdrant::GroupsResult) -> serde_json::Value {
    serde_json::json!({
        "groups": r.groups.into_iter().map(point_group_to_json).collect::<Vec<_>>(),
    })
}

fn batch_result_to_json(r: qdrant::BatchResult) -> serde_json::Value {
    let points: Vec<_> = r.result.into_iter().map(scored_point_to_json).collect();
    serde_json::json!({ "result": { "points": points } })
}

fn point_group_to_json(g: qdrant::PointGroup) -> serde_json::Value {
    let hits: Vec<_> = g.hits.into_iter().map(scored_point_to_json).collect();
    let id =
        g.id.as_ref()
            .map_or(serde_json::Value::Null, group_id_to_json);
    let mut obj = serde_json::Map::new();
    obj.insert("id".into(), id);
    obj.insert("hits".into(), serde_json::json!(hits));
    if let Some(lookup) = g.lookup {
        obj.insert("lookup".into(), retrieved_point_to_json(lookup));
    }
    serde_json::Value::Object(obj)
}

fn list_collections_response_to_json(resp: qdrant::ListCollectionsResponse) -> serde_json::Value {
    serde_json::json!({
        "result": {
            "collections": resp.collections.into_iter()
                .map(|c| serde_json::json!({"name": c.name}))
                .collect::<Vec<_>>(),
        },
        "status": "ok",
        "time": resp.time,
    })
}

fn collection_info_to_json(resp: qdrant::GetCollectionInfoResponse) -> serde_json::Value {
    let info = resp.result.map(|info| {
        let mut obj = serde_json::Map::new();
        obj.insert("status".into(), serde_json::json!(info.status));
        if let Some(os) = info.optimizer_status {
            obj.insert("optimizer_status".into(), serde_json::json!(os.ok));
        }
        obj.insert(
            "segments_count".into(),
            serde_json::json!(info.segments_count),
        );
        if let Some(pc) = info.points_count {
            obj.insert("points_count".into(), serde_json::json!(pc));
        }
        if let Some(ivc) = info.indexed_vectors_count {
            obj.insert("indexed_vectors_count".into(), serde_json::json!(ivc));
        }
        if let Some(cfg) = info.config {
            obj.insert("config".into(), collection_config_to_json(&cfg));
        }
        if !info.payload_schema.is_empty() {
            let schema: serde_json::Map<_, _> = info
                .payload_schema
                .iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        serde_json::json!({
                            "data_type": v.data_type,
                            "points": v.points,
                        }),
                    )
                })
                .collect();
            obj.insert("payload_schema".into(), serde_json::Value::Object(schema));
        }
        serde_json::Value::Object(obj)
    });
    serde_json::json!({
        "result": info,
        "status": "ok",
        "time": resp.time,
    })
}

fn collection_config_to_json(c: &qdrant::CollectionConfig) -> serde_json::Value {
    let mut obj = serde_json::Map::new();
    if let Some(params) = &c.params {
        let mut p = serde_json::Map::new();
        p.insert(
            "shard_number".into(),
            serde_json::json!(params.shard_number),
        );
        p.insert(
            "on_disk_payload".into(),
            serde_json::json!(params.on_disk_payload),
        );
        if let Some(vc) = &params.vectors_config {
            p.insert("vectors".into(), vectors_config_to_json(vc));
        }
        if let Some(rf) = params.replication_factor {
            p.insert("replication_factor".into(), serde_json::json!(rf));
        }
        if let Some(wcf) = params.write_consistency_factor {
            p.insert("write_consistency_factor".into(), serde_json::json!(wcf));
        }
        if let Some(rff) = params.read_fan_out_factor {
            p.insert("read_fan_out_factor".into(), serde_json::json!(rff));
        }
        if let Some(svc) = &params.sparse_vectors_config {
            let map: serde_json::Map<_, _> = svc
                .map
                .iter()
                .map(|(k, v)| {
                    let mut entry = serde_json::Map::new();
                    if let Some(sidx) = &v.index {
                        entry.insert(
                            "index".into(),
                            serde_json::json!({
                                "on_disk": sidx.on_disk,
                            }),
                        );
                    }
                    (k.clone(), serde_json::Value::Object(entry))
                })
                .collect();
            p.insert("sparse_vectors".into(), serde_json::Value::Object(map));
        }
        obj.insert("params".into(), serde_json::Value::Object(p));
    }
    if let Some(hnsw) = &c.hnsw_config {
        obj.insert(
            "hnsw_config".into(),
            serde_json::json!({
                "m": hnsw.m,
                "ef_construct": hnsw.ef_construct,
                "full_scan_threshold": hnsw.full_scan_threshold,
                "max_indexing_threads": hnsw.max_indexing_threads,
                "on_disk": hnsw.on_disk,
                "payload_m": hnsw.payload_m,
            }),
        );
    }
    if let Some(opt) = &c.optimizer_config {
        let max_threads = opt
            .max_optimization_threads
            .as_ref()
            .map(|m| match &m.variant {
                Some(qdrant::max_optimization_threads::Variant::Value(n)) => {
                    serde_json::json!(*n)
                }
                Some(qdrant::max_optimization_threads::Variant::Setting(_)) => {
                    serde_json::json!("auto")
                }
                None => serde_json::Value::Null,
            });
        obj.insert(
            "optimizer_config".into(),
            serde_json::json!({
                "deleted_threshold": opt.deleted_threshold,
                "vacuum_min_vector_number": opt.vacuum_min_vector_number,
                "default_segment_number": opt.default_segment_number,
                "max_segment_size": opt.max_segment_size,
                "memmap_threshold": opt.memmap_threshold,
                "indexing_threshold": opt.indexing_threshold,
                "flush_interval_sec": opt.flush_interval_sec,
                "max_optimization_threads": max_threads,
            }),
        );
    }
    if let Some(wal) = &c.wal_config {
        obj.insert(
            "wal_config".into(),
            serde_json::json!({
                "wal_capacity_mb": wal.wal_capacity_mb,
                "wal_segments_ahead": wal.wal_segments_ahead,
            }),
        );
    }
    if let Some(qc) = &c.quantization_config {
        obj.insert(
            "quantization_config".into(),
            quantization_config_to_json(qc),
        );
    }
    if let Some(sm) = &c.strict_mode_config {
        obj.insert(
            "strict_mode_config".into(),
            serde_json::json!({
                "enabled": sm.enabled,
                "max_collection_vector_size_bytes": sm.max_collection_vector_size_bytes,
                "read_rate_limit": sm.read_rate_limit,
                "write_rate_limit": sm.write_rate_limit,
                "max_query_limit": sm.max_query_limit,
            }),
        );
    }
    serde_json::Value::Object(obj)
}

fn quantization_config_to_json(qc: &qdrant::QuantizationConfig) -> serde_json::Value {
    use qdrant::quantization_config::Quantization;
    let mut obj = serde_json::Map::new();
    match &qc.quantization {
        Some(Quantization::Scalar(s)) => {
            obj.insert(
                "scalar".into(),
                serde_json::json!({
                    "r#type": s.r#type,
                    "quantile": s.quantile,
                    "always_ram": s.always_ram,
                }),
            );
        }
        Some(Quantization::Product(p)) => {
            obj.insert(
                "product".into(),
                serde_json::json!({
                    "compression": p.compression,
                    "always_ram": p.always_ram,
                }),
            );
        }
        Some(Quantization::Binary(b)) => {
            obj.insert(
                "binary".into(),
                serde_json::json!({
                    "always_ram": b.always_ram,
                }),
            );
        }
        Some(Quantization::Turboquant(_)) => {}
        None => {}
    }
    serde_json::Value::Object(obj)
}

fn vectors_config_to_json(vc: &qdrant::VectorsConfig) -> serde_json::Value {
    use qdrant::vectors_config::Config;
    match &vc.config {
        Some(Config::Params(p)) => vector_params_to_json(p),
        Some(Config::ParamsMap(pm)) => {
            let map: serde_json::Map<_, _> = pm
                .map
                .iter()
                .map(|(k, v)| (k.clone(), vector_params_to_json(v)))
                .collect();
            serde_json::Value::Object(map)
        }
        None => serde_json::json!({}),
    }
}

fn vector_params_to_json(vp: &qdrant::VectorParams) -> serde_json::Value {
    let mut obj = serde_json::Map::new();
    obj.insert("size".into(), serde_json::json!(vp.size));
    obj.insert(
        "distance".into(),
        serde_json::json!(distance_to_str(vp.distance)),
    );
    if let Some(od) = vp.on_disk {
        obj.insert("on_disk".into(), serde_json::json!(od));
    }
    if let Some(hnsw) = &vp.hnsw_config {
        obj.insert(
            "hnsw_config".into(),
            serde_json::json!({
                "m": hnsw.m,
                "ef_construct": hnsw.ef_construct,
                "full_scan_threshold": hnsw.full_scan_threshold,
                "max_indexing_threads": hnsw.max_indexing_threads,
                "on_disk": hnsw.on_disk,
                "payload_m": hnsw.payload_m,
            }),
        );
    }
    if let Some(qc) = &vp.quantization_config {
        obj.insert(
            "quantization_config".into(),
            quantization_config_to_json(qc),
        );
    }
    if let Some(mv) = &vp.multivector_config {
        obj.insert(
            "multivector_config".into(),
            serde_json::json!({
                "comparator": multivec_comp_to_str(mv.comparator),
            }),
        );
    }
    serde_json::Value::Object(obj)
}

fn distance_to_str(d: i32) -> &'static str {
    match d {
        1 => "Cosine",
        2 => "Euclid",
        3 => "Dot",
        4 => "Manhattan",
        _ => "UnknownDistance",
    }
}

fn multivec_comp_to_str(c: i32) -> &'static str {
    match c {
        0 => "MaxSim",
        _ => "MaxSim",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qql_core::parser::Parser;
    use qql_plan::routing::route;

    #[test]
    fn test_grpc_route_conversion_all_statements() {
        let statements = [
            "QUERY 'search' FROM docs USING dense LIMIT 10;",
            "QUERY POINTS (1, 2, 'uuid-str') FROM docs WITH PAYLOAD INCLUDE ('title');",
            "SCROLL FROM docs WHERE status = 'active' LIMIT 50;",
            "UPSERT INTO docs VALUES {id: 1, text: 'hello', category: 'tech'} USING DENSE MODEL 'm';",
            "DELETE FROM docs WHERE category = 'old';",
            "UPDATE docs SET VECTOR dense = [0.1, 0.2] WHERE id = 1;",
            "UPDATE docs SET PAYLOAD = {status: 'ok'} WHERE id = 1;",
            "CREATE COLLECTION docs (dense VECTOR(384, COSINE), sparse SPARSE);",
            "ALTER COLLECTION docs WITH HNSW (m = 16);",
            "DROP COLLECTION docs;",
            "CREATE INDEX ON COLLECTION docs FOR title TYPE text;",
            "SHOW COLLECTIONS;",
            "SHOW COLLECTION docs;",
        ];

        for stmt_str in statements {
            let stmt = Parser::parse(stmt_str)
                .unwrap_or_else(|e| panic!("parse failed for {stmt_str}: {e}"));
            let r = route(&stmt);
            match &r.body {
                Some(RequestBody::Query(req)) => {
                    let grpc_req = to_query_points(req, "docs");
                    assert!(
                        grpc_req.is_ok(),
                        "to_query_points failed for {stmt_str}: {:?}",
                        grpc_req.err()
                    );
                }
                Some(RequestBody::QueryGroups(req)) => {
                    let grpc_req = to_query_groups(req, "docs");
                    assert!(
                        grpc_req.is_ok(),
                        "to_query_groups failed for {stmt_str}: {:?}",
                        grpc_req.err()
                    );
                }
                Some(RequestBody::Points(req)) => {
                    assert_eq!(req.ids.len(), 3);
                }
                Some(RequestBody::Scroll(req)) => {
                    assert!(req.filter.is_some());
                }
                Some(RequestBody::Upsert(req)) => {
                    assert_eq!(req.points.len(), 1);
                }
                Some(RequestBody::Delete(req)) => {
                    assert!(req.filter.is_some());
                }
                Some(RequestBody::UpdateVector(_)) => {}
                Some(RequestBody::UpdatePayload(_)) => {}
                Some(RequestBody::CreateCollection(req)) => {
                    assert!(req.vectors.is_some() || req.hnsw_config.is_some());
                }
                Some(RequestBody::UpdateCollection(_)) => {}
                Some(RequestBody::CreateIndex(req)) => {
                    assert_eq!(req.field_name, "title");
                }
                Some(RequestBody::ClearPayload(_)) => {}
                Some(RequestBody::DeleteVector(_)) => {}
                Some(RequestBody::Count(_)) => {}
                Some(RequestBody::CreateShardKey(_)) => {}
                Some(RequestBody::DropShardKey(_)) => {}
                None => {}
            }
        }
    }
}
