use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::ast::FilterExpr;
use crate::error::QqlError;
use crate::token::TokenKind;

use super::{ascii_equal, token_kind_to_op, Parser};

impl<'a> Parser<'a> {
    pub fn parse_filter_expr(&mut self) -> Result<FilterExpr<'a>, QqlError> {
        let left = self.parse_filter_and()?;
        if self.peek()?.kind != TokenKind::Or {
            return Ok(left);
        }
        let mut operands = Vec::new();
        operands.push(left);
        while self.peek()?.kind == TokenKind::Or {
            self.advance()?;
            let right = self.parse_filter_and()?;
            operands.push(right);
        }
        Ok(FilterExpr::Or { operands })
    }

    pub fn parse_filter_and(&mut self) -> Result<FilterExpr<'a>, QqlError> {
        let left = self.parse_filter_not()?;
        if self.peek()?.kind != TokenKind::And {
            return Ok(left);
        }
        let mut operands = Vec::new();
        operands.push(left);
        while self.peek()?.kind == TokenKind::And {
            self.advance()?;
            let right = self.parse_filter_not()?;
            operands.push(right);
        }
        Ok(FilterExpr::And { operands })
    }

    pub fn parse_filter_not(&mut self) -> Result<FilterExpr<'a>, QqlError> {
        if self.peek()?.kind == TokenKind::Not {
            self.advance()?;
            let operand = self.parse_filter_not()?;
            return Ok(FilterExpr::Not {
                operand: Box::new(operand),
            });
        }
        self.parse_filter_primary()
    }

    pub fn parse_filter_primary(&mut self) -> Result<FilterExpr<'a>, QqlError> {
        if self.peek()?.kind == TokenKind::Lparen {
            self.advance()?;
            let expr = self.parse_filter_expr()?;
            self.expect(TokenKind::Rparen)?;
            return Ok(expr);
        }
        if self.peek()?.kind == TokenKind::Identifier && ascii_equal(self.peek()?.text, "NESTED") {
            return self.parse_nested_function();
        }
        self.parse_predicate()
    }

    pub fn parse_nested_function(&mut self) -> Result<FilterExpr<'a>, QqlError> {
        self.advance()?;
        self.expect(TokenKind::Lparen)?;
        let path_tok = self.expect(TokenKind::String)?;
        self.expect(TokenKind::Comma)?;
        let inner = self.parse_filter_expr()?;
        self.expect(TokenKind::Rparen)?;
        Ok(FilterExpr::Nested {
            path: path_tok.text,
            filter: Box::new(inner),
        })
    }

    pub fn parse_predicate(&mut self) -> Result<FilterExpr<'a>, QqlError> {
        let field = self.parse_field_path()?;
        let tok = self.peek()?;

        if tok.kind == TokenKind::Is {
            self.advance()?;
            if self.peek()?.kind == TokenKind::Not {
                self.advance()?;
                if self.peek()?.kind == TokenKind::Null {
                    self.advance()?;
                    return Ok(FilterExpr::IsNotNull { field });
                }
                if self.peek()?.kind == TokenKind::Empty {
                    self.advance()?;
                    return Ok(FilterExpr::IsNotEmpty { field });
                }
                return Err(QqlError::syntax(
                    "expected NULL or EMPTY after IS NOT",
                    self.peek()?.pos,
                ));
            }
            if self.peek()?.kind == TokenKind::Null {
                self.advance()?;
                return Ok(FilterExpr::IsNull { field });
            }
            if self.peek()?.kind == TokenKind::Empty {
                self.advance()?;
                return Ok(FilterExpr::IsEmpty { field });
            }
            return Err(QqlError::syntax(
                "expected NULL, NOT NULL, EMPTY, or NOT EMPTY after IS",
                self.peek()?.pos,
            ));
        }

        if tok.kind == TokenKind::In {
            self.advance()?;
            let values = self.parse_literal_list()?;
            return Ok(FilterExpr::In { field, values });
        }

        if tok.kind == TokenKind::Not {
            self.advance()?;
            self.expect(TokenKind::In)?;
            let values = self.parse_literal_list()?;
            return Ok(FilterExpr::NotIn { field, values });
        }

        if tok.kind == TokenKind::Between {
            self.advance()?;
            let low = self.parse_value()?;
            self.expect(TokenKind::And)?;
            let high = self.parse_value()?;
            return Ok(FilterExpr::Between { field, low, high });
        }

        if tok.kind == TokenKind::Match {
            self.advance()?;
            if self.peek()?.kind == TokenKind::Any {
                self.advance()?;
                let text_tok = self.expect(TokenKind::String)?;
                return Ok(FilterExpr::MatchAny {
                    field,
                    text: text_tok.text,
                });
            }
            if self.peek()?.kind == TokenKind::Phrase {
                self.advance()?;
                let text_tok = self.expect(TokenKind::String)?;
                return Ok(FilterExpr::MatchPhrase {
                    field,
                    text: text_tok.text,
                });
            }
            let text_tok = self.expect(TokenKind::String)?;
            return Ok(FilterExpr::MatchText {
                field,
                text: text_tok.text,
            });
        }

        let op = token_kind_to_op(tok.kind);
        if !op.is_empty() {
            self.advance()?;
            let value = self.parse_value()?;
            return Ok(FilterExpr::Compare { field, op, value });
        }

        Err(QqlError::syntax(
            alloc::format!(
                "expected a filter operator after field '{}', got '{}'",
                field,
                tok.text
            ),
            tok.pos,
        ))
    }
}
