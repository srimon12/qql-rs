//! Decode collection DDL, index, shard-key, and quota request bodies.

use serde_json::Value;

use crate::ConvertError;
use crate::decode::DecodeCtx;
use crate::decode::vector;
use crate::decode::{config, quantization};
use crate::json::{self, child, invalid};
use qql_core::ast::{
    AlterCollectionStmt, CollectionConfig, CollectionMode, CreateCollectionStmt,
    CreateShardKeyStmt, DropShardKeyStmt, SetQuotaStmt, SparseVectorDiff, Value as AstValue,
    VectorDiff,
};

/// Name QQL assigns to a collection's single unnamed vector definition.
pub(crate) const DEFAULT_DENSE_VECTOR: &str = "dense";

/// Decode a `PUT /collections/{c}` (`CreateCollection`) body.
pub(crate) fn create_collection(
    body: &Value,
    ctx: DecodeCtx<'_>,
) -> Result<CreateCollectionStmt, ConvertError> {
    ctx.opts.reject_wait()?;
    ctx.opts.reject_read()?;
    let path = "body";
    let obj = json::object(body, path)?;
    json::reject_unknown(
        obj,
        path,
        &[
            "vectors",
            "sparse_vectors",
            "shard_number",
            "sharding_method",
            "replication_factor",
            "write_consistency_factor",
            "on_disk_payload",
            "payload",
            "hnsw_config",
            "optimizers_config",
            "quantization_config",
            "wal_config",
            "strict_mode_config",
            "metadata",
            "read_fan_out_factor",
            "read_fan_out_delay_ms",
        ],
    )?;
    for key in [
        "wal_config",
        "strict_mode_config",
        "metadata",
        "read_fan_out_factor",
        "read_fan_out_delay_ms",
    ] {
        if obj.contains_key(key) {
            return Err(invalid(
                child(path, key),
                format!("{key} has no QQL representation"),
            ));
        }
    }

    let vectors = decode_vectors(obj.get("vectors"), &child(path, "vectors"))?;
    let sparse_vectors =
        decode_sparse_vectors(obj.get("sparse_vectors"), &child(path, "sparse_vectors"))?;

    let mut config = CollectionConfig {
        vectors: None,
        hnsw: None,
        optimizers: None,
        params: None,
        quantization: None,
        quantization_update: None,
        vector_diffs: Vec::new(),
        sparse_vector_diffs: Vec::new(),
    };
    if let Some(value) = obj.get("hnsw_config").filter(|v| !v.is_null()) {
        config.hnsw = Some(Box::new(config::hnsw(value, &child(path, "hnsw_config"))?));
    }
    if let Some(value) = obj.get("optimizers_config").filter(|v| !v.is_null()) {
        config.optimizers = Some(Box::new(config::optimizers(
            value,
            &child(path, "optimizers_config"),
        )?));
    }
    if let Some(value) = obj.get("quantization_config").filter(|v| !v.is_null()) {
        config.quantization = Some(Box::new(quantization::quantization(
            value,
            &child(path, "quantization_config"),
        )?));
    }
    let params = config::create_params(obj, path)?;
    if params_used(&params) {
        config.params = Some(Box::new(params));
    }
    let config = config_used(&config).then(|| Box::new(config));

    Ok(CreateCollectionStmt {
        collection: ctx.collection.to_string(),
        mode: CollectionMode::Dense { model: None },
        vectors,
        sparse_vectors,
        config,
    })
}

/// Decode the `vectors` field: single unnamed params or a named map.
fn decode_vectors(
    value: Option<&Value>,
    path: &str,
) -> Result<Vec<qql_core::ast::VectorDef>, ConvertError> {
    let Some(value) = value.filter(|v| !v.is_null()) else {
        return Ok(Vec::new());
    };
    let obj = json::object(value, path)?;
    if obj.contains_key("size") {
        return Ok(vec![config::vector_def(DEFAULT_DENSE_VECTOR, value, path)?]);
    }
    obj.iter()
        .map(|(name, params)| config::vector_def(name, params, &child(path, name)))
        .collect()
}

/// Decode the `sparse_vectors` field (named map).
fn decode_sparse_vectors(
    value: Option<&Value>,
    path: &str,
) -> Result<Vec<qql_core::ast::SparseVectorDef>, ConvertError> {
    let Some(value) = value.filter(|v| !v.is_null()) else {
        return Ok(Vec::new());
    };
    let obj = json::object(value, path)?;
    obj.iter()
        .map(|(name, params)| config::sparse_vector_def(name, params, &child(path, name)))
        .collect()
}

/// Whether any create-time collection param was set.
fn params_used(params: &qql_core::ast::CollectionParamsConfig) -> bool {
    params.replication_factor.is_some()
        || params.write_consistency_factor.is_some()
        || params.on_disk_payload.is_some()
        || params.payload_memory.is_some()
        || params.shard_number.is_some()
        || params.sharding_method.is_some()
}

