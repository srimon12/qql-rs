//! Decode collection configuration schemas (vectors, HNSW, optimizers,
//! quantization, sparse vectors, collection params) into AST config blocks.

use serde_json::Value;

use crate::ConvertError;
use crate::decode::quantization::quantization;
use crate::json::{self, child, invalid, type_name};
use qql_core::ast::{
    CollectionParamsConfig, HnswRuntimeConfig, MemoryPlacement, MultivectorComparator,
    MultivectorConfig, OptimizationThreads, OptimizersRuntimeConfig, SparseIndexConfig,
    SparseVectorDef, VectorDatatype, VectorDef, VectorDistance, VectorsConfig,
};

/// Decode a `HnswConfigDiff`.
pub(crate) fn hnsw(value: &Value, path: &str) -> Result<HnswRuntimeConfig, ConvertError> {
    let obj = json::object(value, path)?;
    for key in obj.keys() {
        match key.as_str() {
            "m"
            | "ef_construct"
            | "full_scan_threshold"
            | "max_indexing_threads"
            | "on_disk"
            | "memory"
            | "payload_m"
            | "inline_storage" => {}
            other => return Err(invalid(child(path, other), "unknown HNSW field")),
        }
    }
    Ok(HnswRuntimeConfig {
        m: json::opt_u64(obj, "m", path)?,
        ef_construct: json::opt_u64(obj, "ef_construct", path)?,
        full_scan_threshold: json::opt_u64(obj, "full_scan_threshold", path)?,
        max_indexing_threads: json::opt_u64(obj, "max_indexing_threads", path)?,
        on_disk: json::opt_bool(obj, "on_disk", path)?,
        payload_m: json::opt_u64(obj, "payload_m", path)?,
        inline_storage: json::opt_bool(obj, "inline_storage", path)?,
        memory: memory(obj, "memory", path)?,
    })
}

/// Decode a `Memory` enum member (`cold` / `cached` / `pinned`).
pub(crate) fn memory(
    obj: &json::Obj,
    key: &str,
    path: &str,
) -> Result<Option<MemoryPlacement>, ConvertError> {
    let Some(raw) = json::opt_string(obj, key, path)? else {
        return Ok(None);
    };
    MemoryPlacement::parse(&raw).map(Some).ok_or_else(|| {
        invalid(
            child(path, key),
            format!("unknown memory placement '{raw}' (expected cold, cached, pinned)"),
        )
    })
}

/// Decode an `OptimizersConfigDiff`.
pub(crate) fn optimizers(
    value: &Value,
    path: &str,
) -> Result<OptimizersRuntimeConfig, ConvertError> {
    let obj = json::object(value, path)?;
    for key in obj.keys() {
        match key.as_str() {
            "deleted_threshold"
            | "vacuum_min_vector_number"
            | "default_segment_number"
            | "max_segment_size"
            | "memmap_threshold"
            | "indexing_threshold"
            | "flush_interval_sec"
            | "max_optimization_threads"
            | "prevent_unoptimized" => {}
            other => return Err(invalid(child(path, other), "unknown optimizer field")),
        }
    }
    let threads = match obj.get("max_optimization_threads").filter(|v| !v.is_null()) {
        None => None,
        Some(Value::String(s)) if s.eq_ignore_ascii_case("auto") => Some(OptimizationThreads {
            auto_: true,
            value: 0,
        }),
        Some(Value::Number(_)) => Some(OptimizationThreads {
            auto_: false,
            value: json::u64_at(
                obj.get("max_optimization_threads").expect("value checked"),
                &child(path, "max_optimization_threads"),
            )?,
        }),
        Some(other) => {
            return Err(invalid(
                child(path, "max_optimization_threads"),
                format!(
                    "expected a thread count or \"auto\", got {}",
                    type_name(other)
                ),
            ));
        }
    };
    Ok(OptimizersRuntimeConfig {
        deleted_threshold: json::opt_f64(obj, "deleted_threshold", path)?,
        vacuum_min_vector_number: json::opt_u64(obj, "vacuum_min_vector_number", path)?,
        default_segment_number: json::opt_u64(obj, "default_segment_number", path)?,
        max_segment_size: json::opt_u64(obj, "max_segment_size", path)?,
        memmap_threshold: json::opt_u64(obj, "memmap_threshold", path)?,
        indexing_threshold: json::opt_u64(obj, "indexing_threshold", path)?,
        flush_interval_sec: json::opt_u64(obj, "flush_interval_sec", path)?,
        max_optimization_threads: threads,
        prevent_unoptimized: json::opt_bool(obj, "prevent_unoptimized", path)?,
    })
}

