use super::{ascii_equal, ascii_equal_lower, Parser};
use crate::ast::{PayloadSelector, QuantizationSearchWith, SearchWith, Value, VectorsSelector};
use crate::error::QqlError;
use crate::token::TokenKind;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

impl<'a> Parser<'a> {
    pub fn parse_with_clause(&mut self) -> Result<SearchWith, QqlError> {
        self.expect(TokenKind::Lparen)?;
        let mut hnsw_ef: u64 = 0;
        let mut exact = false;
        let mut acorn = false;
        let mut indexed_only = false;
        let mut quantization: Option<Box<QuantizationSearchWith>> = None;
        let mut mmr_diversity: Option<f64> = None;
        let mut mmr_candidates: Option<u64> = None;
        let mut rrf_k: Option<u64> = None;
        let mut rrf_weights: Vec<f32> = Vec::new();

        while self.peek()?.kind != TokenKind::Rparen {
            let key_tok = self.peek()?;
            if key_tok.kind != TokenKind::Identifier
                && key_tok.kind != TokenKind::Exact
                && key_tok.kind != TokenKind::Acorn
            {
                return Err(QqlError::syntax(
                    alloc::format!("expected a WITH parameter name, got '{}'", key_tok.text),
                    key_tok.pos,
                ));
            }
            self.advance()?;
            self.expect(TokenKind::Equals)?;

            if ascii_equal_lower(key_tok.text, "hnsw_ef") {
                let int_tok = self.expect(TokenKind::Integer)?;
                hnsw_ef = int_tok.text.parse::<u64>().map_err(|_| {
                    QqlError::syntax("hnsw_ef must be a positive integer", int_tok.pos)
                })?;
            } else if ascii_equal_lower(key_tok.text, "exact") {
                exact = self.parse_bool()?;
            } else if ascii_equal_lower(key_tok.text, "acorn") {
                acorn = self.parse_bool()?;
            } else if ascii_equal_lower(key_tok.text, "indexed_only") {
                indexed_only = self.parse_bool()?;
            } else if ascii_equal_lower(key_tok.text, "quantization") {
                quantization = Some(Box::new(self.parse_quantization_search_with()?));
            } else if ascii_equal_lower(key_tok.text, "mmr_diversity") {
                let value = self.parse_number()?;
                let diversity = match value {
                    Value::Int(i) => i as f64,
                    Value::Float(f) => f,
                    _ => {
                        return Err(QqlError::syntax(
                            "mmr_diversity must be numeric",
                            key_tok.pos,
                        ));
                    }
                };
                if !(0.0..=1.0).contains(&diversity) {
                    return Err(QqlError::syntax(
                        alloc::format!(
                            "mmr_diversity must be between 0 and 1, got '{}'",
                            diversity
                        ),
                        key_tok.pos,
                    ));
                }
                mmr_diversity = Some(diversity);
            } else if ascii_equal_lower(key_tok.text, "mmr_candidates") {
                let int_tok = self.expect(TokenKind::Integer)?;
                let candidates: u64 = int_tok.text.parse::<u64>().map_err(|_| {
                    QqlError::syntax("mmr_candidates must be a positive integer", int_tok.pos)
                })?;
                if candidates == 0 {
                    return Err(QqlError::syntax(
                        "mmr_candidates must be a positive integer",
                        int_tok.pos,
                    ));
                }
                mmr_candidates = Some(candidates);
            } else if ascii_equal_lower(key_tok.text, "rrf_k") {
                let int_tok = self.expect(TokenKind::Integer)?;
                let k: u64 = int_tok.text.parse::<u64>().map_err(|_| {
                    QqlError::syntax("rrf_k must be a positive integer", int_tok.pos)
                })?;
                if k == 0 {
                    return Err(QqlError::syntax(
                        "rrf_k must be a positive integer",
                        int_tok.pos,
                    ));
                }
                rrf_k = Some(k);
            } else if ascii_equal_lower(key_tok.text, "rrf_weights") {
                self.expect(TokenKind::Lbracket)?;
                while self.peek()?.kind != TokenKind::Rbracket {
                    let val_tok = self.parse_number()?;
                    match val_tok {
                        Value::Int(i) => rrf_weights.push(i as f32),
                        Value::Float(f) => rrf_weights.push(f as f32),
                        _ => {
                            return Err(QqlError::syntax(
                                "rrf_weights must contain numeric values",
                                key_tok.pos,
                            ));
                        }
                    }
                    if self.peek()?.kind == TokenKind::Comma {
                        self.advance()?;
                        if self.peek()?.kind == TokenKind::Rbracket {
                            break;
                        }
                    } else {
                        break;
                    }
                }
                self.expect(TokenKind::Rbracket)?;
            } else if ascii_equal_lower(key_tok.text, "model") {
                let _ = self.expect(TokenKind::String)?;
            } else {
                return Err(QqlError::syntax(
                    alloc::format!(
                        "unknown WITH parameter '{}'. Expected: hnsw_ef, exact, acorn, indexed_only, quantization, mmr_diversity, mmr_candidates, rrf_k, rrf_weights",
                        key_tok.text
                    ),
                    key_tok.pos,
                ));
            }

            if self.peek()?.kind == TokenKind::Comma {
                self.advance()?;
                if self.peek()?.kind == TokenKind::Rparen {
                    break;
                }
            } else {
                break;
            }
        }
        self.expect(TokenKind::Rparen)?;

        Ok(SearchWith {
            hnsw_ef,
            exact,
            acorn,
            indexed_only,
            quantization,
            mmr_diversity,
            mmr_candidates,
            rrf_k,
            rrf_weights,
        })
    }

