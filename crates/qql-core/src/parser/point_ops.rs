use super::AstLowerer;
use crate::ast::{CountStmt, FacetStmt, ScrollStmt, Stmt};
use crate::error::QqlError;
use crate::token::TokenKind;
use alloc::boxed::Box;

impl<'a> AstLowerer<'a> {
    pub fn parse_scroll(&mut self) -> Result<Stmt, QqlError> {
        self.expect(TokenKind::Scroll)?;
        self.expect(TokenKind::From)?;
        let collection = self.parse_identifier()?;
        let filter = if self.peek()?.kind == TokenKind::Where {
            self.advance()?;
            Some(Box::new(self.parse_filter_expr()?))
        } else {
            None
        };
        let after = if self.peek()?.kind == TokenKind::After {
            self.advance()?;
            Some(self.parse_point_id("SCROLL AFTER")?)
        } else {
            None
        };
        let shard_key = if self.peek()?.kind == TokenKind::Shard {
            self.advance()?;
            Some(self.parse_string()?)
        } else {
            None
        };
        // Optional: WITH VECTOR [true|false|(names)] — bare form means all vectors.
        let with_vector =
            if self.peek()?.kind == TokenKind::With && self.peek_nth(1).kind == TokenKind::Vector {
                self.advance()?;
                self.advance()?;
                Some(self.parse_vector_selector()?)
            } else {
                None
            };
        self.expect(TokenKind::Limit)?;
        let limit = self.parse_positive_u64("SCROLL LIMIT")?;
        Ok(Stmt::Scroll(Box::new(ScrollStmt {
            collection,
            limit,
            filter,
            after,
            shard_key,
            with_vector,
        })))
    }

    pub fn parse_count(&mut self) -> Result<Stmt, QqlError> {
        self.expect(TokenKind::Count)?;
        self.expect(TokenKind::From)?;
        let collection = crate::ast::QueryCollection::Explicit(self.parse_identifier()?);
        // Grammar order (grammar.pest `count`): WHERE → SHARD → WITH, each at
        // most once. The runtime previously accepted any order/repeats.
        let filter = if self.peek()?.kind == TokenKind::Where {
            self.advance()?;
            Some(Box::new(self.parse_filter_expr()?))
        } else {
            None
        };
        let shard_key = if self.peek()?.kind == TokenKind::Shard {
            self.advance()?;
            Some(self.parse_string()?)
        } else {
            None
        };
        let exact = if self.peek()?.kind == TokenKind::With {
            self.advance()?;
            let opts = self.parse_config_block()?;
            let mut exact = None;
            for (key, value) in &opts {
                if !key.eq_ignore_ascii_case("exact") {
                    return Err(QqlError::parse(
                        "QQL-PARSE-COUNT-CONFIG",
                        alloc::format!("unknown COUNT parameter '{}'. Expected: exact", key),
                        self.peek()?.span,
                    ));
                }
                match value {
                    crate::ast::Value::Bool(b) => exact = Some(*b),
                    _ => {
                        return Err(QqlError::parse(
                            "QQL-PARSE-COUNT-CONFIG",
                            "COUNT 'exact' must be true or false",
                            self.peek()?.span,
                        ));
                    }
                }
            }
            exact
        } else {
            None
        };
        if matches!(
            self.peek()?.kind,
            TokenKind::Where | TokenKind::Shard | TokenKind::With
        ) {
            return Err(QqlError::parse(
                "QQL-PARSE-CLAUSE-ORDER",
                "duplicate or out-of-order COUNT clause (grammar order: WHERE, SHARD, WITH)",
                self.peek()?.span,
            ));
        }
        Ok(Stmt::Count(Box::new(CountStmt {
            collection,
            filter,
            shard_key,
            exact,
        })))
    }

    pub fn parse_facet(&mut self) -> Result<Stmt, QqlError> {
        self.expect(TokenKind::Facet)?;
        let (key, collection) = if self.peek()?.kind == TokenKind::From {
            self.advance()?;
            let coll = crate::ast::QueryCollection::Explicit(self.parse_identifier()?);
            if self.peek()?.kind == TokenKind::Key || self.peek_word("KEY")? {
                self.advance()?;
            }
            let k = self.parse_identifier()?;
            (k, coll)
        } else {
            let k = self.parse_identifier()?;
            self.expect(TokenKind::From)?;
            let coll = crate::ast::QueryCollection::Explicit(self.parse_identifier()?);
            (k, coll)
        };

        let mut filter = None;
        let mut limit = None;
        let mut exact = None;
        let mut shard_key = None;

        while self.peek()?.kind != TokenKind::Eof && self.peek()?.kind != TokenKind::Semicolon {
            match self.peek()?.kind {
                TokenKind::Where if filter.is_none() => {
                    self.advance()?;
                    filter = Some(Box::new(self.parse_filter_expr()?));
                }
                TokenKind::Limit if limit.is_none() => {
                    self.advance()?;
                    limit = Some(self.parse_positive_u64("FACET LIMIT")?);
                }
                TokenKind::Exact if exact.is_none() => {
                    self.advance()?;
                    match self.peek()?.kind {
                        TokenKind::True => {
                            self.advance()?;
                            exact = Some(true);
                        }
                        TokenKind::False => {
                            self.advance()?;
                            exact = Some(false);
                        }
                        _ => exact = Some(true),
                    }
                }
                TokenKind::Shard if shard_key.is_none() => {
                    self.advance()?;
                    shard_key = Some(self.parse_string()?);
                }
                TokenKind::With => {
                    self.advance()?;
                    let opts = self.parse_config_block()?;
                    for (k, v) in &opts {
                        if k.eq_ignore_ascii_case("exact") {
                            if let crate::ast::Value::Bool(b) = v {
                                exact = Some(*b);
                            }
                        } else if k.eq_ignore_ascii_case("limit") {
                            if let crate::ast::Value::Int(i) = v {
                                if *i > 0 {
                                    limit = Some(*i as u64);
                                }
                            }
                        }
                    }
                }
                _ if (self.peek_word("EXACT")? && exact.is_none()) => {
                    self.advance()?;
                    match self.peek()?.kind {
                        TokenKind::True => {
                            self.advance()?;
                            exact = Some(true);
                        }
                        TokenKind::False => {
                            self.advance()?;
                            exact = Some(false);
                        }
                        _ => exact = Some(true),
                    }
                }
                _ => break,
            }
        }

        Ok(Stmt::Facet(Box::new(FacetStmt {
            key,
            collection,
            filter,
            limit,
            exact,
            shard_key,
        })))
    }
}
