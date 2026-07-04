pub mod alter_drop_show;
pub mod config_parsers;
pub mod config_validation;
pub mod create;
pub mod cte;
pub mod filter;
pub mod formula;
pub mod helpers;
pub mod insert;
pub mod query;
pub mod query_clauses;
pub mod select;
pub mod r#update;
pub mod with_clause;

pub use config_validation::{
    check_deleted_threshold, config_bool, config_float_range, config_has_key,
    config_max_optimization_threads, config_non_negative_u64, config_positive_u64, config_value,
    merge_collection_config, validate_hnsw_value, validate_optimizers_value, validate_params_value,
    validate_vectors_value,
};
pub use with_clause::merge_search_with;

use crate::ast::{Stmt, Value};
use crate::error::QqlError;
use crate::lexer::Lexer;
use crate::token::{Token, TokenKind};

pub struct Parser<'a> {
    tokens: alloc::vec::Vec<Token<'a>>,
    index: usize,
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
    pub fn new(tokens: alloc::vec::Vec<Token<'a>>) -> Self {
        Self { tokens, index: 0 }
    }

    pub fn parse(input: &'a str) -> Result<Stmt<'a>, QqlError> {
        let lexer = Lexer::new(input);
        let mut tokens = alloc::vec::Vec::new();
        for token_res in lexer {
            tokens.push(token_res?);
        }
        let mut parser = Parser::new(tokens);
        parser.parse_stmt()
    }

    pub fn parse_all(input: &'a str) -> Result<alloc::vec::Vec<Stmt<'a>>, QqlError> {
        let lexer = Lexer::new(input);
        let mut tokens = alloc::vec::Vec::new();
        for token_res in lexer {
            tokens.push(token_res?);
        }
        let mut parser = Parser::new(tokens);
        let mut stmts = alloc::vec::Vec::new();
        while parser.index < parser.tokens.len() {
            if parser.tokens[parser.index].kind == TokenKind::Semicolon {
                parser.index += 1;
                continue;
            }
            stmts.push(parser.parse_stmt()?);
        }
        Ok(stmts)
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
        if self.index < self.tokens.len() {
            Ok(self.tokens[self.index])
        } else {
            Ok(Token::eof())
        }
    }

    pub fn save_pos(&self) -> usize {
        self.index
    }

    pub fn restore_pos(&mut self, saved: usize) {
        self.index = saved;
    }

    pub fn peek_kind(&mut self) -> Result<TokenKind, QqlError> {
        self.peek().map(|t| t.kind)
    }

    pub fn advance(&mut self) -> Result<Token<'a>, QqlError> {
        let tok = self.peek()?;
        if self.index < self.tokens.len() {
            self.index += 1;
        }
        Ok(tok)
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
                Ok(Value::Str(alloc::borrow::Cow::Borrowed(tok.text)))
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
                    Ok(Value::Str(alloc::borrow::Cow::Borrowed(tok.text)))
                }
            }
            TokenKind::Lbrace => self.parse_payload_dict().map(|items| {
                Value::Dict(
                    items
                        .into_iter()
                        .map(|(k, v)| (alloc::borrow::Cow::Borrowed(k), v))
                        .collect(),
                )
            }),
            TokenKind::Lbracket => self.parse_list().map(Value::List),
            _ => Err(QqlError::syntax(
                alloc::format!("unexpected value token '{}'", tok.text),
                tok.pos,
            )),
        }
    }
}
