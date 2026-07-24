use alloc::boxed::Box;

use crate::ast::{
    CollectionConfig, CollectionParamsConfig, HnswRuntimeConfig, MultivectorComparator,
    MultivectorConfig, OptimizersRuntimeConfig, QuantizationConfig, QuantizationType,
    QuantizationUpdate, SparseIndexConfig, Value, VectorsConfig,
};
use crate::error::{QqlError, Span};
use crate::token::TokenKind;

use super::{
    ascii_equal, ascii_equal_lower, config_bool, config_float_range, config_has_key,
    config_max_optimization_threads, config_non_negative_u64, config_positive_u64, config_value,
    merge_collection_config, validate_hnsw_value, validate_optimizers_value, validate_params_value,
    validate_vectors_value, Parser,
};

fn validation_err(
    message: impl Into<alloc::borrow::Cow<'static, str>>,
    position: usize,
) -> QqlError {
    QqlError::validation(
        "QQL-VALIDATION-CONFIG",
        message,
        Some(Span::point(position)),
    )
}

impl<'a> Parser<'a> {
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
                Some(ref mut c) => merge_collection_config(c, block, self.peek()?.pos)?,
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
            _ if tok.kind == TokenKind::Quantize
                || (tok.kind == TokenKind::Identifier
                    && ascii_equal(tok.text, "QUANTIZATION")) =>
            {
                self.advance()?;
                self.parse_quantization_config_block()
            }
            _ => Err(validation_err(
                alloc::format!(
                    "expected HNSW, VECTORS, OPTIMIZERS, PARAMS, or QUANTIZATION after WITH, got '{}'",
                    tok.text
                ),
                tok.pos,
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
                | "inline_storage" => {}
                _ => {
                    return Err(validation_err(
                        alloc::format!(
                            "unknown HNSW parameter '{}'. Expected: m, ef_construct, full_scan_threshold, max_indexing_threads, on_disk, payload_m, inline_storage",
                            key
                        ),
                        self.peek()?.pos,
                    ));
                }
            }
            validate_hnsw_value(key, value, self.peek()?.pos)?;
        }

        if let Some(Value::Int(n)) = config_value(&config, "m") {
            if *n != 0 && *n < 4 {
                return Err(validation_err("m must be 0 or >= 4", self.peek()?.pos));
            }
        }

