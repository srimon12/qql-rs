use crate::types::*;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use qql_core::ast::{
    AlterCollectionStmt, CollectionConfig, CreateCollectionStmt, CreateIndexStmt,
    MultivectorComparator, Value, VectorsConfig,
};
use qql_core::error::QqlError;

pub use crate::ddl_rest::{
    CreateCollectionDeferredParams, CreateCollectionRestBody, CreateIndexRestBody,
    create_collection_deferred_params_rest, create_collection_rest_body, create_index_rest_body,
};

/// Lower `CREATE COLLECTION` to the transport-neutral create request.
pub fn lower_create_collection(
    stmt: &CreateCollectionStmt,
) -> Result<CreateCollectionRequest, QqlError> {
    let mut req = CreateCollectionRequest {
        vectors: None,
        sparse_vectors: None,
        hnsw_config: None,
        optimizers_config: None,
        params: None,
        quantization_config: None,
        wal_config: None,
        strict_mode_config: None,
        metadata: None,
        shard_number: None,
        sharding_method: None,
        shard_keys: None,
    };

    let mut vectors = BTreeMap::new();
    for vd in &stmt.vectors {
        let params = DenseVectorParams {
            size: vd.size,
            distance: vd.distance,
            hnsw_config: vd.hnsw.as_deref().map(lower_hnsw_config),
            quantization_config: vd.quantization.as_deref().map(lower_quantization_config),
            on_disk: vd.vectors.as_deref().and_then(|cfg| cfg.on_disk),
            memory: vd.vectors.as_deref().and_then(|cfg| cfg.memory),
            datatype: vd.vectors.as_deref().and_then(|cfg| cfg.datatype),
            multivector_config: vd.multivector.as_ref().map(|mv| MultiVectorConfig {
                comparator: match mv.comparator {
                    MultivectorComparator::MaxSim => MultiVectorComparator::MaxSim,
                },
            }),
        };
        vectors.insert(vd.name.clone(), params);
    }

    let mut sparse = BTreeMap::new();
    for sv in &stmt.sparse_vectors {
        sparse.insert(
            sv.name.clone(),
            SparseVectorParams {
                index: sv.index.as_deref().map(lower_sparse_index_params),
                modifier: sv
                    .modifier
                    .as_deref()
                    .map(lower_sparse_modifier)
                    .unwrap_or(SparseModifier::Idf),
            },
        );
    }
    if !vectors.is_empty() {
        req.vectors = Some(DenseVectorsConfig::Named(vectors));
    }
    if !sparse.is_empty() {
        req.sparse_vectors = Some(sparse);
    }

    if let Some(ref config) = stmt.config {
        fill_collection_config(&mut req, config)?;
        // Collection-scoped `WITH VECTOR (…)` settings are defaults: they fill
        // unset per-vector values and never override an explicit per-vector
        // setting.
        if let Some(ref default) = config.vectors {
            apply_vector_defaults(req.vectors.as_mut(), default);
        }
    }

    Ok(req)
}

/// Lower `ALTER COLLECTION` to the transport-neutral update request.
pub fn lower_alter_collection(
    stmt: &AlterCollectionStmt,
) -> Result<UpdateCollectionRequest, QqlError> {
    let mut req = UpdateCollectionRequest {
        hnsw_config: None,
        optimizers_config: None,
        params: None,
        quantization_config: None,
        strict_mode_config: None,
        metadata: None,
        vectors: None,
        sparse_vectors: None,
    };
    if let Some(ref config) = stmt.config {
        fill_update_collection_config(&mut req, config)?;
    }
    Ok(req)
}

/// Lower `CREATE INDEX` to the transport-neutral index request.
pub fn lower_create_index(stmt: &CreateIndexStmt) -> Result<CreateIndexRequest, QqlError> {
    let field_schema = IndexFieldType::parse(&stmt.field_type).ok_or_else(|| {
        QqlError::validation(
            "QQL-PLAN-INDEX-TYPE",
            format!("unknown index field type '{}'", stmt.field_type),
            None,
        )
    })?;

    let mut options = IndexOptions::default();
    for (key, value) in &stmt.options {
        let lower = key.to_ascii_lowercase();
        match lower.as_str() {
            "is_tenant" => options.is_tenant = Some(index_bool(key, value)?),
            "on_disk" => options.on_disk = Some(index_bool(key, value)?),
            "enable_hnsw" => options.enable_hnsw = Some(index_bool(key, value)?),
            "lowercase" => options.lowercase = Some(index_bool(key, value)?),
            "ascii_folding" => options.ascii_folding = Some(index_bool(key, value)?),
            "phrase_matching" => options.phrase_matching = Some(index_bool(key, value)?),
            "lookup" => options.lookup = Some(index_bool(key, value)?),
            "range" => options.range = Some(index_bool(key, value)?),
            "is_principal" => options.is_principal = Some(index_bool(key, value)?),
            "prefix" => options.prefix = Some(index_bool(key, value)?),
            "min_token_len" => options.min_token_len = Some(index_u64(key, value)?),
            "max_token_len" => options.max_token_len = Some(index_u64(key, value)?),
            "tokenizer" => {
                let raw = index_str(key, value)?;
                options.tokenizer = Some(TextTokenizer::parse(raw).ok_or_else(|| {
                    index_option_error(format!(
                        "unsupported text tokenizer '{raw}'. Expected: word, whitespace, prefix, multilingual"
                    ))
                })?);
            }
            "stemmer" => options.stemmer = Some(StemmingAlgorithm::parse(index_str(key, value)?)),
            "stopwords" => {
                options.stopwords = Some(lower_stopwords(key, value)?);
            }
            "memory" => {
                let raw = index_str(key, value)?;
                options.memory = Some(MemoryPlacement::parse(raw).ok_or_else(|| {
                    index_option_error(format!(
                        "unsupported memory placement '{raw}'. Expected: cold, cached, pinned"
                    ))
                })?);
            }
            other => {
                return Err(index_option_error(format!(
                    "unknown index option '{other}'"
                )));
            }
        }
    }

    Ok(CreateIndexRequest {
        field_name: stmt.field.clone(),
        field_schema,
        options,
    })
}

fn index_option_error(message: impl Into<alloc::borrow::Cow<'static, str>>) -> QqlError {
    QqlError::validation("QQL-PLAN-INDEX-OPTION", message, None)
}

fn index_bool(key: &str, value: &Value) -> Result<bool, QqlError> {
    match value {
        Value::Bool(value) => Ok(*value),
        _ => Err(index_option_error(format!("{key} must be true or false"))),
    }
}

fn index_u64(key: &str, value: &Value) -> Result<u64, QqlError> {
    match value {
        Value::Int(value) if *value >= 0 => Ok(*value as u64),
        _ => Err(index_option_error(format!(
            "{key} must be a non-negative integer"
        ))),
    }
}

fn index_str<'a>(key: &str, value: &'a Value) -> Result<&'a str, QqlError> {
    match value {
        Value::Str(value) => Ok(value),
        _ => Err(index_option_error(format!("{key} must be a string"))),
    }
}

