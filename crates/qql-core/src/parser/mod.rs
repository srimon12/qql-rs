pub mod create;
pub mod filter;
pub mod formula;
pub mod insert;
pub mod query;
pub mod select;
pub mod r#update;

use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::ast::{SearchWith, Stmt, Value};
use crate::error::QqlError;
use crate::lexer::Lexer;
use crate::token::{Token, TokenKind};

pub struct Parser<'a> {
    tokens: crate::lexer::TokenIter<'a>,
}

pub struct EmbeddingOptions<'a> {
    pub model: Option<&'a str>,
    pub hybrid: bool,
    pub sparse_model: Option<&'a str>,
    pub dense_vector: Option<&'a str>,
    pub sparse_vector: Option<&'a str>,
}

pub fn ascii_equal(s: &str, upper: &str) -> bool {
    if s.len() != upper.len() {
        return false;
    }
    s.as_bytes()
        .iter()
        .zip(upper.as_bytes().iter())
        .all(|(a, b)| a.to_ascii_uppercase() == *b)
}

pub fn ascii_equal_lower(s: &str, lower: &str) -> bool {
    if s.len() != lower.len() {
        return false;
    }
    s.as_bytes()
        .iter()
        .zip(lower.as_bytes().iter())
        .all(|(a, b)| a.to_ascii_lowercase() == *b)
}

pub fn token_kind_to_op(kind: TokenKind) -> &'static str {
    match kind {
        TokenKind::Equals => "=",
        TokenKind::NotEquals => "!=",
        TokenKind::Gt => ">",
        TokenKind::Gte => ">=",
        TokenKind::Lt => "<",
        TokenKind::Lte => "<=",
        TokenKind::GeoBbox => "GEO_BBOX",
        TokenKind::GeoRadius => "GEO_RADIUS",
        TokenKind::ValuesCount => "VALUES_COUNT",
        TokenKind::HasVector => "HAS_VECTOR",
        _ => "",
    }
}

pub fn is_contextual_field_name(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Offset
            | TokenKind::Score
            | TokenKind::Threshold
            | TokenKind::Lookup
            | TokenKind::Id
            | TokenKind::Dense
            | TokenKind::Sparse
            | TokenKind::Vector
            | TokenKind::By
    )
}

fn is_contextual_identifier(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Offset
            | TokenKind::Score
            | TokenKind::Threshold
            | TokenKind::Lookup
            | TokenKind::Id
            | TokenKind::Dense
            | TokenKind::Sparse
            | TokenKind::Vector
    )
}

impl<'a> Parser<'a> {
    pub fn new(tokens: crate::lexer::TokenIter<'a>) -> Self {
        Self { tokens }
    }

    pub fn parse(input: &'a str) -> Result<Stmt<'a>, QqlError> {
        let lexer = Lexer::new(input);
        let mut parser = Parser::new(lexer.peekable());
        parser.parse_stmt()
    }

    pub fn parse_stmt(&mut self) -> Result<Stmt<'a>, QqlError> {
        let tok = self.peek()?;
        match tok.kind {
            TokenKind::Create => self.parse_create(),
            TokenKind::Alter => self.parse_alter(),
            TokenKind::Drop => self.parse_drop(),
            TokenKind::Show => self.parse_show(),
            TokenKind::Insert => self.parse_insert(),
            TokenKind::Select => self.parse_select(),
            TokenKind::Scroll => self.parse_scroll(),
            TokenKind::Query => self.parse_query(),
            TokenKind::With => self.parse_query_with_cte(),
            TokenKind::Delete => self.parse_delete(),
            TokenKind::Update => self.parse_update(),
            _ => Err(QqlError::syntax(
                alloc::format!("expected a QQL statement keyword, got '{}'", tok.text),
                tok.pos,
            )),
        }
    }

    // ── Token stream helpers ────────────────────────────────────

