use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::ast::{QueryMode, QueryStmt, QueryType, CTE};
use crate::error::QqlError;
use crate::token::TokenKind;

use super::Parser;

impl<'a> Parser<'a> {
    pub fn parse_cte_list(&mut self) -> Result<Vec<CTE>, QqlError> {
        let mut ctes = Vec::new();
        loop {
            let name = self.parse_identifier()?;
            self.expect(TokenKind::As)?;
            self.expect(TokenKind::Lparen)?;
            let sub_stmt = self.parse_cte_query()?;
            self.expect(TokenKind::Rparen)?;
            ctes.push(CTE {
                name,
                stmt: sub_stmt,
            });
            if self.peek()?.kind == TokenKind::Comma {
                self.advance()?;
                continue;
            }
            return Ok(ctes);
        }
    }

    pub fn parse_cte_query(&mut self) -> Result<Box<QueryStmt>, QqlError> {
        let mut ctes: Vec<CTE> = Vec::new();
        if self.peek()?.kind == TokenKind::With {
            self.advance()?;
            ctes = self.parse_cte_list()?;
        }

        let tok = self.peek()?;
        if tok.kind == TokenKind::Fusion {
            self.advance()?;
            let mut stmt = new_default_cte_stmt();
            stmt.ctes = ctes;
            let fusion_tok = self.peek()?;
            if fusion_tok.kind == TokenKind::Identifier || fusion_tok.kind == TokenKind::String {
                stmt.fusion_type = Some(fusion_tok.text.to_string());
                self.advance()?;
            }
            if self.peek()?.kind == TokenKind::From {
                self.advance()?;
                let coll = self.parse_identifier()?;
                stmt.collection = Some(coll);
            }
            let pos = self.peek()?.pos;
            self.parse_query_clauses(&mut stmt, pos)?;
            return Ok(Box::new(stmt));
        }

        self.expect(TokenKind::Query)?;

        let mut stmt = new_default_cte_stmt();
        stmt.ctes = ctes;

        if self.peek()?.kind == TokenKind::Nearest {
            self.advance()?;
        }

        let tok = self.peek()?;
        match tok.kind {
            TokenKind::Recommend => {
                stmt.mode = QueryMode::Recommend;
                self.advance()?;
                if self.peek()?.kind == TokenKind::With {
                    self.parse_recommend_with(&mut stmt)?;
                }
            }
            TokenKind::Context => {
                stmt.mode = QueryMode::Context;
                self.advance()?;
                self.expect(TokenKind::Pairs)?;
                stmt.context_pairs = self.parse_context_pairs("CONTEXT")?;
            }
            TokenKind::Discover => {
                stmt.mode = QueryMode::Discover;
                self.advance()?;
                self.expect(TokenKind::Target)?;
                let target_id = self.parse_point_id_value("DISCOVER TARGET")?;
                stmt.target = Some(target_id);
                if self.peek()?.kind == TokenKind::Context {
                    self.advance()?;
                    self.expect(TokenKind::Pairs)?;
                    stmt.context_pairs = self.parse_context_pairs("DISCOVER")?;
                }
            }
            TokenKind::Sample => {
                stmt.mode = QueryMode::Sample;
                self.advance()?;
            }
            _ => {
                stmt.mode = QueryMode::Nearest;
                match tok.kind {
                    TokenKind::String => {
                        stmt.query_text = Some(tok.text.to_string());
                        self.advance()?;
                    }
                    TokenKind::Integer => {
                        let id = self.parse_point_id_value("QUERY")?;
                        stmt.query_id = Some(id);
                    }
                    TokenKind::Lbracket => {
                        let vec = self.parse_raw_vector()?;
                        stmt.raw_vector = vec;
                    }
                    _ => {
                        if !matches!(
                            tok.kind,
                            TokenKind::Limit
                                | TokenKind::Prefetch
                                | TokenKind::Rparen
                                | TokenKind::Eof
                        ) {
                            return Err(QqlError::syntax(
                                "expected string, integer, raw vector [...], or query mode for CTE QUERY",
                                tok.pos,
                            ));
                        }
                    }
                }
            }
        }

        let pos = self.peek()?.pos;
        self.parse_query_clauses(&mut stmt, pos)?;
        Ok(Box::new(stmt))
    }
}

fn new_default_cte_stmt() -> QueryStmt {
    QueryStmt {
        collection: None,
        mode: QueryMode::Nearest,
        query_type: QueryType::Dense,
        query_text: None,
        query_id: None,
        raw_vector: Vec::new(),
        positive_ids: Vec::new(),
        negative_ids: Vec::new(),
        context_pairs: Vec::new(),
        target: None,
        order_by_field: None,
        order_by_asc: None,
        limit: 10,
        offset: 0,
        score_threshold: None,
        strategy: None,
        query_filter: None,
        group_by: None,
        group_size: None,
        with_clause: None,
        with_payload: None,
        with_vector: None,
        lookup_from: None,
        lookup_vector: None,
        with_lookup_collection: None,
        using_: None,
        model: None,
        ctes: Vec::new(),
        prefetch_refs: Vec::new(),
        fusion_type: None,
        rerank: false,
        rerank_model: None,
        formula: None,
        formula_defaults: Vec::new(),
        feedback_target: None,
        feedback_items: Vec::new(),
        feedback_strategy: None,
    }
}
