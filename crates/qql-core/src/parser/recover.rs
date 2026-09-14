//! Panic-mode recovery: sync on `;` or a statement-start keyword.
//!
//! [`super::Parser::parse`] / [`super::Parser::parse_all`] stay fail-fast
//! (execution must not run a partial script). This path is for IDEs, `analyze`,
//! and the REPL diagnostics surface: emit every recoverable error and keep
//! statements that parsed.

use super::{AstLowerer, MAX_STATEMENTS};
use crate::ast::Stmt;
use crate::error::{QqlError, Span};
use crate::token::TokenKind;
use alloc::vec::Vec;

/// A script parse that keeps going after syntax errors.
#[derive(Debug, Clone)]
pub struct RecoveredScript {
    /// Statements that parsed successfully, in source order, with their spans.
    pub statements: Vec<(Stmt, Span)>,
    /// Every recoverable syntax error, in source order.
    pub errors: Vec<QqlError>,
}

impl RecoveredScript {
    /// True when the script parsed with no errors (execution-ready).
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }
}

impl<'a> AstLowerer<'a> {
    pub(crate) fn lower_script_recovering(input: &'a str) -> RecoveredScript {
        let tokens = match Self::lex(input) {
            Ok(tokens) => tokens,
            Err(error) => {
                return RecoveredScript {
                    statements: Vec::new(),
                    errors: alloc::vec![error],
                };
            }
        };
        let mut parser = AstLowerer::new(input, tokens);
        let mut statements = Vec::new();
        let mut errors = Vec::new();

        while parser.peek_kind() != TokenKind::Eof {
            if statements.len() >= MAX_STATEMENTS {
                if let Ok(tok) = parser.peek() {
                    errors.push(QqlError::parse(
                        "QQL-PARSE-STATEMENT-LIMIT",
                        alloc::format!("a script may contain at most {MAX_STATEMENTS} statements"),
                        tok.span,
                    ));
                }
                break;
            }

            while parser.peek_kind() == TokenKind::Semicolon {
                if let Ok(tok) = parser.peek() {
                    errors.push(QqlError::parse(
                        "QQL-PARSE-EMPTY-STATEMENT",
                        "leading or empty statements are not allowed",
                        tok.span,
                    ));
                }
                let _ = parser.advance();
                if parser.peek_kind() == TokenKind::Eof {
                    return RecoveredScript { statements, errors };
                }
            }

            let Ok(start_tok) = parser.peek() else {
                break;
            };
            if start_tok.kind == TokenKind::Eof {
                break;
            }
            let start_pos = start_tok.span.start;
            match parser.parse_stmt() {
                Ok(stmt) => match parser.peek() {
                    Ok(next) if next.kind == TokenKind::Semicolon => {
                        let end = next.span.end;
                        let _ = parser.advance();
                        statements.push((stmt, Span::new(start_pos, end)));
                    }
                    Ok(next) if next.kind == TokenKind::Eof => {
                        let end = parser.prev_span().end;
                        statements.push((stmt, Span::new(start_pos, end)));
                    }
                    Ok(next) if next.kind.is_statement_start() => {
                        errors.push(QqlError::parse(
                            "QQL-PARSE-SEPARATOR",
                            "multiple statements must be separated by a semicolon",
                            next.span,
                        ));
                        let end = parser.prev_span().end;
                        statements.push((stmt, Span::new(start_pos, end)));
                    }
                    Ok(next) => {
                        errors.push(QqlError::parse(
                            "QQL-PARSE-TRAILING",
                            alloc::format!("unexpected trailing token '{}'", next.text),
                            next.span,
                        ));
                        let end = parser.prev_span().end;
                        statements.push((stmt, Span::new(start_pos, end)));
                        parser.synchronize();
                    }
                    Err(error) => {
                        errors.push(error);
                        parser.synchronize();
                    }
                },
                Err(error) => {
                    errors.push(error);
                    let stuck_at = parser.index;
                    parser.synchronize();
                    // Force progress only when we are not already sitting on
                    // the next statement. Skipping a recovery-sync keyword
                    // would drop `COUNT` / `SHOW` after a failed `QUERY`.
                    if parser.index == stuck_at && !parser.peek_kind().is_recovery_sync() {
                        let _ = parser.advance();
                    }
                }
            }
        }

        RecoveredScript { statements, errors }
    }

    fn peek_kind(&mut self) -> TokenKind {
        self.peek().map(|t| t.kind).unwrap_or(TokenKind::Eof)
    }

    /// Skip tokens until `;` (consumed) or a statement-start keyword (left
    /// unconsumed). `WITH` is not a boundary — it is also a clause.
    fn synchronize(&mut self) {
        loop {
            let Ok(tok) = self.peek() else {
                return;
            };
            match tok.kind {
                TokenKind::Eof => return,
                TokenKind::Semicolon => {
                    let _ = self.advance();
                    return;
                }
                kind if kind.is_recovery_sync() => return,
                _ => {
                    let _ = self.advance();
                }
            }
        }
    }
}