    pub fn parse_quantization_search_with(&mut self) -> Result<QuantizationSearchWith, QqlError> {
        self.expect(TokenKind::Lparen)?;
        let mut ignore: Option<bool> = None;
        let mut rescore: Option<bool> = None;
        let mut oversampling: Option<f64> = None;

        while self.peek()?.kind != TokenKind::Rparen {
            let key_tok = self.peek()?;
            if key_tok.kind != TokenKind::Identifier {
                return Err(QqlError::syntax(
                    alloc::format!(
                        "expected a quantization parameter name, got '{}'",
                        key_tok.text
                    ),
                    key_tok.pos,
                ));
            }
            self.advance()?;
            self.expect(TokenKind::Equals)?;

            if ascii_equal_lower(key_tok.text, "ignore") {
                ignore = Some(self.parse_bool()?);
            } else if ascii_equal_lower(key_tok.text, "rescore") {
                rescore = Some(self.parse_bool()?);
            } else if ascii_equal_lower(key_tok.text, "oversampling") {
                let value = self.parse_number()?;
                let v = match value {
                    Value::Int(i) => i as f64,
                    Value::Float(f) => f,
                    _ => {
                        return Err(QqlError::syntax(
                            "oversampling must be numeric",
                            key_tok.pos,
                        ));
                    }
                };
                oversampling = Some(v);
            } else {
                return Err(QqlError::syntax(
                    alloc::format!(
                        "unknown quantization parameter '{}'. Expected: ignore, rescore, oversampling",
                        key_tok.text
                    ),
                    key_tok.pos,
                ));
            }

            if self.peek()?.kind == TokenKind::Comma {
                self.advance()?;
                if self.peek()?.kind == TokenKind::Rparen {
                    break;
                }
            } else {
                break;
            }
        }
        self.expect(TokenKind::Rparen)?;

        Ok(QuantizationSearchWith {
            ignore,
            rescore,
            oversampling,
        })
    }

