//! Config-pair options: WAL, strict mode, metadata, replica states.
//!
//! Pure move from `ddl.rs` (size hygiene split).

use crate::types::*;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use qql_core::ast::Value;
use qql_core::error::QqlError;

use super::collection_config_error;

pub(crate) fn lower_collection_params(
    config: &qql_core::ast::CollectionParamsConfig,
) -> CollectionParams {
    CollectionParams {
        replication_factor: config.replication_factor,
        write_consistency_factor: config.write_consistency_factor,
        read_fan_out_factor: config.read_fan_out_factor,
        read_fan_out_delay_ms: config.read_fan_out_delay_ms,
        on_disk_payload: config.on_disk_payload,
        payload: config.payload_memory.map(|memory| PayloadStorageParams {
            memory: Some(memory),
        }),
    }
}

pub(crate) fn lower_sharding_method(method: &str) -> ShardingMethod {
    if method.eq_ignore_ascii_case("custom") {
        ShardingMethod::Custom
    } else {
        ShardingMethod::Auto
    }
}

/// Find a raw config pair by key (case-insensitive).
fn config_pair<'a>(pairs: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
    pairs
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v)
}

/// Extract an optional non-negative integer pair (`None` when absent).
fn config_pair_u64(pairs: &[(String, Value)], key: &str) -> Result<Option<u64>, QqlError> {
    match config_pair(pairs, key) {
        None => Ok(None),
        Some(Value::Int(n)) if *n >= 0 => Ok(Some(*n as u64)),
        Some(Value::Float(n)) if *n >= 0.0 && *n == (*n as u64) as f64 => Ok(Some(*n as u64)),
        Some(value) => Err(QqlError::validation(
            "QQL-PLAN-COLLECTION-CONFIG",
            format!("{key} must be a non-negative integer"),
            value.param_span(),
        )),
    }
}

/// Extract an optional positive integer pair (`None` when absent).
fn config_pair_positive_u64(pairs: &[(String, Value)], key: &str) -> Result<Option<u64>, QqlError> {
    match config_pair(pairs, key) {
        None => Ok(None),
        Some(Value::Int(n)) if *n > 0 => Ok(Some(*n as u64)),
        Some(Value::Float(n)) if *n > 0.0 && *n == (*n as u64) as f64 => Ok(Some(*n as u64)),
        Some(value) => Err(QqlError::validation(
            "QQL-PLAN-COLLECTION-CONFIG",
            format!("{key} must be a positive integer"),
            value.param_span(),
        )),
    }
}

/// Extract an optional boolean pair (`None` when absent).
fn config_pair_bool(pairs: &[(String, Value)], key: &str) -> Result<Option<bool>, QqlError> {
    match config_pair(pairs, key) {
        None => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(value) => Err(QqlError::validation(
            "QQL-PLAN-COLLECTION-CONFIG",
            format!("{key} must be true or false"),
            value.param_span(),
        )),
    }
}

/// Extract an optional finite number pair (`None` when absent).
fn config_pair_f64(pairs: &[(String, Value)], key: &str) -> Result<Option<f64>, QqlError> {
    match config_pair(pairs, key) {
        None => Ok(None),
        Some(Value::Int(n)) => Ok(Some(*n as f64)),
        Some(Value::Float(n)) if n.is_finite() => Ok(Some(*n)),
        Some(value) => Err(QqlError::validation(
            "QQL-PLAN-COLLECTION-CONFIG",
            format!("{key} must be a number"),
            value.param_span(),
        )),
    }
}

/// Lower `WITH WAL (…)` pairs to the OpenAPI `WalConfigDiff` shape.
pub(crate) fn lower_wal_config(pairs: &[(String, Value)]) -> Result<WalConfig, QqlError> {
    for (key, value) in pairs {
        if !(key.eq_ignore_ascii_case("wal_capacity_mb")
            || key.eq_ignore_ascii_case("wal_segments_ahead")
            || key.eq_ignore_ascii_case("wal_retain_closed"))
        {
            return Err(collection_config_error(
                format!(
                    "unknown WAL parameter '{key}'. Expected: wal_capacity_mb, wal_segments_ahead, wal_retain_closed"
                ),
                value.param_span(),
            ));
        }
    }
    // OpenAPI minimums: capacity ≥ 1, the rest ≥ 0.
    Ok(WalConfig {
        capacity_mb: config_pair_positive_u64(pairs, "wal_capacity_mb")?,
        segments_ahead: config_pair_u64(pairs, "wal_segments_ahead")?,
        retain_closed: config_pair_u64(pairs, "wal_retain_closed")?,
    })
}

