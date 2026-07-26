use core::iter::Peekable;

use crate::error::{QqlError, Span};
use crate::token::{lookup_keyword, Token, TokenKind};

pub type TokenIter<'a> = Peekable<Lexer<'a>>;

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Lexer<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Lexer { input, pos: 0 }
    }

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
            b'-' => self.read_minus_or_number(),
            b'"' | b'\'' => self.read_string(ch),
            _ => {
                if self.input[self.pos..].starts_with('≥') {
                    let pos = self.pos;
                    self.pos += '≥'.len_utf8();
                    return Ok(Token::new(
                        TokenKind::Gte,
                        &self.input[pos..self.pos],
                        Span::new(pos, self.pos),
                    ));
                }
                if self.input[self.pos..].starts_with('≤') {
                    let pos = self.pos;
                    self.pos += '≤'.len_utf8();
                    return Ok(Token::new(
                        TokenKind::Lte,
                        &self.input[pos..self.pos],
                        Span::new(pos, self.pos),
                    ));
                }
                if self.input[self.pos..].starts_with('≠') {
                    let pos = self.pos;
                    self.pos += '≠'.len_utf8();
                    return Ok(Token::new(
                        TokenKind::NotEquals,
                        &self.input[pos..self.pos],
                        Span::new(pos, self.pos),
                    ));
                }
                if is_digit(ch) {
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
        } else {
            self.single_char(TokenKind::Minus)
        }
    }

    fn read_string(&mut self, quote: u8) -> Result<Token<'a>, QqlError> {
        let start = self.pos;
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

        // Handle scientific notation exponent (e/E, e-5, e+5)
        if self.pos < self.input.len()
            && (self.input.as_bytes()[self.pos] == b'e' || self.input.as_bytes()[self.pos] == b'E')
        {
            let next_pos = self.pos + 1;
            if next_pos < self.input.len() {
                let next_ch = self.input.as_bytes()[next_pos];
                if is_digit(next_ch) || next_ch == b'+' || next_ch == b'-' {
                    is_float = true;
                    self.pos += 1; // consume 'e'/'E'
                    if self.input.as_bytes()[self.pos] == b'+'
                        || self.input.as_bytes()[self.pos] == b'-'
                    {
                        self.pos += 1; // consume '+' or '-'
                    }
                    while self.pos < self.input.len() && is_digit(self.input.as_bytes()[self.pos]) {
                        self.pos += 1;
                    }
                }
            }
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
                if is_alpha(first_byte) || first_byte == b'_' {
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
                if is_alpha(first_byte) || first_byte == b'_' {
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
        if self.pos >= self.input.len() {
            return None;
        }
        let result = self.next_token();
        match &result {
            Ok(t) if t.kind == TokenKind::Eof => None,
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