    pub fn peek(&mut self) -> Result<Token<'a>, QqlError> {
        self.tokens.peek().cloned().unwrap_or(Ok(Token::eof()))
    }

    pub fn peek_kind(&mut self) -> Result<TokenKind, QqlError> {
        self.peek().map(|t| t.kind)
    }

    pub fn advance(&mut self) -> Result<Token<'a>, QqlError> {
        self.tokens.next().unwrap_or(Ok(Token::eof()))
    }

    pub fn expect(&mut self, kind: TokenKind) -> Result<Token<'a>, QqlError> {
        let tok = self.peek()?;
        if tok.kind != kind {
            return Err(QqlError::syntax(
                alloc::format!("expected {} but got '{}'", kind, tok.text),
                tok.pos,
            ));
        }
        self.advance()
    }

    // ── Identifier parsing ──────────────────────────────────────

    pub fn parse_identifier(&mut self) -> Result<&'a str, QqlError> {
        let tok = self.peek()?;
        if tok.kind == TokenKind::Identifier
            || tok.kind == TokenKind::String
            || is_contextual_identifier(tok.kind)
        {
            self.advance()?;
            Ok(tok.text)
        } else {
            Err(QqlError::syntax(
                alloc::format!("expected identifier or quoted name, got '{}'", tok.text),
                tok.pos,
            ))
        }
    }

    // ── Value parsing ───────────────────────────────────────────

    pub fn parse_value(&mut self) -> Result<Value<'a>, QqlError> {
        let tok = self.peek()?;
        match tok.kind {
            TokenKind::String => {
                self.advance()?;
                Ok(Value::Str(tok.text))
            }
            TokenKind::Float => {
                self.advance()?;
                let v: f64 = tok.text.parse().map_err(|_| {
                    QqlError::syntax(
                        alloc::format!("invalid float literal '{}'", tok.text),
                        tok.pos,
                    )
                })?;
                Ok(Value::Float(v))
            }
            TokenKind::Integer => {
                self.advance()?;
                let v: i64 = tok.text.parse().map_err(|_| {
                    QqlError::syntax(
                        alloc::format!("invalid integer literal '{}'", tok.text),
                        tok.pos,
                    )
                })?;
                Ok(Value::Int(v))
            }
            TokenKind::Null => {
                self.advance()?;
                Ok(Value::Null)
            }
            TokenKind::Identifier => {
                self.advance()?;
                if ascii_equal(tok.text, "TRUE") {
                    Ok(Value::Bool(true))
                } else if ascii_equal(tok.text, "FALSE") {
                    Ok(Value::Bool(false))
                } else if ascii_equal(tok.text, "NULL") {
                    Ok(Value::Null)
                } else {
                    Ok(Value::Str(tok.text))
                }
            }
            TokenKind::Lbrace => self.parse_payload_dict().map(Value::Dict),
            TokenKind::Lbracket => self.parse_list().map(Value::List),
            _ => Err(QqlError::syntax(
                alloc::format!("unexpected value token '{}'", tok.text),
                tok.pos,
            )),
        }
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
                Ok(Value::Str(tok.text))
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
                Ok(Value::Str(tok.text))
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
        if tok.kind != TokenKind::Identifier && !is_contextual_field_name(tok.kind) {
            return Err(QqlError::syntax(
                alloc::format!("expected a field name, got '{}'", tok.text),
                tok.pos,
            ));
        }
        self.advance()?;
        Ok(tok.text)
    }

    // ── WITH clause helpers ─────────────────────────────────────

    pub fn parse_with_clause(&mut self) -> Result<SearchWith, QqlError> {
        self.expect(TokenKind::Lparen)?;
        let mut hnsw_ef: u64 = 0;
        let mut exact = false;
        let mut acorn = false;
        let mut indexed_only = false;
        let mut quantization: Option<Box<crate::ast::QuantizationSearchWith>> = None;
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

    pub fn parse_quantization_search_with(
        &mut self,
    ) -> Result<crate::ast::QuantizationSearchWith, QqlError> {
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

        Ok(crate::ast::QuantizationSearchWith {
            ignore,
            rescore,
            oversampling,
        })
    }

    pub fn parse_with_payload(&mut self) -> Result<Box<crate::ast::PayloadSelector<'a>>, QqlError> {
        if self.peek()?.kind == TokenKind::Identifier
            && (ascii_equal(self.peek()?.text, "TRUE") || ascii_equal(self.peek()?.text, "FALSE"))
        {
            let tok = self.advance()?;
            let val = ascii_equal(tok.text, "TRUE");
            return Ok(Box::new(crate::ast::PayloadSelector {
                enable: Some(val),
                include: Vec::new(),
                exclude: Vec::new(),
            }));
        }
        self.expect(TokenKind::Lparen)?;
        let mut include: Vec<&'a str> = Vec::new();
        let mut exclude: Vec<&'a str> = Vec::new();
        while self.peek()?.kind != TokenKind::Rparen {
            let key_tok = self.expect(TokenKind::Identifier)?;
            self.expect(TokenKind::Equals)?;
            self.expect(TokenKind::Lbracket)?;
            let mut fields = Vec::new();
            while self.peek()?.kind != TokenKind::Rbracket {
                let val_tok = self.expect(TokenKind::String)?;
                fields.push(val_tok.text);
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
        Ok(Box::new(crate::ast::PayloadSelector {
            enable: None,
            include,
            exclude,
        }))
    }

    pub fn parse_with_vectors(&mut self) -> Result<Box<crate::ast::VectorsSelector<'a>>, QqlError> {
        if self.peek()?.kind == TokenKind::Identifier
            && (ascii_equal(self.peek()?.text, "TRUE") || ascii_equal(self.peek()?.text, "FALSE"))
        {
            let tok = self.advance()?;
            let val = ascii_equal(tok.text, "TRUE");
            return Ok(Box::new(crate::ast::VectorsSelector {
                enable: Some(val),
                vectors: Vec::new(),
            }));
        }
        self.expect(TokenKind::Lparen)?;
        let mut vectors = Vec::new();
        while self.peek()?.kind != TokenKind::Rparen {
            let val_tok = self.expect(TokenKind::String)?;
            vectors.push(val_tok.text);
            if self.peek()?.kind == TokenKind::Comma {
                self.advance()?;
            } else {
                break;
            }
        }
        self.expect(TokenKind::Rparen)?;
        Ok(Box::new(crate::ast::VectorsSelector {
            enable: None,
            vectors,
        }))
    }
}