/// Whether any collection config block was set.
fn config_used(config: &CollectionConfig) -> bool {
    config.vectors.is_some()
        || config.hnsw.is_some()
        || config.optimizers.is_some()
        || config.params.is_some()
        || config.quantization.is_some()
}

/// Decode a `PATCH /collections/{c}` (`UpdateCollection`) body.
pub(crate) fn alter_collection(
    body: &Value,
    ctx: DecodeCtx<'_>,
) -> Result<AlterCollectionStmt, ConvertError> {
    ctx.opts.reject_wait()?;
    ctx.opts.reject_read()?;
    let path = "body";
    let obj = json::object(body, path)?;
    json::reject_unknown(
        obj,
        path,
        &[
            "vectors",
            "optimizers_config",
            "params",
            "hnsw_config",
            "quantization_config",
            "sparse_vectors",
            "strict_mode_config",
            "metadata",
        ],
    )?;
    for key in ["strict_mode_config", "metadata"] {
        if obj.contains_key(key) {
            return Err(invalid(
                child(path, key),
                format!("{key} has no QQL representation"),
            ));
        }
    }

    let mut config = CollectionConfig {
        vectors: None,
        hnsw: None,
        optimizers: None,
        params: None,
        quantization: None,
        quantization_update: None,
        vector_diffs: Vec::new(),
        sparse_vector_diffs: Vec::new(),
    };
    if let Some(value) = obj.get("hnsw_config").filter(|v| !v.is_null()) {
        config.hnsw = Some(Box::new(config::hnsw(value, &child(path, "hnsw_config"))?));
    }
    if let Some(value) = obj.get("optimizers_config").filter(|v| !v.is_null()) {
        config.optimizers = Some(Box::new(config::optimizers(
            value,
            &child(path, "optimizers_config"),
        )?));
    }
    if let Some(value) = obj.get("params").filter(|v| !v.is_null()) {
        config.params = Some(Box::new(config::update_params(
            value,
            &child(path, "params"),
        )?));
    }
    if let Some(value) = obj.get("quantization_config").filter(|v| !v.is_null()) {
        config.quantization_update = Some(Box::new(quantization::quantization_update(
            value,
            &child(path, "quantization_config"),
        )?));
    }
    config.vector_diffs = decode_vector_diffs(obj.get("vectors"), &child(path, "vectors"))?;
    config.sparse_vector_diffs =
        decode_sparse_diffs(obj.get("sparse_vectors"), &child(path, "sparse_vectors"))?;

    let config = (!config_all_empty(&config)).then(|| Box::new(config));
    Ok(AlterCollectionStmt {
        collection: ctx.collection.to_string(),
        config,
    })
}

/// Whether every alter-config block is unset.
fn config_all_empty(config: &CollectionConfig) -> bool {
    config.hnsw.is_none()
        && config.optimizers.is_none()
        && config.params.is_none()
        && config.quantization_update.is_none()
        && config.vector_diffs.is_empty()
        && config.sparse_vector_diffs.is_empty()
}

/// Decode `VectorsConfigDiff` into per-vector dense diffs.
fn decode_vector_diffs(value: Option<&Value>, path: &str) -> Result<Vec<VectorDiff>, ConvertError> {
    let Some(value) = value.filter(|v| !v.is_null()) else {
        return Ok(Vec::new());
    };
    let obj = json::object(value, path)?;
    obj.iter()
        .map(|(name, diff)| {
            let diff_path = child(path, name);
            let diff_obj = json::object(diff, &diff_path)?;
            for key in diff_obj.keys() {
                match key.as_str() {
                    "hnsw_config" | "quantization_config" | "on_disk" | "memory" | "datatype" => {}
                    other => {
                        return Err(invalid(
                            child(&diff_path, other),
                            "unknown VectorParamsDiff field",
                        ));
                    }
                }
            }
            if diff_obj.contains_key("datatype") {
                return Err(invalid(
                    child(&diff_path, "datatype"),
                    "VectorParamsDiff has no datatype field; datatype cannot be changed",
                ));
            }
            let storage = config::storage_fields(diff_obj, &diff_path, false)?;
            let has_storage =
                storage.on_disk.is_some() || storage.memory.is_some() || storage.datatype.is_some();
            Ok(VectorDiff {
                name: name.clone(),
                hnsw: match diff_obj.get("hnsw_config").filter(|v| !v.is_null()) {
                    None => None,
                    Some(value) => Some(Box::new(config::hnsw(
                        value,
                        &child(&diff_path, "hnsw_config"),
                    )?)),
                },
                quantization: match diff_obj.get("quantization_config").filter(|v| !v.is_null()) {
                    None => None,
                    Some(value) => Some(Box::new(quantization::quantization_update(
                        value,
                        &child(&diff_path, "quantization_config"),
                    )?)),
                },
                vectors: has_storage.then(|| Box::new(storage)),
            })
        })
        .collect()
}

