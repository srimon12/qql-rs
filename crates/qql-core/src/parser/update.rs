use super::Parser;
use crate::ast::{
    DeleteStmt, FilterExpr, PointIdPredicate, PointSelector, Stmt, UpdatePayloadStmt,
    UpdateVectorStmt,
};
use crate::error::QqlError;
use crate::token::TokenKind;
use alloc::boxed::Box;

impl<'a> Parser<'a> {
    pub fn parse_update(&mut self) -> Result<Stmt, QqlError> {
        self.expect(TokenKind::Update)?;
        let collection = self.parse_identifier()?;
        self.expect(TokenKind::Set)?;
        match self.peek()?.kind {
            TokenKind::Vector => {
                self.advance()?;
                let vector_name = if self.peek()?.kind != TokenKind::Equals {
                    Some(self.parse_identifier()?)
                } else {
                    None
                };
                self.expect(TokenKind::Equals)?;
                let vector = self.parse_vector_value()?;
                self.expect(TokenKind::Where)?;
                self.expect(TokenKind::Id)?;
                self.expect(TokenKind::Equals)?;
                let point_id = self.parse_point_id("UPDATE VECTOR")?;
                Ok(Stmt::UpdateVector(Box::new(UpdateVectorStmt {
                    collection,
                    point_id,
                    vector,
                    vector_name,
                })))
            }
            TokenKind::Payload => {
                self.advance()?;
                self.expect(TokenKind::Equals)?;
                let payload = self.parse_payload_dict()?;
                self.expect(TokenKind::Where)?;
                let selector = selector_from_filter(self.parse_filter_expr()?);
                Ok(Stmt::UpdatePayload(Box::new(UpdatePayloadStmt {
                    collection,
                    selector,
                    payload,
                })))
            }
            _ => Err(QqlError::parse(
                "QQL-PARSE-UPDATE",
                "expected VECTOR or PAYLOAD after SET",
                self.peek()?.span,
            )),
        }
    }

    pub fn parse_delete(&mut self) -> Result<Stmt, QqlError> {
        self.expect(TokenKind::Delete)?;
        self.expect(TokenKind::From)?;
        let collection = self.parse_identifier()?;
        self.expect(TokenKind::Where)?;
        let selector = selector_from_filter(self.parse_filter_expr()?);
        let shard_key = if self.peek()?.kind == TokenKind::Shard {
            self.advance()?;
            Some(self.parse_string()?)
        } else {
            None
        };
        Ok(Stmt::Delete(Box::new(DeleteStmt {
            collection,
            selector,
            shard_key,
        })))
    }
}

fn selector_from_filter(filter: FilterExpr) -> PointSelector {
    match filter {
        FilterExpr::PointId(PointIdPredicate::Eq(id)) => PointSelector::Id(id),
        FilterExpr::PointId(PointIdPredicate::In(ids)) => PointSelector::Ids(ids),
        filter => PointSelector::Filter(Box::new(filter)),
    }
}