// ── Standalone merge helper ─────────────────────────────────────

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

// ── Config dict helpers (case-insensitive lookup) ──────────────

pub fn config_value<'a>(config: &'a [(&'a str, Value<'a>)], key: &str) -> Option<&'a Value<'a>> {
    for (k, v) in config {
        if ascii_equal_lower(k, key) {
            return Some(v);
        }
    }
    None
}

pub fn config_has_key(config: &[(&str, Value)], key: &str) -> bool {
    config_value(config, key).is_some()
}

pub fn config_bool(config: &[(&str, Value)], key: &str) -> Option<bool> {
    match config_value(config, key)? {
        Value::Bool(b) => Some(*b),
        _ => None,
    }
}

pub fn config_positive_u64(
    config: &[(&str, Value)],
    key: &str,
    pos: usize,
) -> Result<Option<u64>, QqlError> {
    match config_value(config, key) {
        None => Ok(None),
        Some(Value::Int(n)) if *n > 0 => Ok(Some(*n as u64)),
        Some(Value::Float(n)) if *n > 0.0 && *n == (*n as u64) as f64 => Ok(Some(*n as u64)),
        _ => Err(QqlError::syntax(
            alloc::format!("{} must be a positive integer", key),
            pos,
        )),
    }
}

pub fn config_non_negative_u64(
    config: &[(&str, Value)],
    key: &str,
    pos: usize,
) -> Result<Option<u64>, QqlError> {
    match config_value(config, key) {
        None => Ok(None),
        Some(Value::Int(n)) if *n >= 0 => Ok(Some(*n as u64)),
        Some(Value::Float(n)) if *n >= 0.0 && *n == (*n as u64) as f64 => Ok(Some(*n as u64)),
        _ => Err(QqlError::syntax(
            alloc::format!("{} must be a non-negative integer", key),
            pos,
        )),
    }
}

pub fn config_float_range(config: &[(&str, Value)], key: &str, min: f64, max: f64) -> Option<f64> {
    match config_value(config, key)? {
        Value::Int(n) => {
            let f = *n as f64;
            if (min..=max).contains(&f) {
                Some(f)
            } else {
                None
            }
        }
        Value::Float(f) => {
            if (min..=max).contains(f) {
                Some(*f)
            } else {
                None
            }
        }
        _ => None,
    }
}

pub fn config_max_optimization_threads(
    config: &[(&str, Value)],
    key: &str,
) -> Option<crate::ast::OptimizationThreads> {
    match config_value(config, key)? {
        Value::Int(n) if *n > 0 => Some(crate::ast::OptimizationThreads {
            auto_: false,
            value: *n as u64,
        }),
        Value::Str(s) if ascii_equal_lower(s, "auto") => Some(crate::ast::OptimizationThreads {
            auto_: true,
            value: 0,
        }),
        _ => None,
    }
}

pub fn validate_hnsw_value(key: &str, value: &Value, pos: usize) -> Result<(), QqlError> {
    let lower = key.to_ascii_lowercase();
    match lower.as_str() {
        "m" | "ef_construct" | "full_scan_threshold" | "max_indexing_threads" | "payload_m" => {
            if !matches!(value, Value::Int(_)) {
                return Err(QqlError::syntax(
                    alloc::format!("{} must be an integer", key),
                    pos,
                ));
            }
        }
        "on_disk" | "inline_storage" if !matches!(value, Value::Bool(_)) => {
            return Err(QqlError::syntax(
                alloc::format!("{} must be true or false", key),
                pos,
            ));
        }
        _ => {}
    }
    Ok(())
}