fn index_str_list(key: &str, value: &Value) -> Result<Vec<String>, QqlError> {
    match value {
        Value::List(items) => items
            .iter()
            .map(|item| match item {
                Value::Str(item) => Ok(item.clone()),
                _ => Err(index_option_error(format!(
                    "{key} must be a list of strings"
                ))),
            })
            .collect(),
        _ => Err(index_option_error(format!(
            "{key} must be a list of strings"
        ))),
    }
}

/// Lower a `stopwords` index option: a custom list, a bare language name, or
/// a `{languages: […], custom: […]}` set mirroring OpenAPI `StopwordsSet`.
fn lower_stopwords(key: &str, value: &Value) -> Result<StopwordsSet, QqlError> {
    match value {
        Value::List(_) => Ok(StopwordsSet {
            languages: Vec::new(),
            custom: index_str_list(key, value)?,
        }),
        Value::Str(language) => Ok(StopwordsSet {
            languages: vec![check_stopword_language(language)?],
            custom: Vec::new(),
        }),
        Value::Dict(entries) => {
            let mut languages = Vec::new();
            let mut custom = Vec::new();
            for (entry_key, entry_value) in entries {
                if entry_key.eq_ignore_ascii_case("languages") {
                    let items = match entry_value {
                        Value::List(items) => items,
                        _ => {
                            return Err(index_option_error(
                                "stopwords languages must be a list of strings",
                            ));
                        }
                    };
                    for item in items {
                        match item {
                            Value::Str(language) => {
                                languages.push(check_stopword_language(language)?);
                            }
                            _ => {
                                return Err(index_option_error(
                                    "stopwords languages must be a list of strings",
                                ));
                            }
                        }
                    }
                } else if entry_key.eq_ignore_ascii_case("custom") {
                    match entry_value {
                        Value::List(items) => {
                            for item in items {
                                match item {
                                    Value::Str(word) => custom.push(word.clone()),
                                    _ => {
                                        return Err(index_option_error(
                                            "stopwords custom must be a list of strings",
                                        ));
                                    }
                                }
                            }
                        }
                        _ => {
                            return Err(index_option_error(
                                "stopwords custom must be a list of strings",
                            ));
                        }
                    }
                } else {
                    return Err(index_option_error(format!(
                        "unknown stopwords set key '{entry_key}'. Expected: languages, custom"
                    )));
                }
            }
            Ok(StopwordsSet { languages, custom })
        }
        _ => Err(index_option_error(format!(
            "{key} must be a list of strings, a language name, or {{languages: […], custom: […]}}"
        ))),
    }
}

/// Validate a stopword language against the OpenAPI `Language` enum
/// (case-insensitive); returns the canonical lowercase name.
fn check_stopword_language(raw: &str) -> Result<String, QqlError> {
    let lower = raw.to_ascii_lowercase();
    if STOPWORD_LANGUAGES.contains(&lower.as_str()) {
        Ok(lower)
    } else {
        Err(index_option_error(format!(
            "unknown stopwords language '{raw}'"
        )))
    }
}

/// OpenAPI `Language` names accepted in `stopwords` language lists.
const STOPWORD_LANGUAGES: &[&str] = &[
    "arabic",
    "azerbaijani",
    "basque",
    "bengali",
    "catalan",
    "chinese",
    "danish",
    "dutch",
    "english",
    "finnish",
    "french",
    "german",
    "greek",
    "hebrew",
    "hinglish",
    "hungarian",
    "indonesian",
    "italian",
    "japanese",
    "kazakh",
    "nepali",
    "norwegian",
    "portuguese",
    "romanian",
    "russian",
    "slovene",
    "spanish",
    "swedish",
    "tajik",
    "turkish",
];

fn fill_collection_config(
    req: &mut CreateCollectionRequest,
    config: &CollectionConfig,
) -> Result<(), QqlError> {
    if let Some(ref h) = config.hnsw {
        req.hnsw_config = Some(lower_hnsw_config(h));
    }
    if let Some(ref o) = config.optimizers {
        req.optimizers_config = Some(lower_optimizers_config(o));
    }
    if let Some(ref p) = config.params {
        req.params = Some(lower_collection_params(p));
        req.shard_number = p.shard_number;
        req.sharding_method = p.sharding_method.as_deref().map(lower_sharding_method);
        req.shard_keys = p.shard_keys.as_ref().map(|keys| {
            keys.iter()
                .map(crate::semantic::PlanShardKey::from)
                .collect()
        });
    }
    if let Some(ref q) = config.quantization {
        req.quantization_config = Some(lower_quantization_config(q));
    }
    if let Some(ref wal) = config.wal {
        req.wal_config = Some(Box::new(lower_wal_config(wal)?));
    }
    if let Some(ref strict) = config.strict_mode {
        req.strict_mode_config = Some(Box::new(lower_strict_mode_config(strict)?));
    }
    if let Some(ref metadata) = config.metadata {
        req.metadata = Some(Box::new(lower_metadata_map(metadata)));
    }
    Ok(())
}

fn fill_update_collection_config(
    req: &mut UpdateCollectionRequest,
    config: &CollectionConfig,
) -> Result<(), QqlError> {
    if let Some(ref h) = config.hnsw {
        req.hnsw_config = Some(lower_hnsw_config(h));
    }
    if let Some(ref o) = config.optimizers {
        req.optimizers_config = Some(lower_optimizers_config(o));
    }
    if let Some(ref p) = config.params {
        req.params = Some(lower_collection_params(p));
    }
    req.quantization_config = lower_quantization_diff(config);
    if let Some(ref strict) = config.strict_mode {
        req.strict_mode_config = Some(Box::new(lower_strict_mode_config(strict)?));
    }
    if let Some(ref metadata) = config.metadata {
        req.metadata = Some(Box::new(lower_metadata_map(metadata)));
    }
    // The update wire shape has no WAL field; the parser rejects
    // `ALTER … WITH WAL`, so a hand-built AST reaching here fails closed.
    if config.wal.is_some() {
        return Err(collection_config_error(
            "WITH WAL is supported only for CREATE COLLECTION",
        ));
    }

    // `ALTER COLLECTION … WITH VECTOR (…)` addresses the default unnamed vector
    // (REST documents the empty-string map key for exactly that case).
    if let Some(ref storage) = config.vectors {
        let diff = lower_storage_diff(storage)?;
        insert_vector_diff(&mut req.vectors, String::new(), diff)?;
    }
    for diff in &config.vector_diffs {
        let mut params = match diff.vectors.as_deref() {
            Some(storage) => lower_storage_diff(storage)?,
            None => VectorParamsDiff::default(),
        };
        params.hnsw_config = diff.hnsw.as_deref().map(lower_hnsw_config);
        params.quantization_config = lower_quantization_update_diff(diff.quantization.as_deref());
        insert_vector_diff(&mut req.vectors, diff.name.clone(), params)?;
    }
    for diff in &config.sparse_vector_diffs {
        let params = SparseVectorParamsDiff {
            index: diff.index.as_deref().map(lower_sparse_index_params),
            modifier: diff.modifier.as_deref().map(lower_sparse_modifier),
        };
        insert_sparse_vector_diff(&mut req.sparse_vectors, diff.name.clone(), params)?;
    }
    Ok(())
}

