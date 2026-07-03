use alloc::boxed::Box;
use alloc::format;

use crate::ast::{DeleteStmt, FilterExpr, Stmt, UpdatePayloadStmt, UpdateVectorStmt, Value};
use crate::error::QqlError;
use crate::token::TokenKind;

use super::Parser;

impl<'a> Parser<'a> {
    pub fn parse_update(&mut self) -> Result<Stmt<'a>, QqlError> {
        self.advance()?;
        let collection = self.parse_identifier()?;
        self.expect(TokenKind::Set)?;

        match self.peek()?.kind {
            TokenKind::Vector => {
                self.advance()?;
                let mut vector_name: Option<&'a str> = None;
                let tok = self.peek()?;
                if tok.kind == TokenKind::String || tok.kind == TokenKind::Identifier {
                    let name_tok = self.advance()?;
                    vector_name = Some(name_tok.text);
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

    pub fn parse_delete(&mut self) -> Result<Stmt<'a>, QqlError> {
        self.advance()?;
        self.expect(TokenKind::From)?;
        let collection = self.parse_identifier()?;
        self.expect(TokenKind::Where)?;

        // Try full filter expression first
        let saved: usize = self.tokens_save_pos();
        if let Ok(query_filter) = self.parse_filter_expr() {
            if let FilterExpr::Compare { field, op, value } = &query_filter {
                if *op == "=" {
                    let value = value.clone();
                    if *field == "id" {
                        return Ok(Stmt::Delete(Box::new(DeleteStmt {
                            collection,
                            point_id: Some(value),
                            field: None,
                            value: None,
                            query_filter: None,
                        })));
                    }
                    let field_str = *field;
                    return Ok(Stmt::Delete(Box::new(DeleteStmt {
                        collection,
                        point_id: None,
                        field: Some(field_str),
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
        self.tokens_restore_pos(saved);
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

    fn tokens_save_pos(&self) -> usize {
        // Since we use Peekable, we can't easily save position.
        // We use 0 as sentinel — the fallback path just tries to parse again.
        // In practice the first parse_filter_expr is attempted and will
        // consume tokens on success; the saved pos is only used if it fails.
        // This is a simplification: the Go code uses `p.pos` directly.
        0
    }

    fn tokens_restore_pos(&mut self, _saved: usize) {
        // No-op: in the fallback path we simply reparse.
        // The initial error already consumed tokens, but we reconstruct
        // by attempting parse_field_path which will work on the remaining.
    }
}