pub fn validate_vectors_value(key: &str, value: &Value, pos: usize) -> Result<(), QqlError> {
    if ascii_equal_lower(key, "on_disk") && !matches!(value, Value::Bool(_)) {
        return Err(QqlError::syntax(
            alloc::format!("{} must be true or false", key),
            pos,
        ));
    }
    Ok(())
}

pub fn validate_optimizers_value(key: &str, value: &Value, pos: usize) -> Result<(), QqlError> {
    let lower = key.to_ascii_lowercase();
    match lower.as_str() {
        "deleted_threshold" => {
            if !matches!(value, Value::Int(_) | Value::Float(_)) {
                return Err(QqlError::syntax(
                    alloc::format!("{} must be a number", key),
                    pos,
                ));
            }
        }
        "vacuum_min_vector_number"
        | "default_segment_number"
        | "max_segment_size"
        | "memmap_threshold"
        | "indexing_threshold"
        | "flush_interval_sec" => {
            if !matches!(value, Value::Int(_)) {
                return Err(QqlError::syntax(
                    alloc::format!("{} must be an integer", key),
                    pos,
                ));
            }
        }
        "max_optimization_threads" => {
            if !matches!(value, Value::Int(_) | Value::Str(_)) {
                return Err(QqlError::syntax(
                    alloc::format!("{} must be a positive integer or 'auto'", key),
                    pos,
                ));
            }
        }
        "prevent_unoptimized" if !matches!(value, Value::Bool(_)) => {
            return Err(QqlError::syntax(
                alloc::format!("{} must be true or false", key),
                pos,
            ));
        }
        _ => {}
    }
    Ok(())
}

pub fn validate_params_value(key: &str, value: &Value, pos: usize) -> Result<(), QqlError> {
    let lower = key.to_ascii_lowercase();
    match lower.as_str() {
        "replication_factor"
        | "write_consistency_factor"
        | "read_fan_out_factor"
        | "read_fan_out_delay_ms" => {
            if !matches!(value, Value::Int(_)) {
                return Err(QqlError::syntax(
                    alloc::format!("{} must be an integer", key),
                    pos,
                ));
            }
        }
        "on_disk_payload" if !matches!(value, Value::Bool(_)) => {
            return Err(QqlError::syntax(
                alloc::format!("{} must be true or false", key),
                pos,
            ));
        }
        _ => {}
    }
    Ok(())
}

pub fn merge_collection_config(
    current: &mut crate::ast::CollectionConfig,
    new: crate::ast::CollectionConfig,
    pos: usize,
) -> Result<(), QqlError> {
    if new.vectors.is_some() {
        if current.vectors.is_some() {
            return Err(QqlError::syntax("VECTORS clause may only appear once", pos));
        }
        current.vectors = new.vectors;
    }
    if new.hnsw.is_some() {
        if current.hnsw.is_some() {
            return Err(QqlError::syntax("HNSW clause may only appear once", pos));
        }
        current.hnsw = new.hnsw;
    }
    if new.optimizers.is_some() {
        if current.optimizers.is_some() {
            return Err(QqlError::syntax(
                "OPTIMIZERS clause may only appear once",
                pos,
            ));
        }
        current.optimizers = new.optimizers;
    }
    if new.params.is_some() {
        if current.params.is_some() {
            return Err(QqlError::syntax("PARAMS clause may only appear once", pos));
        }
        current.params = new.params;
    }
    if new.quantization.is_some() {
        if current.quantization.is_some() {
            return Err(QqlError::syntax(
                "QUANTIZATION clause may only appear once",
                pos,
            ));
        }
        current.quantization = new.quantization;
    }
    if new.quantization_update.is_some() {
        if current.quantization_update.is_some() {
            return Err(QqlError::syntax(
                "QUANTIZATION clause may only appear once",
                pos,
            ));
        }
        current.quantization_update = new.quantization_update;
    }
    Ok(())
}

pub fn check_deleted_threshold(value: &Value, pos: usize) -> Result<(), QqlError> {
    match value {
        Value::Int(n) => {
            let f = *n as f64;
            if !(0.0..=1.0).contains(&f) {
                return Err(QqlError::syntax(
                    "deleted_threshold must be between 0.0 and 1.0",
                    pos,
                ));
            }
        }
        Value::Float(f) if !(0.0..=1.0).contains(f) => {
            return Err(QqlError::syntax(
                "deleted_threshold must be between 0.0 and 1.0",
                pos,
            ));
        }
        _ => {}
    }
    Ok(())
}
