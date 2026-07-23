use super::helpers::point_id_from_value;
use super::{ascii_equal, Parser};
use crate::ast::{EmbedDirective, EmbedKind, PointVectors, Stmt, UpsertPoint, UpsertStmt, Value};
use crate::error::QqlError;
use crate::token::TokenKind;
use alloc::boxed::Box;
use alloc::vec::Vec;

impl<'a> Parser<'a> {
    pub fn parse_upsert(&mut self) -> Result<Stmt, QqlError> {
        let span = self.expect(TokenKind::Upsert)?.span;
        self.expect(TokenKind::Into)?;
        let collection = self.parse_identifier()?;
        self.expect(TokenKind::Values)?;

        let mut points = Vec::new();
        loop {
            let mut row = self.parse_payload_dict()?;
            let id_index = row
                .iter()
                .position(|(key, _)| key.eq_ignore_ascii_case("id"))
                .ok_or_else(|| {
                    QqlError::validation(
                        "QQL-VALIDATION-UPSERT-ID",
                        "each UPSERT row requires an id",
                        Some(span),
                    )
                })?;
            let (_, id) = row.remove(id_index);
            let id = point_id_from_value(id, span)?;

            let vectors = if let Some(index) = row
                .iter()
                .position(|(key, _)| key.eq_ignore_ascii_case("vector"))
            {
                let (_, value) = row.remove(index);
                Some(point_vectors_from_value(value, span)?)
            } else {
                None
            };
            points.push(UpsertPoint {
                id,
                vectors,
                payload: row,
            });
            if self.peek()?.kind != TokenKind::Comma {
                break;
            }
            self.advance()?;
        }

        let embedding = self.parse_embedding_options()?;
        let embed = if self.peek()?.kind == TokenKind::Identifier
            && ascii_equal(self.peek()?.text, "EMBED")
        {
            self.parse_embed_clause()?
        } else {
            Vec::new()
        };

        let shard_key = if self.peek()?.kind == TokenKind::Shard {
            self.advance()?;
            Some(self.parse_string()?)
        } else {
            None
        };

        Ok(Stmt::Upsert(Box::new(UpsertStmt {
            collection,
            points,
            embedding,
            embed,
            shard_key,
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
                } else {
                    EmbedKind::Dense {
                        model: Some(self.parse_required_model_string()?),
                    }
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

fn point_vectors_from_value(
    value: Value,
    span: crate::error::Span,
) -> Result<PointVectors, QqlError> {
    match value {
        Value::Dict(items)
            if !items.iter().any(|(key, _)| {
                key.eq_ignore_ascii_case("indices") || key.eq_ignore_ascii_case("values")
            }) =>
        {
            let mut vectors = Vec::new();
            for (name, value) in items {
                vectors.push((name, super::helpers::vector_from_value(value, span)?));
            }
            Ok(PointVectors::Named(vectors))
        }
        value => super::helpers::vector_from_value(value, span).map(PointVectors::Unnamed),
    }
}
