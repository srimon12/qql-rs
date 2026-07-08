use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::ast::{
    ContextPair, FeedbackItem, FeedbackStrategy, FeedbackStrategyType, PrefetchRef, QueryStmt,
    QueryType, SearchWith, CTE,
};
use crate::error::QqlError;
use crate::token::TokenKind;

use super::{ascii_equal, ascii_equal_lower, merge_search_with, Parser};

impl<'a> Parser<'a> {
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
            let pos_id = match self.parse_point_id_value(&alloc::format!("{} POSITIVE", label)) {
                Ok(id) => id,
                _ => return pairs,
            };
            if self.expect(TokenKind::Comma).is_err() {
                return pairs;
            }
            let neg_id = match self.parse_point_id_value(&alloc::format!("{} NEGATIVE", label)) {
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

    pub fn parse_query_clauses(
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
                            cte_name: alloc::borrow::Cow::Borrowed(""),
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
                            prefetch_ref.cte_name = alloc::borrow::Cow::Owned(cte_name);
                            stmt.ctes.push(CTE {
                                name: prefetch_ref.cte_name.clone(),
                                stmt: inline_stmt,
                            });
                        } else if pk == TokenKind::Identifier
                            || pk == TokenKind::Dense
                            || pk == TokenKind::Sparse
                        {
                            let name = self.parse_identifier()?;
                            prefetch_ref.cte_name = alloc::borrow::Cow::Borrowed(name);
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
                        let tok = self.peek()?;
                        return Err(QqlError::syntax("duplicate FUSION clause", tok.pos));
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
                    stmt.fusion_type = Some(upper);
                }
                TokenKind::Where => {
                    if seen_where {
                        let tok = self.peek()?;
                        return Err(QqlError::syntax("duplicate WHERE clause", tok.pos));
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
                        let tok = self.peek()?;
                        return Err(QqlError::syntax("duplicate RERANK clause", tok.pos));
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
                        let tok = self.peek()?;
                        return Err(QqlError::syntax("duplicate EXACT clause", tok.pos));
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
                        let tok = self.peek()?;
                        return Err(QqlError::syntax("duplicate GROUP BY clause", tok.pos));
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
                        let tok = self.peek()?;
                        return Err(QqlError::syntax("duplicate GROUP SIZE clause", tok.pos));
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
                        let tok = self.peek()?;
                        return Err(QqlError::syntax("duplicate STRATEGY clause", tok.pos));
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
                    let expr = self.parse_formula_expr(super::formula::PRECEDENCE_LOWEST)?;
                    stmt.formula = Some(Box::new(expr));
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
}
