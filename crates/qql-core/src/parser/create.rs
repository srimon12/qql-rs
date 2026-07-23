use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::ast::{
    CollectionMode, CreateCollectionStmt, CreateShardKeyStmt, SparseVectorDef, Stmt, VectorDef,
    VectorDistance,
};
use crate::error::QqlError;
use crate::token::TokenKind;

use super::{ascii_equal, Parser};

impl<'a> Parser<'a> {
    pub fn parse_create(&mut self) -> Result<Stmt, QqlError> {
        self.advance()?;
        let tok = self.peek()?;
        if tok.kind == TokenKind::Index {
            return self.parse_create_index();
        }
        if tok.kind == TokenKind::Shard {
            return self.parse_create_shard_key();
        }
        self.expect(TokenKind::Collection)?;
        let collection = self.parse_identifier()?;

        let mut hybrid = false;
        let mut rerank = false;
        let mut model: Option<String> = None;
        let mut dense_vector: Option<String> = None;
        let mut sparse_vector: Option<String> = None;
        let mut explicit_vectors: Vec<VectorDef> = Vec::new();
        let mut explicit_sparse_vectors: Vec<SparseVectorDef> = Vec::new();

        // Parse mode keyword (HYBRID, RERANK, DENSE/USING MODEL) before vectors
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
                        let v = self.parse_string()?;
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

        if self.peek()?.kind == TokenKind::Identifier && ascii_equal(self.peek()?.text, "VECTORS") {
            self.advance()?;
        }

        // Parse explicit vector definitions in parentheses
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

                    let mut hnsw = None;
                    let mut quant = None;
                    let mut multiv = None;

                    while self.peek()?.kind == TokenKind::With {
                        self.advance()?;
                        if self.peek()?.kind == TokenKind::Hnsw {
                            self.advance()?;
                            hnsw = self.parse_hnsw_config_block()?.hnsw;
                        } else if self.peek()?.kind == TokenKind::Quantize
                            || (self.peek()?.kind == TokenKind::Identifier
                                && ascii_equal(self.peek()?.text, "QUANTIZATION"))
                        {
                            self.advance()?;
                            quant = self.parse_quantization_config_block()?.quantization;
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

        // Fallback HYBRID/DENSE mode check (only if not already set before vectors)
        if !hybrid && !rerank && model.is_none() {
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
                            || (tok.kind == TokenKind::Identifier
                                && ascii_equal(tok.text, "VECTOR"))
                        {
                            self.advance()?;
                            let v = self.parse_string()?;
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
        }

        let config = self.parse_collection_config_blocks(false)?;

        let mode = if rerank {
            CollectionMode::Rerank
        } else if hybrid {
            CollectionMode::Hybrid {
                dense_vector,
                sparse_vector,
            }
        } else {
            CollectionMode::Dense { model }
        };

        Ok(Stmt::CreateCollection(Box::new(CreateCollectionStmt {
            collection,
            mode,
            vectors: explicit_vectors,
            sparse_vectors: explicit_sparse_vectors,
            config,
        })))
    }

    pub fn parse_create_shard_key(&mut self) -> Result<Stmt, QqlError> {
        // Consume the SHARD token (parse_create() only peeked at it)
        self.expect(TokenKind::Shard)?;
        self.expect(TokenKind::Key)?;
        let shard_name = self.parse_string()?;
        self.expect(TokenKind::On)?;
        self.expect(TokenKind::Collection)?;
        let collection = self.parse_identifier()?;
        let mut shards_number = None;
        let mut replication_factor = None;
        if self.peek()?.kind == TokenKind::With {
            self.advance()?;
            let opts = self.parse_config_block()?;
            for (key, val) in &opts {
                let key_lower = key.to_ascii_lowercase();
                if key_lower == "shards_number" {
                    if let crate::ast::Value::Int(n) = val {
                        if *n > 0 {
                            shards_number = Some(*n as u64);
                        }
                    }
                } else if key_lower == "replication_factor" {
                    if let crate::ast::Value::Int(n) = val {
                        if *n > 0 {
                            replication_factor = Some(*n as u64);
                        }
                    }
                }
            }
        }
        Ok(Stmt::CreateShardKey(Box::new(CreateShardKeyStmt {
            collection,
            shard_key: shard_name,
            shards_number,
            replication_factor,
        })))
    }
}
