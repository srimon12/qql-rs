use alloc::boxed::Box;

use crate::ast::{
    CollectionConfig, CollectionParamsConfig, HnswRuntimeConfig, MemoryPlacement,
    MultivectorComparator, MultivectorConfig, OptimizersRuntimeConfig, QuantizationConfig,
    QuantizationType, QuantizationUpdate, ShardKey, SparseIndexConfig, Value, VectorDatatype,
    VectorsConfig,
};
use crate::error::{QqlError, Span};
use crate::token::TokenKind;

use super::config_validation::is_integer_val;
use super::{
    AstLowerer, ascii_equal, config_bool, config_float_range, config_has_key,
    config_max_optimization_threads, config_non_negative_u64, config_positive_u64, config_value,
    merge_collection_config, validate_hnsw_value, validate_optimizers_value, validate_params_value,
    validate_vectors_value,
};

fn validation_err(message: impl Into<alloc::borrow::Cow<'static, str>>, span: Span) -> QqlError {
    QqlError::validation("QQL-VALIDATION-CONFIG", message, Some(span))
}

/// Parse a `memory = 'cold' | 'cached' | 'pinned'` placement value.
///
/// Returns `Ok(None)` when the key is absent. Any non-string or unknown value
/// is a validation error (the enum is closed; a typo must not silently hit the
/// backend default). When `allow_pinned` is false (payload storage), `pinned`
/// is rejected here so callers cannot bypass the closed set.
fn config_memory(
    config: &[(alloc::string::String, Value)],
    key: &str,
    span: Span,
    allow_pinned: bool,
) -> Result<Option<MemoryPlacement>, QqlError> {
    match config_value(config, key) {
        None => Ok(None),
        Some(Value::Str(s)) => match MemoryPlacement::parse(s) {
            Some(MemoryPlacement::Pinned) if !allow_pinned => Err(validation_err(
                alloc::format!("{key} does not support 'pinned'"),
                span,
            )),
            Some(m) => Ok(Some(m)),
            None => Err(validation_err(
                alloc::format!("{key} must be 'cold', 'cached', or 'pinned', got '{s}'"),
                span,
            )),
        },
        Some(_) => Err(validation_err(
            alloc::format!("{key} must be a string ('cold', 'cached', or 'pinned')"),
            span,
        )),
    }
}

fn config_dense_datatype(
    config: &[(alloc::string::String, Value)],
    span: Span,
) -> Result<Option<VectorDatatype>, QqlError> {
    match config_value(config, "datatype") {
        None => Ok(None),
        Some(Value::Str(s)) => match VectorDatatype::parse(s) {
            Some(dt) => Ok(Some(dt)),
            None => Err(validation_err(
                "datatype must be float32, float16, uint8, or turbo4 for VECTOR",
                span,
            )),
        },
        Some(_) => Err(validation_err("datatype must be a string for VECTOR", span)),
    }
}

impl<'a> AstLowerer<'a> {
    // ── Config blocks ───────────────────────────────────────────

    pub fn parse_collection_config_blocks(
        &mut self,
        for_alter: bool,
    ) -> Result<Option<Box<CollectionConfig>>, QqlError> {
        let mut config: Option<CollectionConfig> = None;
        while self.peek()?.kind == TokenKind::With {
            self.advance()?;
            let block = self.parse_collection_config_clause(for_alter)?;
            match &mut config {
                None => config = Some(block),
                Some(c) => merge_collection_config(c, block, self.peek()?.span)?,
            }
        }
        Ok(config.map(Box::new))
    }

