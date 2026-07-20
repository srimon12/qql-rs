use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::ast::{QueryMode, QueryStmt, QueryType, Stmt, CTE};
use crate::error::QqlError;
use crate::token::TokenKind;

use super::Parser;

impl<'a> Parser<'a> {
    pub fn parse_query(&mut self) -> Result<Stmt, QqlError> {
        let tok = self.peek()?;
        if tok.kind == TokenKind::Fusion {
            self.advance()?;
            let mut stmt = new_default_query_stmt();
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
            let fusion_pos = self.peek()?.pos;
            self.parse_query_clauses(&mut stmt, fusion_pos)?;
            return Ok(Stmt::Query(Box::new(stmt)));
        }

        self.expect(TokenKind::Query)?;
        Ok(Stmt::Query(self.parse_query_body(None)?))
    }

    pub fn parse_query_with_cte(&mut self) -> Result<Stmt, QqlError> {
        self.expect(TokenKind::With)?;
        let ctes = self.parse_cte_list()?;
        let tok = self.peek()?;

        if tok.kind == TokenKind::Fusion {
            self.advance()?;
            let mut stmt = new_default_query_stmt();
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
            return Ok(Stmt::Query(Box::new(stmt)));
        }

        self.expect(TokenKind::Query)?;
        let stmt = self.parse_query_body(Some(ctes))?;
        Ok(Stmt::Query(stmt))
    }

    fn parse_query_body(
        &mut self,
        existing_ctes: Option<Vec<CTE>>,
    ) -> Result<Box<QueryStmt>, QqlError> {
        let mut stmt = new_default_query_stmt();
        stmt.ctes = existing_ctes.unwrap_or_default();

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
                    stmt.context_pairs = self.parse_context_pairs("DISCOVER CONTEXT")?;
                }
            }
            TokenKind::Order => {
                stmt.mode = QueryMode::OrderBy;
                self.advance()?;
                self.expect(TokenKind::By)?;
                let field = self.parse_identifier()?;
                stmt.order_by_field = Some(field);
                let tok = self.peek()?;
                match tok.kind {
                    TokenKind::Asc => {
                        self.advance()?;
                        stmt.order_by_asc = Some(true);
                    }
                    TokenKind::Desc => {
                        self.advance()?;
                        stmt.order_by_asc = Some(false);
                    }
                    _ => {}
                }
            }
            TokenKind::Sample => {
                stmt.mode = QueryMode::Sample;
                self.advance()?;
            }
            TokenKind::Relevance => {
                stmt.mode = QueryMode::RelevanceFeedback;
                self.advance()?;
                self.expect(TokenKind::Feedback)?;
                self.expect(TokenKind::Target)?;
                let target_val = self.parse_value()?;
                stmt.feedback_target = Some(target_val);
                self.expect(TokenKind::Feedback)?;
                stmt.feedback_items = self.parse_feedback_items()?;
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
                            TokenKind::From
                                | TokenKind::Limit
                                | TokenKind::Prefetch
                                | TokenKind::Eof
                        ) {
                            return Err(QqlError::syntax(
                                "expected a string query, a point ID, a raw vector [...], or a query mode (RECOMMEND/DISCOVER/CONTEXT) after QUERY",
                                tok.pos,
                            ));
                        }
                    }
                }
            }
        }

        if self.peek()?.kind == TokenKind::From {
            self.advance()?;
            let coll = self.parse_identifier()?;
            stmt.collection = Some(coll);
        }

        let pos = self.peek()?.pos;
        self.parse_query_clauses(&mut stmt, pos)?;

        if stmt.mode == QueryMode::Nearest
            && stmt.query_text.is_none()
            && stmt.query_id.is_none()
            && stmt.raw_vector.is_empty()
            && stmt.prefetch_refs.is_empty()
            && stmt.ctes.is_empty()
            && stmt.query_type != QueryType::Hybrid
            && stmt.fusion_type.is_none()
        {
            return Err(QqlError::syntax(
                "expected a string query, a point ID, a raw vector [...], or a query mode (RECOMMEND/DISCOVER/CONTEXT) after QUERY",
                tok.pos,
            ));
        }

        Ok(Box::new(stmt))
    }
}

fn new_default_query_stmt() -> QueryStmt {
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
        with_vectors: None,
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
