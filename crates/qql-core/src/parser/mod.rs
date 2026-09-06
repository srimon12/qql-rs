pub(crate) mod alter_drop_show;
pub(crate) mod config_parsers;
pub(crate) mod config_validation;
pub(crate) mod create;
pub(crate) mod filter;
pub(crate) mod formula;
pub(crate) mod helpers;
pub(crate) mod point_ops;
pub(crate) mod query;
pub(crate) mod r#update;
pub(crate) mod upsert;
pub(crate) mod with_clause;

use crate::ast::Stmt;
use crate::error::{QqlError, Span};
use crate::lexer::Lexer;
use crate::token::{Token, TokenKind};
use alloc::string::String;
use alloc::vec::Vec;
pub use config_validation::{
    check_deleted_threshold, config_bool, config_float_range, config_has_key,
    config_max_optimization_threads, config_non_negative_u64, config_positive_u64, config_value,
    merge_collection_config, validate_hnsw_value, validate_index_options,
    validate_optimizers_value, validate_params_value, validate_vectors_value,
};

/// Canonical QQL parser facade.
///
/// Production parsing is **only** the hand-written AST lowerer
/// (lexer → tokens → typed AST). There is no parallel PEG/pest frontend in
/// this crate: `language/v1/grammar.pest` is the language contract for docs
/// and CI (`qql-grammar-gen`), not a runtime dependency of `qql-core`.
pub struct Parser;

pub(crate) struct AstLowerer<'a> {
    pub input: &'a str,
    tokens: Vec<Token<'a>>,
    index: usize,
    positional_param_count: usize,
}

/// Hard upper bound for one parsed script. Callers that need larger imports
/// should split them into bounded batches before parsing.
pub const MAX_STATEMENTS: usize = 256;

/// Returns true when `s` equals `upper`, ignoring ASCII case.
pub fn ascii_equal(s: &str, upper: &str) -> bool {
    s.eq_ignore_ascii_case(upper)
}

/// Returns true when `s` equals `lower`, ignoring ASCII case.
pub fn ascii_equal_lower(s: &str, lower: &str) -> bool {
    s.eq_ignore_ascii_case(lower)
}

/// Returns true when a token kind can serve as a contextual field name.
pub fn is_contextual_field_name(kind: TokenKind) -> bool {
    kind.is_keyword_or_identifier()
}

impl Parser {
    /// Parses a single QQL statement from the input string.
    pub fn parse(input: &str) -> Result<Stmt, QqlError> {
        AstLowerer::lower_statement(input)
    }

    /// Parses a `;`-separated script into a list of statements.
    pub fn parse_all(input: &str) -> Result<Vec<Stmt>, QqlError> {
        AstLowerer::lower_script(input)
    }

    /// Parses a script, returning each statement paired with its source span.
    pub fn parse_all_with_spans(input: &str) -> Result<Vec<(Stmt, Span)>, QqlError> {
        AstLowerer::lower_script_with_spans(input)
    }

    /// Parse a standalone literal value (string, number, boolean, null, list, or dict).
    ///
    /// Errors if parsing fails or if unexpected trailing tokens exist after the value.
    pub fn parse_value(input: &str) -> Result<crate::ast::Value, QqlError> {
        let tokens = AstLowerer::lex(input)?;
        let mut parser = AstLowerer::new(input, tokens);
        let val = parser.parse_value()?;
        parser.expect_end()?;
        Ok(val)
    }
}

impl<'a> AstLowerer<'a> {
    fn new(input: &'a str, tokens: Vec<Token<'a>>) -> Self {
        Self {
            input,
            tokens,
            index: 0,
            positional_param_count: 0,
        }
    }

    fn lower_statement(input: &'a str) -> Result<Stmt, QqlError> {
        let tokens = Self::lex(input)?;
        let mut parser = AstLowerer::new(input, tokens);
        let stmt = parser.parse_stmt()?;
        if parser.peek()?.kind == TokenKind::Semicolon {
            parser.advance()?;
        }
        parser.expect_end()?;
        Ok(stmt)
    }

    fn lower_script(input: &'a str) -> Result<Vec<Stmt>, QqlError> {
        let with_spans = Self::lower_script_with_spans(input)?;
        Ok(with_spans.into_iter().map(|(s, _)| s).collect())
    }