/// Lower `WITH STRICT_MODE (…)` pairs to the OpenAPI `StrictModeConfig` shape.
pub(crate) fn lower_strict_mode_config(
    pairs: &[(String, Value)],
) -> Result<StrictModeConfig, QqlError> {
    for (key, value) in pairs {
        if !is_strict_mode_key(key) {
            return Err(collection_config_error(
                format!("unknown STRICT_MODE parameter '{key}'"),
                value.param_span(),
            ));
        }
    }
    let mut config = StrictModeConfig {
        enabled: config_pair_bool(pairs, "enabled")?,
        max_query_limit: config_pair_positive_u64(pairs, "max_query_limit")?,
        max_timeout: config_pair_positive_u64(pairs, "max_timeout")?,
        unindexed_filtering_retrieve: config_pair_bool(pairs, "unindexed_filtering_retrieve")?,
        unindexed_filtering_update: config_pair_bool(pairs, "unindexed_filtering_update")?,
        search_max_hnsw_ef: config_pair_u64(pairs, "search_max_hnsw_ef")?,
        search_allow_exact: config_pair_bool(pairs, "search_allow_exact")?,
        search_max_oversampling: config_pair_f64(pairs, "search_max_oversampling")?,
        upsert_max_batchsize: config_pair_u64(pairs, "upsert_max_batchsize")?,
        search_max_batchsize: config_pair_u64(pairs, "search_max_batchsize")?,
        max_collection_vector_size_bytes: config_pair_u64(
            pairs,
            "max_collection_vector_size_bytes",
        )?,
        read_rate_limit: config_pair_positive_u64(pairs, "read_rate_limit")?,
        write_rate_limit: config_pair_positive_u64(pairs, "write_rate_limit")?,
        max_collection_payload_size_bytes: config_pair_u64(
            pairs,
            "max_collection_payload_size_bytes",
        )?,
        max_points_count: config_pair_positive_u64(pairs, "max_points_count")?,
        filter_max_conditions: config_pair_u64(pairs, "filter_max_conditions")?,
        condition_max_size: config_pair_u64(pairs, "condition_max_size")?,
        multivector_config: None,
        sparse_config: None,
        max_payload_index_count: config_pair_u64(pairs, "max_payload_index_count")?,
        max_resident_memory_percent: None,
    };
    if let Some(value) = config_pair(pairs, "multivector_config") {
        config.multivector_config = Some(lower_strict_multivector_config(value)?);
    }
    if let Some(value) = config_pair(pairs, "sparse_config") {
        config.sparse_config = Some(lower_strict_sparse_config(value)?);
    }
    if let Some(value) = config_pair(pairs, "max_resident_memory_percent") {
        match value {
            Value::Int(n) if (1..=100).contains(n) => {
                config.max_resident_memory_percent = Some(*n as u64);
            }
            _ => {
                return Err(QqlError::validation(
                    "QQL-PLAN-COLLECTION-CONFIG",
                    "max_resident_memory_percent must be an integer in [1, 100]",
                    value.param_span(),
                ));
            }
        }
    }
    Ok(config)
}

/// Lower a `multivector_config` strict-mode value (`{name: {max_vectors}}`).
fn lower_strict_multivector_config(value: &Value) -> Result<StrictModeMultivectorConfig, QqlError> {
    let Value::Dict(entries) = value else {
        return Err(QqlError::validation(
            "QQL-PLAN-COLLECTION-CONFIG",
            "multivector_config must be an object",
            value.param_span(),
        ));
    };
    let mut vectors = BTreeMap::new();
    for (name, caps) in entries {
        let Value::Dict(fields) = caps else {
            return Err(QqlError::validation(
                "QQL-PLAN-COLLECTION-CONFIG",
                format!("multivector_config '{name}' must be an object"),
                caps.param_span(),
            ));
        };
        let mut entry = StrictModeMultivector::default();
        for (key, val) in fields {
            if !key.eq_ignore_ascii_case("max_vectors") {
                return Err(collection_config_error(
                    format!("unknown multivector_config key '{key}'. Expected: max_vectors"),
                    val.param_span(),
                ));
            }
            match val {
                Value::Int(n) if *n >= 1 => entry.max_vectors = Some(*n as u64),
                _ => {
                    return Err(QqlError::validation(
                        "QQL-PLAN-COLLECTION-CONFIG",
                        "max_vectors must be a positive integer",
                        val.param_span(),
                    ));
                }
            }
        }
        vectors.insert(name.clone(), entry);
    }
    Ok(StrictModeMultivectorConfig { vectors })
}

