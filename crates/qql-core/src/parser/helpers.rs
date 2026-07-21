use super::{ascii_equal, Parser};
use crate::ast::{EmbeddingSpec, PointId, PointVectors, Value, VectorValue};
use crate::error::{QqlError, Span};
use crate::token::{Token, TokenKind};
use alloc::string::{String, ToString};
use alloc::vec::Vec;

impl<'a> Parser<'a> {
    pub fn parse_string(&mut self) -> Result<String, QqlError> {
        let token = self.expect(TokenKind::String)?;
        self.decode_string(token)
    }

    pub fn parse_required_model_string(&mut self) -> Result<String, QqlError> {
        self.expect(TokenKind::Model)?;
        self.parse_string()
    }

    pub fn parse_optional_model_string(&mut self) -> Result<Option<String>, QqlError> {
        if self.peek()?.kind != TokenKind::Model {
            return Ok(None);
        }
        self.advance()?;
        self.parse_string().map(Some)
    }

    pub fn parse_optional_vector_name(&mut self) -> Result<Option<String>, QqlError> {
        if self.peek()?.kind != TokenKind::Vector {
            return Ok(None);
        }
        self.advance()?;
        self.parse_identifier().map(Some)
    }

    pub fn parse_embedding_options(&mut self) -> Result<Option<EmbeddingSpec>, QqlError> {
        if self.peek()?.kind != TokenKind::Using {
            return Ok(None);
        }
        self.advance()?;
        if self.peek()?.kind != TokenKind::Hybrid {
            if self.peek()?.kind == TokenKind::Dense {
                self.advance()?;
            }
            let model = self.parse_optional_model_string()?;
            let vector = self.parse_optional_vector_name()?;
            if model.is_none() && vector.is_none() {
                return Err(QqlError::parse(
                    "QQL-PARSE-EMBEDDING",
                    "USING requires MODEL or VECTOR",
                    self.peek()?.span,
                ));
            }
            return Ok(Some(EmbeddingSpec::Dense { model, vector }));
        }

        self.advance()?;
        let mut dense_model = None;
        let mut dense_vector = None;
        let mut sparse_model = None;
        let mut sparse_vector = None;
        for expected in [TokenKind::Dense, TokenKind::Sparse] {
            if self.peek()?.kind != expected {
                continue;
            }
            self.advance()?;
            let model = self.parse_optional_model_string()?;
            let vector = self.parse_optional_vector_name()?;
            if model.is_none() && vector.is_none() {
                return Err(QqlError::parse(
                    "QQL-PARSE-EMBEDDING",
                    "DENSE and SPARSE require MODEL or VECTOR",
                    self.peek()?.span,
                ));
            }
            if expected == TokenKind::Dense {
                dense_model = model;
                dense_vector = vector;
            } else {
                sparse_model = model;
                sparse_vector = vector;
            }
        }
        Ok(Some(EmbeddingSpec::Hybrid {
            dense_model,
            dense_vector,
            sparse_model,
            sparse_vector,
        }))
    }

    pub fn parse_point_id(&mut self, context: &str) -> Result<PointId, QqlError> {
        let token = self.peek()?;
        match token.kind {
            TokenKind::String => {
                self.advance()?;
                self.decode_string(token).map(PointId::String)
            }
            TokenKind::Integer => {
                self.advance()?;
                token.text.parse::<u64>().map(PointId::Number).map_err(|_| {
                    QqlError::parse(
                        "QQL-PARSE-POINT-ID",
                        alloc::format!(
                            "{} requires an unsigned integer or string point ID",
                            context
                        ),
                        token.span,
                    )
                })
            }
            _ => Err(QqlError::parse(
                "QQL-PARSE-POINT-ID",
                alloc::format!(
                    "{} requires an unsigned integer or string point ID",
                    context
                ),
                token.span,
            )),
        }
    }