    pub fn parse_collection_config_clause(
        &mut self,
        for_alter: bool,
    ) -> Result<CollectionConfig, QqlError> {
        let tok = self.peek()?;
        match tok.kind {
            TokenKind::Hnsw => {
                self.advance()?;
                self.parse_hnsw_config_block()
            }
            TokenKind::Vector => {
                self.advance()?;
                self.parse_vectors_config_block()
            }
            TokenKind::Optimizers => {
                self.advance()?;
                self.parse_optimizers_config_block()
            }
            TokenKind::Params => {
                self.advance()?;
                self.parse_collection_params_config_block(for_alter)
            }
            TokenKind::Quantization => {
                self.advance()?;
                self.parse_quantization_config_block()
            }
            _ if tok.is_keyword_or_identifier() && ascii_equal(tok.text, "QUANTIZATION") => {
                self.advance()?;
                self.parse_quantization_config_block()
            }
            _ => Err(validation_err(
                alloc::format!(
                    "expected HNSW, VECTOR, OPTIMIZERS, PARAMS, or QUANTIZATION after WITH, got '{}'",
                    tok.text
                ),
                tok.span,
            )),
        }
    }

    pub fn parse_hnsw_config_block(&mut self) -> Result<CollectionConfig, QqlError> {
        let config = self.parse_config_block()?;
        for (key, value) in &config {
            let lower = key.to_ascii_lowercase();
            match lower.as_str() {
                "m"
                | "ef_construct"
                | "full_scan_threshold"
                | "max_indexing_threads"
                | "on_disk"
                | "payload_m"
                | "inline_storage"
                | "memory" => {}
                _ => {
                    return Err(validation_err(
                        alloc::format!(
                            "unknown HNSW parameter '{}'. Expected: m, ef_construct, full_scan_threshold, max_indexing_threads, on_disk, payload_m, inline_storage, memory",
                            key
                        ),
                        self.peek()?.span,
                    ));
                }
            }
            validate_hnsw_value(key, value, self.peek()?.span)?;
        }

        if let Some(value) = config_value(&config, "m") {
            let m = match value {
                Value::Int(n) => Some(*n),
                Value::Float(f) if is_integer_val(value) => Some(*f as i64),
                _ => None,
            };
            if let Some(n) = m
                && n != 0
                && n < 4
            {
                return Err(validation_err("m must be 0 or >= 4", self.peek()?.span));
            }
        }

        let m_val = config_non_negative_u64(&config, "m", self.peek()?.span)?;
        let ef_construct = config_positive_u64(&config, "ef_construct", self.peek()?.span)?;
        let full_scan_threshold =
            config_non_negative_u64(&config, "full_scan_threshold", self.peek()?.span)?;
        let max_indexing_threads =
            config_positive_u64(&config, "max_indexing_threads", self.peek()?.span)?;
        let payload_m = config_positive_u64(&config, "payload_m", self.peek()?.span)?;

        Ok(CollectionConfig {
            vectors: None,
            hnsw: Some(Box::new(HnswRuntimeConfig {
                m: m_val,
                ef_construct,
                full_scan_threshold,
                max_indexing_threads,
                on_disk: config_bool(&config, "on_disk"),
                payload_m,
                inline_storage: config_bool(&config, "inline_storage"),
                memory: config_memory(&config, "memory", self.peek()?.span, true)?,
            })),
            optimizers: None,
            params: None,
            quantization: None,
            quantization_update: None,
        })
    }

    pub fn parse_vectors_config_block(&mut self) -> Result<CollectionConfig, QqlError> {
        let config = self.parse_config_block()?;
        for (key, value) in &config {
            if !matches!(
                key.to_ascii_lowercase().as_str(),
                "on_disk" | "memory" | "datatype"
            ) {
                return Err(validation_err(
                    alloc::format!(
                        "unknown VECTOR parameter '{}'. Expected: on_disk, memory, datatype",
                        key
                    ),
                    self.peek()?.span,
                ));
            }
            validate_vectors_value(key, value, self.peek()?.span)?;
        }
        Ok(CollectionConfig {
            vectors: Some(Box::new(VectorsConfig {
                on_disk: config_bool(&config, "on_disk"),
                memory: config_memory(&config, "memory", self.peek()?.span, true)?,
                datatype: config_dense_datatype(&config, self.peek()?.span)?,
            })),
            hnsw: None,
            optimizers: None,
            params: None,
            quantization: None,
            quantization_update: None,
        })
    }

