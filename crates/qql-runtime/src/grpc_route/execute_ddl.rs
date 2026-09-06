//! DDL execution: collections / indexes / shard keys.

use qql_core::error::QqlError;

use crate::grpc::GrpcQdrant;
use crate::grpc::memory::memory_from_str;
use crate::qdrant_grpc::qdrant;

use super::ddl::{
    collection_params_diff, hnsw_config_from_plan, optimizers_config_from_plan,
    payload_index_params, quantization_config_diff, quantization_config_from_plan,
    sparse_vector_params, vector_params,
};
use super::responses::{
    collection_info_to_json, collection_mutation_response, list_collections_response_to_json,
    mutation_response_from, mutation_response_ok,
};

/// Create a collection, then apply deferred params and shard keys.
pub(crate) async fn execute_create_collection(
    client: &GrpcQdrant,
    collection: &str,
    request: &qql_plan::types::CreateCollectionRequest,
) -> Result<serde_json::Value, QqlError> {
    let deferred_params = request
        .params
        .as_ref()
        .map(collection_params_diff)
        .filter(|params| {
            params.read_fan_out_factor.is_some() || params.read_fan_out_delay_ms.is_some()
        });
    let grpc_req = qdrant::CreateCollection {
        collection_name: collection.to_owned(),
        vectors_config: request.vectors.as_ref().map(|v| {
            let map = v
                .iter()
                .map(|(name, cfg)| (name.clone(), vector_params(cfg)))
                .collect();
            qdrant::VectorsConfig {
                config: Some(qdrant::vectors_config::Config::ParamsMap(
                    qdrant::VectorParamsMap { map },
                )),
            }
        }),
        sparse_vectors_config: request.sparse_vectors.as_ref().map(|sv| {
            let map = sv
                .iter()
                .map(|(name, cfg)| (name.clone(), sparse_vector_params(cfg)))
                .collect();
            qdrant::SparseVectorConfig { map }
        }),
        hnsw_config: request.hnsw_config.as_ref().map(hnsw_config_from_plan),
        optimizers_config: request
            .optimizers_config
            .as_ref()
            .map(optimizers_config_from_plan),
        shard_number: request
            .shard_number
            .or_else(|| {
                request
                    .params
                    .as_ref()
                    .and_then(|p| p.get("shard_number"))
                    .and_then(|v| v.as_u64())
            })
            .map(|n| n as u32),
        replication_factor: request
            .params
            .as_ref()
            .and_then(|p| p.get("replication_factor"))
            .and_then(|v| v.as_u64())
            .map(|n| n as u32),
        on_disk_payload: request
            .params
            .as_ref()
            .and_then(|p| p.get("on_disk_payload"))
            .and_then(|v| v.as_bool()),
        payload: request.payload.as_ref().and_then(|p| {
            p.get("memory")
                .and_then(serde_json::Value::as_str)
                .and_then(memory_from_str)
                .map(|memory| qdrant::PayloadStorageParams {
                    memory: Some(memory),
                })
        }),
        write_consistency_factor: request
            .params
            .as_ref()
            .and_then(|p| p.get("write_consistency_factor"))
            .and_then(serde_json::Value::as_u64)
            .map(|n| n as u32),
        quantization_config: request
            .quantization_config
            .as_ref()
            .and_then(quantization_config_from_plan),
        sharding_method: request.sharding_method.as_ref().map(|method| {
            match method.to_ascii_lowercase().as_str() {
                "custom" => qdrant::ShardingMethod::Custom as i32,
                _ => qdrant::ShardingMethod::Auto as i32,
            }
        }),
        ..Default::default()
    };
    let resp = client
        .create_collection_raw(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("create_collection: {e}"), None))?;
    if let Some(params) = deferred_params {
        client
            .update_collection_raw(qdrant::UpdateCollection {
                collection_name: collection.to_owned(),
                params: Some(params),
                ..Default::default()
            })
            .await
            .map_err(|e| {
                QqlError::backend("QQL-GRPC", format!("update_collection_params: {e}"), None)
            })?;
    }
    if let Some(shard_keys) = &request.shard_keys {
        for shard_key in shard_keys {
            client
                .create_shard_key(qdrant::CreateShardKeyRequest {
                    collection_name: collection.to_owned(),
                    request: Some(qdrant::CreateShardKey {
                        shard_key: Some(qdrant::ShardKey {
                            key: Some(qdrant::shard_key::Key::Keyword(shard_key.clone())),
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                })
                .await
                .map_err(|e| {
                    QqlError::backend(
                        "QQL-GRPC",
                        format!("create_shard_key {shard_key}: {e}"),
                        None,
                    )
                })?;
        }
    }
    Ok(collection_mutation_response(resp))
}

/// Patch collection params / HNSW / quantization.
pub(crate) async fn execute_update_collection(
    client: &GrpcQdrant,
    collection: &str,
    request: &qql_plan::types::UpdateCollectionRequest,
) -> Result<serde_json::Value, QqlError> {
    let grpc_req = qdrant::UpdateCollection {
        collection_name: collection.to_owned(),
        optimizers_config: request
            .optimizers_config
            .as_ref()
            .map(optimizers_config_from_plan),
        params: request.params.as_ref().map(collection_params_diff),
        hnsw_config: request.hnsw_config.as_ref().map(hnsw_config_from_plan),
        quantization_config: request
            .quantization_config
            .as_ref()
            .and_then(quantization_config_diff),
        ..Default::default()
    };
    let resp = client
        .update_collection_raw(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("update_collection: {e}"), None))?;
    Ok(collection_mutation_response(resp))
}

/// Drop a collection.
pub(crate) async fn execute_drop_collection(
    client: &GrpcQdrant,
    collection: &str,
) -> Result<serde_json::Value, QqlError> {
    let grpc_req = qdrant::DeleteCollection {
        collection_name: collection.to_owned(),
        ..Default::default()
    };
    let resp = client
        .delete_collection_raw(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("drop_collection: {e}"), None))?;
    Ok(collection_mutation_response(resp))
}

/// Create a payload field index.
pub(crate) async fn execute_create_index(
    client: &GrpcQdrant,
    collection: &str,
    request: &qql_plan::types::CreateIndexRequest,
) -> Result<serde_json::Value, QqlError> {
    let field_type = match request.field_schema.as_str() {
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
        collection_name: collection.to_owned(),
        wait: Some(true),
        field_name: request.field_name.clone(),
        field_type: Some(field_type),
        field_index_params: Some(payload_index_params(&request.field_schema, &request.extra)?),
        ..Default::default()
    };
    let resp = client
        .create_field_index(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("create_index: {e}"), None))?;
    Ok(mutation_response_from(resp))
}

