use super::AstLowerer;
use crate::ast::{EmbeddingSpec, PointId, PointVectors, Value, VectorValue};
use crate::error::{QqlError, Span};
use crate::token::{Token, TokenKind};
use alloc::string::{String, ToString};
use alloc::vec::Vec;

impl<'a> AstLowerer<'a> {
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

    pub fn parse_embedding_options(&mut self) -> Result<Option<EmbeddingSpec>, QqlError> {
        if self.peek()?.kind != TokenKind::Using {
            return Ok(None);
        }
        self.advance()?;

        let mut specs = Vec::new();
        loop {
            let spec = self.parse_single_embedding_spec()?;
            specs.push(spec);
            if self.peek()?.kind != TokenKind::Comma {
                break;
            }
            self.advance()?;
        }

        if specs.len() == 1 {
            Ok(Some(specs.remove(0)))
        } else {
            Ok(Some(EmbeddingSpec::Multi(specs)))
        }
    }

    fn parse_single_embedding_spec(&mut self) -> Result<EmbeddingSpec, QqlError> {
        if self.peek()?.kind == TokenKind::Hybrid {
            self.advance()?;
            let mut dense_model = None;
            let mut dense_vector = None;
            let mut dense_field = None;
            let mut sparse_model = None;
            let mut sparse_vector = None;
            let mut sparse_field = None;
            let mut has_dense = false;
            let mut has_sparse = false;

            while self.peek()?.kind == TokenKind::Dense || self.peek()?.kind == TokenKind::Sparse {
                let is_d = self.peek()?.kind == TokenKind::Dense;
                if (is_d && has_dense) || (!is_d && has_sparse) {
                    return Err(QqlError::parse(
                        "QQL-PARSE-EMBEDDING",
                        "duplicate clause in HYBRID embedding spec",
                        self.peek()?.span,
                    ));
                }
                self.advance()?;
                let (model, vector, field) = self.parse_embedding_spec_modifiers()?;
                if is_d {
                    has_dense = true;
                    dense_model = model;
                    dense_vector = vector;
                    dense_field = field;
                } else {
                    has_sparse = true;
                    sparse_model = model;
                    sparse_vector = vector;
                    sparse_field = field;
                }
            }

            return Ok(EmbeddingSpec::Hybrid {
                dense_model,
                dense_vector,
                dense_field,
                sparse_model,
                sparse_vector,
                sparse_field,
            });
        }

        // MULTI / MULTIVECTOR / IMAGE as bare words (same as AS MULTI — not reserved).
        let is_multi = super::ascii_equal(self.peek()?.text, "MULTI")
            || super::ascii_equal(self.peek()?.text, "MULTIVECTOR");
        let is_image = super::ascii_equal(self.peek()?.text, "IMAGE");
        let is_sparse = self.peek()?.kind == TokenKind::Sparse;
        if self.peek()?.kind == TokenKind::Dense || is_sparse || is_multi || is_image {
            self.advance()?;
        } else if self.peek()?.kind != TokenKind::Model
            && self.peek()?.kind != TokenKind::Vector
            && self.peek()?.kind != TokenKind::Into
            && self.peek()?.kind != TokenKind::On
        {
            return Err(QqlError::parse(
                "QQL-PARSE-EMBEDDING",
                "USING requires DENSE, SPARSE, HYBRID, MULTI, IMAGE, MODEL, VECTOR, INTO, or ON FIELD",
                self.peek()?.span,
            ));
        }

        let (model, vector, field) = self.parse_embedding_spec_modifiers()?;

        if is_multi {
            Ok(EmbeddingSpec::MultiVector {
                model,
                vector,
                field,
            })
        } else if is_image {
            Ok(EmbeddingSpec::Image {
                model,
                vector,
                field,
            })
        } else if is_sparse {
            Ok(EmbeddingSpec::Sparse {
                model,
                vector,
                field,
            })
        } else {
            Ok(EmbeddingSpec::Dense {
                model,
                vector,
                field,
            })
        }
    }

