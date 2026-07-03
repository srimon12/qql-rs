use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::ast::{
    AlterCollectionStmt, CollectionConfig, CollectionParamsConfig, CreateCollectionStmt,
    CreateIndexStmt, DropCollectionStmt, HnswRuntimeConfig, MultivectorConfig,
    OptimizersRuntimeConfig, QuantizationConfig, QuantizationType, QuantizationUpdate,
    SparseVectorDef, Stmt, Value, VectorDef, VectorDistance, VectorsConfig,
};
use crate::error::QqlError;
use crate::token::TokenKind;

use super::{
    ascii_equal, ascii_equal_lower, config_bool, config_float_range, config_has_key,
    config_max_optimization_threads, config_non_negative_u64, config_positive_u64, config_value,
    merge_collection_config, validate_hnsw_value, validate_optimizers_value, validate_params_value,
    validate_vectors_value, Parser,
};

impl<'a> Parser<'a> {
    pub fn parse_create(&mut self) -> Result<Stmt<'a>, QqlError> {
        self.advance()?;
        let tok = self.peek()?;
        if tok.kind == TokenKind::Index {
            return self.parse_create_index();
        }
        self.expect(TokenKind::Collection)?;
        let collection = self.parse_identifier()?;

        let mut hybrid = false;
        let mut rerank = false;
        let mut model: Option<&'a str> = None;
        let mut dense_vector: Option<&'a str> = None;
        let mut sparse_vector: Option<&'a str> = None;
        let mut explicit_vectors: Vec<VectorDef<'a>> = Vec::new();
        let mut explicit_sparse_vectors: Vec<SparseVectorDef<'a>> = Vec::new();

        if self.peek()?.kind == TokenKind::Lparen {
            self.advance()?;
            while self.peek()?.kind != TokenKind::Rparen && self.peek()?.kind != TokenKind::Eof {
                let name = self.parse_identifier()?;

                if self.peek()?.kind == TokenKind::Vector {
                    self.advance()?;
                    self.expect(TokenKind::Lparen)?;
                    let size_tok = self.peek()?;
                    let size = self.parse_numeric_literal()?;
                    if size <= 0.0 || size != (size as u64) as f64 {
                        return Err(QqlError::syntax(
                            "vector size must be a positive integer",
                            size_tok.pos,
                        ));
                    }
                    self.expect(TokenKind::Comma)?;
                    let dist_tok = self.peek()?;
                    let distance = match dist_tok.kind {
                        TokenKind::Cosine => VectorDistance::Cosine,
                        TokenKind::Dot => VectorDistance::Dot,
                        TokenKind::Euclid => VectorDistance::Euclid,
                        TokenKind::Manhattan => VectorDistance::Manhattan,
                        _ => {
                            return Err(QqlError::syntax(
                                "expected distance metric (COSINE, DOT, EUCLID, MANHATTAN)",
                                dist_tok.pos,
                            ));
                        }
                    };
                    self.advance()?;
                    self.expect(TokenKind::Rparen)?;

                    let mut hnsw: Option<Box<HnswRuntimeConfig>> = None;
                    let mut quant: Option<Box<QuantizationConfig>> = None;
                    let mut multiv: Option<MultivectorConfig> = None;

                    while self.peek()?.kind == TokenKind::With {
                        self.advance()?;
                        if self.peek()?.kind == TokenKind::Hnsw {
                            self.advance()?;
                            let block = self.parse_hnsw_config_block()?;
                            hnsw = block.hnsw;
                        } else if self.peek()?.kind == TokenKind::Quantize
                            || (self.peek()?.kind == TokenKind::Identifier
                                && ascii_equal(self.peek()?.text, "QUANTIZATION"))
                        {
                            self.advance()?;
                            let block = self.parse_quantization_config_block()?;
                            quant = block.quantization;
                        } else if self.peek()?.kind == TokenKind::Identifier
                            && ascii_equal(self.peek()?.text, "MULTIVECTOR")
                        {
                            self.advance()?;
                            multiv = Some(self.parse_multivector_config_block()?);
                        } else {
                            return Err(QqlError::syntax(
                                "expected HNSW, QUANTIZATION, or MULTIVECTOR after WITH for vector configuration",
                                self.peek()?.pos,
                            ));
                        }
                    }

                    explicit_vectors.push(VectorDef {
                        name,
                        size: size as u64,
                        distance,
                        hnsw,
                        quantization: quant,
                        multivector: multiv,
                    });
                } else if self.peek()?.kind == TokenKind::Sparse {
                    self.advance()?;
                    explicit_sparse_vectors.push(SparseVectorDef { name });
                } else {
                    return Err(QqlError::syntax(
                        "expected VECTOR or SPARSE after vector name",
                        self.peek()?.pos,
                    ));
                }

                if self.peek()?.kind == TokenKind::Comma {
                    self.advance()?;
                } else if self.peek()?.kind != TokenKind::Rparen {
                    return Err(QqlError::syntax("expected comma or )", self.peek()?.pos));
                }
            }
            self.expect(TokenKind::Rparen)?;
        }

