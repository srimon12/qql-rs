use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::ast::{BatchStmt, Stmt};
use crate::error::QqlError;
use crate::token::TokenKind;

use super::AstLowerer;

impl<'a> AstLowerer<'a> {
    pub fn parse_batch(&mut self) -> Result<Stmt, QqlError> {
        self.advance()?; // consume BATCH
        self.expect(TokenKind::Lbrace)?;
        let mut statements = Vec::new();
        loop {
            let tok = self.peek()?;
            if tok.kind == TokenKind::Rbrace {
                self.advance()?;
                break;
            }
            if tok.kind == TokenKind::Eof {
                return Err(QqlError::parse(
                    "QQL-PARSE-SYNTAX",
                    "unterminated BATCH block, expected '}'",
                    tok.span,
                ));
            }
            statements.push(self.parse_stmt()?);
            let next = self.peek()?;
            if next.kind == TokenKind::Semicolon {
                self.advance()?;
            } else if next.kind != TokenKind::Rbrace {
                return Err(QqlError::parse(
                    "QQL-PARSE-SYNTAX",
                    "expected ';' or '}' after batch member",
                    next.span,
                ));
            }
        }
        if statements.is_empty() {
            return Err(QqlError::validation(
                "QQL-VALIDATION-BATCH-EMPTY",
                "BATCH requires at least one member statement",
                None,
            ));
        }
        for member in &statements {
            if !is_batch_member(member) {
                return Err(QqlError::validation(
                    "QQL-VALIDATION-BATCH-MEMBER",
                    alloc::format!(
                        "BATCH members must be queries or mutations, got {}",
                        member_kind(member)
                    ),
                    None,
                ));
            }
        }
        // Trailing shared opts: WAIT for mutation batches, PARAMS
        // (timeout / consistency) for query batches. Family applicability
        // is validated by the lowerer, mirroring per-endpoint rules.
        let wait = self.parse_optional_wait()?;
        let params = if self.peek()?.kind == TokenKind::Params {
            self.advance()?;
            Some(self.parse_search_params()?)
        } else {
            None
        };
        Ok(Stmt::Batch(Box::new(BatchStmt {
            statements,
            wait,
            params,
        })))
    }
}

/// Statements that may appear inside `BATCH { … }`.
///
/// Single-request operations only; the planner further requires one
/// collection and one family (all queries or all mutations).
fn is_batch_member(stmt: &Stmt) -> bool {
    matches!(
        stmt,
        Stmt::Query(_)
            | Stmt::Upsert(_)
            | Stmt::Delete(_)
            | Stmt::ClearPayload(_)
            | Stmt::DeletePayload(_)
            | Stmt::DeleteVector(_)
            | Stmt::UpdateVector(_)
            | Stmt::UpdatePayload(_)
    )
}

/// Human-readable member kind for the validation error.
fn member_kind(stmt: &Stmt) -> &'static str {
    match stmt {
        Stmt::Query(_) => "QUERY",
        Stmt::Scroll(_) => "SCROLL",
        Stmt::Upsert(_) => "UPSERT",
        Stmt::CreateCollection(_)
        | Stmt::CreateIndex(_)
        | Stmt::CreateShardKey(_)
        | Stmt::AlterCollection(_) => "DDL",
        Stmt::DropCollection(_) | Stmt::DropIndex(_) | Stmt::DropShardKey(_) => "DROP",
        Stmt::ShowCollections
        | Stmt::ShowQuotas
        | Stmt::ShowCollection(_)
        | Stmt::ShowShardKeys(_) => "SHOW",
        Stmt::SetQuota(_) => "SET QUOTA",
        Stmt::Delete(_)
        | Stmt::ClearPayload(_)
        | Stmt::DeletePayload(_)
        | Stmt::DeleteVector(_)
        | Stmt::UpdateVector(_)
        | Stmt::UpdatePayload(_) => "mutation",
        Stmt::Count(_) => "COUNT",
        Stmt::Facet(_) => "FACET",
        Stmt::Batch(_) => "BATCH",
    }
}