    pub fn parse_optimizers_config_block(&mut self) -> Result<CollectionConfig, QqlError> {
        let config = self.parse_config_block()?;
        for (key, value) in &config {
            let lower = key.to_ascii_lowercase();
            match lower.as_str() {
                "deleted_threshold"
                | "vacuum_min_vector_number"
                | "default_segment_number"
                | "max_segment_size"
                | "memmap_threshold"
                | "indexing_threshold"
                | "flush_interval_sec"
                | "max_optimization_threads"
                | "prevent_unoptimized" => {}
                _ => {
                    return Err(validation_err(
                        alloc::format!(
                            "unknown OPTIMIZERS parameter '{}'. Expected: deleted_threshold, vacuum_min_vector_number, default_segment_number, max_segment_size, memmap_threshold, indexing_threshold, flush_interval_sec, max_optimization_threads, prevent_unoptimized",
                            key
                        ),
                        self.peek()?.span,
                    ));
                }
            }
            validate_optimizers_value(key, value, self.peek()?.span)?;

            if lower.as_str() == "deleted_threshold" {
                super::check_deleted_threshold(value, self.peek()?.span)?;
            }
            if lower.as_str() == "max_optimization_threads" {
                match value {
                    Value::Int(n) if *n <= 0 => {
                        return Err(validation_err(
                            "max_optimization_threads must be a positive integer or 'auto'",
                            self.peek()?.span,
                        ));
                    }
                    Value::Str(s) if !ascii_equal(s, "auto") => {
                        return Err(validation_err(
                            "max_optimization_threads must be a positive integer or 'auto'",
                            self.peek()?.span,
                        ));
                    }
                    _ => {}
                }
            }
        }

        Ok(CollectionConfig {
            vectors: None,
            hnsw: None,
            optimizers: Some(Box::new(OptimizersRuntimeConfig {
                deleted_threshold: config_float_range(&config, "deleted_threshold", 0.0, 1.0),
                vacuum_min_vector_number: config_positive_u64(
                    &config,
                    "vacuum_min_vector_number",
                    self.peek()?.span,
                )?,
                default_segment_number: config_positive_u64(
                    &config,
                    "default_segment_number",
                    self.peek()?.span,
                )?,
                max_segment_size: config_positive_u64(
                    &config,
                    "max_segment_size",
                    self.peek()?.span,
                )?,
                memmap_threshold: config_non_negative_u64(
                    &config,
                    "memmap_threshold",
                    self.peek()?.span,
                )?,
                indexing_threshold: config_non_negative_u64(
                    &config,
                    "indexing_threshold",
                    self.peek()?.span,
                )?,
                flush_interval_sec: config_positive_u64(
                    &config,
                    "flush_interval_sec",
                    self.peek()?.span,
                )?,
                max_optimization_threads: config_max_optimization_threads(
                    &config,
                    "max_optimization_threads",
                ),
                prevent_unoptimized: config_bool(&config, "prevent_unoptimized"),
            })),
            params: None,
            quantization: None,
            quantization_update: None,
        })
    }

