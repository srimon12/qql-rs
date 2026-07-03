use super::ascii_equal_lower;
use crate::ast::{CollectionConfig, OptimizationThreads, Value};
use crate::error::QqlError;

pub fn config_value<'a>(config: &'a [(&'a str, Value<'a>)], key: &str) -> Option<&'a Value<'a>> {
    for (k, v) in config {
        if ascii_equal_lower(k, key) {
            return Some(v);
        }
    }
    None
}

pub fn config_has_key(config: &[(&str, Value)], key: &str) -> bool {
    config_value(config, key).is_some()
}

pub fn config_bool(config: &[(&str, Value)], key: &str) -> Option<bool> {
    match config_value(config, key)? {
        Value::Bool(b) => Some(*b),
        _ => None,
    }
}

pub fn config_positive_u64(
    config: &[(&str, Value)],
    key: &str,
    pos: usize,
) -> Result<Option<u64>, QqlError> {
    match config_value(config, key) {
        None => Ok(None),
        Some(Value::Int(n)) if *n > 0 => Ok(Some(*n as u64)),
        Some(Value::Float(n)) if *n > 0.0 && *n == (*n as u64) as f64 => Ok(Some(*n as u64)),
        _ => Err(QqlError::syntax(
            alloc::format!("{} must be a positive integer", key),
            pos,
        )),
    }
}

pub fn config_non_negative_u64(
    config: &[(&str, Value)],
    key: &str,
    pos: usize,
) -> Result<Option<u64>, QqlError> {
    match config_value(config, key) {
        None => Ok(None),
        Some(Value::Int(n)) if *n >= 0 => Ok(Some(*n as u64)),
        Some(Value::Float(n)) if *n >= 0.0 && *n == (*n as u64) as f64 => Ok(Some(*n as u64)),
        _ => Err(QqlError::syntax(
            alloc::format!("{} must be a non-negative integer", key),
            pos,
        )),
    }
}

pub fn config_float_range(config: &[(&str, Value)], key: &str, min: f64, max: f64) -> Option<f64> {
    match config_value(config, key)? {
        Value::Int(n) => {
            let f = *n as f64;
            if (min..=max).contains(&f) {
                Some(f)
            } else {
                None
            }
        }
        Value::Float(f) => {
            if (min..=max).contains(f) {
                Some(*f)
            } else {
                None
            }
        }
        _ => None,
    }
}

pub fn config_max_optimization_threads(
    config: &[(&str, Value)],
    key: &str,
) -> Option<OptimizationThreads> {
    match config_value(config, key)? {
        Value::Int(n) if *n > 0 => Some(OptimizationThreads {
            auto_: false,
            value: *n as u64,
        }),
        Value::Str(s) if ascii_equal_lower(s, "auto") => Some(OptimizationThreads {
            auto_: true,
            value: 0,
        }),
        _ => None,
    }
}

pub fn validate_hnsw_value(key: &str, value: &Value, pos: usize) -> Result<(), QqlError> {
    let lower = key.to_ascii_lowercase();
    match lower.as_str() {
        "m" | "ef_construct" | "full_scan_threshold" | "max_indexing_threads" | "payload_m" => {
            if !matches!(value, Value::Int(_)) {
                return Err(QqlError::syntax(
                    alloc::format!("{} must be an integer", key),
                    pos,
                ));
            }
        }
        "on_disk" | "inline_storage" if !matches!(value, Value::Bool(_)) => {
            return Err(QqlError::syntax(
                alloc::format!("{} must be true or false", key),
                pos,
            ));
        }
        _ => {}
    }
    Ok(())
}

pub fn validate_vectors_value(key: &str, value: &Value, pos: usize) -> Result<(), QqlError> {
    if ascii_equal_lower(key, "on_disk") && !matches!(value, Value::Bool(_)) {
        return Err(QqlError::syntax(
            alloc::format!("{} must be true or false", key),
            pos,
        ));
    }
    Ok(())
}

