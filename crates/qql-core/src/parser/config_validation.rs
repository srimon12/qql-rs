use super::ascii_equal;
use crate::ast::{CollectionConfig, OptimizationThreads, Value};
use crate::error::QqlError;
use alloc::string::String;

/// Looks up a config entry by key, comparing ASCII case-insensitively.
pub fn config_value<'a>(config: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
    for (k, v) in config {
        if ascii_equal(k, key) {
            return Some(v);
        }
    }
    None
}

/// Returns true when the config contains the given key (case-insensitive).
pub fn config_has_key(config: &[(String, Value)], key: &str) -> bool {
    config_value(config, key).is_some()
}

/// Reads a boolean config value, returning `None` when absent or not a bool.
pub fn config_bool(config: &[(String, Value)], key: &str) -> Option<bool> {
    match config_value(config, key)? {
        Value::Bool(b) => Some(*b),
        _ => None,
    }
}

use crate::error::Span;

fn validation_err(message: impl Into<alloc::borrow::Cow<'static, str>>, span: Span) -> QqlError {
    QqlError::validation("QQL-VALIDATION-CONFIG", message, Some(span))
}

/// Reads a positive integer config value; `None` when absent, error when invalid.
pub fn config_positive_u64(
    config: &[(String, Value)],
    key: &str,
    span: Span,
) -> Result<Option<u64>, QqlError> {
    match config_value(config, key) {
        None => Ok(None),
        Some(Value::Int(n)) if *n > 0 => Ok(Some(*n as u64)),
        Some(Value::Float(n)) if *n > 0.0 && *n == (*n as u64) as f64 => Ok(Some(*n as u64)),
        _ => Err(validation_err(
            alloc::format!("{} must be a positive integer", key),
            span,
        )),
    }
}

/// Reads a non-negative integer config value; `None` when absent, error when invalid.
pub fn config_non_negative_u64(
    config: &[(String, Value)],
    key: &str,
    span: Span,
) -> Result<Option<u64>, QqlError> {
    match config_value(config, key) {
        None => Ok(None),
        Some(Value::Int(n)) if *n >= 0 => Ok(Some(*n as u64)),
        Some(Value::Float(n)) if *n >= 0.0 && *n == (*n as u64) as f64 => Ok(Some(*n as u64)),
        _ => Err(validation_err(
            alloc::format!("{} must be a non-negative integer", key),
            span,
        )),
    }
}