/// Lower `VECTOR (on_disk = …, memory = …)` storage settings into the dense
/// diff. `datatype` has no `VectorParamsDiff` wire field, so it fails closed.
fn lower_storage_diff(storage: &VectorsConfig) -> Result<VectorParamsDiff, QqlError> {
    if storage.datatype.is_some() {
        return Err(vector_diff_error(
            "datatype cannot be changed per vector: the Qdrant VectorParamsDiff wire shape has no datatype field. Recreate the collection or set the datatype at CREATE COLLECTION",
        ));
    }
    Ok(VectorParamsDiff {
        on_disk: storage.on_disk,
        memory: storage.memory,
        ..Default::default()
    })
}

/// Lower a per-vector `QUANTIZATION (…)` replacement: `disabled = true` clears
/// the vector's quantization, otherwise the config replaces it.
fn lower_quantization_update_diff(
    update: Option<&qql_core::ast::QuantizationUpdate>,
) -> Option<QuantizationConfigDiff> {
    let update = update?;
    if update.disabled {
        return Some(QuantizationConfigDiff::Disabled);
    }
    update
        .config
        .as_deref()
        .map(|config| QuantizationConfigDiff::Config(lower_quantization_config(config)))
}

/// Insert one dense diff, rejecting a duplicate name (the AST can be built by
/// hand, bypassing the parser's duplicate check).
fn insert_vector_diff(
    map: &mut Option<BTreeMap<String, VectorParamsDiff>>,
    name: String,
    diff: VectorParamsDiff,
) -> Result<(), QqlError> {
    let map = map.get_or_insert_with(BTreeMap::new);
    if map.contains_key(&name) {
        return Err(vector_diff_error(format!(
            "duplicate vector diff for '{}'",
            display_vector_name(&name)
        )));
    }
    map.insert(name, diff);
    Ok(())
}

/// Insert one sparse diff, rejecting a duplicate name.
fn insert_sparse_vector_diff(
    map: &mut Option<BTreeMap<String, SparseVectorParamsDiff>>,
    name: String,
    diff: SparseVectorParamsDiff,
) -> Result<(), QqlError> {
    let map = map.get_or_insert_with(BTreeMap::new);
    if map.contains_key(&name) {
        return Err(vector_diff_error(format!(
            "duplicate sparse vector diff for '{}'",
            display_vector_name(&name)
        )));
    }
    map.insert(name, diff);
    Ok(())
}

fn display_vector_name(name: &str) -> &str {
    if name.is_empty() { "<default>" } else { name }
}

/// `none` is the only non-modifying sparse modifier; every other accepted
/// spelling is `idf`.
fn lower_sparse_modifier(raw: &str) -> SparseModifier {
    if raw.eq_ignore_ascii_case("none") {
        SparseModifier::None
    } else {
        SparseModifier::Idf
    }
}

fn vector_diff_error(message: impl Into<alloc::borrow::Cow<'static, str>>) -> QqlError {
    QqlError::validation("QQL-PLAN-VECTOR-DIFF", message, None)
}

fn lower_sparse_index_params(index: &qql_core::ast::SparseIndexConfig) -> SparseIndexParams {
    SparseIndexParams {
        full_scan_threshold: index.full_scan_threshold,
        on_disk: index.on_disk,
        memory: index.memory,
        datatype: index.datatype,
    }
}

/// `ALTER COLLECTION` quantization replacement: `disabled` wins, otherwise the
/// replacement config (falling back to a plain `WITH QUANTIZATION` block).
fn lower_quantization_diff(config: &CollectionConfig) -> Option<QuantizationConfigDiff> {
    if let Some(ref update) = config.quantization_update {
        if update.disabled {
            return Some(QuantizationConfigDiff::Disabled);
        }
        return update
            .config
            .as_deref()
            .or(config.quantization.as_deref())
            .map(|q| QuantizationConfigDiff::Config(lower_quantization_config(q)));
    }
    config
        .quantization
        .as_deref()
        .map(|q| QuantizationConfigDiff::Config(lower_quantization_config(q)))
}

fn lower_collection_params(config: &qql_core::ast::CollectionParamsConfig) -> CollectionParams {
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

fn lower_sharding_method(method: &str) -> ShardingMethod {
    if method.eq_ignore_ascii_case("custom") {
        ShardingMethod::Custom
    } else {
        ShardingMethod::Auto
    }
}

fn collection_config_error(message: impl Into<alloc::borrow::Cow<'static, str>>) -> QqlError {
    QqlError::validation("QQL-PLAN-COLLECTION-CONFIG", message, None)
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
        Some(_) => Err(collection_config_error(format!(
            "{key} must be a non-negative integer"
        ))),
    }
}

/// Extract an optional positive integer pair (`None` when absent).
fn config_pair_positive_u64(pairs: &[(String, Value)], key: &str) -> Result<Option<u64>, QqlError> {
    match config_pair(pairs, key) {
        None => Ok(None),
        Some(Value::Int(n)) if *n > 0 => Ok(Some(*n as u64)),
        Some(Value::Float(n)) if *n > 0.0 && *n == (*n as u64) as f64 => Ok(Some(*n as u64)),
        Some(_) => Err(collection_config_error(format!(
            "{key} must be a positive integer"
        ))),
    }
}

/// Extract an optional boolean pair (`None` when absent).
fn config_pair_bool(pairs: &[(String, Value)], key: &str) -> Result<Option<bool>, QqlError> {
    match config_pair(pairs, key) {
        None => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(collection_config_error(format!(
            "{key} must be true or false"
        ))),
    }
}

/// Extract an optional finite number pair (`None` when absent).
fn config_pair_f64(pairs: &[(String, Value)], key: &str) -> Result<Option<f64>, QqlError> {
    match config_pair(pairs, key) {
        None => Ok(None),
        Some(Value::Int(n)) => Ok(Some(*n as f64)),
        Some(Value::Float(n)) if n.is_finite() => Ok(Some(*n)),
        Some(_) => Err(collection_config_error(format!("{key} must be a number"))),
    }
}