    pub fn parse_collection_params_config_block(
        &mut self,
        for_alter: bool,
    ) -> Result<CollectionConfig, QqlError> {
        let config = self.parse_config_block()?;
        for (key, value) in &config {
            let lower = key.to_ascii_lowercase();
            match lower.as_str() {
                "replication_factor"
                | "write_consistency_factor"
                | "read_fan_out_factor"
                | "read_fan_out_delay_ms"
                | "on_disk_payload"
                | "payload_memory"
                | "shard_number"
                | "sharding_method"
                | "shard_keys" => {}
                _ => {
                    return Err(validation_err(
                        alloc::format!(
                            "unknown PARAMS parameter '{}'. Expected: replication_factor, write_consistency_factor, read_fan_out_factor, read_fan_out_delay_ms, on_disk_payload, payload_memory, shard_number, sharding_method, shard_keys",
                            key
                        ),
                        self.peek()?.span,
                    ));
                }
            }
            validate_params_value(key, value, self.peek()?.span)?;
        }

        if !for_alter
            && (config_has_key(&config, "read_fan_out_factor")
                || config_has_key(&config, "read_fan_out_delay_ms"))
        {
            return Err(validation_err(
                "WITH PARAMS (read_fan_out_factor, read_fan_out_delay_ms) is supported only for ALTER COLLECTION",
                self.peek()?.span,
            ));
        }

        Ok(CollectionConfig {
            vectors: None,
            hnsw: None,
            optimizers: None,
            params: Some(Box::new(CollectionParamsConfig {
                replication_factor: config_positive_u64(
                    &config,
                    "replication_factor",
                    self.peek()?.span,
                )?,
                write_consistency_factor: config_positive_u64(
                    &config,
                    "write_consistency_factor",
                    self.peek()?.span,
                )?,
                read_fan_out_factor: config_positive_u64(
                    &config,
                    "read_fan_out_factor",
                    self.peek()?.span,
                )?,
                read_fan_out_delay_ms: config_non_negative_u64(
                    &config,
                    "read_fan_out_delay_ms",
                    self.peek()?.span,
                )?,
                on_disk_payload: config_bool(&config, "on_disk_payload"),
                payload_memory: config_memory(&config, "payload_memory", self.peek()?.span, false)?,
                shard_number: config_positive_u64(&config, "shard_number", self.peek()?.span)?,
                sharding_method: match config_value(&config, "sharding_method") {
                    Some(Value::Str(s)) => Some(s.clone()),
                    Some(_) => {
                        return Err(validation_err(
                            "sharding_method must be a string ('auto' or 'custom')",
                            self.peek()?.span,
                        ));
                    }
                    None => None,
                },
                shard_keys: match config_value(&config, "shard_keys") {
                    Some(Value::List(items)) => {
                        let mut keys = Vec::with_capacity(items.len());
                        let entry_span = self.peek()?.span;
                        for item in items {
                            match item {
                                Value::Str(s) => keys.push(ShardKey::Keyword(s.clone())),
                                Value::Int(n) if *n >= 0 => {
                                    keys.push(ShardKey::Number(*n as u64));
                                }
                                Value::Param(name, span) => keys.push(ShardKey::Param(
                                    name.clone(),
                                    span.clone()
                                        .or_else(|| Some(alloc::boxed::Box::new(entry_span))),
                                )),
                                Value::PositionalParam(idx, span) => {
                                    keys.push(ShardKey::PositionalParam(
                                        *idx,
                                        span.clone()
                                            .or_else(|| Some(alloc::boxed::Box::new(entry_span))),
                                    ));
                                }
                                _ => {
                                    return Err(validation_err(
                                        "shard_keys entries must all be strings or non-negative integers",
                                        self.peek()?.span,
                                    ));
                                }
                            }
                        }
                        if keys.is_empty() {
                            return Err(validation_err(
                                "shard_keys must be a non-empty list of strings or non-negative integers",
                                self.peek()?.span,
                            ));
                        }
                        Some(keys)
                    }
                    Some(_) => {
                        return Err(validation_err(
                            "shard_keys must be a list of strings or non-negative integers",
                            self.peek()?.span,
                        ));
                    }
                    None => None,
                },
            })),
            quantization: None,
            quantization_update: None,
        })
    }