/// Extract `on_disk` / `memory` / `datatype` from an object that may carry
/// additional fields (e.g. the rest of a `VectorParams` or a `VectorParamsDiff`).
pub(crate) fn storage_fields(
    obj: &json::Obj,
    path: &str,
    allow_datatype: bool,
) -> Result<VectorsConfig, ConvertError> {
    if !allow_datatype && obj.contains_key("datatype") {
        return Err(invalid(
            child(path, "datatype"),
            "VectorParamsDiff has no datatype field; datatype cannot be changed",
        ));
    }
    let datatype = if allow_datatype {
        match json::opt_string(obj, "datatype", path)? {
            None => None,
            Some(raw) => Some(VectorDatatype::parse(&raw).ok_or_else(|| {
                invalid(
                    child(path, "datatype"),
                    format!("unknown vector datatype '{raw}'"),
                )
            })?),
        }
    } else {
        None
    };
    Ok(VectorsConfig {
        on_disk: json::opt_bool(obj, "on_disk", path)?,
        memory: memory(obj, "memory", path)?,
        datatype,
    })
}

/// Decode one `VectorParams` into a named [`VectorDef`].
pub(crate) fn vector_def(name: &str, value: &Value, path: &str) -> Result<VectorDef, ConvertError> {
    let obj = json::object(value, path)?;
    for key in obj.keys() {
        match key.as_str() {
            "size"
            | "distance"
            | "hnsw_config"
            | "quantization_config"
            | "on_disk"
            | "memory"
            | "datatype"
            | "multivector_config" => {}
            other => return Err(invalid(child(path, other), "unknown VectorParams field")),
        }
    }
    let size =
        json::required(obj, "size", path).and_then(|v| json::u64_at(v, &child(path, "size")))?;
    if size == 0 {
        return Err(invalid(child(path, "size"), "vector size must be positive"));
    }
    let distance = parse_distance(
        json::required(obj, "distance", path)
            .and_then(|v| json::string_at(v, &child(path, "distance")))?,
        &child(path, "distance"),
    )?;
    let multivector = match obj.get("multivector_config").filter(|v| !v.is_null()) {
        None => None,
        Some(value) => {
            let mv_path = child(path, "multivector_config");
            let mv = json::object(value, &mv_path)?;
            let comparator = match json::string_at(
                json::required(mv, "comparator", &mv_path)?,
                &child(&mv_path, "comparator"),
            )? {
                "max_sim" => MultivectorComparator::MaxSim,
                other => {
                    return Err(invalid(
                        child(&mv_path, "comparator"),
                        format!("unknown multivector comparator '{other}'"),
                    ));
                }
            };
            Some(MultivectorConfig { comparator })
        }
    };
    let storage = storage_fields(obj, path, true)?;
    let has_storage =
        storage.on_disk.is_some() || storage.memory.is_some() || storage.datatype.is_some();
    Ok(VectorDef {
        name: name.to_string(),
        size,
        distance,
        hnsw: match obj.get("hnsw_config").filter(|v| !v.is_null()) {
            None => None,
            Some(value) => Some(Box::new(hnsw(value, &child(path, "hnsw_config"))?)),
        },
        quantization: match obj.get("quantization_config").filter(|v| !v.is_null()) {
            None => None,
            Some(value) => Some(Box::new(quantization(
                value,
                &child(path, "quantization_config"),
            )?)),
        },
        multivector,
        vectors: has_storage.then(|| Box::new(storage)),
    })
}

/// Decode a distance metric name (`Cosine` / `Dot` / `Euclid` / `Manhattan`).
fn parse_distance(raw: &str, path: &str) -> Result<VectorDistance, ConvertError> {
    match raw.to_ascii_lowercase().as_str() {
        "cosine" => Ok(VectorDistance::Cosine),
        "dot" => Ok(VectorDistance::Dot),
        "euclid" => Ok(VectorDistance::Euclid),
        "manhattan" => Ok(VectorDistance::Manhattan),
        _ => Err(invalid(
            path,
            format!("unknown distance metric '{raw}' (expected Cosine, Dot, Euclid, Manhattan)"),
        )),
    }
}

/// Decode a `SparseVectorParams` into a named [`SparseVectorDef`].
pub(crate) fn sparse_vector_def(
    name: &str,
    value: &Value,
    path: &str,
) -> Result<SparseVectorDef, ConvertError> {
    let obj = json::object(value, path)?;
    for key in obj.keys() {
        match key.as_str() {
            "index" | "modifier" => {}
            other => {
                return Err(invalid(
                    child(path, other),
                    "unknown SparseVectorParams field",
                ));
            }
        }
    }
    let modifier = match json::opt_string(obj, "modifier", path)? {
        None => None,
        Some(raw) if raw.eq_ignore_ascii_case("none") => Some("none".to_string()),
        Some(raw) if raw.eq_ignore_ascii_case("idf") => Some("idf".to_string()),
        Some(raw) => {
            return Err(invalid(
                child(path, "modifier"),
                format!("unknown sparse modifier '{raw}'"),
            ));
        }
    };
    let index = match obj.get("index").filter(|v| !v.is_null()) {
        None => None,
        Some(value) => Some(Box::new(sparse_index(value, &child(path, "index"))?)),
    };
    Ok(SparseVectorDef {
        name: name.to_string(),
        index,
        modifier,
    })
}

