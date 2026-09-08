use super::helpers::{point_id_from_value, point_vectors_from_value};
use super::{AstLowerer, ascii_equal};
use crate::ast::{EmbedDirective, EmbedKind, PointEntry, Stmt, UpsertPoint, UpsertStmt};
use crate::error::QqlError;
use crate::token::TokenKind;
use alloc::boxed::Box;
use alloc::vec::Vec;

impl<'a> AstLowerer<'a> {
    pub fn parse_upsert(&mut self) -> Result<Stmt, QqlError> {
        self.expect(TokenKind::Upsert)?;
        self.expect(TokenKind::Into)?;
        let collection = self.parse_identifier()?;
        self.expect(TokenKind::Values)?;

        let mut points = Vec::new();
        loop {
            // Whole-point placeholders (`VALUES :p0, :p1` / `VALUES ?, ?`)
            // bind later to point dicts (or lists of point dicts).
            if self.peek()?.kind == TokenKind::Colon {
                let colon_tok = self.advance()?;
                let name = self.parse_param_name()?;
                let span = crate::error::Span::new(colon_tok.span.start, self.prev_span().end);
                points.push(PointEntry::Param(name, Some(Box::new(span))));
            } else if self.peek()?.kind == TokenKind::Question {
                let q_tok = self.advance()?;
                let idx = self.next_positional_param();
                points.push(PointEntry::PositionalParam(idx, Some(Box::new(q_tok.span))));
            } else {
                let row_start = self.peek()?.span.start;
                let mut row = self.parse_payload_dict()?;
                let row_span = crate::error::Span::new(row_start, self.prev_span().end);
                let id_index = row
                    .iter()
                    .position(|(key, _)| key.eq_ignore_ascii_case("id"))
                    .ok_or_else(|| {
                        QqlError::validation(
                            "QQL-VALIDATION-UPSERT-ID",
                            "each UPSERT row requires an id",
                            Some(row_span),
                        )
                    })?;
                let (_, id) = row.remove(id_index);
                let id = point_id_from_value(id, row_span)?;

                let vectors = if let Some(index) = row
                    .iter()
                    .position(|(key, _)| key.eq_ignore_ascii_case("vector"))
                {
                    let (_, value) = row.remove(index);
                    Some(point_vectors_from_value(value, Some(row_span))?)
                } else {
                    None
                };
                points.push(PointEntry::Inline(UpsertPoint {
                    id,
                    vectors,
                    payload: row,
                }));
            }
            if self.peek()?.kind != TokenKind::Comma {
                break;
            }
            self.advance()?;
        }

        let embedding = self.parse_embedding_options()?;
        let embed = if self.peek()?.kind == TokenKind::Embed
            || (self.peek()?.is_keyword_or_identifier() && ascii_equal(self.peek()?.text, "EMBED"))
        {
            self.parse_embed_clause()?
        } else {
            Vec::new()
        };
        let (shard_key, wait) = self.parse_optional_typed_shard_and_wait()?;

        Ok(Stmt::Upsert(Box::new(UpsertStmt {
            collection,
            points,
            embedding,
            embed,
            shard_key,
            wait,
        })))
    }

    fn parse_embed_clause(&mut self) -> Result<Vec<EmbedDirective>, QqlError> {
        self.advance()?;
        let mut directives = Vec::new();
        loop {
            let source_field = self.parse_identifier()?;
            self.expect(TokenKind::Into)?;
            let target_vector = self.parse_identifier()?;
            let kind = if self.peek()?.kind == TokenKind::Using {
                self.advance()?;
                if self.peek()?.kind == TokenKind::Sparse {
                    self.advance()?;
                    EmbedKind::Sparse {
                        model: self.parse_optional_model_string()?,
                    }
                } else if self.peek()?.kind == TokenKind::Dense {
                    self.advance()?;
                    EmbedKind::Dense {
                        model: self.parse_optional_model_string()?,
                    }
                } else if ascii_equal(self.peek()?.text, "MULTI")
                    || ascii_equal(self.peek()?.text, "MULTIVECTOR")
                {
                    self.advance()?;
                    EmbedKind::Multi {
                        model: self.parse_optional_model_string()?,
                    }
                } else if ascii_equal(self.peek()?.text, "IMAGE") {
                    self.advance()?;
                    EmbedKind::Image {
                        model: self.parse_optional_model_string()?,
                    }
                } else if self.peek()?.kind == TokenKind::Model {
                    EmbedKind::Dense {
                        model: Some(self.parse_required_model_string()?),
                    }
                } else {
                    return Err(QqlError::parse(
                        "QQL-PARSE-EMBED",
                        "EMBED USING requires DENSE, SPARSE, MULTI, IMAGE, or MODEL",
                        self.peek()?.span,
                    ));
                }
            } else {
                EmbedKind::Dense { model: None }
            };
            directives.push(EmbedDirective {
                source_field,
                target_vector,
                kind,
            });
            if self.peek()?.kind != TokenKind::Comma {
                break;
            }
            self.advance()?;
        }
        Ok(directives)
    }
}
