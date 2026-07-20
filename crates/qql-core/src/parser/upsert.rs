use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::ast::{EmbedDirective, Stmt, UpsertStmt};
use crate::error::QqlError;
use crate::token::TokenKind;

use super::{ascii_equal, Parser};

impl<'a> Parser<'a> {
    pub fn parse_upsert(&mut self) -> Result<Stmt, QqlError> {
        self.advance()?;
        self.expect(TokenKind::Into)?;
        let collection = self.parse_identifier()?;
        self.expect(TokenKind::Values)?;

        let mut values_list: Vec<Vec<(String, crate::ast::Value)>> = Vec::new();
        loop {
            let dict = self.parse_payload_dict()?;
            values_list.push(dict);
            if self.peek()?.kind == TokenKind::Comma {
                self.advance()?;
                continue;
            }
            break;
        }
        if values_list.is_empty() {
            return Err(QqlError::syntax(
                "UPSERT VALUES requires at least one row",
                self.peek()?.pos,
            ));
        }

        let opts = self.parse_embedding_options()?;
        let model = opts.model;
        let hybrid = opts.hybrid;
        let sparse_model = opts.sparse_model;
        let dense_vector = opts.dense_vector;
        let sparse_vector = opts.sparse_vector;

        let mut embed_directives = Vec::new();
        if self.peek()?.kind == TokenKind::Identifier && ascii_equal(self.peek()?.text, "EMBED") {
            embed_directives = self.parse_embed_clause()?;
        }

        Ok(Stmt::Upsert(Box::new(UpsertStmt {
            collection,
            values_list,
            model,
            hybrid,
            sparse_model,
            dense_vector,
            sparse_vector,
            embed_directives,
        })))
    }

    pub fn parse_embed_clause(&mut self) -> Result<Vec<EmbedDirective>, QqlError> {
        self.advance()?;

        let mut directives = Vec::new();
        loop {
            let source_field = self.parse_identifier()?;
            self.expect(TokenKind::Into)?;
            let target_vector = self.parse_identifier()?;

            let mut dir = EmbedDirective {
                source_field,
                target_vector,
                model: None,
                sparse_model: None,
            };

            if self.peek()?.kind == TokenKind::Using {
                self.advance()?;
                if self.peek()?.kind == TokenKind::Sparse {
                    self.advance()?;
                    let sm = self.parse_optional_model_string()?;
                    dir.sparse_model = match sm {
                        Some(m) => Some(m),
                        None => Some(String::new()), // mark as sparse directive
                    };
                } else if self.peek()?.kind == TokenKind::Model {
                    self.advance()?;
                    let m = self.parse_string()?;
                    dir.model = Some(m);
                }
            }

            directives.push(dir);

            if self.peek()?.kind == TokenKind::Comma {
                self.advance()?;
                continue;
            }
            break;
        }

        if directives.is_empty() {
            return Err(QqlError::syntax(
                "EMBED requires at least one directive",
                self.peek()?.pos,
            ));
        }
        Ok(directives)
    }
}
