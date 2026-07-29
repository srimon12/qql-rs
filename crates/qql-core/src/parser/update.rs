use super::AstLowerer;
use crate::ast::{
    ClearPayloadStmt, DeleteStmt, DeleteVectorStmt, FilterExpr, PointIdPredicate, PointSelector,
    Stmt, UpdatePayloadStmt, UpdateVectorStmt,
};
use crate::error::QqlError;
use crate::token::TokenKind;
use alloc::boxed::Box;

impl<'a> AstLowerer<'a> {
    /// Optional trailing `SHARD '<key>'` on mutations that support custom sharding.
    fn parse_optional_shard_key(&mut self) -> Result<Option<String>, QqlError> {
        if self.peek()?.kind == TokenKind::Shard {
            self.advance()?;
            Ok(Some(self.parse_string()?))
        } else {
            Ok(None)
        }
    }

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
                let shard_key = self.parse_optional_shard_key()?;
                Ok(Stmt::UpdateVector(Box::new(UpdateVectorStmt {
                    collection,
                    point_id,
                    vector,
                    vector_name,
                    shard_key,
                })))
            }
            TokenKind::Payload => {
                self.advance()?;
                self.expect(TokenKind::Equals)?;
                let payload = self.parse_payload_dict()?;
                self.expect(TokenKind::Where)?;
                let selector = selector_from_filter(self.parse_filter_expr()?);
                let shard_key = self.parse_optional_shard_key()?;
                Ok(Stmt::UpdatePayload(Box::new(UpdatePayloadStmt {
                    collection,
                    selector,
                    payload,
                    shard_key,
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
        // Check if this is DELETE VECTOR or DELETE FROM
        if self.peek()?.kind == TokenKind::Vector {
            self.advance()?; // consume VECTOR
            let mut vector_names = Vec::new();
            vector_names.push(self.parse_identifier()?);
            while self.peek()?.kind == TokenKind::Comma {
                self.advance()?;
                vector_names.push(self.parse_identifier()?);
            }
            self.expect(TokenKind::From)?;
            let collection = self.parse_identifier()?;
            self.expect(TokenKind::Where)?;
            let selector = selector_from_filter(self.parse_filter_expr()?);
            let shard_key = self.parse_optional_shard_key()?;
            return Ok(Stmt::DeleteVector(Box::new(DeleteVectorStmt {
                collection,
                selector,
                vector_names,
                shard_key,
            })));
        }
        // DELETE FROM
        self.expect(TokenKind::From)?;
        let collection = self.parse_identifier()?;
        self.expect(TokenKind::Where)?;
        let selector = selector_from_filter(self.parse_filter_expr()?);
        let shard_key = self.parse_optional_shard_key()?;
        Ok(Stmt::Delete(Box::new(DeleteStmt {
            collection,
            selector,
            shard_key,
        })))
    }

    pub fn parse_clear(&mut self) -> Result<Stmt, QqlError> {
        self.expect(TokenKind::Clear)?;
        self.expect(TokenKind::Payload)?;
        self.expect(TokenKind::From)?;
        let collection = self.parse_identifier()?;
        self.expect(TokenKind::Where)?;
        let selector = selector_from_filter(self.parse_filter_expr()?);
        let shard_key = self.parse_optional_shard_key()?;
        Ok(Stmt::ClearPayload(Box::new(ClearPayloadStmt {
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
