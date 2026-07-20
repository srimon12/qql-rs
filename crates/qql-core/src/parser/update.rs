use alloc::boxed::Box;
use alloc::format;

use crate::ast::{DeleteStmt, FilterExpr, Stmt, UpdatePayloadStmt, UpdateVectorStmt, Value};
use crate::error::QqlError;
use crate::token::TokenKind;

use super::Parser;

impl<'a> Parser<'a> {
    pub fn parse_update(&mut self) -> Result<Stmt, QqlError> {
        self.advance()?;
        let collection = self.parse_identifier()?;
        self.expect(TokenKind::Set)?;

        match self.peek()?.kind {
            TokenKind::Vector => {
                self.advance()?;
                let mut vector_name: Option<String> = None;
                let tok = self.peek()?;
                if tok.kind == TokenKind::String || tok.kind == TokenKind::Identifier {
                    let name_val = self.parse_identifier()?;
                    vector_name = Some(name_val);
                }
                self.expect(TokenKind::Equals)?;
                let vector_value = self.parse_value()?;
                let raw_values = match vector_value {
                    Value::List(items) => items,
                    _ => {
                        return Err(QqlError::syntax(
                            "expected a vector list [...] after SET VECTOR =",
                            self.peek()?.pos,
                        ));
                    }
                };
                let vector = self.coerce_vector_values(raw_values)?;
                self.expect(TokenKind::Where)?;
                self.expect(TokenKind::Id)?;
                self.expect(TokenKind::Equals)?;
                let point_id = self.parse_point_id_value("UPDATE SET VECTOR")?;
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
                if self.peek()?.kind == TokenKind::Id {
                    self.advance()?;
                    self.expect(TokenKind::Equals)?;
                    let point_id = self.parse_point_id_value("UPDATE SET PAYLOAD")?;
                    Ok(Stmt::UpdatePayload(Box::new(UpdatePayloadStmt {
                        collection,
                        point_id: Some(point_id),
                        query_filter: None,
                        payload,
                    })))
                } else {
                    let query_filter = self.parse_filter_expr()?;
                    Ok(Stmt::UpdatePayload(Box::new(UpdatePayloadStmt {
                        collection,
                        point_id: None,
                        query_filter: Some(Box::new(query_filter)),
                        payload,
                    })))
                }
            }
            _ => {
                let tok = self.peek()?;
                Err(QqlError::syntax(
                    format!("expected VECTOR or PAYLOAD after SET, got '{}'", tok.text),
                    tok.pos,
                ))
            }
        }
    }

    pub fn parse_delete(&mut self) -> Result<Stmt, QqlError> {
        self.advance()?;
        self.expect(TokenKind::From)?;
        let collection = self.parse_identifier()?;
        self.expect(TokenKind::Where)?;

        // Try full filter expression first
        let saved = self.save_pos();
        if let Ok(query_filter) = self.parse_filter_expr() {
            if let FilterExpr::Compare { field, op, value } = &query_filter {
                if op == "=" {
                    let value = value.clone();
                    if field == "id" {
                        return Ok(Stmt::Delete(Box::new(DeleteStmt {
                            collection,
                            point_id: Some(value),
                            field: None,
                            value: None,
                            query_filter: None,
                        })));
                    }
                    return Ok(Stmt::Delete(Box::new(DeleteStmt {
                        collection,
                        point_id: None,
                        field: Some(field.clone()),
                        value: Some(value),
                        query_filter: None,
                    })));
                }
            }
            return Ok(Stmt::Delete(Box::new(DeleteStmt {
                collection,
                point_id: None,
                field: None,
                value: None,
                query_filter: Some(Box::new(query_filter)),
            })));
        }

        // Fall back to simple field = value
        self.restore_pos(saved);
        let field = self.parse_field_path()?;
        self.expect(TokenKind::Equals)?;
        let value = self.parse_value()?;
        Ok(Stmt::Delete(Box::new(DeleteStmt {
            collection,
            point_id: None,
            field: Some(field),
            value: Some(value),
            query_filter: None,
        })))
    }
}
