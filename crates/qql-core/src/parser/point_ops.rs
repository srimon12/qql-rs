use super::Parser;
use crate::ast::{CountStmt, ScrollStmt, Stmt};
use crate::error::QqlError;
use crate::token::TokenKind;
use alloc::boxed::Box;

impl<'a> Parser<'a> {
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
        self.expect(TokenKind::Limit)?;
        let limit = self.parse_positive_u64("SCROLL LIMIT")?;
        Ok(Stmt::Scroll(Box::new(ScrollStmt {
            collection,
            limit,
            filter,
            after,
            shard_key,
        })))
    }

    pub fn parse_count(&mut self) -> Result<Stmt, QqlError> {
        self.expect(TokenKind::Count)?;
        self.expect(TokenKind::From)?;
        let collection = self.parse_identifier()?;
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
        Ok(Stmt::Count(Box::new(CountStmt {
            collection,
            filter,
            shard_key,
        })))
    }
}
