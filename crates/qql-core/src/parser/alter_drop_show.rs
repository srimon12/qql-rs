use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::ast::{
    AlterCollectionStmt, CreateIndexStmt, DropCollectionStmt, DropIndexStmt, DropShardKeyStmt, Stmt,
};
use crate::error::QqlError;
use crate::token::TokenKind;

use super::Parser;

impl<'a> Parser<'a> {
    // ── ALTER ───────────────────────────────────────────────────

    pub fn parse_alter(&mut self) -> Result<Stmt, QqlError> {
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

    pub fn parse_drop(&mut self) -> Result<Stmt, QqlError> {
        self.advance()?; // consume DROP
        if self.peek()?.kind == TokenKind::Index {
            self.advance()?; // consume INDEX
            self.expect(TokenKind::On)?;
            self.expect(TokenKind::Collection)?;
            let collection = self.parse_identifier()?;
            self.expect(TokenKind::For)?;
            let field = self.parse_identifier()?;
            return Ok(Stmt::DropIndex(Box::new(DropIndexStmt {
                collection,
                field,
            })));
        }
        if self.peek()?.kind == TokenKind::Shard {
            self.advance()?; // consume SHARD
            self.expect(TokenKind::Key)?;
            let shard_key = self.parse_string()?;
            self.expect(TokenKind::On)?;
            self.expect(TokenKind::Collection)?;
            let collection = self.parse_identifier()?;
            return Ok(Stmt::DropShardKey(Box::new(DropShardKeyStmt {
                collection,
                shard_key,
            })));
        }
        self.expect(TokenKind::Collection)?;
        let collection = self.parse_identifier()?;
        Ok(Stmt::DropCollection(Box::new(DropCollectionStmt {
            collection,
        })))
    }

    // ── SHOW ────────────────────────────────────────────────────

    pub fn parse_show(&mut self) -> Result<Stmt, QqlError> {
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
        if self.peek()?.kind == TokenKind::Shard {
            self.advance()?; // consume SHARD
            self.expect(TokenKind::Keys)?;
            self.expect(TokenKind::On)?;
            self.expect(TokenKind::Collection)?;
            let collection = self.parse_identifier()?;
            return Ok(Stmt::ShowShardKeys(collection));
        }
        Err(QqlError::syntax(
            alloc::format!(
                "expected COLLECTION, COLLECTIONS, or SHARD KEYS after SHOW, got '{}'",
                self.peek()?.text
            ),
            self.peek()?.pos,
        ))
    }

    // ── CREATE INDEX ────────────────────────────────────────────

    pub fn parse_create_index(&mut self) -> Result<Stmt, QqlError> {
        self.advance()?;
        self.expect(TokenKind::On)?;
        self.expect(TokenKind::Collection)?;
        let collection = self.parse_identifier()?;
        self.expect(TokenKind::For)?;
        let field = self.parse_identifier()?;
        let mut field_type = String::from("keyword");
        if self.peek()?.kind == TokenKind::Type {
            self.advance()?;
            field_type = self.parse_identifier()?.to_ascii_lowercase();
        }
        let mut options = Vec::new();
        if self.peek()?.kind == TokenKind::With {
            let pos = self.peek()?.pos;
            self.advance()?;
            options = self.parse_config_block()?;
            super::validate_index_options(&options, pos)?;
        }
        Ok(Stmt::CreateIndex(Box::new(CreateIndexStmt {
            collection,
            field,
            field_type,
            options,
        })))
    }
}
