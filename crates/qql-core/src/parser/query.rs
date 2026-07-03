use alloc::boxed::Box;
use alloc::format;
use alloc::vec::Vec;

use crate::ast::{
    ContextPair, FeedbackItem, FeedbackStrategy, FeedbackStrategyType, PrefetchRef, QueryMode,
    QueryStmt, QueryType, SearchWith, Stmt, CTE,
};
use crate::error::QqlError;
use crate::token::TokenKind;

use super::{ascii_equal, ascii_equal_lower, merge_search_with, Parser};

impl<'a> Parser<'a> {
    pub fn parse_query(&mut self) -> Result<Stmt<'a>, QqlError> {
        let tok = self.peek()?;
        if tok.kind == TokenKind::Fusion {
            self.advance()?;
            let mut stmt = QueryStmt {
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
            };
            let fusion_tok = self.peek()?;
            if fusion_tok.kind == TokenKind::Identifier || fusion_tok.kind == TokenKind::String {
                stmt.fusion_type = Some(fusion_tok.text);
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

    pub fn parse_query_with_cte(&mut self) -> Result<Stmt<'a>, QqlError> {
        self.expect(TokenKind::With)?;
        let ctes = self.parse_cte_list()?;
        let tok = self.peek()?;

        if tok.kind == TokenKind::Fusion {
            self.advance()?;
            let mut stmt = QueryStmt {
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
                ctes,
                prefetch_refs: Vec::new(),
                fusion_type: None,
                rerank: false,
                rerank_model: None,
                formula: None,
                formula_defaults: Vec::new(),
                feedback_target: None,
                feedback_items: Vec::new(),
                feedback_strategy: None,
            };
            let fusion_tok = self.peek()?;
            if fusion_tok.kind == TokenKind::Identifier || fusion_tok.kind == TokenKind::String {
                stmt.fusion_type = Some(fusion_tok.text);
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
        existing_ctes: Option<Vec<CTE<'a>>>,
    ) -> Result<Box<QueryStmt<'a>>, QqlError> {
        let mut stmt = QueryStmt {
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
            ctes: existing_ctes.unwrap_or_default(),
            prefetch_refs: Vec::new(),
            fusion_type: None,
            rerank: false,
            rerank_model: None,
            formula: None,
            formula_defaults: Vec::new(),
            feedback_target: None,
            feedback_items: Vec::new(),
            feedback_strategy: None,
        };

        if self.peek()?.kind == TokenKind::Nearest {
            self.advance()?;
        }

        let tok = self.peek()?;
        match tok.kind {
            TokenKind::Recommend => {
                stmt.mode = QueryMode::Recommend;
                self.advance()?;
                if self.peek()?.kind == TokenKind::With {
                    self.parse_recommend_with(&mut stmt);
                }
            }
            TokenKind::Context => {
                stmt.mode = QueryMode::Context;
                self.advance()?;
                self.expect(TokenKind::Pairs)?;
                stmt.context_pairs = self.parse_context_pairs("CONTEXT");
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
                    stmt.context_pairs = self.parse_context_pairs("DISCOVER CONTEXT");
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
                        stmt.query_text = Some(tok.text);
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
                        if tok.kind == TokenKind::From
                            || tok.kind == TokenKind::Limit
                            || tok.kind == TokenKind::Prefetch
                            || tok.kind == TokenKind::Eof
                        {
                        } else {
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

    pub fn parse_cte_list(&mut self) -> Result<Vec<CTE<'a>>, QqlError> {
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

    pub fn parse_cte_query(&mut self) -> Result<Box<QueryStmt<'a>>, QqlError> {
        let mut ctes: Vec<CTE<'a>> = Vec::new();
        if self.peek()?.kind == TokenKind::With {
            self.advance()?;
            ctes = self.parse_cte_list()?;
        }

        let tok = self.peek()?;
        if tok.kind == TokenKind::Fusion {
            self.advance()?;
            let mut stmt = QueryStmt {
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
                ctes,
                prefetch_refs: Vec::new(),
                fusion_type: None,
                rerank: false,
                rerank_model: None,
                formula: None,
                formula_defaults: Vec::new(),
                feedback_target: None,
                feedback_items: Vec::new(),
                feedback_strategy: None,
            };
            let fusion_tok = self.peek()?;
            if fusion_tok.kind == TokenKind::Identifier || fusion_tok.kind == TokenKind::String {
                stmt.fusion_type = Some(fusion_tok.text);
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

        let mut stmt = QueryStmt {
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
            ctes,
            prefetch_refs: Vec::new(),
            fusion_type: None,
            rerank: false,
            rerank_model: None,
            formula: None,
            formula_defaults: Vec::new(),
            feedback_target: None,
            feedback_items: Vec::new(),
            feedback_strategy: None,
        };

        if self.peek()?.kind == TokenKind::Nearest {
            self.advance()?;
        }

        let tok = self.peek()?;
        match tok.kind {
            TokenKind::Recommend => {
                stmt.mode = QueryMode::Recommend;
                self.advance()?;
                if self.peek()?.kind == TokenKind::With {
                    self.parse_recommend_with(&mut stmt);
                }
            }
            TokenKind::Context => {
                stmt.mode = QueryMode::Context;
                self.advance()?;
                self.expect(TokenKind::Pairs)?;
                stmt.context_pairs = self.parse_context_pairs("CONTEXT");
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
                    stmt.context_pairs = self.parse_context_pairs("DISCOVER");
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
                        stmt.query_text = Some(tok.text);
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
                        if tok.kind == TokenKind::Limit
                            || tok.kind == TokenKind::Prefetch
                            || tok.kind == TokenKind::Rparen
                            || tok.kind == TokenKind::Eof
                        {
                        } else {
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

    // ── RECOMMEND WITH handler ──────────────────────────────────

    pub fn parse_recommend_with(&mut self, stmt: &mut QueryStmt<'a>) {
        if self.advance().is_err() {
            return;
        }
        if self.expect(TokenKind::Lparen).is_err() {
            return;
        }
        while self.peek().map(|t| t.kind).unwrap_or(TokenKind::Eof) != TokenKind::Rparen {
            let key_tok = match self.peek() {
                Ok(t) => t,
                _ => return,
            };
            if key_tok.kind != TokenKind::Identifier {
                return;
            }
            if self.advance().is_err() {
                return;
            }
            if self.expect(TokenKind::Equals).is_err() {
                return;
            }
            if ascii_equal_lower(key_tok.text, "positive") {
                if let Ok(ids) = self.parse_point_id_list() {
                    stmt.positive_ids = ids;
                } else {
                    return;
                }
            } else if ascii_equal_lower(key_tok.text, "negative") {
                if let Ok(ids) = self.parse_point_id_list() {
                    stmt.negative_ids = ids;
                } else {
                    return;
                }
            }
            if self.peek().map(|t| t.kind).unwrap_or(TokenKind::Eof) == TokenKind::Comma {
                if self.advance().is_err() {
                    return;
                }
            } else {
                break;
            }
        }
        let _ = self.expect(TokenKind::Rparen);
    }

    // ── Context pairs ───────────────────────────────────────────

    pub fn parse_context_pairs(&mut self, label: &str) -> Vec<ContextPair<'a>> {
        let mut pairs = Vec::new();
        loop {
            if self.expect(TokenKind::Lparen).is_err() {
                return pairs;
            }
            let pos_id = match self.parse_point_id_value(&format!("{} POSITIVE", label)) {
                Ok(id) => id,
                _ => return pairs,
            };
            if self.expect(TokenKind::Comma).is_err() {
                return pairs;
            }
            let neg_id = match self.parse_point_id_value(&format!("{} NEGATIVE", label)) {
                Ok(id) => id,
                _ => return pairs,
            };
            if self.expect(TokenKind::Rparen).is_err() {
                return pairs;
            }
            pairs.push(ContextPair {
                positive: pos_id,
                negative: neg_id,
            });
            if self.peek().map(|t| t.kind).unwrap_or(TokenKind::Eof) == TokenKind::Comma {
                if self.advance().is_err() {
                    return pairs;
                }
                continue;
            }
            return pairs;
        }
    }

    // ── Feedback items ──────────────────────────────────────────

    pub fn parse_feedback_items(&mut self) -> Result<Vec<FeedbackItem<'a>>, QqlError> {
        self.expect(TokenKind::Lparen)?;
        let mut items = Vec::new();
        loop {
            self.expect(TokenKind::Lparen)?;
            let example_val = self.parse_value()?;
            self.expect(TokenKind::Comma)?;
            let score = self.parse_numeric_literal()?;
            self.expect(TokenKind::Rparen)?;
            items.push(FeedbackItem {
                example: example_val,
                score,
            });
            if self.peek()?.kind == TokenKind::Comma {
                self.advance()?;
                if self.peek()?.kind == TokenKind::Rparen {
                    break;
                }
                continue;
            }
            break;
        }
        self.expect(TokenKind::Rparen)?;
        Ok(items)
    }

    // ── Query clauses ───────────────────────────────────────────

    fn parse_query_clauses(
        &mut self,
        stmt: &mut QueryStmt<'a>,
        _pos: usize,
    ) -> Result<(), QqlError> {
        if self.peek()?.kind == TokenKind::Limit {
            self.advance()?;
            let limit_tok = self.expect(TokenKind::Integer)?;
            let limit: i64 = limit_tok
                .text
                .parse()
                .map_err(|_| QqlError::syntax("invalid limit", limit_tok.pos))?;
            stmt.limit = limit;
        } else {
            stmt.limit = 10;
        }

        let mut seen_where = false;
        let mut seen_rerank = false;
        let mut seen_group = false;
        let mut seen_group_size = false;
        let mut seen_exact = false;
        let mut seen_fusion = false;
        let mut seen_strategy = false;

        loop {
            match self.peek()?.kind {
                TokenKind::Offset => {
                    self.advance()?;
                    let offset_tok = self.advance()?;
                    let offset: i64 = match offset_tok.kind {
                        TokenKind::Integer => offset_tok.text.parse().unwrap_or(0),
                        _ => return Ok(()),
                    };
                    if offset < 0 {
                        return Ok(());
                    }
                    stmt.offset = offset;
                }
                TokenKind::Score => {
                    self.advance()?;
                    self.expect(TokenKind::Threshold)?;
                    let score_tok = self.peek()?;
                    if score_tok.kind == TokenKind::Float || score_tok.kind == TokenKind::Integer {
                        self.advance()?;
                        let f = score_tok.text.parse::<f64>().unwrap_or(0.0);
                        stmt.score_threshold = Some(f);
                    }
                }
                TokenKind::Lookup => {
                    self.advance()?;
                    self.expect(TokenKind::From)?;
                    let lookup_from = self.parse_identifier()?;
                    stmt.lookup_from = Some(lookup_from);
                    let tok = self.peek()?;
                    if tok.kind == TokenKind::Vector
                        || (tok.kind == TokenKind::Identifier && ascii_equal(tok.text, "VECTOR"))
                    {
                        self.advance()?;
                        if let Ok(lv) = self.parse_string_ptr() {
                            stmt.lookup_vector = Some(lv);
                        }
                    }
                }
                TokenKind::Using => {
                    self.advance()?;
                    if self.peek()?.kind == TokenKind::Hybrid {
                        self.advance()?;
                        stmt.query_type = QueryType::Hybrid;
                    } else if self.peek()?.kind == TokenKind::Sparse {
                        self.advance()?;
                        stmt.query_type = QueryType::Sparse;
                        stmt.using_ = Some("sparse");
                        if self.peek()?.kind == TokenKind::String {
                            let vec = self.advance()?;
                            stmt.using_ = Some(vec.text);
                        }
                    } else if self.peek()?.kind == TokenKind::Dense {
                        self.advance()?;
                        stmt.query_type = QueryType::Dense;
                        stmt.using_ = Some("dense");
                        if self.peek()?.kind == TokenKind::String {
                            let vec = self.advance()?;
                            stmt.using_ = Some(vec.text);
                        }
                    } else if self.peek()?.kind == TokenKind::String {
                        let vec = self.advance()?;
                        stmt.using_ = Some(vec.text);
                        stmt.query_type = QueryType::Dense;
                    }
                }
                TokenKind::Prefetch => {
                    self.advance()?;
                    self.expect(TokenKind::Lparen)?;
                    let mut inline_idx = 0;
                    while self.peek()?.kind != TokenKind::Rparen {
                        let mut prefetch_ref = PrefetchRef {
                            cte_name: "",
                            filter: None,
                            score_threshold: None,
                            lookup_from: None,
                            lookup_vector: None,
                        };
                        let pk = self.peek()?.kind;
                        if pk == TokenKind::Query
                            || pk == TokenKind::Recommend
                            || pk == TokenKind::Discover
                            || pk == TokenKind::With
                        {
                            let inline_stmt = self.parse_cte_query()?;
                            let cte_name = alloc::format!("__inline_pf{}", inline_idx);
                            inline_idx += 1;
                            prefetch_ref.cte_name = self.intern_string(cte_name);
                            stmt.ctes.push(CTE {
                                name: prefetch_ref.cte_name,
                                stmt: inline_stmt,
                            });
                        } else if pk == TokenKind::Identifier
                            || pk == TokenKind::Dense
                            || pk == TokenKind::Sparse
                        {
                            let name = self.parse_identifier()?;
                            prefetch_ref.cte_name = name;
                        } else {
                            return Ok(());
                        }

                        if self.peek()?.kind == TokenKind::Where {
                            self.advance()?;
                            if let Ok(filter) = self.parse_filter_expr() {
                                prefetch_ref.filter = Some(Box::new(filter));
                            } else {
                                return Ok(());
                            }
                        }

                        if self.peek()?.kind == TokenKind::Score {
                            self.advance()?;
                            if self.expect(TokenKind::Threshold).is_ok() {
                                let score_tok = self.peek()?;
                                if score_tok.kind == TokenKind::Float
                                    || score_tok.kind == TokenKind::Integer
                                {
                                    self.advance()?;
                                    if let Ok(f) = score_tok.text.parse::<f64>() {
                                        prefetch_ref.score_threshold = Some(f);
                                    }
                                }
                            }
                        }

                        if self.peek()?.kind == TokenKind::Lookup {
                            self.advance()?;
                            if self.expect(TokenKind::From).is_ok() {
                                if let Ok(lookup_from) = self.parse_identifier() {
                                    prefetch_ref.lookup_from = Some(lookup_from);
                                    let tok = self.peek()?;
                                    if tok.kind == TokenKind::Vector
                                        || (tok.kind == TokenKind::Identifier
                                            && ascii_equal(tok.text, "VECTOR"))
                                    {
                                        self.advance()?;
                                        if let Ok(lv) = self.parse_string_ptr() {
                                            prefetch_ref.lookup_vector = Some(lv);
                                        }
                                    }
                                }
                            }
                        }

                        stmt.prefetch_refs.push(prefetch_ref);

                        if self.peek()?.kind == TokenKind::Comma {
                            self.advance()?;
                        } else {
                            break;
                        }
                    }
                    let _ = self.expect(TokenKind::Rparen);
                }
                TokenKind::Fusion => {
                    if seen_fusion {
                        return Ok(());
                    }
                    seen_fusion = true;
                    self.advance()?;
                    let fusion_tok = self.advance()?;
                    if fusion_tok.kind != TokenKind::Identifier
                        || (!ascii_equal(fusion_tok.text, "RRF")
                            && !ascii_equal(fusion_tok.text, "DBSF"))
                    {
                        return Ok(());
                    }
                    let upper = if ascii_equal(fusion_tok.text, "RRF") {
                        "RRF"
                    } else {
                        "DBSF"
                    };
                    stmt.fusion_type = Some(self.intern_string(alloc::string::String::from(upper)));
                }
                TokenKind::Where => {
                    if seen_where {
                        return Ok(());
                    }
                    seen_where = true;
                    self.advance()?;
                    if let Ok(filter) = self.parse_filter_expr() {
                        stmt.query_filter = Some(Box::new(filter));
                    } else {
                        return Ok(());
                    }
                }
                TokenKind::Rerank => {
                    if seen_rerank {
                        return Ok(());
                    }
                    seen_rerank = true;
                    self.advance()?;
                    stmt.rerank = true;
                    if self.peek()?.kind == TokenKind::Model {
                        self.advance()?;
                        if let Ok(m) = self.parse_string_ptr() {
                            stmt.rerank_model = Some(m);
                        }
                    }
                }
                TokenKind::Exact => {
                    if seen_exact {
                        return Ok(());
                    }
                    seen_exact = true;
                    self.advance()?;
                    let sw = SearchWith {
                        hnsw_ef: 0,
                        exact: true,
                        acorn: false,
                        indexed_only: false,
                        quantization: None,
                        mmr_diversity: None,
                        mmr_candidates: None,
                        rrf_k: None,
                        rrf_weights: Vec::new(),
                    };
                    merge_search_with(&mut stmt.with_clause, sw);
                }
                TokenKind::With => {
                    self.advance()?;
                    if self.peek()?.kind == TokenKind::Model {
                        self.advance()?;
                        let model_tok = self.expect(TokenKind::String)?;
                        stmt.model = Some(model_tok.text);
                    } else if self.peek()?.kind == TokenKind::Payload {
                        self.advance()?;
                        if let Ok(parsed) = self.parse_with_payload() {
                            stmt.with_payload = Some(parsed);
                        } else {
                            return Ok(());
                        }
                    } else if self.peek()?.kind == TokenKind::Vectors {
                        self.advance()?;
                        if let Ok(parsed) = self.parse_with_vectors() {
                            stmt.with_vectors = Some(parsed);
                        } else {
                            return Ok(());
                        }
                    } else if self.peek()?.kind == TokenKind::Lookup {
                        self.advance()?;
                        if self.expect(TokenKind::From).is_ok() {
                            if let Ok(collection) = self.parse_identifier() {
                                stmt.with_lookup_collection = Some(collection);
                            }
                        }
                    } else {
                        if let Ok(parsed) = self.parse_with_clause() {
                            merge_search_with(&mut stmt.with_clause, parsed);
                        } else {
                            return Ok(());
                        }
                    }
                }
                TokenKind::Group => {
                    if seen_group {
                        return Ok(());
                    }
                    seen_group = true;
                    self.advance()?;
                    self.expect(TokenKind::By)?;
                    if let Ok(group_field) = self.parse_string_ptr() {
                        stmt.group_by = Some(group_field);
                    } else {
                        return Ok(());
                    }
                }
                TokenKind::GroupSize => {
                    if seen_group_size {
                        return Ok(());
                    }
                    seen_group_size = true;
                    self.advance()?;
                    if let Ok(val) = self.parse_numeric_literal() {
                        if val > 0.0 && val == (val as u64) as f64 {
                            stmt.group_size = Some(val as i64);
                        } else {
                            return Ok(());
                        }
                    } else {
                        return Ok(());
                    }
                }
                TokenKind::Strategy => {
                    if seen_strategy {
                        return Ok(());
                    }
                    seen_strategy = true;
                    self.advance()?;
                    if self.peek()?.kind == TokenKind::Identifier
                        && ascii_equal_lower(self.peek()?.text, "naive")
                    {
                        self.advance()?;
                        self.expect(TokenKind::Lparen)?;
                        let mut strat = FeedbackStrategy {
                            strategy_type: FeedbackStrategyType::Naive,
                            a: 0.0,
                            b: 0.0,
                            c: 0.0,
                        };
                        while self.peek()?.kind != TokenKind::Rparen {
                            if let Ok(key) = self.parse_identifier() {
                                self.expect(TokenKind::Equals)?;
                                if let Ok(val) = self.parse_numeric_literal() {
                                    if ascii_equal_lower(key, "a") {
                                        strat.a = val;
                                    } else if ascii_equal_lower(key, "b") {
                                        strat.b = val;
                                    } else if ascii_equal_lower(key, "c") {
                                        strat.c = val;
                                    }
                                }
                                if self.peek()?.kind == TokenKind::Comma {
                                    self.advance()?;
                                }
                            }
                        }
                        self.advance()?;
                        stmt.feedback_strategy = Some(Box::new(strat));
                    } else {
                        if let Ok(s) = self.parse_string_ptr() {
                            stmt.strategy = Some(s);
                        }
                    }
                }
                TokenKind::Limit => {
                    self.advance()?;
                    let limit_tok = self.expect(TokenKind::Integer)?;
                    let limit: i64 = limit_tok
                        .text
                        .parse()
                        .map_err(|_| QqlError::syntax("invalid limit", limit_tok.pos))?;
                    stmt.limit = limit;
                }
                TokenKind::Boost => {
                    self.advance()?;
                    if let Ok(expr) = self.parse_formula_expr(super::formula::PRECEDENCE_LOWEST) {
                        stmt.formula = Some(Box::new(expr));
                    } else {
                        return Ok(());
                    }
                }
                TokenKind::Defaults => {
                    self.advance()?;
                    self.expect(TokenKind::Lparen)?;
                    let mut defaults = Vec::new();
                    while self.peek()?.kind != TokenKind::Rparen {
                        if let Ok(key) = self.parse_identifier() {
                            self.expect(TokenKind::Equals)?;
                            if let Ok(val) = self.parse_value() {
                                defaults.push((key, val));
                            }
                            if self.peek()?.kind == TokenKind::Comma {
                                self.advance()?;
                            } else {
                                break;
                            }
                        }
                    }
                    let _ = self.expect(TokenKind::Rparen);
                    stmt.formula_defaults = defaults;
                }
                _ => return Ok(()),
            }
        }
    }

    /// Helper: store an owned string so we can borrow it as `&'a str`.
    /// Since the parser borrows from the input, we use this sparingly for
    /// dynamically-constructed CTE names.
    fn intern_string(&self, s: alloc::string::String) -> &'a str {
        // SAFETY: leak the string to get a 'static reference, then cast.
        // This is only used for CTE names like "__inline_pf0".
        let leaked: &'static str = Box::leak(s.into_boxed_str());
        // We lie about the lifetime — this is safe because the leaked
        // memory lives for the rest of the program.
        unsafe { &*(leaked as *const str) }
    }
}