/// Decode a `SparseIndexParams`.
pub(crate) fn sparse_index(value: &Value, path: &str) -> Result<SparseIndexConfig, ConvertError> {
    let obj = json::object(value, path)?;
    for key in obj.keys() {
        match key.as_str() {
            "full_scan_threshold" | "on_disk" | "memory" | "datatype" => {}
            other => return Err(invalid(child(path, other), "unknown sparse index field")),
        }
    }
    let datatype = match json::opt_string(obj, "datatype", path)? {
        None => None,
        Some(raw) => Some(VectorDatatype::parse_sparse(&raw).ok_or_else(|| {
            invalid(
                child(path, "datatype"),
                format!("unsupported sparse index datatype '{raw}'"),
            )
        })?),
    };
    Ok(SparseIndexConfig {
        full_scan_threshold: json::opt_u64(obj, "full_scan_threshold", path)?,
        on_disk: json::opt_bool(obj, "on_disk", path)?,
        datatype,
        memory: memory(obj, "memory", path)?,
    })
}

/// Decode a free-form config object (`wal_config`, `strict_mode_config`,
/// `metadata`) into ordered AST pairs (sorted for canonical output).
///
/// Values decode generically; the plan layer validates keys and types
/// fail-closed when lowering.
pub(crate) fn raw_options(
    value: &Value,
    path: &str,
) -> Result<Vec<(String, qql_core::ast::Value)>, ConvertError> {
    let obj = json::object(value, path)?;
    let mut pairs = Vec::with_capacity(obj.len());
    for (key, item) in obj {
        pairs.push((
            key.clone(),
            crate::json::json_to_ast_value(item, &child(path, key))?,
        ));
    }
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(pairs)
}

/// Decode `CreateCollection` top-level collection params.
///
/// The OpenAPI create body hoists replication / consistency / payload storage
/// to the top level, so callers pass the whole body object.
pub(crate) fn create_params(
    obj: &json::Obj,
    path: &str,
) -> Result<CollectionParamsConfig, ConvertError> {
    Ok(CollectionParamsConfig {
        replication_factor: json::opt_u64(obj, "replication_factor", path)?,
        write_consistency_factor: json::opt_u64(obj, "write_consistency_factor", path)?,
        read_fan_out_factor: None,
        read_fan_out_delay_ms: None,
        on_disk_payload: json::opt_bool(obj, "on_disk_payload", path)?,
        payload_memory: payload_memory(obj, path)?,
        shard_number: json::opt_u64(obj, "shard_number", path)?,
        sharding_method: sharding_method(obj, path)?,
        shard_keys: None,
    })
}

/// Decode `UpdateCollection.params` (`CollectionParamsDiff`).
pub(crate) fn update_params(
    value: &Value,
    path: &str,
) -> Result<CollectionParamsConfig, ConvertError> {
    let obj = json::object(value, path)?;
    for key in obj.keys() {
        match key.as_str() {
            "replication_factor"
            | "write_consistency_factor"
            | "read_fan_out_factor"
            | "read_fan_out_delay_ms"
            | "on_disk_payload"
            | "payload" => {}
            other => {
                return Err(invalid(
                    child(path, other),
                    "unknown collection param field",
                ));
            }
        }
    }
    Ok(CollectionParamsConfig {
        replication_factor: json::opt_u64(obj, "replication_factor", path)?,
        write_consistency_factor: json::opt_u64(obj, "write_consistency_factor", path)?,
        read_fan_out_factor: json::opt_u64(obj, "read_fan_out_factor", path)?,
        read_fan_out_delay_ms: json::opt_u64(obj, "read_fan_out_delay_ms", path)?,
        on_disk_payload: json::opt_bool(obj, "on_disk_payload", path)?,
        payload_memory: payload_memory(obj, path)?,
        shard_number: None,
        sharding_method: None,
        shard_keys: None,
    })
}

/// Decode `payload: {memory}` into a memory placement.
fn payload_memory(obj: &json::Obj, path: &str) -> Result<Option<MemoryPlacement>, ConvertError> {
    let Some(payload) = json::opt_object(obj, "payload", path)? else {
        return Ok(None);
    };
    let payload_path = child(path, "payload");
    for key in payload.keys() {
        if key != "memory" {
            return Err(invalid(
                child(&payload_path, key),
                "unknown payload storage field",
            ));
        }
    }
    memory(payload, "memory", &payload_path)
}

/// Decode `sharding_method` (`auto` / `custom`).
fn sharding_method(obj: &json::Obj, path: &str) -> Result<Option<String>, ConvertError> {
    match json::opt_string(obj, "sharding_method", path)? {
        None => Ok(None),
        Some(raw) if raw.eq_ignore_ascii_case("auto") => Ok(Some("auto".to_string())),
        Some(raw) if raw.eq_ignore_ascii_case("custom") => Ok(Some("custom".to_string())),
        Some(raw) => Err(invalid(
            child(path, "sharding_method"),
            format!("unknown sharding method '{raw}'"),
        )),
    }
}
