use core::iter::Peekable;

use crate::error::{QqlError, Span};
use crate::token::{lookup_keyword, Token, TokenKind};

/// Peekable iterator over the token stream produced by a `Lexer`.
pub type TokenIter<'a> = Peekable<Lexer<'a>>;

/// QQL lexer yielding tokens with byte spans; halts after the first lexical error.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Lexer<'a> {
    input: &'a str,
    pos: usize,
    /// Set after a lex error has been yielded. `next_token` may fail without
    /// advancing `pos`, so the iterator must terminate instead of re-yielding
    /// the same error forever.
    halted: bool,
}

impl<'a> Lexer<'a> {
    /// Creates a lexer over the given source input.
    pub fn new(input: &'a str) -> Self {
        Lexer {
            input,
            pos: 0,
            halted: false,
        }
    }

    /// Lexes and returns the next token, `Eof` at end of input, or a lexical error.
    pub fn next_token(&mut self) -> Result<Token<'a>, QqlError> {
        self.skip_whitespace();

        if self.pos >= self.input.len() {
            return Ok(Token::new(TokenKind::Eof, "", Span::point(self.pos)));
        }

        let bytes = self.input.as_bytes();
        let ch = bytes[self.pos];

        match ch {
            b'{' => self.single_char(TokenKind::Lbrace),
            b'}' => self.single_char(TokenKind::Rbrace),
            b'[' => self.single_char(TokenKind::Lbracket),
            b']' => self.single_char(TokenKind::Rbracket),
            b'(' => self.single_char(TokenKind::Lparen),
            b')' => self.single_char(TokenKind::Rparen),
            b'*' => self.single_char(TokenKind::Star),
            b':' => self.single_char(TokenKind::Colon),
            b',' => self.single_char(TokenKind::Comma),
            b'=' => self.single_char(TokenKind::Equals),
            b'!' => self.read_not_equals(),
            b'>' => self.read_gt_or_gte(),
            b'<' => self.read_lt_or_lte(),
            b'+' => self.single_char(TokenKind::Plus),
            b'/' => self.single_char(TokenKind::Slash),
            b';' => self.single_char(TokenKind::Semicolon),
            b'?' => self.single_char(TokenKind::Question),
            b'-' => self.read_minus_or_number(),
            b'`' => self.read_backtick_string(),
            b'"' | b'\'' => self.read_string(ch),
            _ => {
                // Raw strings use a lowercase `r` prefix only; the grammar
                // (`raw_string`) has no uppercase form, so `R'…'` must lex as
                // the identifier `R` followed by a string.
                if ch == b'r'
                    && self.pos + 1 < self.input.len()
                    && (bytes[self.pos + 1] == b'\'' || bytes[self.pos + 1] == b'"')
                {
                    self.read_raw_string(bytes[self.pos + 1])
                } else if self.input[self.pos..].starts_with('≥') {
                    let pos = self.pos;
                    self.pos += '≥'.len_utf8();
                    Ok(Token::new(
                        TokenKind::Gte,
                        &self.input[pos..self.pos],
                        Span::new(pos, self.pos),
                    ))
                } else if self.input[self.pos..].starts_with('≤') {
                    let pos = self.pos;
                    self.pos += '≤'.len_utf8();
                    Ok(Token::new(
                        TokenKind::Lte,
                        &self.input[pos..self.pos],
                        Span::new(pos, self.pos),
                    ))
                } else if self.input[self.pos..].starts_with('≠') {
                    let pos = self.pos;
                    self.pos += '≠'.len_utf8();
                    Ok(Token::new(
                        TokenKind::NotEquals,
                        &self.input[pos..self.pos],
                        Span::new(pos, self.pos),
                    ))
                } else if is_digit(ch) {
                    self.read_number()
                } else if is_alpha(ch) || ch == b'_' || ch == b'$' {
                    self.read_identifier()
                } else {
                    let c = self.input[self.pos..].chars().next().unwrap_or('?');
                    let len = c.len_utf8();
                    let err_msg = alloc::format!("Unexpected character '{}'", c);
                    Err(QqlError::lex(
                        "QQL-LEX-CHAR",
                        err_msg,
                        Span::new(self.pos, self.pos + len),
                    ))
                }
            }
        }
    }

    fn single_char(&mut self, kind: TokenKind) -> Result<Token<'a>, QqlError> {
        let pos = self.pos;
        self.pos += 1;
        Ok(Token::new(
            kind,
            &self.input[pos..pos + 1],
            Span::new(pos, pos + 1),
        ))
    }

    fn read_not_equals(&mut self) -> Result<Token<'a>, QqlError> {
        let bytes = self.input.as_bytes();
        if self.pos + 1 < self.input.len() && bytes[self.pos + 1] == b'=' {
            let pos = self.pos;
            self.pos += 2;
            Ok(Token::new(
                TokenKind::NotEquals,
                &self.input[pos..pos + 2],
                Span::new(pos, pos + 2),
            ))
        } else {
            Err(QqlError::lex(
                "QQL-LEX-CHAR",
                "Unexpected character '!'",
                Span::new(self.pos, self.pos + 1),
            ))
        }
    }

    fn read_gt_or_gte(&mut self) -> Result<Token<'a>, QqlError> {
        let bytes = self.input.as_bytes();
        if self.pos + 1 < self.input.len() && bytes[self.pos + 1] == b'=' {
            let pos = self.pos;
            self.pos += 2;
            Ok(Token::new(
                TokenKind::Gte,
                &self.input[pos..pos + 2],
                Span::new(pos, pos + 2),
            ))
        } else {
            self.single_char(TokenKind::Gt)
        }
    }

    fn read_lt_or_lte(&mut self) -> Result<Token<'a>, QqlError> {
        let bytes = self.input.as_bytes();
        if self.pos + 1 < self.input.len() && bytes[self.pos + 1] == b'=' {
            let pos = self.pos;
            self.pos += 2;
            Ok(Token::new(
                TokenKind::Lte,
                &self.input[pos..pos + 2],
                Span::new(pos, pos + 2),
            ))
        } else {
            self.single_char(TokenKind::Lt)
        }
    }

    fn read_minus_or_number(&mut self) -> Result<Token<'a>, QqlError> {
        let bytes = self.input.as_bytes();
        if self.pos + 1 < self.input.len() && is_digit(bytes[self.pos + 1]) {
            self.read_number()
        } else if self.pos + 1 < self.input.len() && bytes[self.pos + 1] == b'.' {
            // `-.5`: grammar `float`/`integer` require digits after the sign,
            // so a `-` followed by `.` is never a number. Error at lex time
            // instead of emitting `-` and a confusing bare-dot error.
            Err(QqlError::lex(
                "QQL-LEX-NUMBER",
                "malformed numeric literal: '-' must be followed by a digit",
                Span::new(self.pos, self.pos + 2),
            ))
        } else {
            self.single_char(TokenKind::Minus)
        }
    }

    fn read_string(&mut self, quote: u8) -> Result<Token<'a>, QqlError> {
        let start = self.pos;
        // A run of three quotes only starts a triple-quoted string when a
        // matching closing delimiter exists later in the input. Otherwise the
        // run is single-quoted content: `''''` (four quotes) is the
        // SQL-escaped one-apostrophe string `'` + `''` + `'` per
        // `single_quoted_string` in grammar.pest, and must fall back below.
        let triple = if quote == b'\'' { "'''" } else { "\"\"\"" };
        if self.input[start..].starts_with(triple)
            && self.input[start + triple.len()..].find(triple).is_some()
        {
            return self.read_triple_quoted_string(quote);
        }

        self.pos += 1;
        let content_start = self.pos;

        while self.pos < self.input.len() {
            let bytes = self.input.as_bytes();
            if bytes[self.pos] == b'\\' {
                if self.pos + 1 >= self.input.len() {
                    return Err(QqlError::lex(
                        "QQL-LEX-STRING",
                        "unterminated string literal",
                        Span::new(start, self.input.len()),
                    ));
                }
                self.pos += 2;
                continue;
            }
            if bytes[self.pos] == quote {
                // SQL-style double single quotes ('') inside single-quoted strings
                if quote == b'\'' && self.pos + 1 < self.input.len() && bytes[self.pos + 1] == b'\''
                {
                    self.pos += 2;
                    continue;
                }
                let text = &self.input[content_start..self.pos];
                self.pos += 1;
                return Ok(Token::new(
                    TokenKind::String,
                    text,
                    Span::new(start, self.pos),
                ));
            }
            self.pos += 1;
        }

        Err(QqlError::lex(
            "QQL-LEX-STRING",
            "unterminated string literal",
            Span::new(start, self.input.len()),
        ))
    }

    fn read_triple_quoted_string(&mut self, quote: u8) -> Result<Token<'a>, QqlError> {
        let start = self.pos;
        self.pos += 3;
        let content_start = self.pos;
        let delimiter = if quote == b'\'' { "'''" } else { "\"\"\"" };

        if let Some(rel_pos) = self.input[self.pos..].find(delimiter) {
            let content_end = self.pos + rel_pos;
            let text = &self.input[content_start..content_end];
            self.pos = content_end + 3;
            return Ok(Token::new(
                TokenKind::String,
                text,
                Span::new(start, self.pos),
            ));
        }

        Err(QqlError::lex(
            "QQL-LEX-STRING",
            "unterminated triple-quoted string literal",
            Span::new(start, self.input.len()),
        ))
    }

    fn read_raw_string(&mut self, quote: u8) -> Result<Token<'a>, QqlError> {
        let start = self.pos;
        self.pos += 2;
        let content_start = self.pos;

        while self.pos < self.input.len() {
            if self.input.as_bytes()[self.pos] == quote {
                let text = &self.input[content_start..self.pos];
                self.pos += 1;
                return Ok(Token::new(
                    TokenKind::String,
                    text,
                    Span::new(start, self.pos),
                ));
            }
            self.pos += 1;
        }

        Err(QqlError::lex(
            "QQL-LEX-STRING",
            "unterminated raw string literal",
            Span::new(start, self.input.len()),
        ))
    }

    fn read_backtick_string(&mut self) -> Result<Token<'a>, QqlError> {
        let start = self.pos;
        self.pos += 1;
        let content_start = self.pos;

        while self.pos < self.input.len() {
            if self.input.as_bytes()[self.pos] == b'`' {
                let text = &self.input[content_start..self.pos];
                self.pos += 1;
                return Ok(Token::new(
                    TokenKind::String,
                    text,
                    Span::new(start, self.pos),
                ));
            }
            self.pos += 1;
        }

        Err(QqlError::lex(
            "QQL-LEX-STRING",
            "unterminated backtick string literal",
            Span::new(start, self.input.len()),
        ))
    }

    fn read_number(&mut self) -> Result<Token<'a>, QqlError> {
        let start = self.pos;
        if self.input.as_bytes()[self.pos] == b'-' {
            self.pos += 1;
        }

        while self.pos < self.input.len() && is_digit(self.input.as_bytes()[self.pos]) {
            self.pos += 1;
        }

        let mut is_float = false;
        if self.pos < self.input.len()
            && self.input.as_bytes()[self.pos] == b'.'
            && self.pos + 1 < self.input.len()
            && is_digit(self.input.as_bytes()[self.pos + 1])
        {
            is_float = true;
            self.pos += 1;
            while self.pos < self.input.len() && is_digit(self.input.as_bytes()[self.pos]) {
                self.pos += 1;
            }
        }

        // Handle scientific notation exponent (e/E, e-5, e+5). An exponent
        // must have at least one digit (after an optional sign): `1e`, `5e-`,
        // `1e+` are malformed (grammar `exponent` = (^"e" | ^"E") ~ ("+" |
        // "-")? ~ ASCII_DIGIT+), so error at lex time with a structured code
        // instead of emitting a token that fails downstream `f64` parsing.
        if self.pos < self.input.len()
            && (self.input.as_bytes()[self.pos] == b'e' || self.input.as_bytes()[self.pos] == b'E')
        {
            let mut cursor = self.pos + 1;
            if cursor < self.input.len()
                && (self.input.as_bytes()[cursor] == b'+' || self.input.as_bytes()[cursor] == b'-')
            {
                cursor += 1;
            }
            if cursor >= self.input.len() || !is_digit(self.input.as_bytes()[cursor]) {
                return Err(QqlError::lex(
                    "QQL-LEX-NUMBER",
                    "malformed numeric literal: exponent requires at least one digit",
                    Span::new(start, cursor),
                ));
            }
            is_float = true;
            self.pos = cursor;
            while self.pos < self.input.len() && is_digit(self.input.as_bytes()[self.pos]) {
                self.pos += 1;
            }
        }

        // A trailing `.` (e.g. `1.` or `1e5.3`) is never part of a valid
        // number (grammar `float` requires digits after the decimal point).
        if self.pos < self.input.len() && self.input.as_bytes()[self.pos] == b'.' {
            return Err(QqlError::lex(
                "QQL-LEX-NUMBER",
                "malformed numeric literal: unexpected '.'",
                Span::new(start, self.pos + 1),
            ));
        }

        if is_float {
            Ok(Token::new(
                TokenKind::Float,
                &self.input[start..self.pos],
                Span::new(start, self.pos),
            ))
        } else {
            Ok(Token::new(
                TokenKind::Integer,
                &self.input[start..self.pos],
                Span::new(start, self.pos),
            ))
        }
    }

    fn read_identifier(&mut self) -> Result<Token<'a>, QqlError> {
        let start = self.pos;
        let bytes = self.input.as_bytes();

        while self.pos < self.input.len() && (is_alnum(bytes[self.pos]) || bytes[self.pos] == b'_')
        {
            self.pos += 1;
        }

        loop {
            if self.pos >= self.input.len() {
                break;
            }
            if self.input[self.pos..].starts_with('.') {
                let rest = &self.input[self.pos + 1..];
                let first_byte = rest.as_bytes().first().copied().unwrap_or(0);
                // `identifier_segment` starts with a letter or `_` only
                // (grammar.pest); `$` cannot begin a dotted segment.
                if first_byte.is_ascii_alphabetic() || first_byte == b'_' {
                    self.pos += 1;
                    while self.pos < self.input.len()
                        && (is_alnum(self.input.as_bytes()[self.pos])
                            || self.input.as_bytes()[self.pos] == b'_')
                    {
                        self.pos += 1;
                    }
                } else {
                    break;
                }
            } else if self.input[self.pos..].starts_with("[].") {
                let rest = &self.input[self.pos + 3..];
                let first_byte = rest.as_bytes().first().copied().unwrap_or(0);
                if first_byte.is_ascii_alphabetic() || first_byte == b'_' {
                    self.pos += 3;
                    while self.pos < self.input.len()
                        && (is_alnum(self.input.as_bytes()[self.pos])
                            || self.input.as_bytes()[self.pos] == b'_')
                    {
                        self.pos += 1;
                    }
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        let word = &self.input[start..self.pos];

        if !word.contains('.') {
            if let Some(kind) = lookup_keyword(word) {
                return Ok(Token::new(kind, word, Span::new(start, self.pos)));
            }
        }

        Ok(Token::new(
            TokenKind::Identifier,
            word,
            Span::new(start, self.pos),
        ))
    }

    fn skip_whitespace(&mut self) {
        let bytes = self.input.as_bytes();
        loop {
            while self.pos < self.input.len() && is_whitespace(bytes[self.pos]) {
                self.pos += 1;
            }
            // Skip `--` line comments
            if self.pos + 1 < self.input.len()
                && bytes[self.pos] == b'-'
                && bytes[self.pos + 1] == b'-'
            {
                self.pos += 2;
                while self.pos < self.input.len() && bytes[self.pos] != b'\n' {
                    self.pos += 1;
                }
                continue;
            }
            break;
        }
    }
}

impl<'a> Iterator for Lexer<'a> {
    type Item = Result<Token<'a>, QqlError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.halted || self.pos >= self.input.len() {
            return None;
        }
        let result = self.next_token();
        match &result {
            Ok(t) if t.kind == TokenKind::Eof => None,
            // Surface the first lex error exactly once, then terminate: the
            // failing production may not advance `pos`, so re-polling would
            // loop forever (and `flatten()`-style consumers would hang).
            Err(_) => {
                self.halted = true;
                Some(result)
            }
            _ => Some(result),
        }
    }
}

fn is_whitespace(ch: u8) -> bool {
    ch == b' ' || ch == b'\t' || ch == b'\n' || ch == b'\r'
}

fn is_digit(ch: u8) -> bool {
    ch.is_ascii_digit()
}

fn is_alpha(ch: u8) -> bool {
    ch == b'$' || ch.is_ascii_alphabetic()
}

fn is_alnum(ch: u8) -> bool {
    is_alpha(ch) || is_digit(ch)
}