    pub fn parse_quantization_config_block(&mut self) -> Result<CollectionConfig, QqlError> {
        let config = self.parse_config_block()?;

        if let Some(disabled_val) = config_bool(&config, "disabled")
            && disabled_val
        {
            return Ok(CollectionConfig {
                vectors: None,
                hnsw: None,
                optimizers: None,
                params: None,
                quantization: None,
                quantization_update: Some(Box::new(QuantizationUpdate {
                    disabled: true,
                    config: None,
                })),
            });
        }

        let err_span = self.peek()?.span;
        let type_raw = config_value(&config, "type").ok_or_else(|| {
            validation_err(
                "QUANTIZATION config requires a 'type' (scalar, binary, product, turbo)",
                err_span,
            )
        })?;

        let type_str = match type_raw {
            Value::Str(s) => s,
            _ => {
                return Err(validation_err(
                    "QUANTIZATION 'type' must be a string",
                    self.peek()?.span,
                ));
            }
        };

        let qtype = match &type_str.to_ascii_lowercase()[..] {
            "scalar" => QuantizationType::Scalar,
            "binary" => QuantizationType::Binary,
            "product" => QuantizationType::Product,
            "turbo" => QuantizationType::Turbo,
            _ => {
                return Err(validation_err(
                    alloc::format!(
                        "unknown QUANTIZATION type '{}'. Expected scalar, binary, product, turbo",
                        type_str
                    ),
                    self.peek()?.span,
                ));
            }
        };

        let always_ram = config_bool(&config, "always_ram").unwrap_or(false);

        let mut quantile: Option<f64> = None;
        if qtype == QuantizationType::Scalar && config_has_key(&config, "quantile") {
            quantile = config_float_range(&config, "quantile", 0.0, 1.0);
            if quantile.is_none() {
                return Err(validation_err(
                    "quantile must be between 0.0 and 1.0",
                    self.peek()?.span,
                ));
            }
        }

        let mut bits: Option<f64> = None;
        if qtype == QuantizationType::Turbo
            && let Some(v) = config_value(&config, "bits")
        {
            let bits_val = match v {
                Value::Int(n) => Some(*n as f64),
                Value::Float(f) => Some(*f),
                _ => None,
            };
            if let Some(b) = bits_val {
                if b != 1.0 && b != 1.5 && b != 2.0 && b != 4.0 {
                    return Err(validation_err(
                        "bits must be one of 1, 1.5, 2, or 4 for TURBO quantization",
                        self.peek()?.span,
                    ));
                }
                bits = Some(b);
            }
        }

        let mut compression: Option<String> = None;
        if qtype == QuantizationType::Product
            && let Some(Value::Str(c)) = config_value(&config, "compression")
        {
            let c_lower = c.to_ascii_lowercase();
            if matches!(c_lower.as_str(), "x4" | "x8" | "x16" | "x32" | "x64") {
                compression = Some(c_lower);
            } else {
                return Err(validation_err(
                    "compression must be x4, x8, x16, x32, or x64 for PRODUCT quantization",
                    self.peek()?.span,
                ));
            }
        }

        let mut encoding: Option<String> = None;
        let mut query_encoding: Option<String> = None;
        if qtype == QuantizationType::Binary {
            if let Some(e) = config_value(&config, "encoding") {
                let raw = match e {
                    Value::Str(s) => s.to_ascii_lowercase(),
                    Value::Int(n) => n.to_string(),
                    Value::Float(f) => {
                        if (*f - 1.5).abs() < f64::EPSILON {
                            "1.5".into()
                        } else if f.fract() == 0.0 {
                            format!("{}", *f as i64)
                        } else {
                            f.to_string()
                        }
                    }
                    _ => {
                        return Err(validation_err(
                            "encoding must be a string or number for BINARY quantization",
                            self.peek()?.span,
                        ));
                    }
                };
                // Canonicalize aliases so dump/plan always see one_bit|two_bits|one_and_half_bits.
                encoding = Some(match raw.as_str() {
                    "one_bit" | "onebit" | "1" => "one_bit".into(),
                    "two_bits" | "twobits" | "2" => "two_bits".into(),
                    "one_and_half_bits" | "oneandhalfbits" | "1.5" => "one_and_half_bits".into(),
                    _ => {
                        return Err(validation_err(
                            "encoding must be one_bit (1), two_bits (2), or one_and_half_bits (1.5) for BINARY quantization",
                            self.peek()?.span,
                        ));
                    }
                });
            }

            if let Some(Value::Str(qe)) = config_value(&config, "query_encoding") {
                let qe_lower = qe.to_ascii_lowercase();
                if matches!(
                    qe_lower.as_str(),
                    "default" | "binary" | "scalar4bits" | "scalar8bits"
                ) {
                    query_encoding = Some(qe_lower);
                } else {
                    return Err(validation_err(
                        "query_encoding must be default, binary, scalar4bits, or scalar8bits for BINARY quantization",
                        self.peek()?.span,
                    ));
                }
            }
        }

        let q_config = QuantizationConfig {
            qtype,
            always_ram,
            quantile,
            bits,
            compression,
            encoding,
            query_encoding,
            memory: config_memory(&config, "memory", self.peek()?.span, true)?,
        };

        Ok(CollectionConfig {
            vectors: None,
            hnsw: None,
            optimizers: None,
            params: None,
            quantization: Some(Box::new(q_config.clone())),
            quantization_update: Some(Box::new(QuantizationUpdate {
                disabled: false,
                config: Some(Box::new(q_config)),
            })),
        })
    }