    #[allow(clippy::type_complexity)]
    fn parse_embedding_spec_modifiers(
        &mut self,
    ) -> Result<(Option<String>, Option<String>, Option<String>), QqlError> {
        let mut model = None;
        let mut vector = None;
        let mut field = None;

        loop {
            let kind = self.peek()?.kind;
            if kind == TokenKind::Model && model.is_none() {
                self.advance()?;
                model = Some(self.parse_string()?);
            } else if (kind == TokenKind::Vector || kind == TokenKind::Into) && vector.is_none() {
                self.advance()?;
                vector = Some(self.parse_identifier()?);
            } else if kind == TokenKind::On && field.is_none() {
                self.advance()?;
                if self.peek()?.kind == TokenKind::Field {
                    self.advance()?;
                }
                field = Some(self.parse_identifier()?);
            } else {
                break;
            }
        }

        Ok((model, vector, field))
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
            TokenKind::Colon => {
                let colon_tok = self.advance()?;
                let name = self.parse_param_name()?;
                let span = Span::new(colon_tok.span.start, self.prev_span().end);
                Ok(PointId::Param(name, Some(alloc::boxed::Box::new(span))))
            }
            TokenKind::Question => {
                let q_tok = self.advance()?;
                let idx = self.next_positional_param();
                Ok(PointId::PositionalParam(
                    idx,
                    Some(alloc::boxed::Box::new(q_tok.span)),
                ))
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
        let start = self.peek()?.span;
        let value = self.parse_value()?;
        if matches!(value, Value::Dict(_) | Value::List(_)) {
            return Err(QqlError::parse(
                "QQL-PARSE-LITERAL",
                "expected a scalar literal",
                Span::new(start.start, self.prev_span().end),
            ));
        }
        Ok(value)
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
        if token.kind == TokenKind::String {
            self.advance()?;
            return self.decode_string(token);
        }
        if !token.is_keyword_or_identifier() {
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
            self.reject_trailing_comma(TokenKind::Rbrace)?;
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
            self.reject_trailing_comma(TokenKind::Rparen)?;
        }
        self.expect(TokenKind::Rparen)?;
        Ok(values)
    }

    pub(crate) fn parse_object_key(&mut self) -> Result<Token<'a>, QqlError> {
        let token = self.peek()?;
        let ok = token.is_keyword_or_identifier()
            || matches!(
                token.kind,
                TokenKind::String | TokenKind::Integer | TokenKind::Float
            );
        if !ok {
            return Err(QqlError::parse(
                "QQL-PARSE-OBJECT-KEY",
                alloc::format!("expected an object key, got '{}'", token.text),
                token.span,
            ));
        }
        self.advance()
    }

    /// Parse `:name` or `?` into the stored placeholder form (`":name"` / `"?{idx}"`).
    pub(crate) fn parse_placeholder_param(&mut self) -> Result<Option<(String, Span)>, QqlError> {
        match self.peek()?.kind {
            TokenKind::Colon => {
                let colon_tok = self.advance()?;
                let name = self.parse_param_name()?;
                Ok(Some((
                    alloc::format!(":{name}"),
                    Span::new(colon_tok.span.start, self.prev_span().end),
                )))
            }
            TokenKind::Question => {
                let q_tok = self.advance()?;
                let idx = self.next_positional_param();
                Ok(Some((alloc::format!("?{idx}"), q_tok.span)))
            }
            _ => Ok(None),
        }
    }

    pub(crate) fn reject_trailing_comma(&mut self, closer: TokenKind) -> Result<(), QqlError> {
        if self.peek()?.kind == closer {
            return Err(QqlError::parse(
                "QQL-PARSE-TRAILING-COMMA",
                "trailing commas are not allowed",
                self.peek()?.span,
            ));
        }
        Ok(())
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
            self.reject_trailing_comma(TokenKind::Rbracket)?;
        }
        self.expect(TokenKind::Rbracket)?;
        Ok(values)
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
        let value = token.text.parse::<f64>().map_err(|_| {
            QqlError::parse(
                "QQL-PARSE-NUMBER",
                alloc::format!("invalid number '{}'", token.text),
                token.span,
            )
        })?;
        // grammar.pest numbers are finite; exponent overflow (`1e999`) must
        // not leak inf/NaN into the AST.
        if !value.is_finite() {
            return Err(QqlError::parse(
                "QQL-PARSE-NUMBER",
                alloc::format!("number '{}' is not finite", token.text),
                token.span,
            ));
        }
        Ok(value)
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
        vector_from_value(value, Some(span))
    }

    pub fn parse_bool(&mut self) -> Result<bool, QqlError> {
        match self.peek()?.kind {
            TokenKind::True => {
                self.advance()?;
                Ok(true)
            }
            TokenKind::False => {
                self.advance()?;
                Ok(false)
            }
            _ => Err(QqlError::parse(
                "QQL-PARSE-BOOL",
                "expected true or false",
                self.peek()?.span,
            )),
        }
    }

    pub fn parse_optional_wait(&mut self) -> Result<Option<bool>, QqlError> {
        if self.peek()?.kind == TokenKind::Wait {
            self.advance()?;
            Ok(Some(self.parse_bool()?))
        } else {
            Ok(None)
        }
    }

    pub fn parse_shard_key_atom(&mut self) -> Result<crate::ast::ShardKey, QqlError> {
        match self.peek()?.kind {
            TokenKind::Integer => {
                let n = self.parse_non_negative_u64("shard key")?;
                Ok(crate::ast::ShardKey::Number(n))
            }
            TokenKind::Colon => {
                let colon_tok = self.advance()?;
                let name = self.parse_param_name()?;
                let span = Span::new(colon_tok.span.start, self.prev_span().end);
                Ok(crate::ast::ShardKey::param_with_span(name, span))
            }
            TokenKind::Question => {
                let q_tok = self.advance()?;
                let idx = self.next_positional_param();
                Ok(crate::ast::ShardKey::PositionalParam(
                    idx,
                    Some(alloc::boxed::Box::new(q_tok.span)),
                ))
            }
            _ => Ok(crate::ast::ShardKey::Keyword(self.parse_string()?)),
        }
    }

    pub fn parse_optional_typed_shard_and_wait(
        &mut self,
    ) -> Result<(Option<crate::ast::ShardKey>, Option<bool>), QqlError> {
        let mut shard_key = None;
        let mut wait = None;
        loop {
            if self.peek()?.kind == TokenKind::Shard && shard_key.is_none() {
                self.advance()?;
                shard_key = Some(self.parse_shard_key_atom()?);
            } else if let Some(w) = self.parse_optional_wait()? {
                if wait.is_some() {
                    return Err(QqlError::parse(
                        "QQL-PARSE-DUPLICATE-CLAUSE",
                        "duplicate WAIT clause",
                        self.peek()?.span,
                    ));
                }
                wait = Some(w);
            } else {
                break;
            }
        }
        Ok((shard_key, wait))
    }
}