/// Drop a payload field index.
pub(crate) async fn execute_drop_index(
    client: &GrpcQdrant,
    collection: &str,
    field: &str,
) -> Result<serde_json::Value, QqlError> {
    let grpc_req = qdrant::DeleteFieldIndexCollection {
        collection_name: collection.to_owned(),
        field_name: field.to_owned(),
        ..Default::default()
    };
    let resp = client
        .delete_field_index(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("drop_index: {e}"), None))?;
    Ok(mutation_response_from(resp))
}

/// Create a shard key.
pub(crate) async fn execute_create_shard_key(
    client: &GrpcQdrant,
    collection: &str,
    request: &qql_plan::types::CreateShardKeyRequest,
) -> Result<serde_json::Value, QqlError> {
    let grpc_req = qdrant::CreateShardKeyRequest {
        collection_name: collection.to_owned(),
        request: Some(qdrant::CreateShardKey {
            shard_key: Some(qdrant::ShardKey {
                key: Some(qdrant::shard_key::Key::Keyword(request.shard_key.clone())),
            }),
            shards_number: request.shards_number.map(|n| n as u32),
            replication_factor: request.replication_factor.map(|n| n as u32),
            ..Default::default()
        }),
        ..Default::default()
    };
    client
        .create_shard_key(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("create_shard_key: {e}"), None))?;
    Ok(mutation_response_ok())
}

/// Drop a shard key.
pub(crate) async fn execute_drop_shard_key(
    client: &GrpcQdrant,
    collection: &str,
    request: &qql_plan::types::DropShardKeyRequest,
) -> Result<serde_json::Value, QqlError> {
    let grpc_req = qdrant::DeleteShardKeyRequest {
        collection_name: collection.to_owned(),
        request: Some(qdrant::DeleteShardKey {
            shard_key: Some(qdrant::ShardKey {
                key: Some(qdrant::shard_key::Key::Keyword(request.shard_key.clone())),
            }),
        }),
        ..Default::default()
    };
    client
        .delete_shard_key(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("drop_shard_key: {e}"), None))?;
    Ok(mutation_response_ok())
}

/// List collection names.
pub(crate) async fn execute_list_collections(
    client: &GrpcQdrant,
) -> Result<serde_json::Value, QqlError> {
    let resp = client
        .list_collections_raw()
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("list: {e}"), None))?;
    Ok(list_collections_response_to_json(resp))
}

/// Fetch collection info.
pub(crate) async fn execute_get_collection(
    client: &GrpcQdrant,
    collection: &str,
) -> Result<serde_json::Value, QqlError> {
    let resp = client
        .collection_info_raw(collection.to_owned())
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("get_collection: {e}"), None))?;
    Ok(collection_info_to_json(resp))
}

/// List shard keys.
pub(crate) async fn execute_list_shard_keys(
    client: &GrpcQdrant,
    collection: &str,
) -> Result<serde_json::Value, QqlError> {
    let grpc_req = qdrant::ListShardKeysRequest {
        collection_name: collection.to_owned(),
    };
    let resp = client
        .list_shard_keys(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("list_shard_keys: {e}"), None))?;
    let keys: Vec<serde_json::Value> = resp
        .shard_keys
        .into_iter()
        .filter_map(|d| d.key)
        .map(|sk| match sk.key {
            Some(qdrant::shard_key::Key::Keyword(s)) => serde_json::Value::String(s),
            Some(qdrant::shard_key::Key::Number(n)) => serde_json::Value::Number((n).into()),
            None => serde_json::Value::Null,
        })
        .collect();
    Ok(serde_json::json!({
        "result": { "shard_keys": keys },
        "status": "ok",
        "time": 0.0_f64,
    }))
}
