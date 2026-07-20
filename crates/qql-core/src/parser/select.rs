use alloc::boxed::Box;

use crate::ast::{ScrollStmt, SelectStmt, Stmt};
use crate::error::QqlError;
use crate::token::TokenKind;

use super::Parser;

impl<'a> Parser<'a> {
    pub fn parse_scroll(&mut self) -> Result<Stmt, QqlError> {
        self.advance()?;
        self.expect(TokenKind::From)?;
        let collection = self.parse_identifier()?;
        let mut query_filter = None;
        if self.peek()?.kind == TokenKind::Where {
            self.advance()?;
            let f = self.parse_filter_expr()?;
            query_filter = Some(Box::new(f));
        }
        let mut after = None;
        if self.peek()?.kind == TokenKind::After {
            self.advance()?;
            let a = self.parse_point_id_value("SCROLL AFTER")?;
            after = Some(a);
        }
        self.expect(TokenKind::Limit)?;
        let limit_tok = self.expect(TokenKind::Integer)?;
        let limit: i64 = limit_tok
            .text
            .parse()
            .map_err(|_| QqlError::syntax("invalid limit for SCROLL", limit_tok.pos))?;
        if limit <= 0 {
            return Err(QqlError::syntax(
                "limit for SCROLL must be a positive integer",
                limit_tok.pos,
            ));
        }
        Ok(Stmt::Scroll(Box::new(ScrollStmt {
            collection,
            limit,
            query_filter,
            after,
        })))
    }

    pub fn parse_select(&mut self) -> Result<Stmt, QqlError> {
        self.advance()?;
        self.expect(TokenKind::Star)?;
        self.expect(TokenKind::From)?;
        let collection = self.parse_identifier()?;
        self.expect(TokenKind::Where)?;
        self.expect(TokenKind::Id)?;
        self.expect(TokenKind::Equals)?;
        let point_id = self.parse_point_id_value("SELECT")?;
        Ok(Stmt::Select(Box::new(SelectStmt {
            collection,
            point_id,
        })))
    }
}
