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
    merge_collection_config, validate_hnsw_value, validate_index_options,
    validate_optimizers_value, validate_params_value, validate_vectors_value,
};
pub use with_clause::merge_search_with;

use crate::ast::Stmt;
use crate::error::QqlError;
use crate::lexer::Lexer;
use crate::token::{Token, TokenKind};
use alloc::string::String;
use alloc::vec::Vec;

pub struct Parser<'a> {
    pub input: &'a str,
    tokens: Vec<Token<'a>>,
    index: usize,
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
    pub fn new(input: &'a str, tokens: Vec<Token<'a>>) -> Self {
        Self {
            input,
            tokens,
            index: 0,
        }
    }

    pub fn parse(input: &'a str) -> Result<Stmt, QqlError> {
        let tokens = Self::lex(input)?;
        let mut parser = Parser::new(input, tokens);
        let stmt = parser.parse_stmt()?;
        parser.expect_end()?;
        Ok(stmt)
    }

    pub fn try_parse(input: &'a str) -> Result<(), QqlError> {
        let tokens = Self::lex(input)?;
        let mut parser = Parser::new(input, tokens);
        parser.parse_stmt()?;
        parser.expect_end()?;
        Ok(())
    }

    pub fn parse_all(input: &'a str) -> Result<Vec<Stmt>, QqlError> {
        let tokens = Self::lex(input)?;
        let mut parser = Parser::new(input, tokens);
        let mut stmts = Vec::new();
        while parser.index < parser.tokens.len() {
            if parser.tokens[parser.index].kind == TokenKind::Semicolon {
                parser.index += 1;
                continue;
            }
            stmts.push(parser.parse_stmt()?);
        }
        Ok(stmts)
    }

    fn lex(input: &'a str) -> Result<Vec<Token<'a>>, QqlError> {
        let lexer = Lexer::new(input);
        let mut tokens = Vec::new();
        for token_res in lexer {
            tokens.push(token_res?);
        }
        Ok(tokens)
    }

    fn expect_end(&mut self) -> Result<(), QqlError> {
        while self.index < self.tokens.len() && self.tokens[self.index].kind == TokenKind::Semicolon
        {
            self.index += 1;
        }

        if self.index < self.tokens.len() {
            let tok = self.tokens[self.index];
            return Err(QqlError::syntax(
                alloc::format!("unexpected trailing token '{}'", tok.text),
                tok.pos,
            ));
        }

        Ok(())
    }

    pub fn parse_stmt(&mut self) -> Result<Stmt, QqlError> {
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

    pub fn peek_nth(&self, offset: usize) -> Token<'a> {
        let idx = self.index + offset;
        if idx < self.tokens.len() {
            self.tokens[idx]
        } else {
            Token::eof()
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

    pub fn parse_identifier(&mut self) -> Result<String, QqlError> {
        let tok = self.peek()?;
        if tok.kind == TokenKind::Identifier
            || tok.kind == TokenKind::String
            || is_contextual_identifier(tok.kind)
        {
            self.advance()?;
            Ok(tok.text.to_string())
        } else {
            Err(QqlError::syntax(
                alloc::format!("expected identifier or quoted name, got '{}'", tok.text),
                tok.pos,
            ))
        }
    }

    // ── Value parsing ───────────────────────────────────────────

    pub fn parse_value(&mut self) -> Result<crate::ast::Value, QqlError> {
        let tok = self.peek()?;
        match tok.kind {
            TokenKind::String => {
                self.advance()?;
                Ok(crate::ast::Value::Str(tok.text.to_string()))
            }
            TokenKind::Float => {
                self.advance()?;
                let v: f64 = tok.text.parse().map_err(|_| {
                    QqlError::syntax(
                        alloc::format!("invalid float literal '{}'", tok.text),
                        tok.pos,
                    )
                })?;
                Ok(crate::ast::Value::Float(v))
            }
            TokenKind::Integer => {
                self.advance()?;
                let v: i64 = tok.text.parse().map_err(|_| {
                    QqlError::syntax(
                        alloc::format!("invalid integer literal '{}'", tok.text),
                        tok.pos,
                    )
                })?;
                Ok(crate::ast::Value::Int(v))
            }
            TokenKind::Null => {
                self.advance()?;
                Ok(crate::ast::Value::Null)
            }
            TokenKind::Identifier => {
                self.advance()?;
                if ascii_equal(tok.text, "TRUE") {
                    Ok(crate::ast::Value::Bool(true))
                } else if ascii_equal(tok.text, "FALSE") {
                    Ok(crate::ast::Value::Bool(false))
                } else if ascii_equal(tok.text, "NULL") {
                    Ok(crate::ast::Value::Null)
                } else {
                    Ok(crate::ast::Value::Str(tok.text.to_string()))
                }
            }
            TokenKind::Lbrace => self
                .parse_payload_dict()
                .map(|items| crate::ast::Value::Dict(items.into_iter().collect())),
            TokenKind::Lbracket => self.parse_list().map(crate::ast::Value::List),
            _ => Err(QqlError::syntax(
                alloc::format!("unexpected value token '{}'", tok.text),
                tok.pos,
            )),
        }
    }
}