pub fn point_id_from_value(value: Value, span: Span) -> Result<PointId, QqlError> {
    match value {
        Value::Int(value) if value >= 0 => Ok(PointId::Number(value as u64)),
        Value::Str(value) => Ok(PointId::String(value)),
        Value::Param(name, param_span) => Ok(PointId::Param(
            name,
            param_span.or_else(|| Some(alloc::boxed::Box::new(span))),
        )),
        Value::PositionalParam(idx, param_span) => Ok(PointId::PositionalParam(
            idx,
            param_span.or_else(|| Some(alloc::boxed::Box::new(span))),
        )),
        _ => Err(QqlError::validation(
            "QQL-VALIDATION-POINT-ID",
            "point IDs must be unsigned integers or strings",
            Some(span),
        )),
    }
}

pub(crate) fn vector_from_value(value: Value, span: Option<Span>) -> Result<VectorValue, QqlError> {
    match value {
        Value::F32Array(values) => {
            if values.is_empty() {
                Err(vector_error("dense vector cannot be empty", span))
            } else {
                Ok(VectorValue::Dense(values))
            }
        }
        Value::Param(name, param_span) => Ok(VectorValue::Param(
            name,
            param_span.or_else(|| span.map(alloc::boxed::Box::new)),
        )),
        Value::PositionalParam(idx, param_span) => Ok(VectorValue::PositionalParam(
            idx,
            param_span.or_else(|| span.map(alloc::boxed::Box::new)),
        )),
        Value::List(values)
            if values
                .iter()
                .all(|value| matches!(value, Value::List(_) | Value::F32Array(_))) =>
        {
            if values.is_empty() {
                return Err(vector_error("multidense vector cannot be empty", span));
            }
            let mut rows = Vec::with_capacity(values.len());
            for value in values {
                match value {
                    Value::F32Array(row) => {
                        if row.is_empty() {
                            return Err(vector_error(
                                "multidense vector rows cannot be empty",
                                span,
                            ));
                        }
                        rows.push(row);
                    }
                    Value::List(row) => {
                        let row_vec = numeric_vector(row, span)?;
                        if row_vec.is_empty() {
                            return Err(vector_error(
                                "multidense vector rows cannot be empty",
                                span,
                            ));
                        }
                        rows.push(row_vec);
                    }
                    // Guarded by the `all(List | F32Array)` match above.
                    _ => unreachable!("multidense row is List or F32Array"),
                }
            }
            Ok(VectorValue::MultiDense(rows))
        }
        Value::List(values) => numeric_vector(values, span).and_then(|values| {
            if values.is_empty() {
                Err(vector_error("dense vector cannot be empty", span))
            } else {
                Ok(VectorValue::Dense(values))
            }
        }),
        Value::Dict(items) => {
            let mut data_v = None;
            let mut dim_v = None;
            let mut indices_v = None;
            let mut values_v = None;
            for (key, value) in items {
                if key.eq_ignore_ascii_case("data") {
                    data_v = Some(value);
                } else if key.eq_ignore_ascii_case("dim") {
                    dim_v = Some(value);
                } else if key.eq_ignore_ascii_case("indices") {
                    indices_v = Some(value);
                } else if key.eq_ignore_ascii_case("values") {
                    values_v = Some(value);
                }
            }
            if data_v.is_some() || dim_v.is_some() {
                if indices_v.is_some() || values_v.is_some() {
                    return Err(vector_error(
                        "vector object must be either flat multivector {data, dim} or sparse {indices, values}, not both",
                        span,
                    ));
                }
                let (Some(data), Some(dim)) = (data_v, dim_v) else {
                    return Err(vector_error(
                        "flat multivector requires data and dim (e.g. {data: [...], dim: 128})",
                        span,
                    ));
                };
                let dim = match dim {
                    Value::Int(n) if n > 0 => usize::try_from(n)
                        .map_err(|_| vector_error("multivector dim is out of range", span))?,
                    _ => {
                        return Err(vector_error(
                            "multivector dim must be a positive integer",
                            span,
                        ));
                    }
                };
                let flat = match data {
                    Value::F32Array(flat) => flat,
                    Value::List(items) => numeric_vector(items, span)?,
                    _ => {
                        return Err(vector_error(
                            "multivector data must be a flat list of numbers",
                            span,
                        ));
                    }
                };
                if flat.is_empty() || flat.len() % dim != 0 {
                    return Err(vector_error(
                        "multivector data length must be a non-empty multiple of dim",
                        span,
                    ));
                }
                return Ok(VectorValue::MultiDense(
                    flat.chunks_exact(dim).map(<[f32]>::to_vec).collect(),
                ));
            }
            let (Some(indices), Some(values)) = (indices_v, values_v) else {
                return Err(vector_error(
                    "sparse vectors require indices and values lists",
                    span,
                ));
            };
            let Value::List(indices) = indices else {
                return Err(vector_error(
                    "sparse vector indices must be non-negative integers",
                    span,
                ));
            };
            let indices = indices
                .into_iter()
                .map(|value| match value {
                    Value::Int(value) if value >= 0 => u32::try_from(value)
                        .map_err(|_| vector_error("sparse vector index is out of range", span)),
                    _ => Err(vector_error(
                        "sparse vector indices must be non-negative integers",
                        span,
                    )),
                })
                .collect::<Result<Vec<_>, _>>()?;
            let values = match values {
                Value::F32Array(flat) => flat,
                Value::List(items) => numeric_vector(items, span)?,
                _ => {
                    return Err(vector_error(
                        "sparse vector values must be a list of numbers",
                        span,
                    ));
                }
            };
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

fn numeric_vector(values: Vec<Value>, span: Option<Span>) -> Result<Vec<f32>, QqlError> {
    values
        .into_iter()
        .map(|value| {
            let value = match value {
                Value::Int(value) => value as f64,
                Value::Float(value) => value,
                _ => {
                    return Err(QqlError::validation(
                        "QQL-BIND-TYPE-MISMATCH",
                        "vector elements must be numeric",
                        span,
                    ));
                }
            };
            let converted = value as f32;
            if !value.is_finite() || !converted.is_finite() {
                return Err(QqlError::validation(
                    "QQL-VALIDATION-VECTOR",
                    "vector elements must be finite f32 values",
                    span,
                ));
            }
            Ok(converted)
        })
        .collect()
}

fn vector_error(message: &'static str, span: Option<Span>) -> QqlError {
    QqlError::validation("QQL-VALIDATION-VECTOR", message, span)
}

pub fn point_vectors_from_value(
    value: Value,
    span: Option<Span>,
) -> Result<PointVectors, QqlError> {
    match value {
        Value::Param(name, param_span) => Ok(PointVectors::Param(
            name,
            param_span.or_else(|| span.map(alloc::boxed::Box::new)),
        )),
        Value::PositionalParam(idx, param_span) => Ok(PointVectors::PositionalParam(
            idx,
            param_span.or_else(|| span.map(alloc::boxed::Box::new)),
        )),
        Value::Dict(items)
            if !items.iter().any(|(key, _)| {
                key.eq_ignore_ascii_case("indices") || key.eq_ignore_ascii_case("values")
            }) =>
        {
            let mut vectors = Vec::new();
            for (name, value) in items {
                vectors.push((name, vector_from_value(value, span)?));
            }
            Ok(PointVectors::Named(vectors))
        }
        value => vector_from_value(value, span).map(PointVectors::Unnamed),
    }
}
