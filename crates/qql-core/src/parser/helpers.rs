use super::{ascii_equal, EmbeddingOptions, Parser};
use crate::ast::Value;
use crate::error::QqlError;
use crate::token::TokenKind;
use alloc::vec::Vec;

impl<'a> Parser<'a> {
    // ── String pointer helpers ──────────────────────────────────

    pub fn parse_string_ptr(&mut self) -> Result<&'a str, QqlError> {
        let tok = self.expect(TokenKind::String)?;
        Ok(tok.text)
    }

    pub fn parse_required_model_string(&mut self) -> Result<&'a str, QqlError> {
        self.expect(TokenKind::Model)?;
        self.parse_string_ptr()
    }

    pub fn parse_optional_model_string(&mut self) -> Result<Option<&'a str>, QqlError> {
        if self.peek()?.kind != TokenKind::Model {
            return Ok(None);
        }
        self.advance()?;
        self.parse_string_ptr().map(Some)
    }

    pub fn parse_optional_vector_string(&mut self) -> Result<Option<&'a str>, QqlError> {
        let tok = self.peek()?;
        if tok.kind == TokenKind::Vector
            || (tok.kind == TokenKind::Identifier && ascii_equal(tok.text, "VECTOR"))
        {
            self.advance()?;
            return self.parse_string_ptr().map(Some);
        }
        Ok(None)
    }

    // ── Embedding options ───────────────────────────────────────

    pub fn parse_embedding_options(&mut self) -> Result<EmbeddingOptions<'a>, QqlError> {
        if self.peek()?.kind != TokenKind::Using {
            return Ok(EmbeddingOptions {
                model: None,
                hybrid: false,
                sparse_model: None,
                dense_vector: None,
                sparse_vector: None,
            });
        }
        self.advance()?;

        if self.peek()?.kind != TokenKind::Hybrid {
            if self.peek()?.kind == TokenKind::Dense {
                self.advance()?;
            }
            let mut dense_vector = self.parse_optional_vector_string()?;
            let mut model = self.parse_optional_model_string()?;
            if dense_vector.is_none() {
                dense_vector = self.parse_optional_vector_string()?;
            }
            if model.is_none() && dense_vector.is_none() {
                model = Some(self.parse_required_model_string()?);
            }
            return Ok(EmbeddingOptions {
                model,
                hybrid: false,
                sparse_model: None,
                dense_vector,
                sparse_vector: None,
            });
        }

        self.advance()?; // consume HYBRID
        let mut model: Option<&'a str> = None;
        let mut sparse_model: Option<&'a str> = None;
        let mut dense_vector: Option<&'a str> = None;
        let mut sparse_vector: Option<&'a str> = None;

        while self.peek()?.kind == TokenKind::Dense || self.peek()?.kind == TokenKind::Sparse {
            let mode = self.advance()?.kind;
            let mut current_vector = self.parse_optional_vector_string()?;
            let current_model = self.parse_optional_model_string()?;
            if current_vector.is_none() {
                current_vector = self.parse_optional_vector_string()?;
            }
            if current_model.is_none() && current_vector.is_none() {
                return Err(QqlError::syntax(
                    "expected MODEL or VECTOR after DENSE/SPARSE",
                    self.peek()?.pos,
                ));
            }
            if mode == TokenKind::Dense {
                model = current_model;
                dense_vector = current_vector;
            } else {
                sparse_model = current_model;
                sparse_vector = current_vector;
            }
        }

        Ok(EmbeddingOptions {
            model,
            hybrid: true,
            sparse_model,
            dense_vector,
            sparse_vector,
        })
    }

    // ── Point ID helpers ────────────────────────────────────────

    pub fn parse_point_id_value(&mut self, context: &str) -> Result<Value<'a>, QqlError> {
        let tok = self.peek()?;
        match tok.kind {
            TokenKind::String => {
                self.advance()?;
                Ok(Value::Str(alloc::borrow::Cow::Borrowed(tok.text)))
            }
            TokenKind::Integer => {
                self.advance()?;
                let v: i64 = tok.text.parse().map_err(|_| {
                    QqlError::syntax(alloc::format!("invalid integer '{}'", tok.text), tok.pos)
                })?;
                Ok(Value::Int(v))
            }
            _ => Err(QqlError::syntax(
                alloc::format!(
                    "{} requires a string or integer point id, got '{}'",
                    context,
                    tok.text
                ),
                tok.pos,
            )),
        }
    }

    pub fn parse_point_id_list(&mut self) -> Result<Vec<Value<'a>>, QqlError> {
        let values = self.parse_literal_list()?;
        for v in &values {
            match v {
                Value::Str(_) | Value::Int(_) => {}
                _ => {
                    return Err(QqlError::syntax(
                        "point ids must be strings or integers",
                        self.peek()?.pos,
                    ));
                }
            }
        }
        Ok(values)
    }

    // ── Literal / Number helpers ────────────────────────────────

    pub fn parse_literal(&mut self) -> Result<Value<'a>, QqlError> {
        let tok = self.peek()?;
        match tok.kind {
            TokenKind::String => {
                self.advance()?;
                Ok(Value::Str(alloc::borrow::Cow::Borrowed(tok.text)))
            }
            TokenKind::Integer => {
                self.advance()?;
                let v: i64 = tok.text.parse().map_err(|_| {
                    QqlError::syntax(alloc::format!("invalid integer '{}'", tok.text), tok.pos)
                })?;
                Ok(Value::Int(v))
            }
            TokenKind::Float => {
                self.advance()?;
                let v: f64 = tok.text.parse().map_err(|_| {
                    QqlError::syntax(alloc::format!("invalid float '{}'", tok.text), tok.pos)
                })?;
                Ok(Value::Float(v))
            }
            TokenKind::Identifier => {
                self.advance()?;
                if ascii_equal(tok.text, "TRUE") {
                    Ok(Value::Bool(true))
                } else if ascii_equal(tok.text, "FALSE") {
                    Ok(Value::Bool(false))
                } else {
                    Err(QqlError::syntax(
                        alloc::format!("expected a literal value, got '{}'", tok.text),
                        tok.pos,
                    ))
                }
            }
            _ => Err(QqlError::syntax(
                alloc::format!("expected a literal value, got '{}'", tok.text),
                tok.pos,
            )),
        }
    }

    pub fn parse_number(&mut self) -> Result<Value<'a>, QqlError> {
        let tok = self.peek()?;
        match tok.kind {
            TokenKind::Integer => {
                self.advance()?;
                let v: i64 = tok.text.parse().map_err(|_| {
                    QqlError::syntax(alloc::format!("invalid integer '{}'", tok.text), tok.pos)
                })?;
                Ok(Value::Int(v))
            }
            TokenKind::Float => {
                self.advance()?;
                let v: f64 = tok.text.parse().map_err(|_| {
                    QqlError::syntax(alloc::format!("invalid float '{}'", tok.text), tok.pos)
                })?;
                Ok(Value::Float(v))
            }
            _ => Err(QqlError::syntax(
                alloc::format!("expected a number, got '{}'", tok.text),
                tok.pos,
            )),
        }
    }

    pub fn parse_literal_list(&mut self) -> Result<Vec<Value<'a>>, QqlError> {
        self.expect(TokenKind::Lparen)?;
        let mut items = Vec::new();
        if self.peek()?.kind == TokenKind::Rparen {
            self.advance()?;
            return Ok(items);
        }
        loop {
            let val = self.parse_literal()?;
            items.push(val);
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
        Ok(items)
    }

    // ── Field path ──────────────────────────────────────────────

    pub fn parse_field_path(&mut self) -> Result<&'a str, QqlError> {
        let tok = self.peek()?;
        if tok.kind != TokenKind::Identifier && !super::is_contextual_field_name(tok.kind) {
            return Err(QqlError::syntax(
                alloc::format!("expected a field name, got '{}'", tok.text),
                tok.pos,
            ));
        }
        self.advance()?;
        Ok(tok.text)
    }

    // ── Dict / Config / List helpers ────────────────────────────

    pub fn parse_payload_dict(&mut self) -> Result<Vec<(&'a str, Value<'a>)>, QqlError> {
        self.expect(TokenKind::Lbrace)?;
        let mut result = Vec::new();
        if self.peek()?.kind == TokenKind::Rbrace {
            self.advance()?;
            return Ok(result);
        }
        loop {
            let key_tok = self.peek()?;
            if key_tok.kind != TokenKind::String
                && key_tok.kind != TokenKind::Identifier
                && key_tok.kind != TokenKind::Id
            {
                return Err(QqlError::syntax(
                    alloc::format!("expected string key in dict, got '{}'", key_tok.text),
                    key_tok.pos,
                ));
            }
            self.advance()?;
            let key = key_tok.text;
            self.expect(TokenKind::Colon)?;
            let value = self.parse_value()?;
            result.push((key, value));
            if self.peek()?.kind == TokenKind::Comma {
                self.advance()?;
                if self.peek()?.kind == TokenKind::Rbrace {
                    break;
                }
            } else {
                break;
            }
        }
        self.expect(TokenKind::Rbrace)?;
        Ok(result)
    }

    pub fn parse_config_block(&mut self) -> Result<Vec<(&'a str, Value<'a>)>, QqlError> {
        self.expect(TokenKind::Lparen)?;
        let mut result = Vec::new();
        if self.peek()?.kind == TokenKind::Rparen {
            self.advance()?;
            return Ok(result);
        }
        loop {
            let key_tok = self.peek()?;
            match key_tok.kind {
                TokenKind::Lparen
                | TokenKind::Rparen
                | TokenKind::Equals
                | TokenKind::Comma
                | TokenKind::Eof => {
                    return Err(QqlError::syntax(
                        alloc::format!("expected configuration key, got '{}'", key_tok.text),
                        key_tok.pos,
                    ));
                }
                _ => {}
            }
            self.advance()?;
            let key = key_tok.text;
            self.expect(TokenKind::Equals)?;
            let value = self.parse_value()?;
            result.push((key, value));
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
        Ok(result)
    }

    pub fn parse_list(&mut self) -> Result<Vec<Value<'a>>, QqlError> {
        self.expect(TokenKind::Lbracket)?;
        let mut items = Vec::new();
        if self.peek()?.kind == TokenKind::Rbracket {
            self.advance()?;
            return Ok(items);
        }
        loop {
            let value = self.parse_value()?;
            items.push(value);
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
        Ok(items)
    }

    // ── Boolean / Numeric helpers ───────────────────────────────

    pub fn parse_bool(&mut self) -> Result<bool, QqlError> {
        let tok = self.peek()?;
        if tok.kind == TokenKind::Identifier {
            self.advance()?;
            if ascii_equal(tok.text, "TRUE") {
                return Ok(true);
            }
            if ascii_equal(tok.text, "FALSE") {
                return Ok(false);
            }
        }
        Err(QqlError::syntax(
            alloc::format!("expected TRUE or FALSE, got '{}'", tok.text),
            tok.pos,
        ))
    }

    pub fn parse_numeric_literal(&mut self) -> Result<f64, QqlError> {
        let tok = self.peek()?;
        match tok.kind {
            TokenKind::Integer => {
                self.advance()?;
                tok.text.parse::<i64>().map(|v| v as f64).map_err(|_| {
                    QqlError::syntax(alloc::format!("invalid integer '{}'", tok.text), tok.pos)
                })
            }
            TokenKind::Float => {
                self.advance()?;
                tok.text.parse::<f64>().map_err(|_| {
                    QqlError::syntax(alloc::format!("invalid float '{}'", tok.text), tok.pos)
                })
            }
            _ => Err(QqlError::syntax(
                alloc::format!("expected a number, got '{}'", tok.text),
                tok.pos,
            )),
        }
    }

    // ── Vector helpers ──────────────────────────────────────────

    pub fn parse_raw_vector(&mut self) -> Result<Vec<f64>, QqlError> {
        self.expect(TokenKind::Lbracket)?;
        let mut vec = Vec::new();
        while self.peek()?.kind != TokenKind::Rbracket && self.peek()?.kind != TokenKind::Eof {
            let tok = self.peek()?;
            if tok.kind != TokenKind::Float && tok.kind != TokenKind::Integer {
                return Err(QqlError::syntax(
                    alloc::format!("expected numeric value in raw vector, got '{}'", tok.text),
                    tok.pos,
                ));
            }
            let f = self.parse_numeric_literal()?;
            vec.push(f);
            if self.peek()?.kind == TokenKind::Comma {
                self.advance()?;
            }
        }
        self.expect(TokenKind::Rbracket)?;
        Ok(vec)
    }

    pub fn coerce_vector_values(&self, values: Vec<Value<'a>>) -> Result<Vec<f32>, QqlError> {
        let mut vector = Vec::with_capacity(values.len());
        for v in values {
            match v {
                Value::Int(i) => vector.push(i as f32),
                Value::Float(f) => vector.push(f as f32),
                _ => return Err(QqlError::syntax("vector elements must be numeric", 0)),
            }
        }
        Ok(vector)
    }
}
