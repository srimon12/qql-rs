use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::ast::{
    AlterCollectionStmt, CreateIndexStmt, DropCollectionStmt, DropIndexStmt, DropShardKeyStmt,
    SetQuotaStmt, Stmt,
};
use crate::error::QqlError;
use crate::token::TokenKind;

use super::AstLowerer;

/// The closed set of `CREATE INDEX … TYPE` values, mirroring
/// `field_type` in language/v1/grammar.pest.
const INDEX_FIELD_TYPES: &[&str] = &[
    "keyword", "integer", "float", "geo", "text", "bool", "datetime", "uuid",
];

impl<'a> AstLowerer<'a> {
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
        if self.peek()?.kind == TokenKind::Quotas {
            self.advance()?;
            return Ok(Stmt::ShowQuotas);
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
                "expected COLLECTION, COLLECTIONS, QUOTAS, or SHARD KEYS after SHOW, got '{}'",
                self.peek()?.text
            ),
            self.peek()?.pos,
        ))
    }

    // ── SET QUOTA ────────────────────────────────────────────────

    pub fn parse_set_quota(&mut self) -> Result<Stmt, QqlError> {
        self.advance()?; // consume SET
        self.expect(TokenKind::Quota)?;
        let config = self.parse_config_block()?;
        let wait = if self.peek()?.kind == TokenKind::Wait {
            self.advance()?;
            match self.peek()?.kind {
                TokenKind::True => {
                    self.advance()?;
                    Some(true)
                }
                TokenKind::False => {
                    self.advance()?;
                    Some(false)
                }
                _ => {
                    return Err(QqlError::parse(
                        "QQL-PARSE-QUOTA",
                        "WAIT requires true or false",
                        self.peek()?.span,
                    ));
                }
            }
        } else {
            None
        };
        Ok(Stmt::SetQuota(Box::new(SetQuotaStmt { config, wait })))
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
            let field_type_token = self.peek()?;
            if field_type_token.kind == TokenKind::String {
                return Err(QqlError::parse(
                    "QQL-PARSE-INDEX-TYPE",
                    "index TYPE must be an unquoted canonical type name",
                    field_type_token.span,
                ));
            }
            field_type = self.parse_identifier()?.to_ascii_lowercase();
            // `field_type` is a closed enum in grammar.pest; reject unknown
            // types at parse time instead of forwarding them to the backend.
            if !INDEX_FIELD_TYPES.contains(&field_type.as_str()) {
                return Err(QqlError::parse(
                    "QQL-PARSE-INDEX-TYPE",
                    alloc::format!("unknown index field type '{field_type}'"),
                    field_type_token.span,
                ));
            }
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