    pub(crate) fn lower_script_with_spans(input: &'a str) -> Result<Vec<(Stmt, Span)>, QqlError> {
        let tokens = Self::lex(input)?;
        let mut parser = AstLowerer::new(input, tokens);
        let mut statements = Vec::new();
        if parser.peek()?.kind == TokenKind::Semicolon {
            return Err(QqlError::parse(
                "QQL-PARSE-EMPTY-STATEMENT",
                "leading or empty statements are not allowed",
                parser.peek()?.span,
            ));
        }

        while parser.peek()?.kind != TokenKind::Eof {
            if statements.len() >= MAX_STATEMENTS {
                return Err(QqlError::parse(
                    "QQL-PARSE-STATEMENT-LIMIT",
                    alloc::format!("a script may contain at most {MAX_STATEMENTS} statements"),
                    parser.peek()?.span,
                ));
            }
            let start_tok = parser.peek()?;
            let start_pos = start_tok.span.start;
            let stmt = parser.parse_stmt()?;
            let end_pos = match parser.peek()?.kind {
                TokenKind::Semicolon => {
                    let semi_span = parser.peek()?.span;
                    parser.advance()?;
                    if parser.peek()?.kind == TokenKind::Semicolon {
                        return Err(QqlError::parse(
                            "QQL-PARSE-EMPTY-STATEMENT",
                            "repeated semicolons are not allowed",
                            parser.peek()?.span,
                        ));
                    }
                    semi_span.end
                }
                TokenKind::Eof => {
                    let prev_idx = parser.index.saturating_sub(1);
                    parser
                        .tokens
                        .get(prev_idx)
                        .map(|t| t.span.end)
                        .unwrap_or(start_tok.span.end)
                }
                _ => {
                    return Err(QqlError::parse(
                        "QQL-PARSE-SEPARATOR",
                        "multiple statements must be separated by a semicolon",
                        parser.peek()?.span,
                    ));
                }
            };
            statements.push((stmt, Span::new(start_pos, end_pos)));
        }
        Ok(statements)
    }

    fn lex(input: &'a str) -> Result<Vec<Token<'a>>, QqlError> {
        let lexer = Lexer::new(input);
        let mut tokens = Vec::with_capacity(input.len() / 6 + 1);
        for token_res in lexer {
            tokens.push(token_res?);
        }
        Ok(tokens)
    }

    fn expect_end(&mut self) -> Result<(), QqlError> {
        if self.index < self.tokens.len() {
            let tok = self.tokens[self.index];
            return Err(QqlError::parse(
                "QQL-PARSE-TRAILING",
                alloc::format!("unexpected trailing token '{}'", tok.text),
                tok.span,
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
            TokenKind::Upsert => self.parse_upsert(),
            TokenKind::Scroll => self.parse_scroll(),
            TokenKind::Query => self.parse_query(),
            TokenKind::With => self.parse_query_with_cte(),
            TokenKind::Delete => self.parse_delete(),
            TokenKind::Clear => self.parse_clear(),
            TokenKind::Update => self.parse_update(),
            TokenKind::Count => self.parse_count(),
            TokenKind::Facet => self.parse_facet(),
            TokenKind::Set => self.parse_set_quota(),
            _ => Err(QqlError::parse(
                "QQL-PARSE-STATEMENT",
                alloc::format!("expected a QQL statement keyword, got '{}'", tok.text),
                tok.span,
            )),
        }
    }

    // ── Token stream helpers ────────────────────────────────────

    pub fn peek(&mut self) -> Result<Token<'a>, QqlError> {
        if self.index < self.tokens.len() {
            Ok(self.tokens[self.index])
        } else {
            Ok(Token::eof(self.input.len()))
        }
    }

    pub fn peek_nth(&self, offset: usize) -> Token<'a> {
        let idx = self.index + offset;
        if idx < self.tokens.len() {
            self.tokens[idx]
        } else {
            Token::eof(self.input.len())
        }
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
            return Err(QqlError::parse(
                "QQL-PARSE-EXPECTED",
                alloc::format!("expected {} but got '{}'", kind, tok.text),
                tok.span,
            ));
        }
        self.advance()
    }

    // ── Identifier parsing ──────────────────────────────────────