    pub fn parse_point_id_list(&mut self) -> Result<Vec<PointId>, QqlError> {
        self.expect(TokenKind::Lparen)?;
        let mut ids = Vec::new();
        if self.peek()?.kind == TokenKind::Rparen {
            return Err(QqlError::parse(
                "QQL-PARSE-POINT-IDS",
                "point ID list cannot be empty",
                self.peek()?.span,
            ));
        }
        loop {
            ids.push(self.parse_point_id("point ID list")?);
            if self.peek()?.kind != TokenKind::Comma {
                break;
            }
            self.advance()?;
        }
        self.expect(TokenKind::Rparen)?;
        Ok(ids)
    }

    pub fn parse_literal(&mut self) -> Result<Value, QqlError> {
        let value = self.parse_value()?;
        if matches!(value, Value::Dict(_) | Value::List(_)) {
            return Err(QqlError::parse(
                "QQL-PARSE-LITERAL",
                "expected a scalar literal",
                self.peek()?.span,
            ));
        }
        Ok(value)
    }

    pub fn parse_number(&mut self) -> Result<Value, QqlError> {
        let token = self.peek()?;
        match token.kind {
            TokenKind::Integer | TokenKind::Float => self.parse_value(),
            _ => Err(QqlError::parse(
                "QQL-PARSE-NUMBER",
                alloc::format!("expected a number, got '{}'", token.text),
                token.span,
            )),
        }
    }

    pub fn parse_literal_list(&mut self) -> Result<Vec<Value>, QqlError> {
        self.expect(TokenKind::Lparen)?;
        let mut values = Vec::new();
        if self.peek()?.kind == TokenKind::Rparen {
            self.advance()?;
            return Ok(values);
        }
        loop {
            values.push(self.parse_literal()?);
            if self.peek()?.kind != TokenKind::Comma {
                break;
            }
            self.advance()?;
        }
        self.expect(TokenKind::Rparen)?;
        Ok(values)
    }

    pub fn parse_field_path(&mut self) -> Result<String, QqlError> {
        let token = self.peek()?;
        if token.kind != TokenKind::Identifier && !super::is_contextual_field_name(token.kind) {
            return Err(QqlError::parse(
                "QQL-PARSE-FIELD",
                alloc::format!("expected a field name, got '{}'", token.text),
                token.span,
            ));
        }
        self.advance()?;
        Ok(token.text.to_string())
    }

    pub fn parse_payload_dict(&mut self) -> Result<Vec<(String, Value)>, QqlError> {
        self.expect(TokenKind::Lbrace)?;
        let mut values = Vec::new();
        if self.peek()?.kind == TokenKind::Rbrace {
            self.advance()?;
            return Ok(values);
        }
        loop {
            let key_token = self.parse_object_key()?;
            let key = if key_token.kind == TokenKind::String {
                self.decode_string(key_token)?
            } else {
                key_token.text.to_string()
            };
            if values
                .iter()
                .any(|(candidate, _): &(String, Value)| candidate.eq_ignore_ascii_case(&key))
            {
                return Err(QqlError::parse(
                    "QQL-PARSE-DUPLICATE-KEY",
                    alloc::format!("duplicate payload key '{}'", key),
                    key_token.span,
                ));
            }
            self.expect(TokenKind::Colon)?;
            values.push((key, self.parse_value()?));
            if self.peek()?.kind != TokenKind::Comma {
                break;
            }
            self.advance()?;
            if self.peek()?.kind == TokenKind::Rbrace {
                break;
            }
        }
        self.expect(TokenKind::Rbrace)?;
        Ok(values)
    }

    pub fn parse_config_block(&mut self) -> Result<Vec<(String, Value)>, QqlError> {
        self.expect(TokenKind::Lparen)?;
        let mut values = Vec::new();
        if self.peek()?.kind == TokenKind::Rparen {
            self.advance()?;
            return Ok(values);
        }
        loop {
            let key_token = self.parse_object_key()?;
            let key = key_token.text.to_string();
            if values
                .iter()
                .any(|(candidate, _): &(String, Value)| candidate.eq_ignore_ascii_case(&key))
            {
                return Err(QqlError::parse(
                    "QQL-PARSE-DUPLICATE-KEY",
                    alloc::format!("duplicate configuration key '{}'", key),
                    key_token.span,
                ));
            }
            self.expect(TokenKind::Equals)?;
            values.push((key, self.parse_value()?));
            if self.peek()?.kind != TokenKind::Comma {
                break;
            }
            self.advance()?;
            if self.peek()?.kind == TokenKind::Rparen {
                break;
            }
        }
        self.expect(TokenKind::Rparen)?;
        Ok(values)
    }

