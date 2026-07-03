use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::ast::{AlterCollectionStmt, CreateIndexStmt, DropCollectionStmt, Stmt};
use crate::error::QqlError;
use crate::token::TokenKind;

use super::Parser;

impl<'a> Parser<'a> {
    // ── ALTER ───────────────────────────────────────────────────

    pub fn parse_alter(&mut self) -> Result<Stmt<'a>, QqlError> {
        self.advance()?;
        self.expect(TokenKind::Collection)?;
        let collection = self.parse_identifier()?;
        let config = self.parse_collection_config_blocks(true)?;
        Ok(Stmt::AlterCollection(Box::new(AlterCollectionStmt {
            collection,
            config,
        })))
    }

    // ── DROP ────────────────────────────────────────────────────

    pub fn parse_drop(&mut self) -> Result<Stmt<'a>, QqlError> {
        self.advance()?;
        self.expect(TokenKind::Collection)?;
        let collection = self.parse_identifier()?;
        Ok(Stmt::DropCollection(Box::new(DropCollectionStmt {
            collection,
        })))
    }

    // ── SHOW ────────────────────────────────────────────────────

    pub fn parse_show(&mut self) -> Result<Stmt<'a>, QqlError> {
        self.advance()?;
        if self.peek()?.kind == TokenKind::Collections {
            self.advance()?;
            return Ok(Stmt::ShowCollections);
        }
        if self.peek()?.kind == TokenKind::Collection {
            self.advance()?;
            let collection = self.parse_identifier()?;
            return Ok(Stmt::ShowCollection(collection));
        }
        Err(QqlError::syntax(
            alloc::format!(
                "expected COLLECTION or COLLECTIONS after SHOW, got '{}'",
                self.peek()?.text
            ),
            self.peek()?.pos,
        ))
    }

    // ── CREATE INDEX ────────────────────────────────────────────

    pub fn parse_create_index(&mut self) -> Result<Stmt<'a>, QqlError> {
        self.advance()?;
        self.expect(TokenKind::On)?;
        self.expect(TokenKind::Collection)?;
        let collection = self.parse_identifier()?;
        self.expect(TokenKind::For)?;
        let field = self.parse_identifier()?;
        let mut field_type: &'a str = "keyword";
        if self.peek()?.kind == TokenKind::Type {
            self.advance()?;
            let type_tok = self.expect(TokenKind::Identifier)?;
            let lowered = type_tok.text.to_ascii_lowercase();
            let leaked: &'static str = Box::leak(lowered.into_boxed_str());
            field_type = unsafe { &*(leaked as *const str) };
        }
        let mut options = Vec::new();
        if self.peek()?.kind == TokenKind::With {
            self.advance()?;
            options = self.parse_config_block()?;
        }
        Ok(Stmt::CreateIndex(Box::new(CreateIndexStmt {
            collection,
            field,
            field_type,
            options,
        })))
    }
}
