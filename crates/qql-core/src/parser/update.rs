use super::AstLowerer;
use super::helpers::{point_id_from_value, point_vectors_from_value, vector_from_value};
use crate::ast::{
    ClearPayloadStmt, DeletePayloadStmt, DeleteStmt, DeleteVectorStmt, FilterExpr,
    PointIdPredicate, PointSelector, PointVectors, Stmt, UpdatePayloadStmt, UpdateVectorPoint,
    UpdateVectorStmt,
};
use crate::error::QqlError;
use crate::token::TokenKind;
use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;

impl<'a> AstLowerer<'a> {
    pub fn parse_update(&mut self) -> Result<Stmt, QqlError> {
        self.expect(TokenKind::Update)?;
        let collection = self.parse_identifier()?;
        self.expect(TokenKind::Set)?;
        match self.peek()?.kind {
            TokenKind::Vector => {
                self.advance()?;
                let points = if self.peek()?.kind == TokenKind::Values {
                    self.advance()?;
                    self.parse_update_vector_values()?
                } else {
                    vec![self.parse_update_vector_compact()?]
                };
                let (shard_key, wait) = self.parse_optional_typed_shard_and_wait()?;
                Ok(Stmt::UpdateVector(Box::new(UpdateVectorStmt {
                    collection,
                    points,
                    shard_key,
                    wait,
                })))
            }
            TokenKind::Payload => {
                self.advance()?;
                self.expect(TokenKind::Equals)?;
                let payload = self.parse_payload_dict()?;
                self.expect(TokenKind::Where)?;
                let selector = selector_from_filter(self.parse_filter_expr()?);
                let (shard_key, wait) = self.parse_optional_typed_shard_and_wait()?;
                Ok(Stmt::UpdatePayload(Box::new(UpdatePayloadStmt {
                    collection,
                    selector,
                    payload,
                    shard_key,
                    wait,
                })))
            }
            _ => Err(QqlError::parse(
                "QQL-PARSE-UPDATE",
                "expected VECTOR or PAYLOAD after SET",
                self.peek()?.span,
            )),
        }
    }

    /// `SET VECTOR [name] = <vectors> WHERE id = <id>`.
    fn parse_update_vector_compact(&mut self) -> Result<UpdateVectorPoint, QqlError> {
        let vector_name = if self.peek()?.kind != TokenKind::Equals {
            Some(self.parse_identifier()?)
        } else {
            None
        };
        self.expect(TokenKind::Equals)?;
        let span = self.peek()?.span;
        let value = self.parse_value()?;
        let vectors = if let Some(name) = vector_name {
            PointVectors::Named(vec![(name, vector_from_value(value, Some(span))?)])
        } else {
            point_vectors_from_value(value, Some(span))?
        };
        self.expect(TokenKind::Where)?;
        self.expect(TokenKind::Id)?;
        self.expect(TokenKind::Equals)?;
        let id = self.parse_point_id("UPDATE VECTOR")?;
        Ok(UpdateVectorPoint { id, vectors })
    }

    /// `SET VECTOR VALUES {id, vector}, …`.
    fn parse_update_vector_values(&mut self) -> Result<Vec<UpdateVectorPoint>, QqlError> {
        let mut points = Vec::new();
        loop {
            points.push(self.parse_update_vector_row()?);
            if self.peek()?.kind != TokenKind::Comma {
                break;
            }
            self.advance()?;
        }
        if points.is_empty() {
            return Err(QqlError::validation(
                "QQL-VALIDATION-UPDATE-VECTOR",
                "UPDATE VECTOR VALUES requires at least one point",
                Some(self.prev_span()),
            ));
        }
        Ok(points)
    }

    fn parse_update_vector_row(&mut self) -> Result<UpdateVectorPoint, QqlError> {
        let row_start = self.peek()?.span.start;
        let mut row = self.parse_payload_dict()?;
        let row_span = crate::error::Span::new(row_start, self.prev_span().end);
        let id_index = row
            .iter()
            .position(|(key, _)| key.eq_ignore_ascii_case("id"))
            .ok_or_else(|| {
                QqlError::validation(
                    "QQL-VALIDATION-UPDATE-VECTOR",
                    "each UPDATE VECTOR row requires an id",
                    Some(row_span),
                )
            })?;
        let (_, id) = row.remove(id_index);
        let id = point_id_from_value(id, row_span)?;
        let vector_index = row
            .iter()
            .position(|(key, _)| key.eq_ignore_ascii_case("vector"))
            .ok_or_else(|| {
                QqlError::validation(
                    "QQL-VALIDATION-UPDATE-VECTOR",
                    "each UPDATE VECTOR row requires a vector",
                    Some(row_span),
                )
            })?;
        let (_, value) = row.remove(vector_index);
        let vectors = point_vectors_from_value(value, Some(row_span))?;
        if let Some((key, _)) = row.first() {
            return Err(QqlError::validation(
                "QQL-VALIDATION-UPDATE-VECTOR",
                alloc::format!("UPDATE VECTOR VALUES rows accept only id and vector, not '{key}'"),
                Some(row_span),
            ));
        }
        Ok(UpdateVectorPoint { id, vectors })
    }

    pub fn parse_delete(&mut self) -> Result<Stmt, QqlError> {
        self.expect(TokenKind::Delete)?;
        if self.peek()?.kind == TokenKind::Payload {
            self.advance()?; // consume PAYLOAD
            let mut keys = Vec::new();
            keys.push(self.parse_identifier()?);
            while self.peek()?.kind == TokenKind::Comma {
                self.advance()?;
                keys.push(self.parse_identifier()?);
            }
            self.expect(TokenKind::From)?;
            let collection = self.parse_identifier()?;
            self.expect(TokenKind::Where)?;
            let selector = selector_from_filter(self.parse_filter_expr()?);
            let (shard_key, wait) = self.parse_optional_typed_shard_and_wait()?;
            return Ok(Stmt::DeletePayload(Box::new(DeletePayloadStmt {
                collection,
                keys,
                selector,
                shard_key,
                wait,
            })));
        }
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
            let (shard_key, wait) = self.parse_optional_typed_shard_and_wait()?;
            return Ok(Stmt::DeleteVector(Box::new(DeleteVectorStmt {
                collection,
                selector,
                vector_names,
                shard_key,
                wait,
            })));
        }
        // DELETE FROM
        self.expect(TokenKind::From)?;
        let collection = self.parse_identifier()?;
        self.expect(TokenKind::Where)?;
        let selector = selector_from_filter(self.parse_filter_expr()?);
        let (shard_key, wait) = self.parse_optional_typed_shard_and_wait()?;
        Ok(Stmt::Delete(Box::new(DeleteStmt {
            collection,
            selector,
            shard_key,
            wait,
        })))
    }

    pub fn parse_clear(&mut self) -> Result<Stmt, QqlError> {
        self.expect(TokenKind::Clear)?;
        self.expect(TokenKind::Payload)?;
        self.expect(TokenKind::From)?;
        let collection = self.parse_identifier()?;
        self.expect(TokenKind::Where)?;
        let selector = selector_from_filter(self.parse_filter_expr()?);
        let (shard_key, wait) = self.parse_optional_typed_shard_and_wait()?;
        Ok(Stmt::ClearPayload(Box::new(ClearPayloadStmt {
            collection,
            selector,
            shard_key,
            wait,
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