    fn parse_object_key(&mut self) -> Result<Token<'a>, QqlError> {
        let token = self.peek()?;
        if matches!(
            token.kind,
            TokenKind::Lbrace
                | TokenKind::Rbrace
                | TokenKind::Lbracket
                | TokenKind::Rbracket
                | TokenKind::Lparen
                | TokenKind::Rparen
                | TokenKind::Colon
                | TokenKind::Comma
                | TokenKind::Equals
                | TokenKind::Semicolon
                | TokenKind::Eof
        ) {
            return Err(QqlError::parse(
                "QQL-PARSE-OBJECT-KEY",
                alloc::format!("expected an object key, got '{}'", token.text),
                token.span,
            ));
        }
        self.advance()
    }

    pub fn parse_list(&mut self) -> Result<Vec<Value>, QqlError> {
        self.expect(TokenKind::Lbracket)?;
        let mut values = Vec::new();
        if self.peek()?.kind == TokenKind::Rbracket {
            self.advance()?;
            return Ok(values);
        }
        loop {
            values.push(self.parse_value()?);
            if self.peek()?.kind != TokenKind::Comma {
                break;
            }
            self.advance()?;
            if self.peek()?.kind == TokenKind::Rbracket {
                break;
            }
        }
        self.expect(TokenKind::Rbracket)?;
        Ok(values)
    }

    pub fn parse_bool(&mut self) -> Result<bool, QqlError> {
        let token = self.peek()?;
        if token.kind == TokenKind::Identifier {
            self.advance()?;
            if ascii_equal(token.text, "TRUE") {
                return Ok(true);
            }
            if ascii_equal(token.text, "FALSE") {
                return Ok(false);
            }
        }
        Err(QqlError::parse(
            "QQL-PARSE-BOOL",
            alloc::format!("expected TRUE or FALSE, got '{}'", token.text),
            token.span,
        ))
    }

    pub fn parse_numeric_literal(&mut self) -> Result<f64, QqlError> {
        let token = self.peek()?;
        if !matches!(token.kind, TokenKind::Integer | TokenKind::Float) {
            return Err(QqlError::parse(
                "QQL-PARSE-NUMBER",
                alloc::format!("expected a number, got '{}'", token.text),
                token.span,
            ));
        }
        self.advance()?;
        token.text.parse::<f64>().map_err(|_| {
            QqlError::parse(
                "QQL-PARSE-NUMBER",
                alloc::format!("invalid number '{}'", token.text),
                token.span,
            )
        })
    }

    pub fn parse_positive_u64(&mut self, label: &str) -> Result<u64, QqlError> {
        let token = self.expect(TokenKind::Integer)?;
        let value = token.text.parse::<u64>().map_err(|_| {
            QqlError::parse(
                "QQL-PARSE-POSITIVE-INTEGER",
                alloc::format!("{} must be a positive integer", label),
                token.span,
            )
        })?;
        if value == 0 {
            return Err(QqlError::parse(
                "QQL-PARSE-POSITIVE-INTEGER",
                alloc::format!("{} must be a positive integer", label),
                token.span,
            ));
        }
        Ok(value)
    }

    pub fn parse_non_negative_u64(&mut self, label: &str) -> Result<u64, QqlError> {
        let token = self.expect(TokenKind::Integer)?;
        token.text.parse::<u64>().map_err(|_| {
            QqlError::parse(
                "QQL-PARSE-NONNEGATIVE-INTEGER",
                alloc::format!("{} must be a non-negative integer", label),
                token.span,
            )
        })
    }

    pub fn parse_vector_value(&mut self) -> Result<VectorValue, QqlError> {
        let span = self.peek()?.span;
        let value = self.parse_value()?;
        vector_from_value(value, span)
    }

    pub fn parse_point_vectors(&mut self) -> Result<PointVectors, QqlError> {
        let span = self.peek()?.span;
        let value = self.parse_value()?;
        match value {
            Value::Dict(items)
                if !items.iter().any(|(key, _)| {
                    key.eq_ignore_ascii_case("indices") || key.eq_ignore_ascii_case("values")
                }) =>
            {
                let vectors = items
                    .into_iter()
                    .map(|(name, value)| vector_from_value(value, span).map(|value| (name, value)))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(PointVectors::Named(vectors))
            }
            other => vector_from_value(other, span).map(PointVectors::Unnamed),
        }
    }
}