pub fn validate_optimizers_value(key: &str, value: &Value, pos: usize) -> Result<(), QqlError> {
    let lower = key.to_ascii_lowercase();
    match lower.as_str() {
        "deleted_threshold" => {
            if !matches!(value, Value::Int(_) | Value::Float(_)) {
                return Err(QqlError::syntax(
                    alloc::format!("{} must be a number", key),
                    pos,
                ));
            }
        }
        "vacuum_min_vector_number"
        | "default_segment_number"
        | "max_segment_size"
        | "memmap_threshold"
        | "indexing_threshold"
        | "flush_interval_sec" => {
            if !matches!(value, Value::Int(_)) {
                return Err(QqlError::syntax(
                    alloc::format!("{} must be an integer", key),
                    pos,
                ));
            }
        }
        "max_optimization_threads" => {
            if !matches!(value, Value::Int(_) | Value::Str(_)) {
                return Err(QqlError::syntax(
                    alloc::format!("{} must be a positive integer or 'auto'", key),
                    pos,
                ));
            }
        }
        "prevent_unoptimized" if !matches!(value, Value::Bool(_)) => {
            return Err(QqlError::syntax(
                alloc::format!("{} must be true or false", key),
                pos,
            ));
        }
        _ => {}
    }
    Ok(())
}

pub fn validate_params_value(key: &str, value: &Value, pos: usize) -> Result<(), QqlError> {
    let lower = key.to_ascii_lowercase();
    match lower.as_str() {
        "replication_factor"
        | "write_consistency_factor"
        | "read_fan_out_factor"
        | "read_fan_out_delay_ms" => {
            if !matches!(value, Value::Int(_)) {
                return Err(QqlError::syntax(
                    alloc::format!("{} must be an integer", key),
                    pos,
                ));
            }
        }
        "on_disk_payload" if !matches!(value, Value::Bool(_)) => {
            return Err(QqlError::syntax(
                alloc::format!("{} must be true or false", key),
                pos,
            ));
        }
        _ => {}
    }
    Ok(())
}

pub fn merge_collection_config(
    current: &mut CollectionConfig,
    new: CollectionConfig,
    pos: usize,
) -> Result<(), QqlError> {
    if new.vectors.is_some() {
        if current.vectors.is_some() {
            return Err(QqlError::syntax("VECTORS clause may only appear once", pos));
        }
        current.vectors = new.vectors;
    }
    if new.hnsw.is_some() {
        if current.hnsw.is_some() {
            return Err(QqlError::syntax("HNSW clause may only appear once", pos));
        }
        current.hnsw = new.hnsw;
    }
    if new.optimizers.is_some() {
        if current.optimizers.is_some() {
            return Err(QqlError::syntax(
                "OPTIMIZERS clause may only appear once",
                pos,
            ));
        }
        current.optimizers = new.optimizers;
    }
    if new.params.is_some() {
        if current.params.is_some() {
            return Err(QqlError::syntax("PARAMS clause may only appear once", pos));
        }
        current.params = new.params;
    }
    if new.quantization.is_some() {
        if current.quantization.is_some() {
            return Err(QqlError::syntax(
                "QUANTIZATION clause may only appear once",
                pos,
            ));
        }
        current.quantization = new.quantization;
    }
    if new.quantization_update.is_some() {
        if current.quantization_update.is_some() {
            return Err(QqlError::syntax(
                "QUANTIZATION clause may only appear once",
                pos,
            ));
        }
        current.quantization_update = new.quantization_update;
    }
    Ok(())
}

pub fn check_deleted_threshold(value: &Value, pos: usize) -> Result<(), QqlError> {
    match value {
        Value::Int(n) => {
            let f = *n as f64;
            if !(0.0..=1.0).contains(&f) {
                return Err(QqlError::syntax(
                    "deleted_threshold must be between 0.0 and 1.0",
                    pos,
                ));
            }
        }
        Value::Float(f) if !(0.0..=1.0).contains(f) => {
            return Err(QqlError::syntax(
                "deleted_threshold must be between 0.0 and 1.0",
                pos,
            ));
        }
        _ => {}
    }
    Ok(())
}