    pub fn parse_multivector_config_block(&mut self) -> Result<MultivectorConfig, QqlError> {
        let config = self.parse_config_block()?;
        let err_span = self.peek()?.span;
        let comp = config_value(&config, "comparator")
            .ok_or_else(|| validation_err("MULTIVECTOR config requires 'comparator'", err_span))?;
        let comparator = match comp {
            Value::Str(s) => s.to_ascii_lowercase(),
            _ => {
                return Err(validation_err(
                    "MULTIVECTOR comparator must be a string",
                    self.peek()?.span,
                ));
            }
        };
        if comparator != "max_sim" {
            return Err(validation_err(
                alloc::format!(
                    "MULTIVECTOR comparator must be 'max_sim', got '{}'",
                    comparator
                ),
                self.peek()?.span,
            ));
        }
        Ok(MultivectorConfig {
            comparator: MultivectorComparator::MaxSim,
        })
    }

    pub fn parse_sparse_config_block(
        &mut self,
    ) -> Result<(Option<Box<SparseIndexConfig>>, Option<String>), QqlError> {
        let config = self.parse_config_block()?;
        for (key, _) in &config {
            let lower = key.to_ascii_lowercase();
            if !matches!(
                lower.as_str(),
                "modifier" | "full_scan_threshold" | "on_disk" | "datatype" | "memory"
            ) {
                return Err(validation_err(
                    alloc::format!(
                        "unknown SPARSE/INDEX parameter '{}'. Expected: modifier, full_scan_threshold, on_disk, datatype, memory",
                        key
                    ),
                    self.peek()?.span,
                ));
            }
        }

        let mut modifier = None;
        if let Some(Value::Str(m)) = config_value(&config, "modifier") {
            let m_lower = m.to_ascii_lowercase();
            if matches!(m_lower.as_str(), "none" | "idf") {
                modifier = Some(m_lower);
            } else {
                return Err(validation_err(
                    "modifier must be none or idf for SPARSE vector",
                    self.peek()?.span,
                ));
            }
        }
        let full_scan_threshold =
            config_non_negative_u64(&config, "full_scan_threshold", self.peek()?.span)?;
        let on_disk = config_bool(&config, "on_disk");
        // `default` (and omitting the key) both mean "backend default" → None.
        let datatype = match config_value(&config, "datatype") {
            Some(Value::Str(s)) if s.eq_ignore_ascii_case("default") => None,
            Some(Value::Str(s)) => match VectorDatatype::parse_sparse(s) {
                Some(dt) => Some(dt),
                None => {
                    return Err(validation_err(
                        "datatype must be float32, uint8, float16, or default for SPARSE index",
                        self.peek()?.span,
                    ));
                }
            },
            Some(_) => {
                return Err(validation_err(
                    "datatype must be a string for SPARSE index",
                    self.peek()?.span,
                ));
            }
            None => None,
        };

        let index = if full_scan_threshold.is_some()
            || on_disk.is_some()
            || datatype.is_some()
            || config_has_key(&config, "memory")
        {
            Some(Box::new(SparseIndexConfig {
                full_scan_threshold,
                on_disk,
                datatype,
                memory: config_memory(&config, "memory", self.peek()?.span, true)?,
            }))
        } else {
            None
        };
        Ok((index, modifier))
    }
}