/// Decode `SparseVectorsConfig` into per-sparse-vector diffs.
fn decode_sparse_diffs(
    value: Option<&Value>,
    path: &str,
) -> Result<Vec<SparseVectorDiff>, ConvertError> {
    let Some(value) = value.filter(|v| !v.is_null()) else {
        return Ok(Vec::new());
    };
    let obj = json::object(value, path)?;
    obj.iter()
        .map(|(name, diff)| {
            let diff_path = child(path, name);
            let diff_obj = json::object(diff, &diff_path)?;
            let def =
                config::sparse_vector_def(name, &Value::Object(diff_obj.clone()), &diff_path)?;
            Ok(SparseVectorDiff {
                name: name.clone(),
                index: def.index,
                modifier: def.modifier,
            })
        })
        .collect()
}

/// Decode a `PUT /collections/{c}/shards` (`CreateShardingKey`) body.
pub(crate) fn create_shard_key(
    body: &Value,
    ctx: DecodeCtx<'_>,
) -> Result<CreateShardKeyStmt, ConvertError> {
    ctx.opts.reject_wait()?;
    ctx.opts.reject_read()?;
    let path = "body";
    let obj = json::object(body, path)?;
    json::reject_unknown(
        obj,
        path,
        &[
            "shard_key",
            "shards_number",
            "replication_factor",
            "placement",
            "initial_state",
        ],
    )?;
    for key in ["placement", "initial_state"] {
        if obj.contains_key(key) {
            return Err(invalid(
                child(path, key),
                format!("{key} has no QQL representation"),
            ));
        }
    }
    let shard_key = vector::shard_key(
        json::required(obj, "shard_key", path)?,
        &child(path, "shard_key"),
    )?;
    Ok(CreateShardKeyStmt {
        collection: ctx.collection.to_string(),
        shard_key,
        shards_number: json::opt_u64(obj, "shards_number", path)?,
        replication_factor: json::opt_u64(obj, "replication_factor", path)?,
    })
}

/// Decode a `POST /collections/{c}/shards/delete` (`DropShardingKey`) body.
pub(crate) fn drop_shard_key(
    body: &Value,
    ctx: DecodeCtx<'_>,
) -> Result<DropShardKeyStmt, ConvertError> {
    ctx.opts.reject_wait()?;
    ctx.opts.reject_read()?;
    let path = "body";
    let obj = json::object(body, path)?;
    json::reject_unknown(obj, path, &["shard_key"])?;
    Ok(DropShardKeyStmt {
        collection: ctx.collection.to_string(),
        shard_key: vector::shard_key(
            json::required(obj, "shard_key", path)?,
            &child(path, "shard_key"),
        )?,
    })
}

/// Decode a `PUT /quotas` (`QuotaConfig`) body into `SET QUOTA`.
pub(crate) fn set_quota(body: &Value, ctx: DecodeCtx<'_>) -> Result<SetQuotaStmt, ConvertError> {
    ctx.opts.reject_read()?;
    let path = "body";
    let obj = json::object(body, path)?;
    let mut config = Vec::new();
    for (key, value) in obj {
        match key.as_str() {
            "enabled" => config.push((
                key.clone(),
                AstValue::Bool(json::bool_at(value, &child(path, key))?),
            )),
            "max_resident_memory_percent" | "max_disk_usage_percent" => {
                let n = json::u64_at(value, &child(path, key))?;
                if !(1..=100).contains(&n) {
                    return Err(invalid(
                        child(path, key),
                        "quota percent must be in [1, 100]",
                    ));
                }
                config.push((
                    key.clone(),
                    AstValue::Int(i64::try_from(n).map_err(|_| {
                        invalid(child(path, key), "value exceeds the signed 64-bit range")
                    })?),
                ));
            }
            "release_margin_percent" => {
                let n = json::u64_at(value, &child(path, key))?;
                if n > 100 {
                    return Err(invalid(
                        child(path, key),
                        "release margin percent must be in [0, 100]",
                    ));
                }
                config.push((
                    key.clone(),
                    AstValue::Int(i64::try_from(n).map_err(|_| {
                        invalid(child(path, key), "value exceeds the signed 64-bit range")
                    })?),
                ));
            }
            other => {
                return Err(invalid(child(path, other), "unknown quota parameter"));
            }
        }
    }
    // Same feature-unification trap as CREATE INDEX options: sort so the
    // formatter does not inherit `serde_json` map iteration order.
    config.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(SetQuotaStmt {
        config,
        wait: ctx.opts.wait,
    })
}