/// Lower `WITH WAL (…)` pairs to the OpenAPI `WalConfigDiff` shape.
fn lower_wal_config(pairs: &[(String, Value)]) -> Result<WalConfig, QqlError> {
    for (key, _) in pairs {
        if !(key.eq_ignore_ascii_case("wal_capacity_mb")
            || key.eq_ignore_ascii_case("wal_segments_ahead")
            || key.eq_ignore_ascii_case("wal_retain_closed"))
        {
            return Err(collection_config_error(format!(
                "unknown WAL parameter '{key}'. Expected: wal_capacity_mb, wal_segments_ahead, wal_retain_closed"
            )));
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
fn lower_strict_mode_config(pairs: &[(String, Value)]) -> Result<StrictModeConfig, QqlError> {
    for (key, _) in pairs {
        if !is_strict_mode_key(key) {
            return Err(collection_config_error(format!(
                "unknown STRICT_MODE parameter '{key}'"
            )));
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
                return Err(collection_config_error(
                    "max_resident_memory_percent must be an integer in [1, 100]",
                ));
            }
        }
    }
    Ok(config)
}

/// Lower a `multivector_config` strict-mode value (`{name: {max_vectors}}`).
fn lower_strict_multivector_config(value: &Value) -> Result<StrictModeMultivectorConfig, QqlError> {
    let Value::Dict(entries) = value else {
        return Err(collection_config_error(
            "multivector_config must be an object",
        ));
    };
    let mut vectors = BTreeMap::new();
    for (name, caps) in entries {
        let Value::Dict(fields) = caps else {
            return Err(collection_config_error(format!(
                "multivector_config '{name}' must be an object"
            )));
        };
        let mut entry = StrictModeMultivector::default();
        for (key, val) in fields {
            if !key.eq_ignore_ascii_case("max_vectors") {
                return Err(collection_config_error(format!(
                    "unknown multivector_config key '{key}'. Expected: max_vectors"
                )));
            }
            match val {
                Value::Int(n) if *n >= 1 => entry.max_vectors = Some(*n as u64),
                _ => {
                    return Err(collection_config_error(
                        "max_vectors must be a positive integer",
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
        return Err(collection_config_error("sparse_config must be an object"));
    };
    let mut vectors = BTreeMap::new();
    for (name, caps) in entries {
        let Value::Dict(fields) = caps else {
            return Err(collection_config_error(format!(
                "sparse_config '{name}' must be an object"
            )));
        };
        let mut entry = StrictModeSparse::default();
        for (key, val) in fields {
            if !key.eq_ignore_ascii_case("max_length") {
                return Err(collection_config_error(format!(
                    "unknown sparse_config key '{key}'. Expected: max_length"
                )));
            }
            match val {
                Value::Int(n) if *n >= 1 => entry.max_length = Some(*n as u64),
                _ => {
                    return Err(collection_config_error(
                        "max_length must be a positive integer",
                    ));
                }
            }
        }
        vectors.insert(name.clone(), entry);
    }
    Ok(StrictModeSparseConfig { vectors })
}

/// Lower `WITH METADATA (…)` pairs to the free-form metadata map.
///
/// `ensure_no_unbound_params` (via `validate_no_unbound_ddl_options`) rejects
/// placeholders before lowering, so `value_to_json` never sees one here
/// through the supported entry points.
fn lower_metadata_map(pairs: &[(String, Value)]) -> serde_json::Map<String, serde_json::Value> {
    let mut map = serde_json::Map::with_capacity(pairs.len());
    for (key, value) in pairs {
        map.insert(key.clone(), crate::filter::value_to_json(value));
    }
    map
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

fn apply_vector_defaults(vectors: Option<&mut DenseVectorsConfig>, default: &VectorsConfig) {
    fn apply(params: &mut DenseVectorParams, default: &VectorsConfig) {
        params.on_disk = params.on_disk.or(default.on_disk);
        params.memory = params.memory.or(default.memory);
        params.datatype = params.datatype.or(default.datatype);
    }
    match vectors {
        Some(DenseVectorsConfig::Single(params)) => apply(params, default),
        Some(DenseVectorsConfig::Named(map)) => {
            for params in map.values_mut() {
                apply(params, default);
            }
        }
        None => {}
    }
}

/// Lower an AST HNSW config into the typed plan `HnswConfig` IR.
pub fn lower_hnsw_config(config: &qql_core::ast::HnswRuntimeConfig) -> HnswConfig {
    HnswConfig {
        m: config.m,
        ef_construct: config.ef_construct,
        full_scan_threshold: config.full_scan_threshold,
        max_indexing_threads: config.max_indexing_threads,
        on_disk: config.on_disk,
        payload_m: config.payload_m,
        inline_storage: config.inline_storage,
        memory: config.memory,
    }
}

/// Lower an AST optimizer config into the typed plan `OptimizersConfig` IR.
pub fn lower_optimizers_config(
    config: &qql_core::ast::OptimizersRuntimeConfig,
) -> OptimizersConfig {
    OptimizersConfig {
        deleted_threshold: config.deleted_threshold,
        vacuum_min_vector_number: config.vacuum_min_vector_number,
        default_segment_number: config.default_segment_number,
        max_segment_size: config.max_segment_size,
        memmap_threshold: config.memmap_threshold,
        indexing_threshold: config.indexing_threshold,
        flush_interval_sec: config.flush_interval_sec,
        max_optimization_threads: config.max_optimization_threads.as_ref().map(|threads| {
            if threads.auto_ {
                MaxOptimizationThreads::Auto
            } else {
                MaxOptimizationThreads::Threads(threads.value)
            }
        }),
        prevent_unoptimized: config.prevent_unoptimized,
    }
}

/// Lower an AST quantization config into the typed plan `QuantizationConfig`.
///
/// `compression` defaults to `x4` (the OpenAPI enum has no empty string) and is
/// normalized to lowercase, mirroring the REST projection this replaces.
pub fn lower_quantization_config(config: &qql_core::ast::QuantizationConfig) -> QuantizationConfig {
    match config.qtype {
        qql_core::ast::QuantizationType::Scalar => QuantizationConfig::Scalar {
            scalar: ScalarQuantization {
                qtype: "int8".into(),
                quantile: config.quantile,
                always_ram: Some(config.always_ram),
                memory: config.memory,
            },
        },
        qql_core::ast::QuantizationType::Product => QuantizationConfig::Product {
            product: ProductQuantization {
                compression: config
                    .compression
                    .as_deref()
                    .map(|c| c.to_ascii_lowercase())
                    .unwrap_or_else(|| "x4".into()),
                always_ram: Some(config.always_ram),
                memory: config.memory,
            },
        },
        qql_core::ast::QuantizationType::Binary => QuantizationConfig::Binary {
            binary: BinaryQuantization {
                always_ram: Some(config.always_ram),
                encoding: config.encoding.clone(),
                query_encoding: config.query_encoding.clone(),
                memory: config.memory,
            },
        },
        qql_core::ast::QuantizationType::Turbo => QuantizationConfig::Turbo {
            turbo: TurboQuantization {
                bits: turbo_bits_label(config.bits),
                always_ram: Some(config.always_ram),
                memory: config.memory,
            },
        },
    }
}

/// Map numeric turbo bits onto the OpenAPI `TurboQuantBitSize` label.
///
/// Unknown values still emit a best-effort `bits<value>` label so the backend
/// can reject them clearly instead of silently mislabeling as `bits1`.
fn turbo_bits_label(bits: Option<f64>) -> Option<String> {
    let bits = bits?;
    let label = if (bits - 1.5).abs() < f64::EPSILON {
        "bits1_5"
    } else if (bits - 2.0).abs() < f64::EPSILON {
        "bits2"
    } else if (bits - 4.0).abs() < f64::EPSILON {
        "bits4"
    } else if (bits - 1.0).abs() < f64::EPSILON {
        "bits1"
    } else {
        return Some(format!("bits{bits}"));
    };
    Some(label.to_string())
}

/// Lower a `SET QUOTA (…)` statement to a typed quota request.
///
/// `PUT /quotas` **replaces** the entire cluster-wide config. Omitted keys
/// (including `key = null`) are not set on the replacement body, so they
/// become "uncapped / default" in the new config — not a patch of the old
/// one. Callers that want to keep existing limits must restate them.
pub(crate) fn lower_set_quota(
    stmt: &qql_core::ast::SetQuotaStmt,
) -> Result<SetQuotaRequest, QqlError> {
    let mut request = SetQuotaRequest {
        config: QuotaConfig::default(),
        wait: stmt.wait,
    };
    for (key, value) in &stmt.config {
        let lower = key.to_ascii_lowercase();
        match lower.as_str() {
            "enabled" => match value {
                Value::Bool(b) => request.config.enabled = Some(*b),
                _ => {
                    return Err(QqlError::validation(
                        "QQL-PLAN-QUOTA",
                        "enabled must be true or false",
                        None,
                    ));
                }
            },
            "max_resident_memory_percent" | "max_disk_usage_percent" => {
                request = apply_quota_percent(request, &lower, value, 1, 100)?;
            }
            "release_margin_percent" => {
                request = apply_quota_percent(request, &lower, value, 0, 100)?;
            }
            _ => {
                return Err(QqlError::validation(
                    "QQL-PLAN-QUOTA",
                    format!(
                        "unknown quota parameter '{key}'. Expected: enabled, max_resident_memory_percent, max_disk_usage_percent, release_margin_percent"
                    ),
                    None,
                ));
            }
        }
    }
    Ok(request)
}

fn apply_quota_percent(
    mut request: SetQuotaRequest,
    key: &str,
    value: &Value,
    min: u64,
    max: u64,
) -> Result<SetQuotaRequest, QqlError> {
    match value {
        // Explicit null → leave field unset so the replacement config has no
        // cap for this resource (full PUT replace semantics).
        Value::Null => {}
        Value::Int(n) if *n >= min as i64 && (*n as u64) <= max => {
            let n = *n as u64;
            match key {
                "max_resident_memory_percent" => {
                    request.config.max_resident_memory_percent = Some(n)
                }
                "max_disk_usage_percent" => request.config.max_disk_usage_percent = Some(n),
                _ => request.config.release_margin_percent = Some(n),
            }
        }
        _ => {
            return Err(QqlError::validation(
                "QQL-PLAN-QUOTA",
                format!("{key} must be an integer in [{min}, {max}] or null"),
                None,
            ));
        }
    }
    Ok(request)
}

#[cfg(test)]
mod tests {
    use super::*;
    use qql_core::ast::Stmt;
    use qql_core::parser::Parser;

    fn parse_stmt(s: &str) -> Stmt {
        Parser::parse(s).expect("parse failed")
    }

    macro_rules! rest_json {
        ($req:expr) => {
            serde_json::to_value(create_collection_rest_body($req)).unwrap()
        };
    }

    #[test]
    fn create_collection_dense() {
        let stmt = parse_stmt("CREATE COLLECTION docs (dense VECTOR(384, COSINE));");
        let Stmt::CreateCollection(ref cc) = stmt else {
            panic!()
        };
        let req = lower_create_collection(cc).unwrap();
        let json = rest_json!(&req);
        assert_eq!(json["vectors"]["dense"]["size"], 384);
        assert_eq!(json["vectors"]["dense"]["distance"], "Cosine");
    }

    #[test]
    fn single_vector_config_serializes_as_bare_params() {
        let req = CreateCollectionRequest {
            vectors: Some(DenseVectorsConfig::Single(DenseVectorParams {
                size: 8,
                distance: qql_core::ast::VectorDistance::Dot,
                hnsw_config: None,
                quantization_config: None,
                on_disk: None,
                memory: None,
                datatype: None,
                multivector_config: None,
            })),
            ..Default::default()
        };
        let json = rest_json!(&req);
        assert_eq!(json["vectors"]["size"], 8);
        assert_eq!(json["vectors"]["distance"], "Dot");
    }

    #[test]
    fn create_collection_with_config() {
        let stmt =
            parse_stmt("CREATE COLLECTION docs (dense VECTOR(128, EUCLID)) WITH HNSW (m = 16);");
        let Stmt::CreateCollection(ref cc) = stmt else {
            panic!()
        };
        let req = lower_create_collection(cc).unwrap();
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["hnsw_config"]["m"], 16);
    }

    #[test]
    fn create_index() {
        let stmt = parse_stmt(
            "CREATE INDEX ON COLLECTION docs FOR title TYPE text WITH (lowercase = true);",
        );
        let Stmt::CreateIndex(ref ci) = stmt else {
            panic!()
        };
        let req = lower_create_index(ci).unwrap();
        // IR keeps typed options separate from the schema.
        let ir = serde_json::to_value(&req).unwrap();
        assert_eq!(ir["field_name"], "title");
        assert_eq!(ir["field_schema"], "text");
        assert_eq!(ir["options"]["lowercase"], true);
        // REST OpenAPI nests options under field_schema object
        let rest = serde_json::to_value(create_index_rest_body(&req)).unwrap();
        assert_eq!(rest["field_name"], "title");
        assert_eq!(rest["field_schema"]["type"], "text");
        assert_eq!(rest["field_schema"]["lowercase"], true);
        assert!(rest.get("lowercase").is_none());
    }

    #[test]
    fn create_index_without_options_uses_type_string() {
        let stmt = parse_stmt("CREATE INDEX ON COLLECTION docs FOR tag TYPE keyword;");
        let Stmt::CreateIndex(ref ci) = stmt else {
            panic!()
        };
        let req = lower_create_index(ci).unwrap();
        let rest = serde_json::to_value(create_index_rest_body(&req)).unwrap();
        assert_eq!(rest["field_schema"], "keyword");
    }

    #[test]
    fn create_keyword_index_with_prefix_and_memory() {
        let stmt = parse_stmt(
            "CREATE INDEX ON COLLECTION docs FOR tenant TYPE keyword WITH (prefix = true, memory = 'cached', is_tenant = true);",
        );
        let Stmt::CreateIndex(ref ci) = stmt else {
            panic!()
        };
        let req = lower_create_index(ci).unwrap();
        let rest = serde_json::to_value(create_index_rest_body(&req)).unwrap();
        assert_eq!(rest["field_schema"]["type"], "keyword");
        assert_eq!(rest["field_schema"]["prefix"], true);
        assert_eq!(rest["field_schema"]["memory"], "cached");
        assert_eq!(rest["field_schema"]["is_tenant"], true);
        // Typed options survive for the gRPC payload_index_params path.
        assert_eq!(req.options.prefix, Some(true));
        assert_eq!(req.options.memory, Some(MemoryPlacement::Cached));
    }

    #[test]
    fn create_text_index_typed_options() {
        let stmt = parse_stmt(
            "CREATE INDEX ON COLLECTION docs FOR body TYPE text WITH (tokenizer = 'WORD', lowercase = true, min_token_len = 2, max_token_len = 10, stopwords = ['the', 'a'], stemmer = 'English');",
        );
        let Stmt::CreateIndex(ref ci) = stmt else {
            panic!()
        };
        let req = lower_create_index(ci).unwrap();
        assert_eq!(req.options.tokenizer, Some(TextTokenizer::Word));
        assert_eq!(req.options.min_token_len, Some(2));
        assert_eq!(req.options.max_token_len, Some(10));
        assert_eq!(
            req.options.stopwords,
            Some(StopwordsSet {
                languages: Vec::new(),
                custom: vec!["the".into(), "a".into()]
            })
        );
        assert_eq!(
            req.options.stemmer,
            Some(StemmingAlgorithm::Snowball("english".into()))
        );
        let rest = serde_json::to_value(create_index_rest_body(&req)).unwrap();
        assert_eq!(rest["field_schema"]["tokenizer"], "word");
        assert_eq!(rest["field_schema"]["stemmer"]["type"], "snowball");
        assert_eq!(rest["field_schema"]["stemmer"]["language"], "english");
        assert_eq!(rest["field_schema"]["stopwords"]["custom"][0], "the");
    }

    #[test]
    fn create_index_fails_closed_on_unknown_values() {
        let Stmt::CreateIndex(ci) = parse_stmt(
            "CREATE INDEX ON COLLECTION docs FOR body TYPE text WITH (tokenizer = 'bogus');",
        ) else {
            panic!()
        };
        let err = lower_create_index(&ci).unwrap_err();
        assert_eq!(err.code, "QQL-PLAN-INDEX-OPTION");

        // The parser rejects unknown field types; a programmatically built AST
        // still fails closed in the planner.
        let ci = qql_core::ast::CreateIndexStmt {
            collection: "docs".into(),
            field: "body".into(),
            field_type: "bogus".into(),
            options: Vec::new(),
            wait: None,
        };
        let err = lower_create_index(&ci).unwrap_err();
        assert_eq!(err.code, "QQL-PLAN-INDEX-TYPE");
    }

    #[test]
    fn lower_product_quantization_includes_compression() {
        let stmt = parse_stmt(
            "CREATE COLLECTION docs (v VECTOR(128, COSINE) WITH QUANTIZATION (type = 'product', compression = 'x16', always_ram = true));",
        );
        let Stmt::CreateCollection(ref cc) = stmt else {
            panic!()
        };
        let req = lower_create_collection(cc).unwrap();
        let json = rest_json!(&req);
        let quant = &json["vectors"]["v"]["quantization_config"];
        assert_eq!(quant["product"]["compression"], "x16");
        assert_eq!(quant["product"]["always_ram"], true);
    }

    #[test]
    fn lower_product_quantization_defaults_compression() {
        let stmt = parse_stmt(
            "CREATE COLLECTION docs (v VECTOR(128, COSINE) WITH QUANTIZATION (type = 'product', always_ram = true));",
        );
        let Stmt::CreateCollection(ref cc) = stmt else {
            panic!()
        };
        let req = lower_create_collection(cc).unwrap();
        let json = rest_json!(&req);
        assert_eq!(
            json["vectors"]["v"]["quantization_config"]["product"]["compression"],
            "x4"
        );
    }

    #[test]
    fn lower_binary_quantization_includes_encoding() {
        let stmt = parse_stmt(
            "CREATE COLLECTION docs (v VECTOR(128, COSINE) WITH QUANTIZATION (type = 'binary', encoding = 'two_bits', always_ram = true));",
        );
        let Stmt::CreateCollection(ref cc) = stmt else {
            panic!()
        };
        let req = lower_create_collection(cc).unwrap();
        let json = rest_json!(&req);
        let quant = &json["vectors"]["v"]["quantization_config"];
        assert_eq!(quant["binary"]["encoding"], "two_bits");
        assert_eq!(quant["binary"]["always_ram"], true);
    }

    #[test]
    fn lower_turbo_quantization_includes_bits() {
        let stmt = parse_stmt(
            "CREATE COLLECTION docs (v VECTOR(128, COSINE) WITH QUANTIZATION (type = 'turbo', bits = 1.5, always_ram = true));",
        );
        let Stmt::CreateCollection(ref cc) = stmt else {
            panic!()
        };
        let req = lower_create_collection(cc).unwrap();
        let json = rest_json!(&req);
        let quant = &json["vectors"]["v"]["quantization_config"];
        assert_eq!(quant["turbo"]["bits"], "bits1_5");
        assert_eq!(quant["turbo"]["always_ram"], true);
    }

    #[test]
    fn turbo_bits_label_maps_known_values() {
        // Unknown values stay backend-rejectable (no silent "bits1").
        for (bits, expected) in [
            (1.0, "bits1"),
            (1.5, "bits1_5"),
            (2.0, "bits2"),
            (4.0, "bits4"),
        ] {
            assert_eq!(turbo_bits_label(Some(bits)).as_deref(), Some(expected));
        }
        assert_eq!(turbo_bits_label(Some(3.0)).as_deref(), Some("bits3"));
    }

    #[test]
    fn lower_vector_on_disk_and_query_encoding_and_multivector() {
        let stmt = parse_stmt(
            "CREATE COLLECTION docs (v VECTOR(64, COSINE) WITH MULTIVECTOR (comparator = 'max_sim') WITH VECTOR (on_disk = true) WITH QUANTIZATION (type = 'binary', encoding = 'two_bits', query_encoding = 'scalar4bits', always_ram = true));",
        );
        let Stmt::CreateCollection(ref cc) = stmt else {
            panic!()
        };
        let req = lower_create_collection(cc).unwrap();
        let json = rest_json!(&req);
        let v = &json["vectors"]["v"];
        assert_eq!(v["on_disk"], true);
        assert_eq!(v["multivector_config"]["comparator"], "max_sim");
        assert_eq!(v["quantization_config"]["binary"]["encoding"], "two_bits");
        assert_eq!(
            v["quantization_config"]["binary"]["query_encoding"],
            "scalar4bits"
        );
    }

    #[test]
    fn collection_wide_vector_defaults_apply_to_every_vector() {
        let stmt = parse_stmt(
            "CREATE COLLECTION docs (v VECTOR(8, COSINE), w VECTOR(4, DOT)) WITH VECTOR (on_disk = true, memory = 'cached', datatype = 'float16');",
        );
        let Stmt::CreateCollection(ref cc) = stmt else {
            panic!()
        };
        let req = lower_create_collection(cc).unwrap();
        let json = rest_json!(&req);
        for name in ["v", "w"] {
            assert_eq!(json["vectors"][name]["on_disk"], true);
            assert_eq!(json["vectors"][name]["memory"], "cached");
            assert_eq!(json["vectors"][name]["datatype"], "float16");
        }
    }

    #[test]
    fn collection_wide_vector_defaults_do_not_override_per_vector() {
        let stmt = parse_stmt(
            "CREATE COLLECTION docs (v VECTOR(8, COSINE) WITH VECTOR (on_disk = false, memory = 'cold')) WITH VECTOR (on_disk = true, memory = 'cached', datatype = 'float16');",
        );
        let Stmt::CreateCollection(ref cc) = stmt else {
            panic!()
        };
        let req = lower_create_collection(cc).unwrap();
        let json = rest_json!(&req);
        assert_eq!(json["vectors"]["v"]["on_disk"], false);
        assert_eq!(json["vectors"]["v"]["memory"], "cold");
        // Unset per-vector fields still pick up the collection default.
        assert_eq!(json["vectors"]["v"]["datatype"], "float16");
    }

    #[test]
    fn lower_optimizers_auto_threads() {
        let stmt = parse_stmt(
            "CREATE COLLECTION docs (v VECTOR(8, COSINE)) WITH OPTIMIZERS (max_optimization_threads = 'auto', indexing_threshold = 1000);",
        );
        let Stmt::CreateCollection(ref cc) = stmt else {
            panic!()
        };
        let req = lower_create_collection(cc).unwrap();
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(
            json["optimizers_config"]["max_optimization_threads"],
            "auto"
        );
        assert_eq!(json["optimizers_config"]["indexing_threshold"], 1000);
    }

    #[test]
    fn rest_body_flattens_params_and_nests_quantization() {
        let stmt = parse_stmt(
            "CREATE COLLECTION docs (v VECTOR(128, COSINE) WITH QUANTIZATION (type = 'scalar', quantile = 0.99, always_ram = true)) \
             WITH HNSW (m = 16) \
             WITH PARAMS (replication_factor = 2, write_consistency_factor = 1, on_disk_payload = true, shard_number = 4, sharding_method = 'custom', shard_keys = ['a', 'b']);",
        );
        let Stmt::CreateCollection(ref cc) = stmt else {
            panic!()
        };
        let req = lower_create_collection(cc).unwrap();
        let rest = rest_json!(&req);
        // OpenAPI top-level params
        assert_eq!(rest["replication_factor"], 2);
        assert_eq!(rest["write_consistency_factor"], 1);
        assert_eq!(rest["on_disk_payload"], true);
        assert_eq!(rest["shard_number"], 4);
        assert_eq!(rest["sharding_method"], "custom");
        assert!(
            rest.get("params").is_none(),
            "params must not be nested on create"
        );
        assert!(
            rest.get("shard_keys").is_none(),
            "shard_keys not on CreateCollection"
        );
        // Nested quantization
        assert_eq!(
            rest["vectors"]["v"]["quantization_config"]["scalar"]["type"],
            "int8"
        );
        assert_eq!(
            rest["vectors"]["v"]["quantization_config"]["scalar"]["quantile"],
            0.99
        );
        assert_eq!(rest["hnsw_config"]["m"], 16);
        // Deferred fan-out only when IR carries those keys (ALTER-only in grammar;
        // still projected for gRPC/REST multi-step if present).
        let mut with_fanout = req.clone();
        with_fanout
            .params
            .as_mut()
            .expect("params")
            .read_fan_out_factor = Some(3);
        let deferred = serde_json::to_value(
            create_collection_deferred_params_rest(&with_fanout).expect("deferred params"),
        )
        .unwrap();
        assert_eq!(deferred["params"]["read_fan_out_factor"], 3);
        // IR keeps typed vectors and shard keys
        let ir = serde_json::to_value(&req).unwrap();
        assert_eq!(
            ir["vectors"]["v"]["quantization_config"]["scalar"]["type"],
            "int8"
        );
        assert_eq!(ir["params"]["replication_factor"], 2);
        assert_eq!(ir["sharding_method"], "custom");
        assert_eq!(ir["shard_keys"], serde_json::json!(["a", "b"]));
    }

    #[test]
    fn rest_body_config_sections_are_objects_not_null() {
        // Regression: these sections used `serde_json::to_value(...).unwrap_or_default()`,
        // silently inserting JSON `null` on any serialization failure. Assert the
        // create REST body carries real objects on the wire.
        let stmt = parse_stmt(
            "CREATE COLLECTION docs (v VECTOR(128, COSINE)) WITH HNSW (m = 16) WITH OPTIMIZERS (indexing_threshold = 1000) WITH QUANTIZATION (type = 'scalar', quantile = 0.5);",
        );
        let Stmt::CreateCollection(ref cc) = stmt else {
            panic!()
        };
        let req = lower_create_collection(cc).unwrap();
        let rest = rest_json!(&req);
        assert!(rest["hnsw_config"].is_object());
        assert_eq!(rest["hnsw_config"]["m"], 16);
        assert!(rest["optimizers_config"].is_object());
        assert_eq!(rest["optimizers_config"]["indexing_threshold"], 1000);
        assert!(rest["quantization_config"].is_object());
        assert_eq!(rest["quantization_config"]["scalar"]["type"], "int8");
        assert_eq!(rest["quantization_config"]["scalar"]["quantile"], 0.5);
    }

    #[test]
    fn update_rest_body_config_sections_are_objects_not_null() {
        // Regression for the PATCH body (same swallowed-serialization footgun
        // as create). PATCH body must carry real hnsw/optimizers objects.
        let stmt = parse_stmt(
            "ALTER COLLECTION docs WITH HNSW (m = 32) WITH OPTIMIZERS (indexing_threshold = 500);",
        );
        let Stmt::AlterCollection(ref ac) = stmt else {
            panic!()
        };
        let req = lower_alter_collection(ac).unwrap();
        let rest = serde_json::to_value(&req).unwrap();
        assert!(rest["hnsw_config"].is_object());
        assert_eq!(rest["hnsw_config"]["m"], 32);
        assert!(rest["optimizers_config"].is_object());
        assert_eq!(rest["optimizers_config"]["indexing_threshold"], 500);
    }

    #[test]
    fn update_rest_body_disabled_quantization_and_product_default() {
        let stmt = parse_stmt("ALTER COLLECTION docs WITH QUANTIZATION (disabled = true);");
        let Stmt::AlterCollection(ref ac) = stmt else {
            panic!()
        };
        let req = lower_alter_collection(ac).unwrap();
        assert!(matches!(
            req.quantization_config,
            Some(QuantizationConfigDiff::Disabled)
        ));
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["quantization_config"], "Disabled");

        let stmt = parse_stmt(
            "ALTER COLLECTION docs WITH QUANTIZATION (type = 'product', always_ram = true);",
        );
        let Stmt::AlterCollection(ref ac) = stmt else {
            panic!()
        };
        let req = lower_alter_collection(ac).unwrap();
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["quantization_config"]["product"]["compression"], "x4");
        assert_eq!(json["quantization_config"]["product"]["always_ram"], true);
    }

    #[test]
    fn update_rest_body_nests_params_with_payload() {
        let stmt = parse_stmt(
            "ALTER COLLECTION docs WITH PARAMS (replication_factor = 3, payload_memory = 'cached');",
        );
        let Stmt::AlterCollection(ref ac) = stmt else {
            panic!()
        };
        let req = lower_alter_collection(ac).unwrap();
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["params"]["replication_factor"], 3);
        assert_eq!(json["params"]["payload"]["memory"], "cached");
    }

    #[test]
    fn update_rest_body_lowers_named_vector_diffs() {
        let stmt = parse_stmt(
            "ALTER COLLECTION docs \
             WITH VECTOR dense (HNSW (m = 32, memory = 'cold'), QUANTIZATION (type = 'binary', encoding = 'two_bits'), VECTOR (on_disk = true, memory = 'cached')) \
             WITH VECTOR colbert (QUANTIZATION (disabled = true)) \
             WITH SPARSE bm25 (SPARSE (modifier = 'none', full_scan_threshold = 5000, memory = 'pinned', datatype = 'float16'));",
        );
        let Stmt::AlterCollection(ref ac) = stmt else {
            panic!()
        };
        let req = lower_alter_collection(ac).unwrap();
        let json = serde_json::to_value(&req).unwrap();

        let dense = &json["vectors"]["dense"];
        assert_eq!(dense["hnsw_config"]["m"], 32);
        assert_eq!(dense["hnsw_config"]["memory"], "cold");
        assert_eq!(
            dense["quantization_config"]["binary"]["encoding"],
            "two_bits"
        );
        assert_eq!(dense["on_disk"], true);
        assert_eq!(dense["memory"], "cached");
        // Unset keys stay absent (field-wise diff), never serialized as null.
        assert!(dense.get("datatype").is_none());

        // `disabled = true` clears the per-vector quantization config.
        assert_eq!(
            json["vectors"]["colbert"]["quantization_config"],
            "Disabled"
        );

        let sparse = &json["sparse_vectors"]["bm25"];
        assert_eq!(sparse["modifier"], "none");
        assert_eq!(sparse["index"]["full_scan_threshold"], 5000);
        assert_eq!(sparse["index"]["memory"], "pinned");
        assert_eq!(sparse["index"]["datatype"], "float16");
    }

    #[test]
    fn update_rest_body_unnamed_vector_diff_uses_empty_key() {
        let stmt =
            parse_stmt("ALTER COLLECTION docs WITH VECTOR (on_disk = true, memory = 'cold');");
        let Stmt::AlterCollection(ref ac) = stmt else {
            panic!()
        };
        let req = lower_alter_collection(ac).unwrap();
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["vectors"][""]["on_disk"], true);
        assert_eq!(json["vectors"][""]["memory"], "cold");
        assert!(json.get("sparse_vectors").is_none());
    }

    #[test]
    fn update_rest_body_per_vector_hnsw_only() {
        let stmt =
            parse_stmt("ALTER COLLECTION docs WITH VECTOR dense (HNSW (ef_construct = 200));");
        let Stmt::AlterCollection(ref ac) = stmt else {
            panic!()
        };
        let req = lower_alter_collection(ac).unwrap();
        let json = serde_json::to_value(&req).unwrap();
        let dense = &json["vectors"]["dense"];
        assert_eq!(dense["hnsw_config"]["ef_construct"], 200);
        assert_eq!(dense.as_object().unwrap().len(), 1, "only the HNSW key");
    }

    #[test]
    fn update_plan_rejects_datatype_and_duplicate_names() {
        // The parser rejects `datatype` inside a diff; a hand-built AST must
        // fail closed in the planner instead of silently dropping it.
        let stmt = parse_stmt("ALTER COLLECTION docs WITH VECTOR dense (HNSW (m = 16));");
        let Stmt::AlterCollection(mut ac) = stmt else {
            panic!()
        };
        ac.config.as_mut().unwrap().vector_diffs[0].vectors = Some(Box::new(VectorsConfig {
            on_disk: None,
            memory: None,
            datatype: Some(qql_core::ast::VectorDatatype::Float16),
        }));
        let err = lower_alter_collection(&ac).unwrap_err();
        assert_eq!(err.code, "QQL-PLAN-VECTOR-DIFF");
        assert!(err.message.contains("datatype"), "{err}");

        // The unnamed/default form shares the create-shaped VECTOR block
        // parser, so the datatype rejection also lands at planning.
        let stmt = parse_stmt("ALTER COLLECTION docs WITH VECTOR (datatype = 'float16');");
        let Stmt::AlterCollection(ref ac) = stmt else {
            panic!()
        };
        let err = lower_alter_collection(ac).unwrap_err();
        assert_eq!(err.code, "QQL-PLAN-VECTOR-DIFF");
        assert!(err.message.contains("datatype"), "{err}");

        // Duplicate diff names (possible only via a hand-built AST) fail closed
        // instead of letting the BTreeMap keep the last entry.
        let stmt = parse_stmt("ALTER COLLECTION docs WITH VECTOR dense (HNSW (m = 16));");
        let Stmt::AlterCollection(mut ac) = stmt else {
            panic!()
        };
        let mut duplicate = ac.config.as_ref().unwrap().vector_diffs[0].clone();
        duplicate.hnsw = None;
        ac.config.as_mut().unwrap().vector_diffs.push(duplicate);
        let err = lower_alter_collection(&ac).unwrap_err();
        assert_eq!(err.code, "QQL-PLAN-VECTOR-DIFF");
        assert!(err.message.contains("duplicate vector diff"), "{err}");
    }

    #[test]
    fn lower_sparse_vector_full_config_and_sharding_method() {
        let stmt = parse_stmt(
            "CREATE COLLECTION docs (bm25 SPARSE WITH SPARSE (modifier = 'idf', full_scan_threshold = 10000, on_disk = true, datatype = 'float32')) WITH PARAMS (sharding_method = 'custom', shard_number = 2);",
        );
        let Stmt::CreateCollection(ref cc) = stmt else {
            panic!()
        };
        let req = lower_create_collection(cc).unwrap();
        let json = rest_json!(&req);
        let sparse = &json["sparse_vectors"]["bm25"];
        assert_eq!(sparse["modifier"], "idf");
        assert_eq!(sparse["index"]["full_scan_threshold"], 10000);
        assert_eq!(sparse["index"]["on_disk"], true);
        assert_eq!(sparse["index"]["datatype"], "float32");
        assert_eq!(json["sharding_method"], "custom");
        assert_eq!(json["shard_number"], 2);
    }

    #[test]
    fn lower_sparse_default_modifier_is_idf() {
        let stmt = parse_stmt("CREATE COLLECTION docs (bm25 SPARSE);");
        let Stmt::CreateCollection(ref cc) = stmt else {
            panic!()
        };
        let req = lower_create_collection(cc).unwrap();
        let json = rest_json!(&req);
        assert_eq!(json["sparse_vectors"]["bm25"]["modifier"], "idf");
    }
}