    pub fn parse_identifier_str(&mut self) -> Result<&'a str, QqlError> {
        let tok = self.peek()?;
        if tok.is_keyword_or_identifier() || tok.kind == TokenKind::String {
            self.advance()?;
            Ok(tok.text)
        } else {
            Err(QqlError::parse(
                "QQL-PARSE-IDENTIFIER",
                alloc::format!("expected identifier or quoted name, got '{}'", tok.text),
                tok.span,
            ))
        }
    }

    pub fn parse_identifier(&mut self) -> Result<String, QqlError> {
        self.parse_identifier_str().map(String::from)
    }

    // ── Value parsing ───────────────────────────────────────────

    pub fn parse_value(&mut self) -> Result<crate::ast::Value, QqlError> {
        let tok = self.peek()?;
        match tok.kind {
            TokenKind::String => {
                self.advance()?;
                self.decode_string(tok).map(crate::ast::Value::Str)
            }
            TokenKind::Float => {
                self.advance()?;
                let v: f64 = tok.text.parse().map_err(|_| {
                    QqlError::parse(
                        "QQL-PARSE-FLOAT",
                        alloc::format!("invalid float literal '{}'", tok.text),
                        tok.span,
                    )
                })?;
                // grammar.pest `float` can only denote finite values; an
                // exponent overflow like `1e999` must not become inf/NaN.
                if !v.is_finite() {
                    return Err(QqlError::parse(
                        "QQL-PARSE-FLOAT",
                        alloc::format!("float literal '{}' is not finite", tok.text),
                        tok.span,
                    ));
                }
                Ok(crate::ast::Value::Float(v))
            }
            TokenKind::Integer => {
                self.advance()?;
                let v: i64 = tok.text.parse().map_err(|_| {
                    QqlError::parse(
                        "QQL-PARSE-INTEGER",
                        alloc::format!("invalid integer literal '{}'", tok.text),
                        tok.span,
                    )
                })?;
                Ok(crate::ast::Value::Int(v))
            }
            TokenKind::Null => {
                self.advance()?;
                Ok(crate::ast::Value::Null)
            }
            TokenKind::True => {
                self.advance()?;
                Ok(crate::ast::Value::Bool(true))
            }
            TokenKind::False => {
                self.advance()?;
                Ok(crate::ast::Value::Bool(false))
            }
            kind if kind.is_keyword_or_identifier() => {
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
            TokenKind::Colon => {
                self.advance()?;
                let name = self.parse_param_name()?;
                Ok(crate::ast::Value::Param(name))
            }
            TokenKind::Question => {
                self.advance()?;
                let idx = self.next_positional_param();
                Ok(crate::ast::Value::PositionalParam(idx))
            }
            TokenKind::Lbrace => self.parse_payload_dict().map(crate::ast::Value::Dict),
            TokenKind::Lbracket => self.parse_list().map(crate::ast::Value::List),
            _ => Err(QqlError::parse(
                "QQL-PARSE-VALUE",
                alloc::format!("unexpected value token '{}'", tok.text),
                tok.span,
            )),
        }
    }

    pub(crate) fn next_positional_param(&mut self) -> usize {
        let idx = self.positional_param_count;
        self.positional_param_count += 1;
        idx
    }

    pub(crate) fn parse_param_name(&mut self) -> Result<String, QqlError> {
        let tok = self.peek()?;
        if tok.is_keyword_or_identifier() {
            self.advance()?;
            Ok(tok.text.to_string())
        } else {
            Err(QqlError::parse(
                "QQL-PARSE-PARAM",
                alloc::format!(
                    "expected parameter identifier after ':', found '{}'",
                    tok.text
                ),
                tok.span,
            ))
        }
    }

    fn decode_string(&self, token: Token<'a>) -> Result<String, QqlError> {
        let input = self.input.as_bytes();
        let start = token.span.start;
        let end = token.span.end;
        let first_byte = input.get(start).copied().unwrap_or(0);
        let is_raw_or_backtick = first_byte == b'r' || first_byte == b'`';
        // Triple-quoted strings preserve their contents verbatim: no escape
        // decoding and no SQL-style `''` folding. Detect them from the full
        // source span — a token is triple-quoted only when it starts and ends
        // with the same `'''` / `"""` delimiter and spans at least both
        // delimiters (the SQL-escaped `''''` four-quote form is only 4 bytes).
        let triple_quoted = end >= start + 6
            && (input[start..start + 3] == b"'''"[..] || input[start..start + 3] == b"\"\"\""[..])
            && input[start..start + 3] == input[end - 3..end];
        if is_raw_or_backtick
            || triple_quoted
            || !(token.text.contains('\\') || first_byte == b'\'' && token.text.contains("''"))
        {
            return Ok(token.text.to_string());
        }
        let single_quoted = first_byte == b'\'';
        let mut decoded = String::with_capacity(token.text.len());
        let mut chars = token.text.chars().peekable();
        while let Some(ch) = chars.next() {
            if single_quoted && ch == '\'' && chars.peek() == Some(&'\'') {
                chars.next();
                decoded.push('\'');
                continue;
            }
            if ch != '\\' {
                decoded.push(ch);
                continue;
            }
            let escaped = chars.next().ok_or_else(|| {
                QqlError::parse(
                    "QQL-PARSE-ESCAPE",
                    "unterminated escape sequence",
                    token.span,
                )
            })?;
            decoded.push(match escaped {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                '\\' => '\\',
                '\'' => '\'',
                '"' => '"',
                '$' => '$',
                _ => {
                    return Err(QqlError::parse(
                        "QQL-PARSE-ESCAPE",
                        alloc::format!("unsupported escape sequence \\{}", escaped),
                        token.span,
                    ));
                }
            });
        }
        Ok(decoded)
    }
}