    pub fn parse_with_payload(&mut self) -> Result<Box<PayloadSelector>, QqlError> {
        if self.peek()?.kind == TokenKind::Identifier
            && (ascii_equal(self.peek()?.text, "TRUE") || ascii_equal(self.peek()?.text, "FALSE"))
        {
            let tok = self.advance()?;
            let val = ascii_equal(tok.text, "TRUE");
            return Ok(Box::new(PayloadSelector {
                enable: Some(val),
                include: Vec::new(),
                exclude: Vec::new(),
            }));
        }
        self.expect(TokenKind::Lparen)?;
        let mut include: Vec<String> = Vec::new();
        let mut exclude: Vec<String> = Vec::new();
        while self.peek()?.kind != TokenKind::Rparen {
            let key_tok = self.expect(TokenKind::Identifier)?;
            self.expect(TokenKind::Equals)?;
            self.expect(TokenKind::Lbracket)?;
            let mut fields = Vec::new();
            while self.peek()?.kind != TokenKind::Rbracket {
                let val = self.parse_string()?;
                fields.push(val);
                if self.peek()?.kind == TokenKind::Comma {
                    self.advance()?;
                } else {
                    break;
                }
            }
            self.expect(TokenKind::Rbracket)?;
            if ascii_equal_lower(key_tok.text, "include") {
                include = fields;
            } else if ascii_equal_lower(key_tok.text, "exclude") {
                exclude = fields;
            } else {
                return Err(QqlError::syntax(
                    alloc::format!("expected 'include' or 'exclude', got '{}'", key_tok.text),
                    key_tok.pos,
                ));
            }
            if self.peek()?.kind == TokenKind::Comma {
                self.advance()?;
            } else {
                break;
            }
        }
        self.expect(TokenKind::Rparen)?;
        Ok(Box::new(PayloadSelector {
            enable: None,
            include,
            exclude,
        }))
    }

    pub fn parse_with_vectors(&mut self) -> Result<Box<VectorsSelector>, QqlError> {
        if self.peek()?.kind == TokenKind::Identifier
            && (ascii_equal(self.peek()?.text, "TRUE") || ascii_equal(self.peek()?.text, "FALSE"))
        {
            let tok = self.advance()?;
            let val = ascii_equal(tok.text, "TRUE");
            return Ok(Box::new(VectorsSelector {
                enable: Some(val),
                vectors: Vec::new(),
            }));
        }
        self.expect(TokenKind::Lparen)?;
        let mut vectors = Vec::new();
        while self.peek()?.kind != TokenKind::Rparen {
            let val = self.parse_string()?;
            vectors.push(val);
            if self.peek()?.kind == TokenKind::Comma {
                self.advance()?;
            } else {
                break;
            }
        }
        self.expect(TokenKind::Rparen)?;
        Ok(Box::new(VectorsSelector {
            enable: None,
            vectors,
        }))
    }
}

pub fn merge_search_with(dst: &mut Option<Box<SearchWith>>, src: SearchWith) {
    if dst.is_none() {
        *dst = Some(Box::new(SearchWith {
            hnsw_ef: 0,
            exact: false,
            acorn: false,
            indexed_only: false,
            quantization: None,
            mmr_diversity: None,
            mmr_candidates: None,
            rrf_k: None,
            rrf_weights: Vec::new(),
        }));
    }
    let current = dst.as_mut().unwrap();
    if src.hnsw_ef != 0 {
        current.hnsw_ef = src.hnsw_ef;
    }
    if src.exact {
        current.exact = true;
    }
    if src.acorn {
        current.acorn = true;
    }
    if src.indexed_only {
        current.indexed_only = true;
    }
    if src.quantization.is_some() {
        current.quantization = src.quantization;
    }
    if src.mmr_diversity.is_some() {
        current.mmr_diversity = src.mmr_diversity;
    }
    if src.mmr_candidates.is_some() {
        current.mmr_candidates = src.mmr_candidates;
    }
    if src.rrf_k.is_some() {
        current.rrf_k = src.rrf_k;
    }
    if !src.rrf_weights.is_empty() {
        current.rrf_weights = src.rrf_weights;
    }
}
