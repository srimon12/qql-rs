//! DDL execution: collections / indexes / shard keys.
//!
//! Every response converts straight into typed `ExecData`: status-only
//! successes become `Mutation { affected: None }`, collection lists become
//! `Collections`, collection metadata becomes `Collection`, and shard-key
//! lists become `ShardKeys`. No JSON envelope is built anywhere.

use qql_core::error::QqlError;

use crate::executor::response::{BackendResponse, ExecData};
use crate::grpc::GrpcQdrant;
use crate::grpc::memory::memory_to_proto;
use crate::grpc::schema::collection_info_from_grpc;
use crate::qdrant_grpc::qdrant;

use super::common::field_type_to_proto;
use super::ddl::{
    collection_params_diff, hnsw_config_from_plan, optimizers_config_from_plan,
    payload_index_params, quantization_config_diff, quantization_config_from_plan,
    sparse_vector_params, u32_param, vector_params,
};
use super::typed::{
    collection_mutation_to_typed, mutation_response_to_typed, shard_key_to_plan,
    telemetry_from_proto,
};

/// Create a collection, then apply deferred params and shard keys.
pub(crate) async fn execute_create_collection(
    client: &GrpcQdrant,
    collection: &str,
    request: &qql_plan::types::CreateCollectionRequest,
) -> Result<BackendResponse, QqlError> {
    let deferred_params = request
        .params
        .as_ref()
        .map(collection_params_diff)
        .transpose()?
        .filter(|params| {
            params.read_fan_out_factor.is_some() || params.read_fan_out_delay_ms.is_some()
        });
    let params = request.params.as_ref();
    let vectors_config = request
        .vectors
        .as_ref()
        .map(|vectors| qdrant::VectorsConfig {
            config: Some(match vectors {
                qql_plan::DenseVectorsConfig::Single(params) => {
                    qdrant::vectors_config::Config::Params(vector_params(params))
                }
                qql_plan::DenseVectorsConfig::Named(map) => {
                    qdrant::vectors_config::Config::ParamsMap(qdrant::VectorParamsMap {
                        map: map
                            .iter()
                            .map(|(name, params)| (name.clone(), vector_params(params)))
                            .collect(),
                    })
                }
            }),
        });
    let sparse_vectors_config =
        request
            .sparse_vectors
            .as_ref()
            .map(|map| qdrant::SparseVectorConfig {
                map: map
                    .iter()
                    .map(|(name, params)| (name.clone(), sparse_vector_params(params)))
                    .collect(),
            });
    let grpc_req = qdrant::CreateCollection {
        collection_name: collection.to_owned(),
        vectors_config,
        sparse_vectors_config,
        hnsw_config: request.hnsw_config.as_ref().map(hnsw_config_from_plan),
        optimizers_config: request
            .optimizers_config
            .as_ref()
            .map(optimizers_config_from_plan),
        shard_number: request
            .shard_number
            .map(|n| u32_param(n, "shard_number"))
            .transpose()?,
        replication_factor: params
            .and_then(|p| p.replication_factor)
            .map(|n| u32_param(n, "replication_factor"))
            .transpose()?,
        on_disk_payload: params.and_then(|p| p.on_disk_payload),
        payload: params
            .and_then(|p| p.payload.as_ref())
            .and_then(|payload| payload.memory)
            .map(|memory| qdrant::PayloadStorageParams {
                memory: Some(memory_to_proto(memory)),
            }),
        write_consistency_factor: params
            .and_then(|p| p.write_consistency_factor)
            .map(|n| u32_param(n, "write_consistency_factor"))
            .transpose()?,
        quantization_config: request
            .quantization_config
            .as_ref()
            .and_then(quantization_config_from_plan),
        sharding_method: request.sharding_method.map(|method| match method {
            qql_plan::ShardingMethod::Custom => qdrant::ShardingMethod::Custom as i32,
            qql_plan::ShardingMethod::Auto => qdrant::ShardingMethod::Auto as i32,
        }),
        wal_config: request
            .wal_config
            .as_ref()
            .map(|w| super::ddl::wal_config_from_plan(w)),
        strict_mode_config: request
            .strict_mode_config
            .as_ref()
            .map(|s| super::ddl::strict_mode_config_from_plan(s))
            .transpose()?,
        metadata: request
            .metadata
            .as_ref()
            .map(|m| {
                m.iter()
                    .map(|(k, v)| {
                        (
                            k.clone(),
                            crate::grpc_route::values::to_qdrant_value(v.clone()),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default(),
        ..Default::default()
    };
    let resp = client
        .create_collection_raw(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("create_collection: {e}"), None))?;
    let time = resp.time;
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
                        shard_key: Some(super::common::shard_key_proto(shard_key)),
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
    Ok(collection_mutation_to_typed(time))
}

/// Patch collection params / HNSW / quantization / per-vector diffs.
pub(crate) async fn execute_update_collection(
    client: &GrpcQdrant,
    collection: &str,
    request: &qql_plan::types::UpdateCollectionRequest,
) -> Result<BackendResponse, QqlError> {
    let grpc_req = qdrant::UpdateCollection {
        collection_name: collection.to_owned(),
        optimizers_config: request
            .optimizers_config
            .as_ref()
            .map(optimizers_config_from_plan),
        params: request
            .params
            .as_ref()
            .map(collection_params_diff)
            .transpose()?,
        hnsw_config: request.hnsw_config.as_ref().map(hnsw_config_from_plan),
        quantization_config: request
            .quantization_config
            .as_ref()
            .and_then(quantization_config_diff),
        vectors_config: request
            .vectors
            .as_ref()
            .map(super::ddl::vectors_config_diff),
        sparse_vectors_config: request
            .sparse_vectors
            .as_ref()
            .map(super::ddl::sparse_vectors_config_diff),
        strict_mode_config: request
            .strict_mode_config
            .as_ref()
            .map(|s| super::ddl::strict_mode_config_from_plan(s))
            .transpose()?,
        metadata: request
            .metadata
            .as_ref()
            .map(|m| {
                m.iter()
                    .map(|(k, v)| {
                        (
                            k.clone(),
                            crate::grpc_route::values::to_qdrant_value(v.clone()),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default(),
        ..Default::default()
    };
    let resp = client
        .update_collection_raw(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("update_collection: {e}"), None))?;
    Ok(collection_mutation_to_typed(resp.time))
}

/// Drop a collection.
pub(crate) async fn execute_drop_collection(
    client: &GrpcQdrant,
    collection: &str,
) -> Result<BackendResponse, QqlError> {
    let grpc_req = qdrant::DeleteCollection {
        collection_name: collection.to_owned(),
        ..Default::default()
    };
    let resp = client
        .delete_collection_raw(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("drop_collection: {e}"), None))?;
    Ok(collection_mutation_to_typed(resp.time))
}

/// Create a payload field index.
pub(crate) async fn execute_create_index(
    client: &GrpcQdrant,
    collection: &str,
    request: &qql_plan::types::CreateIndexRequest,
    wait: bool,
) -> Result<BackendResponse, QqlError> {
    let grpc_req = qdrant::CreateFieldIndexCollection {
        collection_name: collection.to_owned(),
        wait: Some(wait),
        field_name: request.field_name.clone(),
        field_type: Some(field_type_to_proto(request.field_schema)),
        field_index_params: Some(payload_index_params(request)),
        ..Default::default()
    };
    let resp = client
        .create_field_index(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("create_index: {e}"), None))?;
    Ok(mutation_response_to_typed(resp))
}

/// Drop a payload field index.
pub(crate) async fn execute_drop_index(
    client: &GrpcQdrant,
    collection: &str,
    field: &str,
) -> Result<BackendResponse, QqlError> {
    let grpc_req = qdrant::DeleteFieldIndexCollection {
        collection_name: collection.to_owned(),
        field_name: field.to_owned(),
        ..Default::default()
    };
    let resp = client
        .delete_field_index(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("drop_index: {e}"), None))?;
    Ok(mutation_response_to_typed(resp))
}

/// Create a shard key.
pub(crate) async fn execute_create_shard_key(
    client: &GrpcQdrant,
    collection: &str,
    request: &qql_plan::types::CreateShardKeyRequest,
) -> Result<BackendResponse, QqlError> {
    let grpc_req = qdrant::CreateShardKeyRequest {
        collection_name: collection.to_owned(),
        request: Some(qdrant::CreateShardKey {
            shard_key: Some(super::common::shard_key_proto(&request.shard_key)),
            shards_number: request
                .shards_number
                .map(|n| u32_param(n, "shards_number"))
                .transpose()?,
            replication_factor: request
                .replication_factor
                .map(|n| u32_param(n, "replication_factor"))
                .transpose()?,
            ..Default::default()
        }),
        ..Default::default()
    };
    let resp = client
        .create_shard_key(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("create_shard_key: {e}"), None))?;
    Ok(collection_mutation_to_typed(resp.time))
}

/// Drop a shard key.
pub(crate) async fn execute_drop_shard_key(
    client: &GrpcQdrant,
    collection: &str,
    request: &qql_plan::types::DropShardKeyRequest,
) -> Result<BackendResponse, QqlError> {
    let grpc_req = qdrant::DeleteShardKeyRequest {
        collection_name: collection.to_owned(),
        request: Some(qdrant::DeleteShardKey {
            shard_key: Some(super::common::shard_key_proto(&request.shard_key)),
        }),
        ..Default::default()
    };
    let resp = client
        .delete_shard_key(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("drop_shard_key: {e}"), None))?;
    Ok(collection_mutation_to_typed(resp.time))
}

/// List collection names.
pub(crate) async fn execute_list_collections(
    client: &GrpcQdrant,
) -> Result<BackendResponse, QqlError> {
    let resp = client
        .list_collections_raw()
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("list: {e}"), None))?;
    let telemetry = telemetry_from_proto(resp.time, None);
    Ok(BackendResponse {
        data: ExecData::Collections(resp.collections.into_iter().map(|c| c.name).collect()),
        telemetry,
    })
}

/// Fetch collection info.
pub(crate) async fn execute_get_collection(
    client: &GrpcQdrant,
    collection: &str,
) -> Result<BackendResponse, QqlError> {
    let resp = client
        .collection_info_raw(collection.to_owned())
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("get_collection: {e}"), None))?;
    let info = resp.result.ok_or_else(|| {
        QqlError::backend(
            "QQL-GRPC-NO-RESULT",
            "collection_info response missing result field",
            None,
        )
        .with_collection(collection.to_string())
    })?;
    let telemetry = telemetry_from_proto(resp.time, None);
    Ok(BackendResponse {
        data: ExecData::Collection(collection_info_from_grpc(&info)),
        telemetry,
    })
}

/// List shard keys.
pub(crate) async fn execute_list_shard_keys(
    client: &GrpcQdrant,
    collection: &str,
) -> Result<BackendResponse, QqlError> {
    let grpc_req = qdrant::ListShardKeysRequest {
        collection_name: collection.to_owned(),
    };
    let resp = client
        .list_shard_keys(grpc_req)
        .await
        .map_err(|e| QqlError::backend("QQL-GRPC", format!("list_shard_keys: {e}"), None))?;
    let telemetry = telemetry_from_proto(resp.time, None);
    let keys = resp
        .shard_keys
        .into_iter()
        .map(|description| {
            description
                .key
                .as_ref()
                .ok_or_else(|| {
                    QqlError::backend(
                        "QQL-BACKEND-ENVELOPE",
                        "shard key description is missing its key",
                        None,
                    )
                    .with_collection(collection.to_string())
                })
                .and_then(shard_key_to_plan)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(BackendResponse {
        data: ExecData::ShardKeys(keys),
        telemetry,
    })
}