/// Reads a numeric config value, or `None` when absent or outside `[min, max]`.
pub fn config_float_range(
    config: &[(String, Value)],
    key: &str,
    min: f64,
    max: f64,
) -> Option<f64> {
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

/// Reads a thread count as a positive integer or the string `auto`.
pub fn config_max_optimization_threads(
    config: &[(String, Value)],
    key: &str,
) -> Option<OptimizationThreads> {
    match config_value(config, key)? {
        Value::Int(n) if *n > 0 => Some(OptimizationThreads {
            auto_: false,
            value: *n as u64,
        }),
        Value::Str(s) if ascii_equal(s, "auto") => Some(OptimizationThreads {
            auto_: true,
            value: 0,
        }),
        _ => None,
    }
}

pub fn is_integer_val(value: &Value) -> bool {
    match value {
        Value::Int(_) => true,
        Value::Float(f) => *f >= 0.0 && *f == (*f as u64) as f64,
        _ => false,
    }
}

/// Type-checks one HNSW config option (`m`, `ef_construct`, `on_disk`, `memory`, …).
pub fn validate_hnsw_value(key: &str, value: &Value, span: Span) -> Result<(), QqlError> {
    let lower = key.to_ascii_lowercase();
    match lower.as_str() {
        "m" | "ef_construct" | "full_scan_threshold" | "max_indexing_threads" | "payload_m" => {
            if !is_integer_val(value) {
                return Err(validation_err(
                    alloc::format!("{} must be an integer", key),
                    span,
                ));
            }
        }
        "on_disk" | "inline_storage" if !matches!(value, Value::Bool(_)) => {
            return Err(validation_err(
                alloc::format!("{} must be true or false", key),
                span,
            ));
        }
        "memory" => validate_memory_value(key, value, span, true)?,
        _ => {}
    }
    Ok(())
}

/// Type-checks one vectors config option (`on_disk`, `memory`, `datatype`).
pub fn validate_vectors_value(key: &str, value: &Value, span: Span) -> Result<(), QqlError> {
    let lower = key.to_ascii_lowercase();
    match lower.as_str() {
        "on_disk" if !matches!(value, Value::Bool(_)) => {
            return Err(validation_err(
                alloc::format!("{} must be true or false", key),
                span,
            ));
        }
        "memory" => validate_memory_value(key, value, span, true)?,
        "datatype" => match value {
            Value::Str(s) if crate::ast::VectorDatatype::parse(s).is_some() => {}
            Value::Str(_) => {
                return Err(validation_err(
                    alloc::format!("{key} must be float32, float16, uint8, or turbo4"),
                    span,
                ));
            }
            _ => {
                return Err(validation_err(
                    alloc::format!("{key} must be a string (float32, float16, uint8, or turbo4)"),
                    span,
                ));
            }
        },
        _ => {}
    }
    Ok(())
}

fn validate_memory_value(
    key: &str,
    value: &Value,
    span: Span,
    allow_pinned: bool,
) -> Result<(), QqlError> {
    match value {
        Value::Str(s) => match crate::ast::MemoryPlacement::parse(s) {
            Some(crate::ast::MemoryPlacement::Pinned) if !allow_pinned => Err(validation_err(
                alloc::format!("{key} does not support 'pinned'"),
                span,
            )),
            Some(_) => Ok(()),
            None => Err(validation_err(
                alloc::format!("{key} must be 'cold', 'cached', or 'pinned'"),
                span,
            )),
        },
        _ => Err(validation_err(
            alloc::format!("{key} must be a string ('cold', 'cached', or 'pinned')"),
            span,
        )),
    }
}

/// Type-checks one optimizers config option (`deleted_threshold`, `memmap_threshold`, …).
pub fn validate_optimizers_value(key: &str, value: &Value, span: Span) -> Result<(), QqlError> {
    let lower = key.to_ascii_lowercase();
    match lower.as_str() {
        "deleted_threshold" => {
            if !matches!(value, Value::Int(_) | Value::Float(_)) {
                return Err(validation_err(
                    alloc::format!("{} must be a number", key),
                    span,
                ));
            }
        }
        "vacuum_min_vector_number"
        | "default_segment_number"
        | "max_segment_size"
        | "memmap_threshold"
        | "indexing_threshold"
        | "flush_interval_sec" => {
            if !is_integer_val(value) {
                return Err(validation_err(
                    alloc::format!("{} must be an integer", key),
                    span,
                ));
            }
        }
        "max_optimization_threads" => {
            if !is_integer_val(value) && !matches!(value, Value::Str(_)) {
                return Err(validation_err(
                    alloc::format!("{} must be a positive integer or 'auto'", key),
                    span,
                ));
            }
        }
        "prevent_unoptimized" if !matches!(value, Value::Bool(_)) => {
            return Err(validation_err(
                alloc::format!("{} must be true or false", key),
                span,
            ));
        }
        _ => {}
    }
    Ok(())
}

/// Type-checks one collection `PARAMS` option (replication, sharding, memory, …).
pub fn validate_params_value(key: &str, value: &Value, span: Span) -> Result<(), QqlError> {
    let lower = key.to_ascii_lowercase();
    match lower.as_str() {
        "replication_factor"
        | "write_consistency_factor"
        | "read_fan_out_factor"
        | "read_fan_out_delay_ms"
        | "shard_number" => {
            if !matches!(value, Value::Int(_)) {
                return Err(validation_err(
                    alloc::format!("{} must be an integer", key),
                    span,
                ));
            }
        }
        "on_disk_payload" if !matches!(value, Value::Bool(_)) => {
            return Err(validation_err(
                alloc::format!("{} must be true or false", key),
                span,
            ));
        }
        "payload_memory" => validate_memory_value(key, value, span, false)?,
        "sharding_method" => match value {
            Value::Str(s) if s.eq_ignore_ascii_case("auto") || s.eq_ignore_ascii_case("custom") => {
            }
            Value::Str(_) => {
                return Err(validation_err(
                    "sharding_method must be 'auto' or 'custom'",
                    span,
                ));
            }
            _ => {
                return Err(validation_err(
                    "sharding_method must be a string ('auto' or 'custom')",
                    span,
                ));
            }
        },
        "shard_keys" => match value {
            Value::List(items) if items.is_empty() => {
                return Err(validation_err(
                    "shard_keys must be a non-empty list of strings or non-negative integers",
                    span,
                ));
            }
            Value::List(items) => {
                for item in items {
                    let ok = match item {
                        Value::Str(_) => true,
                        Value::Int(n) => *n >= 0,
                        Value::Param(..) | Value::PositionalParam(..) => true,
                        _ => false,
                    };
                    if !ok {
                        return Err(validation_err(
                            "shard_keys entries must all be strings or non-negative integers",
                            span,
                        ));
                    }
                }
            }
            _ => {
                return Err(validation_err(
                    "shard_keys must be a list of strings or non-negative integers",
                    span,
                ));
            }
        },
        _ => {}
    }
    Ok(())
}

/// Merges new collection config clauses into `current`, erroring on duplicates.
pub fn merge_collection_config(
    current: &mut CollectionConfig,
    new: CollectionConfig,
    span: Span,
) -> Result<(), QqlError> {
    if new.vectors.is_some() {
        if current.vectors.is_some() {
            return Err(validation_err("VECTOR clause may only appear once", span));
        }
        current.vectors = new.vectors;
    }
    if new.hnsw.is_some() {
        if current.hnsw.is_some() {
            return Err(validation_err("HNSW clause may only appear once", span));
        }
        current.hnsw = new.hnsw;
    }
    if new.optimizers.is_some() {
        if current.optimizers.is_some() {
            return Err(validation_err(
                "OPTIMIZERS clause may only appear once",
                span,
            ));
        }
        current.optimizers = new.optimizers;
    }
    if new.params.is_some() {
        if current.params.is_some() {
            return Err(validation_err("PARAMS clause may only appear once", span));
        }
        current.params = new.params;
    }
    if new.quantization.is_some() {
        if current.quantization.is_some() {
            return Err(validation_err(
                "QUANTIZATION clause may only appear once",
                span,
            ));
        }
        current.quantization = new.quantization;
    }
    if new.quantization_update.is_some() {
        if current.quantization_update.is_some() {
            return Err(validation_err(
                "QUANTIZATION clause may only appear once",
                span,
            ));
        }
        current.quantization_update = new.quantization_update;
    }
    for diff in new.vector_diffs {
        if current.vector_diffs.iter().any(|d| d.name == diff.name) {
            return Err(validation_err(
                alloc::format!("VECTOR diff '{}' may only appear once", diff.name),
                span,
            ));
        }
        current.vector_diffs.push(diff);
    }
    for diff in new.sparse_vector_diffs {
        if current
            .sparse_vector_diffs
            .iter()
            .any(|d| d.name == diff.name)
        {
            return Err(validation_err(
                alloc::format!("SPARSE vector diff '{}' may only appear once", diff.name),
                span,
            ));
        }
        current.sparse_vector_diffs.push(diff);
    }
    Ok(())
}

/// Checks that `deleted_threshold` is a number between 0.0 and 1.0.
pub fn check_deleted_threshold(value: &Value, span: Span) -> Result<(), QqlError> {
    match value {
        Value::Int(n) => {
            let f = *n as f64;
            if !(0.0..=1.0).contains(&f) {
                return Err(validation_err(
                    "deleted_threshold must be between 0.0 and 1.0",
                    span,
                ));
            }
        }
        Value::Float(f) if !(0.0..=1.0).contains(f) => {
            return Err(validation_err(
                "deleted_threshold must be between 0.0 and 1.0",
                span,
            ));
        }
        _ => {}
    }
    Ok(())
}

/// Type-checks CREATE INDEX options, erroring on unknown keys or bad value types.
pub fn validate_index_options(options: &[(String, Value)], span: Span) -> Result<(), QqlError> {
    for (k, v) in options {
        let lower = k.to_ascii_lowercase();
        match lower.as_str() {
            "is_tenant" | "on_disk" | "enable_hnsw" | "lowercase" | "ascii_folding"
            | "phrase_matching" | "lookup" | "range" | "is_principal" | "prefix" => {
                if !matches!(v, Value::Bool(_)) {
                    return Err(validation_err(
                        alloc::format!("{} must be true or false", k),
                        span,
                    ));
                }
            }
            "min_token_len" | "max_token_len" => {
                if !matches!(v, Value::Int(n) if *n >= 0) {
                    return Err(validation_err(
                        alloc::format!("{} must be a non-negative integer", k),
                        span,
                    ));
                }
            }
            "tokenizer" | "stemmer" => {
                if !matches!(v, Value::Str(_)) {
                    return Err(validation_err(
                        alloc::format!("{} must be a string", k),
                        span,
                    ));
                }
            }
            "memory" => validate_memory_value(k, v, span, true)?,
            "stopwords" => match v {
                Value::List(items) => {
                    for item in items {
                        if !matches!(item, Value::Str(_)) {
                            return Err(validation_err(
                                alloc::format!("{} must be a list of strings", k),
                                span,
                            ));
                        }
                    }
                }
                _ => {
                    return Err(validation_err(
                        alloc::format!("{} must be a list of strings", k),
                        span,
                    ));
                }
            },
            _ => {
                return Err(validation_err(
                    alloc::format!("unknown index option: {}", k),
                    span,
                ));
            }
        }
    }
    Ok(())
}