        if self.peek()?.kind == TokenKind::Hybrid {
            self.advance()?;
            hybrid = true;
            if self.peek()?.kind == TokenKind::Rerank {
                self.advance()?;
                rerank = true;
            } else {
                while self.peek()?.kind == TokenKind::Dense
                    || self.peek()?.kind == TokenKind::Sparse
                {
                    let mode = self.advance()?.kind;
                    let tok = self.peek()?;
                    if tok.kind == TokenKind::Vector
                        || (tok.kind == TokenKind::Identifier && ascii_equal(tok.text, "VECTOR"))
                    {
                        self.advance()?;
                        let v = self.parse_string_ptr()?;
                        if mode == TokenKind::Dense {
                            dense_vector = Some(v);
                        } else {
                            sparse_vector = Some(v);
                        }
                    } else {
                        return Err(QqlError::syntax(
                            "expected VECTOR after DENSE/SPARSE",
                            self.peek()?.pos,
                        ));
                    }
                }
            }
        } else if self.peek()?.kind == TokenKind::Using {
            self.advance()?;
            if self.peek()?.kind == TokenKind::Hybrid {
                self.advance()?;
                hybrid = true;
                if self.peek()?.kind == TokenKind::Dense {
                    self.advance()?;
                    model = Some(self.parse_required_model_string()?);
                }
            } else {
                model = Some(self.parse_required_model_string()?);
            }
        }

        let config = self.parse_collection_config_blocks(false)?;

        Ok(Stmt::CreateCollection(Box::new(CreateCollectionStmt {
            collection,
            hybrid,
            rerank,
            model,
            dense_vector,
            sparse_vector,
            vectors: explicit_vectors,
            sparse_vectors: explicit_sparse_vectors,
            config,
        })))
    }

    // ── ALTER ───────────────────────────────────────────────────

    pub fn parse_alter(&mut self) -> Result<Stmt<'a>, QqlError> {
        self.advance()?;
        self.expect(TokenKind::Collection)?;
        let collection = self.parse_identifier()?;
        let config = self.parse_collection_config_blocks(true)?;
        Ok(Stmt::AlterCollection(Box::new(AlterCollectionStmt {
            collection,
            config,
        })))
    }

    // ── DROP ────────────────────────────────────────────────────

    pub fn parse_drop(&mut self) -> Result<Stmt<'a>, QqlError> {
        self.advance()?;
        self.expect(TokenKind::Collection)?;
        let collection = self.parse_identifier()?;
        Ok(Stmt::DropCollection(Box::new(DropCollectionStmt {
            collection,
        })))
    }

    // ── SHOW ────────────────────────────────────────────────────

    pub fn parse_show(&mut self) -> Result<Stmt<'a>, QqlError> {
        self.advance()?;
        if self.peek()?.kind == TokenKind::Collections {
            self.advance()?;
            return Ok(Stmt::ShowCollections);
        }
        if self.peek()?.kind == TokenKind::Collection {
            self.advance()?;
            let collection = self.parse_identifier()?;
            return Ok(Stmt::ShowCollection(collection));
        }
        Err(QqlError::syntax(
            alloc::format!(
                "expected COLLECTION or COLLECTIONS after SHOW, got '{}'",
                self.peek()?.text
            ),
            self.peek()?.pos,
        ))
    }

    // ── CREATE INDEX ────────────────────────────────────────────

    pub fn parse_create_index(&mut self) -> Result<Stmt<'a>, QqlError> {
        self.advance()?;
        self.expect(TokenKind::On)?;
        self.expect(TokenKind::Collection)?;
        let collection = self.parse_identifier()?;
        self.expect(TokenKind::For)?;
        let field = self.parse_identifier()?;
        let mut field_type: &'a str = "keyword";
        if self.peek()?.kind == TokenKind::Type {
            self.advance()?;
            let type_tok = self.expect(TokenKind::Identifier)?;
            let lowered = type_tok.text.to_ascii_lowercase();
            let leaked: &'static str = Box::leak(lowered.into_boxed_str());
            field_type = unsafe { &*(leaked as *const str) };
        }
        let mut options = Vec::new();
        if self.peek()?.kind == TokenKind::With {
            self.advance()?;
            options = self.parse_config_block()?;
        }
        Ok(Stmt::CreateIndex(Box::new(CreateIndexStmt {
            collection,
            field,
            field_type,
            options,
        })))
    }

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
            TokenKind::Vectors => {
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
            _ => Err(QqlError::syntax(
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
                    return Err(QqlError::syntax(
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

        let m_val = config_non_negative_u64(&config, "m", self.peek()?.pos)?;
        if let Some(m) = m_val {
            if m != 0 && m < 4 {
                return Err(QqlError::syntax("m must be 0 or >= 4", self.peek()?.pos));
            }
        }
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
                | "on_disk_payload" => {}
                _ => {
                    return Err(QqlError::syntax(
                        alloc::format!(
                            "unknown PARAMS parameter '{}'. Expected: replication_factor, write_consistency_factor, read_fan_out_factor, read_fan_out_delay_ms, on_disk_payload",
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
            return Err(QqlError::syntax(
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

        let mut turbo_bits: Option<f64> = None;
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
                    turbo_bits = Some(b);
                }
            }
        }

        let q_config = QuantizationConfig {
            qtype,
            always_ram,
            quantile,
            turbo_bits,
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
            comparator: "max_sim",
        })
    }
}