        let m_val = config_non_negative_u64(&config, "m", self.peek()?.pos)?;
        let ef_construct = config_positive_u64(&config, "ef_construct", self.peek()?.pos)?;
        let full_scan_threshold =
            config_non_negative_u64(&config, "full_scan_threshold", self.peek()?.pos)?;
        let max_indexing_threads =
            config_positive_u64(&config, "max_indexing_threads", self.peek()?.pos)?;
        let payload_m = config_positive_u64(&config, "payload_m", self.peek()?.pos)?;

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
            if !ascii_equal_lower(key, "on_disk") {
                return Err(QqlError::syntax(
                    alloc::format!("unknown VECTORS parameter '{}'. Expected: on_disk", key),
                    self.peek()?.pos,
                ));
            }
            validate_vectors_value(key, value, self.peek()?.pos)?;
        }
        Ok(CollectionConfig {
            vectors: Some(Box::new(VectorsConfig {
                on_disk: config_bool(&config, "on_disk"),
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
                    return Err(QqlError::syntax(
                        alloc::format!(
                            "unknown OPTIMIZERS parameter '{}'. Expected: deleted_threshold, vacuum_min_vector_number, default_segment_number, max_segment_size, memmap_threshold, indexing_threshold, flush_interval_sec, max_optimization_threads, prevent_unoptimized",
                            key
                        ),
                        self.peek()?.pos,
                    ));
                }
            }
            validate_optimizers_value(key, value, self.peek()?.pos)?;

            if lower.as_str() == "deleted_threshold" {
                super::check_deleted_threshold(value, self.peek()?.pos)?;
            }
            if lower.as_str() == "max_optimization_threads" {
                match value {
                    Value::Int(n) if *n <= 0 => {
                        return Err(QqlError::syntax(
                            "max_optimization_threads must be a positive integer or 'auto'",
                            self.peek()?.pos,
                        ));
                    }
                    Value::Str(s) if !ascii_equal_lower(s, "auto") => {
                        return Err(QqlError::syntax(
                            "max_optimization_threads must be a positive integer or 'auto'",
                            self.peek()?.pos,
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
                    self.peek()?.pos,
                )?,
                default_segment_number: config_positive_u64(
                    &config,
                    "default_segment_number",
                    self.peek()?.pos,
                )?,
                max_segment_size: config_positive_u64(
                    &config,
                    "max_segment_size",
                    self.peek()?.pos,
                )?,
                memmap_threshold: config_non_negative_u64(
                    &config,
                    "memmap_threshold",
                    self.peek()?.pos,
                )?,
                indexing_threshold: config_non_negative_u64(
                    &config,
                    "indexing_threshold",
                    self.peek()?.pos,
                )?,
                flush_interval_sec: config_positive_u64(
                    &config,
                    "flush_interval_sec",
                    self.peek()?.pos,
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
                | "shard_number"
                | "sharding_method"
                | "shard_keys" => {}
                _ => {
                    return Err(validation_err(
                        alloc::format!(
                            "unknown PARAMS parameter '{}'. Expected: replication_factor, write_consistency_factor, read_fan_out_factor, read_fan_out_delay_ms, on_disk_payload, shard_number, sharding_method, shard_keys",
                            key
                        ),
                        self.peek()?.pos,
                    ));
                }
            }
            validate_params_value(key, value, self.peek()?.pos)?;
        }

        if !for_alter
            && (config_has_key(&config, "read_fan_out_factor")
                || config_has_key(&config, "read_fan_out_delay_ms"))
        {
            return Err(validation_err(
                    "WITH PARAMS (read_fan_out_factor, read_fan_out_delay_ms) is supported only for ALTER COLLECTION",
                    self.peek()?.pos,
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
                    self.peek()?.pos,
                )?,
                write_consistency_factor: config_positive_u64(
                    &config,
                    "write_consistency_factor",
                    self.peek()?.pos,
                )?,
                read_fan_out_factor: config_positive_u64(
                    &config,
                    "read_fan_out_factor",
                    self.peek()?.pos,
                )?,
                read_fan_out_delay_ms: config_non_negative_u64(
                    &config,
                    "read_fan_out_delay_ms",
                    self.peek()?.pos,
                )?,
                on_disk_payload: config_bool(&config, "on_disk_payload"),
                shard_number: config_positive_u64(&config, "shard_number", self.peek()?.pos)?,
                sharding_method: match config_value(&config, "sharding_method") {
                    Some(Value::Str(s)) => Some(s.clone()),
                    Some(_) => {
                        return Err(validation_err(
                            "sharding_method must be a string ('auto' or 'custom')",
                            self.peek()?.pos,
                        ));
                    }
                    None => None,
                },
                shard_keys: match config_value(&config, "shard_keys") {
                    Some(Value::List(items)) => {
                        let mut keys = Vec::with_capacity(items.len());
                        for item in items {
                            match item {
                                Value::Str(s) => keys.push(s.clone()),
                                _ => {
                                    return Err(validation_err(
                                        "shard_keys entries must all be strings",
                                        self.peek()?.pos,
                                    ));
                                }
                            }
                        }
                        if keys.is_empty() {
                            return Err(validation_err(
                                "shard_keys must be a non-empty list of strings",
                                self.peek()?.pos,
                            ));
                        }
                        Some(keys)
                    }
                    Some(_) => {
                        return Err(validation_err(
                            "shard_keys must be a list of strings",
                            self.peek()?.pos,
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

        if let Some(disabled_val) = config_bool(&config, "disabled") {
            if disabled_val {
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
        }

        let err_pos = self.peek()?.pos;
        let type_raw = config_value(&config, "type").ok_or_else(|| {
            QqlError::syntax(
                "QUANTIZATION config requires a 'type' (scalar, binary, product, turbo)",
                err_pos,
            )
        })?;

        let type_str = match type_raw {
            Value::Str(s) => s,
            _ => {
                return Err(QqlError::syntax(
                    "QUANTIZATION 'type' must be a string",
                    self.peek()?.pos,
                ));
            }
        };

        let qtype = match &type_str.to_ascii_lowercase()[..] {
            "scalar" => QuantizationType::Scalar,
            "binary" => QuantizationType::Binary,
            "product" => QuantizationType::Product,
            "turbo" => QuantizationType::Turbo,
            _ => {
                return Err(QqlError::syntax(
                    alloc::format!(
                        "unknown QUANTIZATION type '{}'. Expected scalar, binary, product, turbo",
                        type_str
                    ),
                    self.peek()?.pos,
                ));
            }
        };

        let always_ram = config_bool(&config, "always_ram").unwrap_or(false);

        let mut quantile: Option<f64> = None;
        if qtype == QuantizationType::Scalar && config_has_key(&config, "quantile") {
            quantile = config_float_range(&config, "quantile", 0.0, 1.0);
            if quantile.is_none() {
                return Err(QqlError::syntax(
                    "quantile must be between 0.0 and 1.0",
                    self.peek()?.pos,
                ));
            }
        }

        let mut bits: Option<f64> = None;
        if qtype == QuantizationType::Turbo {
            if let Some(v) = config_value(&config, "bits") {
                let bits_val = match v {
                    Value::Int(n) => Some(*n as f64),
                    Value::Float(f) => Some(*f),
                    _ => None,
                };
                if let Some(b) = bits_val {
                    if b != 1.0 && b != 1.5 && b != 2.0 && b != 4.0 {
                        return Err(QqlError::syntax(
                            "bits must be one of 1, 1.5, 2, or 4 for TURBO quantization",
                            self.peek()?.pos,
                        ));
                    }
                    bits = Some(b);
                }
            }
        }

        let mut compression: Option<String> = None;
        if qtype == QuantizationType::Product {
            if let Some(Value::Str(c)) = config_value(&config, "compression") {
                let c_lower = c.to_ascii_lowercase();
                if matches!(c_lower.as_str(), "x4" | "x8" | "x16" | "x32" | "x64") {
                    compression = Some(c_lower);
                } else {
                    return Err(QqlError::syntax(
                        "compression must be x4, x8, x16, x32, or x64 for PRODUCT quantization",
                        self.peek()?.pos,
                    ));
                }
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
                        return Err(QqlError::syntax(
                            "encoding must be a string or number for BINARY quantization",
                            self.peek()?.pos,
                        ));
                    }
                };
                // Canonicalize aliases so dump/plan always see one_bit|two_bits|one_and_half_bits.
                encoding = Some(match raw.as_str() {
                    "one_bit" | "onebit" | "1" => "one_bit".into(),
                    "two_bits" | "twobits" | "2" => "two_bits".into(),
                    "one_and_half_bits" | "oneandhalfbits" | "1.5" => "one_and_half_bits".into(),
                    _ => {
                        return Err(QqlError::syntax(
                            "encoding must be one_bit (1), two_bits (2), or one_and_half_bits (1.5) for BINARY quantization",
                            self.peek()?.pos,
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
                    return Err(QqlError::syntax(
                        "query_encoding must be default, binary, scalar4bits, or scalar8bits for BINARY quantization",
                        self.peek()?.pos,
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
        let err_pos = self.peek()?.pos;
        let comp = config_value(&config, "comparator")
            .ok_or_else(|| QqlError::syntax("MULTIVECTOR config requires 'comparator'", err_pos))?;
        let comparator = match comp {
            Value::Str(s) => s.to_ascii_lowercase(),
            _ => {
                return Err(QqlError::syntax(
                    "MULTIVECTOR comparator must be a string",
                    self.peek()?.pos,
                ));
            }
        };
        if comparator != "max_sim" {
            return Err(QqlError::syntax(
                alloc::format!(
                    "MULTIVECTOR comparator must be 'max_sim', got '{}'",
                    comparator
                ),
                self.peek()?.pos,
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
                "modifier" | "full_scan_threshold" | "on_disk" | "datatype"
            ) {
                return Err(QqlError::syntax(
                    alloc::format!(
                        "unknown SPARSE/INDEX parameter '{}'. Expected: modifier, full_scan_threshold, on_disk, datatype",
                        key
                    ),
                    self.peek()?.pos,
                ));
            }
        }

        let mut modifier = None;
        if let Some(Value::Str(m)) = config_value(&config, "modifier") {
            let m_lower = m.to_ascii_lowercase();
            if matches!(m_lower.as_str(), "none" | "idf") {
                modifier = Some(m_lower);
            } else {
                return Err(QqlError::syntax(
                    "modifier must be none or idf for SPARSE vector",
                    self.peek()?.pos,
                ));
            }
        }
        let full_scan_threshold =
            config_non_negative_u64(&config, "full_scan_threshold", self.peek()?.pos)?;
        let on_disk = config_bool(&config, "on_disk");
        let datatype = match config_value(&config, "datatype") {
            Some(Value::Str(s)) => {
                let s_lower = s.to_ascii_lowercase();
                match s_lower.as_str() {
                    "float32" | "f32" => Some("float32".into()),
                    "uint8" | "u8" => Some("uint8".into()),
                    "float16" | "f16" => Some("float16".into()),
                    "default" => Some("default".into()),
                    _ => {
                        return Err(QqlError::syntax(
                            "datatype must be float32, uint8, float16, or default for SPARSE index",
                            self.peek()?.pos,
                        ));
                    }
                }
            }
            Some(_) => {
                return Err(QqlError::syntax(
                    "datatype must be a string for SPARSE index",
                    self.peek()?.pos,
                ));
            }
            None => None,
        };

        let index = if full_scan_threshold.is_some() || on_disk.is_some() || datatype.is_some() {
            Some(Box::new(SparseIndexConfig {
                full_scan_threshold,
                on_disk,
                datatype,
            }))
        } else {
            None
        };
        Ok((index, modifier))
    }
}