pub fn point_id_from_value(value: Value, span: Span) -> Result<PointId, QqlError> {
    match value {
        Value::Int(value) if value >= 0 => Ok(PointId::Number(value as u64)),
        Value::Str(value) => Ok(PointId::String(value)),
        _ => Err(QqlError::validation(
            "QQL-VALIDATION-POINT-ID",
            "point IDs must be unsigned integers or strings",
            Some(span),
        )),
    }
}

pub(super) fn vector_from_value(value: Value, span: Span) -> Result<VectorValue, QqlError> {
    match value {
        Value::List(values) if values.iter().all(|value| matches!(value, Value::List(_))) => values
            .into_iter()
            .map(|value| match value {
                Value::List(row) => numeric_vector(row, span),
                _ => unreachable!(),
            })
            .collect::<Result<Vec<_>, _>>()
            .and_then(|rows| {
                if rows.is_empty() {
                    Err(vector_error("multidense vector cannot be empty", span))
                } else {
                    Ok(VectorValue::MultiDense(rows))
                }
            }),
        Value::List(values) => numeric_vector(values, span).and_then(|values| {
            if values.is_empty() {
                Err(vector_error("dense vector cannot be empty", span))
            } else {
                Ok(VectorValue::Dense(values))
            }
        }),
        Value::Dict(items) => {
            let indices = items
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case("indices"))
                .map(|(_, value)| value);
            let values = items
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case("values"))
                .map(|(_, value)| value);
            let (Some(Value::List(indices)), Some(Value::List(values))) = (indices, values) else {
                return Err(vector_error(
                    "sparse vectors require indices and values lists",
                    span,
                ));
            };
            let indices = indices
                .iter()
                .map(|value| match value {
                    Value::Int(value) if *value >= 0 => u32::try_from(*value)
                        .map_err(|_| vector_error("sparse vector index is out of range", span)),
                    _ => Err(vector_error(
                        "sparse vector indices must be non-negative integers",
                        span,
                    )),
                })
                .collect::<Result<Vec<_>, _>>()?;
            let values = numeric_vector(values.clone(), span)?;
            if indices.is_empty() || indices.len() != values.len() {
                return Err(vector_error(
                    "sparse vector indices and values must be non-empty and have equal length",
                    span,
                ));
            }
            Ok(VectorValue::Sparse { indices, values })
        }
        _ => Err(vector_error(
            "vector must be a dense list, sparse object, or list of dense lists",
            span,
        )),
    }
}

fn numeric_vector(values: Vec<Value>, span: Span) -> Result<Vec<f32>, QqlError> {
    values
        .into_iter()
        .map(|value| {
            let value = match value {
                Value::Int(value) => value as f64,
                Value::Float(value) => value,
                _ => return Err(vector_error("vector elements must be numeric", span)),
            };
            let converted = value as f32;
            if !value.is_finite() || !converted.is_finite() {
                return Err(vector_error(
                    "vector elements must be finite f32 values",
                    span,
                ));
            }
            Ok(converted)
        })
        .collect()
}

fn vector_error(message: &'static str, span: Span) -> QqlError {
    QqlError::validation("QQL-VALIDATION-VECTOR", message, Some(span))
}