/// Lower a `sparse_config` strict-mode value (`{name: {max_length}}`).
fn lower_strict_sparse_config(value: &Value) -> Result<StrictModeSparseConfig, QqlError> {
    let Value::Dict(entries) = value else {
        return Err(QqlError::validation(
            "QQL-PLAN-COLLECTION-CONFIG",
            "sparse_config must be an object",
            value.param_span(),
        ));
    };
    let mut vectors = BTreeMap::new();
    for (name, caps) in entries {
        let Value::Dict(fields) = caps else {
            return Err(QqlError::validation(
                "QQL-PLAN-COLLECTION-CONFIG",
                format!("sparse_config '{name}' must be an object"),
                caps.param_span(),
            ));
        };
        let mut entry = StrictModeSparse::default();
        for (key, val) in fields {
            if !key.eq_ignore_ascii_case("max_length") {
                return Err(collection_config_error(
                    format!("unknown sparse_config key '{key}'. Expected: max_length"),
                    val.param_span(),
                ));
            }
            match val {
                Value::Int(n) if *n >= 1 => entry.max_length = Some(*n as u64),
                _ => {
                    return Err(QqlError::validation(
                        "QQL-PLAN-COLLECTION-CONFIG",
                        "max_length must be a positive integer",
                        val.param_span(),
                    ));
                }
            }
        }
        vectors.insert(name.clone(), entry);
    }
    Ok(StrictModeSparseConfig { vectors })
}

/// Lower `WITH METADATA (…)` pairs to the free-form metadata list.
///
/// `ensure_no_unbound_params` (via `validate_no_unbound_ddl_options`) rejects
/// placeholders before lowering; pairs are cloned here and rendered to JSON
/// once at the transport boundary.
pub(crate) fn lower_metadata_map(pairs: &[(String, Value)]) -> Vec<(String, Value)> {
    pairs.to_vec()
}

/// Canonical OpenAPI `ReplicaState` names for shard-key `initial_state`.
const REPLICA_STATES: &[&str] = &[
    "Active",
    "Dead",
    "Partial",
    "Initializing",
    "Listener",
    "PartialSnapshot",
    "Recovery",
    "Resharding",
    "ReshardingScaleDown",
    "ActiveRead",
    "ManualRecovery",
];

/// Validate (and canonicalize the casing of) a shard-key `initial_state`.
/// The parser stores the canonical form; a hand-built AST is normalized here
/// and fails closed on unknown states.
pub(crate) fn lower_replica_state(raw: Option<&str>) -> Result<Option<String>, QqlError> {
    match raw {
        None => Ok(None),
        Some(state) => REPLICA_STATES
            .iter()
            .find(|known| known.eq_ignore_ascii_case(state))
            .map(|known| Some(known.to_string()))
            .ok_or_else(|| {
                QqlError::validation(
                    "QQL-PLAN-SHARD-KEY",
                    format!("unknown replica state '{state}'"),
                    None,
                )
            }),
    }
}

/// True when `key` names a `STRICT_MODE` option (case-insensitive).
/// Mirrors `qql_core::parser::is_strict_mode_key` for hand-built ASTs.
fn is_strict_mode_key(key: &str) -> bool {
    const KEYS: &[&str] = &[
        "enabled",
        "max_query_limit",
        "max_timeout",
        "unindexed_filtering_retrieve",
        "unindexed_filtering_update",
        "search_max_hnsw_ef",
        "search_allow_exact",
        "search_max_oversampling",
        "upsert_max_batchsize",
        "search_max_batchsize",
        "max_collection_vector_size_bytes",
        "read_rate_limit",
        "write_rate_limit",
        "max_collection_payload_size_bytes",
        "max_points_count",
        "filter_max_conditions",
        "condition_max_size",
        "multivector_config",
        "sparse_config",
        "max_payload_index_count",
        "max_resident_memory_percent",
    ];
    KEYS.iter().any(|known| known.eq_ignore_ascii_case(key))
}
