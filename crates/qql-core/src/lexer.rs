use core::iter::Peekable;

use crate::error::QqlError;
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
            return Ok(Token::new(TokenKind::Eof, "", self.pos));
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
                if is_digit(ch) {
                    self.read_number()
                } else if is_alpha(ch) || ch == b'_' || ch == b'$' {
                    self.read_identifier()
                } else {
                    let err_msg = alloc::format!("Unexpected character '{}'", ch as char);
                    Err(QqlError::syntax(err_msg, self.pos))
                }
            }
        }
    }

    fn single_char(&mut self, kind: TokenKind) -> Result<Token<'a>, QqlError> {
        let pos = self.pos;
        self.pos += 1;
        Ok(Token::new(kind, &self.input[pos..pos + 1], pos))
    }

    fn read_not_equals(&mut self) -> Result<Token<'a>, QqlError> {
        let bytes = self.input.as_bytes();
        if self.pos + 1 < self.input.len() && bytes[self.pos + 1] == b'=' {
            let pos = self.pos;
            self.pos += 2;
            Ok(Token::new(
                TokenKind::NotEquals,
                &self.input[pos..pos + 2],
                pos,
            ))
        } else {
            Err(QqlError::syntax("Unexpected character '!'", self.pos))
        }
    }

    fn read_gt_or_gte(&mut self) -> Result<Token<'a>, QqlError> {
        let bytes = self.input.as_bytes();
        if self.pos + 1 < self.input.len() && bytes[self.pos + 1] == b'=' {
            let pos = self.pos;
            self.pos += 2;
            Ok(Token::new(TokenKind::Gte, &self.input[pos..pos + 2], pos))
        } else {
            self.single_char(TokenKind::Gt)
        }
    }

    fn read_lt_or_lte(&mut self) -> Result<Token<'a>, QqlError> {
        let bytes = self.input.as_bytes();
        if self.pos + 1 < self.input.len() && bytes[self.pos + 1] == b'=' {
            let pos = self.pos;
            self.pos += 2;
            Ok(Token::new(TokenKind::Lte, &self.input[pos..pos + 2], pos))
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
                    return Err(QqlError::syntax("Unterminated string literal", start));
                }
                self.pos += 2;
                continue;
            }
            if bytes[self.pos] == quote {
                let text = &self.input[content_start..self.pos];
                self.pos += 1;
                return Ok(Token::new(TokenKind::String, text, start));
            }
            self.pos += 1;
        }

        Err(QqlError::syntax("Unterminated string literal", start))
    }

    fn read_number(&mut self) -> Result<Token<'a>, QqlError> {
        let start = self.pos;
        if self.input.as_bytes()[self.pos] == b'-' {
            self.pos += 1;
        }

        while self.pos < self.input.len() && is_digit(self.input.as_bytes()[self.pos]) {
            self.pos += 1;
        }

        if self.pos < self.input.len()
            && self.input.as_bytes()[self.pos] == b'.'
            && self.pos + 1 < self.input.len()
            && is_digit(self.input.as_bytes()[self.pos + 1])
        {
            self.pos += 1;
            while self.pos < self.input.len() && is_digit(self.input.as_bytes()[self.pos]) {
                self.pos += 1;
            }
            Ok(Token::new(
                TokenKind::Float,
                &self.input[start..self.pos],
                start,
            ))
        } else {
            Ok(Token::new(
                TokenKind::Integer,
                &self.input[start..self.pos],
                start,
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
            let b = bytes[self.pos];
            if b == b'.'
                && self.pos + 1 < self.input.len()
                && (is_alpha(bytes[self.pos + 1]) || bytes[self.pos + 1] == b'_')
            {
                self.pos += 1;
                while self.pos < self.input.len()
                    && (is_alnum(bytes[self.pos]) || bytes[self.pos] == b'_')
                {
                    self.pos += 1;
                }
            } else if self.pos + 3 < self.input.len()
                && &self.input[self.pos..self.pos + 3] == "[]."
                && (is_alpha(bytes[self.pos + 3]) || bytes[self.pos + 3] == b'_')
            {
                self.pos += 3;
                while self.pos < self.input.len()
                    && (is_alnum(bytes[self.pos]) || bytes[self.pos] == b'_')
                {
                    self.pos += 1;
                }
            } else {
                break;
            }
        }

        let word = &self.input[start..self.pos];

        if !word.contains('.') {
            if let Some(kind) = lookup_keyword(word) {
                return Ok(Token::new(kind, word, start));
            }
        }

        Ok(Token::new(TokenKind::Identifier, word, start))
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
