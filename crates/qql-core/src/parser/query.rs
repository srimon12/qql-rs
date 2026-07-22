use super::{ascii_equal, Parser};
use crate::ast::{
    ContextPair, Cte, FeedbackItem, FeedbackStrategy, FilterExpr, FusionMethod, GroupSpec,
    LookupSpec, MmrConfig, OrderDirection, PageSpec, Prefetch, PrefetchSource, QueryCollection,
    QueryExpr, QueryInput, QueryOutput, QueryStmt, RecommendStrategy, Stmt,
};
use crate::error::{QqlError, Span};
use crate::token::TokenKind;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

impl<'a> Parser<'a> {
    pub fn parse_query(&mut self) -> Result<Stmt, QqlError> {
        self.expect(TokenKind::Query)?;
        self.parse_query_stmt(true, Vec::new())
            .map(Box::new)
            .map(Stmt::Query)
    }

    pub fn parse_query_with_cte(&mut self) -> Result<Stmt, QqlError> {
        self.expect(TokenKind::With)?;
        let ctes = self.parse_ctes()?;
        self.expect(TokenKind::Query)?;
        self.parse_query_stmt(true, ctes)
            .map(Box::new)
            .map(Stmt::Query)
    }

    fn parse_query_stmt(&mut self, top_level: bool, ctes: Vec<Cte>) -> Result<QueryStmt, QqlError> {
        let expression_span = self.peek()?.span;
        let mut expression = self.parse_query_expression()?;

        let collection = if self.peek()?.kind == TokenKind::From {
            self.advance()?;
            QueryCollection::Explicit(self.parse_identifier()?)
        } else if top_level {
            return Err(QqlError::validation(
                "QQL-VALIDATION-FROM",
                "top-level QUERY requires FROM <collection>",
                Some(self.peek()?.span),
            ));
        } else {
            QueryCollection::Inherited
        };

        let using = if self.peek()?.kind == TokenKind::Using {
            self.advance()?;
            Some(self.parse_identifier()?)
        } else {
            None
        };

        let prefetch = if self.peek()?.kind == TokenKind::Prefetch {
            self.advance()?;
            self.parse_prefetch_list()?
        } else {
            Vec::new()
        };

        let filter = if self.peek()?.kind == TokenKind::Where {
            self.advance()?;
            Some(Box::new(self.parse_filter_expr()?))
        } else {
            None
        };

        let shard_key = if self.peek()?.kind == TokenKind::Shard {
            self.advance()?;
            Some(self.parse_string()?)
        } else {
            None
        };

        let params = if self.peek()?.kind == TokenKind::Params {
            self.advance()?;
            Some(self.parse_search_params()?)
        } else {
            None
        };

        let score_threshold = if self.peek()?.kind == TokenKind::Score {
            self.advance()?;
            self.expect(TokenKind::Threshold)?;
            Some(self.parse_numeric_literal()?)
        } else {
            None
        };

        let group = if self.peek()?.kind == TokenKind::Group {
            self.advance()?;
            self.expect(TokenKind::By)?;
            let field = self.parse_field_path()?;
            let size = if self.peek_word("SIZE")? {
                self.advance()?;
                Some(self.parse_positive_u64("group size")?)
            } else {
                None
            };
            let lookup = if self.peek()?.kind == TokenKind::Lookup {
                self.advance()?;
                self.expect(TokenKind::From)?;
                Some(self.parse_identifier()?)
            } else {
                None
            };
            Some(GroupSpec {
                field,
                size,
                lookup,
            })
        } else {
            None
        };

        let payload = if self.peek()?.kind == TokenKind::With
            && self.peek_nth(1).kind == TokenKind::Payload
        {
            self.advance()?;
            self.advance()?;
            Some(self.parse_payload_selector()?)
        } else {
            None
        };

        let vectors =
            if self.peek()?.kind == TokenKind::With && self.peek_nth(1).kind == TokenKind::Vector {
                self.advance()?;
                self.advance()?;
                Some(self.parse_vector_selector()?)
            } else {
                None
            };

        let limit = if self.peek()?.kind == TokenKind::Limit {
            self.advance()?;
            Some(self.parse_positive_u64("LIMIT")?)
        } else {
            None
        };

        let offset = if self.peek()?.kind == TokenKind::Offset {
            self.advance()?;
            Some(self.parse_non_negative_u64("OFFSET")?)
        } else {
            None
        };

        if self.is_query_clause_start()? {
            return Err(QqlError::parse(
                "QQL-PARSE-CLAUSE-ORDER",
                "duplicate or out-of-order query clause",
                self.peek()?.span,
            ));
        }

        attach_pipeline(&mut expression, using, prefetch, expression_span)?;
        validate_prefetch_references(&expression, &ctes, expression_span)?;
        validate_common_clauses(
            &expression,
            filter.as_deref(),
            params.as_ref(),
            score_threshold,
            group.as_ref(),
            limit,
            offset,
            expression_span,
        )?;

        Ok(QueryStmt {
            ctes,
            collection,
            expression,
            filter,
            params,
            score_threshold,
            group,
            output: QueryOutput { payload, vectors },
            page: PageSpec { limit, offset },
            shard_key,
        })
    }

    fn parse_ctes(&mut self) -> Result<Vec<Cte>, QqlError> {
        let mut ctes = Vec::new();
        loop {
            let name_token = self.peek()?;
            let name = self.parse_identifier()?;
            if ctes
                .iter()
                .any(|cte: &Cte| cte.name.eq_ignore_ascii_case(&name))
            {
                return Err(QqlError::parse(
                    "QQL-PARSE-DUPLICATE-CTE",
                    alloc::format!("duplicate CTE '{}'", name),
                    name_token.span,
                ));
            }
            self.expect(TokenKind::As)?;
            self.expect(TokenKind::Lparen)?;
            self.expect(TokenKind::Query)?;
            let query = self.parse_query_stmt(false, Vec::new())?;
            self.expect(TokenKind::Rparen)?;
            ctes.push(Cte {
                name,
                query: Box::new(query),
            });
            if self.peek()?.kind != TokenKind::Comma {
                break;
            }
            self.advance()?;
        }
        Ok(ctes)
    }

    fn parse_query_expression(&mut self) -> Result<QueryExpr, QqlError> {
        if self.peek_word("POINTS")? {
            self.advance()?;
            return self
                .parse_point_id_list()
                .map(|ids| QueryExpr::Points { ids });
        }
        if self.peek()?.kind == TokenKind::Nearest {
            self.advance()?;
            return self.parse_query_input().map(|input| QueryExpr::Nearest {
                input,
                using: None,
                prefetch: Vec::new(),
                mmr: None,
            });
        }
        if self.peek()?.kind == TokenKind::Recommend {
            return self.parse_recommend();
        }
        if self.peek()?.kind == TokenKind::Context {
            self.advance()?;
            return self.parse_context_pairs().map(|pairs| QueryExpr::Context {
                pairs,
                using: None,
                prefetch: Vec::new(),
            });
        }
        if self.peek()?.kind == TokenKind::Discover {
            return self.parse_discover();
        }
        if self.peek()?.kind == TokenKind::Order {
            self.advance()?;
            self.expect(TokenKind::By)?;
            let field = self.parse_field_path()?;
            let direction = match self.peek()?.kind {
                TokenKind::Desc => {
                    self.advance()?;
                    OrderDirection::Desc
                }
                TokenKind::Asc => {
                    self.advance()?;
                    OrderDirection::Asc
                }
                _ => OrderDirection::Asc,
            };
            return Ok(QueryExpr::OrderBy { field, direction });
        }
        if self.peek()?.kind == TokenKind::Sample {
            self.advance()?;
            if !self.peek_word("RANDOM")? {
                return Err(QqlError::parse(
                    "QQL-PARSE-SAMPLE",
                    "SAMPLE requires RANDOM",
                    self.peek()?.span,
                ));
            }
            self.advance()?;
            return Ok(QueryExpr::SampleRandom);
        }
        if self.peek()?.kind == TokenKind::Fusion {
            self.advance()?;
            return self.parse_fusion_method().map(|method| QueryExpr::Fusion {
                method,
                prefetch: Vec::new(),
            });
        }
        if self.peek_word("FORMULA")? {
            self.advance()?;
            let expression = self.parse_formula_expr(super::formula::PRECEDENCE_LOWEST)?;
            let defaults = if self.peek()?.kind == TokenKind::Defaults {
                self.advance()?;
                self.parse_config_block()?
            } else {
                Vec::new()
            };
            return Ok(QueryExpr::Formula {
                expression: Box::new(expression),
                defaults,
                prefetch: Vec::new(),
            });
        }
        if self.peek()?.kind == TokenKind::Relevance {
            return self.parse_relevance_feedback();
        }
        if self.peek_word("MMR")? {
            return self.parse_mmr();
        }
        if self.peek()?.kind == TokenKind::Hybrid {
            return self.parse_hybrid();
        }
        if self.peek()?.kind == TokenKind::Rerank {
            return self.parse_rerank();
        }

        self.parse_query_input().map(|input| QueryExpr::Nearest {
            input,
            using: None,
            prefetch: Vec::new(),
            mmr: None,
        })
    }

    fn parse_query_input(&mut self) -> Result<QueryInput, QqlError> {
        if self.peek_word("TEXT")? {
            self.advance()?;
            let text = self.parse_string()?;
            let model = self.parse_optional_model_string()?;
            return Ok(QueryInput::Text { text, model });
        }
        if self.peek()?.kind == TokenKind::Vector {
            self.advance()?;
            return self.parse_vector_value().map(QueryInput::Vector);
        }
        if self.peek_word("POINT")? {
            self.advance()?;
            return self.parse_point_id("POINT").map(QueryInput::Point);
        }
        if self.peek()?.kind == TokenKind::String {
            return self
                .parse_string()
                .map(|text| QueryInput::Text { text, model: None });
        }
        Err(QqlError::parse(
            "QQL-PARSE-QUERY-INPUT",
            "query input requires TEXT, VECTOR, or POINT",
            self.peek()?.span,
        ))
    }

    fn parse_recommend(&mut self) -> Result<QueryExpr, QqlError> {
        self.expect(TokenKind::Recommend)?;
        self.expect_word("POSITIVE")?;
        let positive = self
            .parse_point_id_list()?
            .into_iter()
            .map(QueryInput::Point)
            .collect();
        let negative = if self.peek_word("NEGATIVE")? {
            self.advance()?;
            self.parse_point_id_list()?
                .into_iter()
                .map(QueryInput::Point)
                .collect()
        } else {
            Vec::new()
        };
        let strategy = if self.peek()?.kind == TokenKind::Strategy {
            self.advance()?;
            Some(self.parse_recommend_strategy()?)
        } else {
            None
        };
        Ok(QueryExpr::Recommend {
            positive,
            negative,
            strategy,
            using: None,
            prefetch: Vec::new(),
        })
    }

    fn parse_recommend_strategy(&mut self) -> Result<RecommendStrategy, QqlError> {
        let token = self.advance()?;
        if token.text.eq_ignore_ascii_case("average_vector") {
            Ok(RecommendStrategy::AverageVector)
        } else if token.text.eq_ignore_ascii_case("best_score") {
            Ok(RecommendStrategy::BestScore)
        } else if token.text.eq_ignore_ascii_case("sum_scores") {
            Ok(RecommendStrategy::SumScores)
        } else {
            Err(QqlError::validation(
                "QQL-VALIDATION-RECOMMEND-STRATEGY",
                alloc::format!("unknown recommend strategy '{}'", token.text),
                Some(token.span),
            ))
        }
    }

    fn parse_context_pairs(&mut self) -> Result<Vec<ContextPair>, QqlError> {
        self.expect(TokenKind::Lparen)?;
        let mut pairs = Vec::new();
        loop {
            self.expect_word("POSITIVE")?;
            let positive = self.parse_query_input()?;
            self.expect_word("NEGATIVE")?;
            let negative = self.parse_query_input()?;
            pairs.push(ContextPair { positive, negative });
            if self.peek()?.kind != TokenKind::Comma {
                break;
            }
            self.advance()?;
        }
        self.expect(TokenKind::Rparen)?;
        if pairs.is_empty() {
            return Err(QqlError::parse(
                "QQL-PARSE-CONTEXT",
                "CONTEXT requires at least one positive/negative pair",
                self.peek()?.span,
            ));
        }
        Ok(pairs)
    }

    fn parse_discover(&mut self) -> Result<QueryExpr, QqlError> {
        self.expect(TokenKind::Discover)?;
        self.expect(TokenKind::Target)?;
        let target = self.parse_query_input()?;
        self.expect(TokenKind::Context)?;
        let context = self.parse_context_pairs()?;
        Ok(QueryExpr::Discover {
            target,
            context,
            using: None,
            prefetch: Vec::new(),
        })
    }

    fn parse_relevance_feedback(&mut self) -> Result<QueryExpr, QqlError> {
        self.expect(TokenKind::Relevance)?;
        self.expect(TokenKind::Feedback)?;
        self.expect(TokenKind::Target)?;
        let target = self.parse_query_input()?;
        self.expect(TokenKind::Feedback)?;
        self.expect(TokenKind::Lparen)?;
        let mut feedback = Vec::new();
        loop {
            self.expect(TokenKind::Lparen)?;
            let example = self.parse_query_input()?;
            self.expect(TokenKind::Comma)?;
            let score = self.parse_numeric_literal()?;
            self.expect(TokenKind::Rparen)?;
            feedback.push(FeedbackItem { example, score });
            if self.peek()?.kind != TokenKind::Comma {
                break;
            }
            self.advance()?;
        }
        self.expect(TokenKind::Rparen)?;
        self.expect(TokenKind::Strategy)?;
        self.expect_word("NAIVE")?;
        let values = self.parse_config_block()?;
        let strategy = FeedbackStrategy {
            a: required_number(&values, "a")?,
            b: required_number(&values, "b")?,
            c: required_number(&values, "c")?,
        };
        Ok(QueryExpr::RelevanceFeedback {
            target,
            feedback,
            strategy,
            using: None,
            prefetch: Vec::new(),
        })
    }

    fn parse_mmr(&mut self) -> Result<QueryExpr, QqlError> {
        self.expect_word("MMR")?;
        let input = self.parse_query_input()?;
        self.expect_word("DIVERSITY")?;
        let diversity = self.parse_numeric_literal()?;
        if !(0.0..=1.0).contains(&diversity) {
            return Err(QqlError::validation(
                "QQL-VALIDATION-MMR",
                "MMR diversity must be between 0 and 1",
                Some(self.peek()?.span),
            ));
        }
        self.expect_word("CANDIDATES")?;
        let candidates = self.parse_positive_u64("MMR candidates")?;
        Ok(QueryExpr::Nearest {
            input,
            using: None,
            prefetch: Vec::new(),
            mmr: Some(Box::new(MmrConfig {
                diversity,
                candidates,
            })),
        })
    }

    fn parse_hybrid(&mut self) -> Result<QueryExpr, QqlError> {
        self.expect(TokenKind::Hybrid)?;
        let input = self.parse_query_input()?;
        let QueryInput::Text { text, model } = input else {
            return Err(QqlError::validation(
                "QQL-VALIDATION-HYBRID",
                "HYBRID shorthand requires a text input",
                Some(self.peek()?.span),
            ));
        };
        let dense_vector = if self.peek()?.kind == TokenKind::Dense {
            self.advance()?;
            Some(self.parse_identifier()?)
        } else {
            None
        };
        let sparse_vector = if self.peek()?.kind == TokenKind::Sparse {
            self.advance()?;
            Some(self.parse_identifier()?)
        } else {
            None
        };
        let fusion = if self.peek()?.kind == TokenKind::Fusion {
            self.advance()?;
            self.parse_fusion_method()?
        } else {
            FusionMethod::Rrf
        };
        Ok(QueryExpr::Hybrid {
            text,
            model,
            dense_vector,
            sparse_vector,
            fusion,
        })
    }

    fn parse_rerank(&mut self) -> Result<QueryExpr, QqlError> {
        self.expect(TokenKind::Rerank)?;
        let input = if self.peek_word("TEXT")? {
            self.advance()?;
            QueryInput::Text {
                text: self.parse_string()?,
                model: None,
            }
        } else {
            self.parse_query_input()?
        };
        let model = self.parse_required_model_string()?;
        Ok(QueryExpr::Rerank {
            input,
            model,
            using: String::new(),
            prefetch: Vec::new(),
        })
    }

    fn parse_fusion_method(&mut self) -> Result<FusionMethod, QqlError> {
        let token = self.advance()?;
        if token.text.eq_ignore_ascii_case("rrf") {
            Ok(FusionMethod::Rrf)
        } else if token.text.eq_ignore_ascii_case("dbsf") {
            Ok(FusionMethod::Dbsf)
        } else {
            Err(QqlError::validation(
                "QQL-VALIDATION-FUSION",
                "fusion method must be RRF or DBSF",
                Some(token.span),
            ))
        }
    }

    fn parse_prefetch_list(&mut self) -> Result<Vec<Prefetch>, QqlError> {
        self.expect(TokenKind::Lparen)?;
        let mut prefetch = Vec::new();
        if self.peek()?.kind == TokenKind::Rparen {
            return Err(QqlError::parse(
                "QQL-PARSE-PREFETCH",
                "PREFETCH cannot be empty",
                self.peek()?.span,
            ));
        }
        loop {
            let source = if self.peek()?.kind == TokenKind::Query {
                self.advance()?;
                PrefetchSource::Query(Box::new(self.parse_query_stmt(false, Vec::new())?))
            } else {
                PrefetchSource::Cte(self.parse_identifier()?)
            };
            let filter = if self.peek()?.kind == TokenKind::Where {
                self.advance()?;
                Some(Box::new(self.parse_filter_expr()?))
            } else {
                None
            };
            let score_threshold = if self.peek()?.kind == TokenKind::Score {
                self.advance()?;
                self.expect(TokenKind::Threshold)?;
                Some(self.parse_numeric_literal()?)
            } else {
                None
            };
            let lookup = if self.peek()?.kind == TokenKind::Lookup {
                self.advance()?;
                self.expect(TokenKind::From)?;
                let collection = self.parse_identifier()?;
                let vector = if self.peek()?.kind == TokenKind::Vector {
                    self.advance()?;
                    Some(self.parse_identifier()?)
                } else {
                    None
                };
                Some(LookupSpec { collection, vector })
            } else {
                None
            };
            prefetch.push(Prefetch {
                source,
                filter,
                score_threshold,
                lookup,
            });
            if self.peek()?.kind != TokenKind::Comma {
                break;
            }
            self.advance()?;
        }
        self.expect(TokenKind::Rparen)?;
        Ok(prefetch)
    }

    fn peek_word(&mut self, word: &str) -> Result<bool, QqlError> {
        let token = self.peek()?;
        Ok(
            (token.kind == TokenKind::Identifier || token.kind == TokenKind::String)
                && ascii_equal(token.text, word),
        )
    }

    fn expect_word(&mut self, word: &str) -> Result<(), QqlError> {
        if self.peek_word(word)? {
            self.advance()?;
            Ok(())
        } else {
            Err(QqlError::parse(
                "QQL-PARSE-EXPECTED",
                alloc::format!("expected {} but got '{}'", word, self.peek()?.text),
                self.peek()?.span,
            ))
        }
    }

    fn is_query_clause_start(&mut self) -> Result<bool, QqlError> {
        Ok(matches!(
            self.peek()?.kind,
            TokenKind::Using
                | TokenKind::Prefetch
                | TokenKind::Where
                | TokenKind::Params
                | TokenKind::Score
                | TokenKind::Group
                | TokenKind::With
                | TokenKind::Limit
                | TokenKind::Offset
        ))
    }
}

fn attach_pipeline(
    expression: &mut QueryExpr,
    using: Option<String>,
    prefetch: Vec<Prefetch>,
    span: Span,
) -> Result<(), QqlError> {
    match expression {
        QueryExpr::Nearest {
            using: target,
            prefetch: nested,
            ..
        }
        | QueryExpr::Recommend {
            using: target,
            prefetch: nested,
            ..
        }
        | QueryExpr::Context {
            using: target,
            prefetch: nested,
            ..
        }
        | QueryExpr::Discover {
            using: target,
            prefetch: nested,
            ..
        }
        | QueryExpr::RelevanceFeedback {
            using: target,
            prefetch: nested,
            ..
        } => {
            *target = using;
            *nested = prefetch;
        }
        QueryExpr::Fusion {
            prefetch: nested, ..
        } => {
            reject_using(using, span)?;
            if prefetch.is_empty() {
                return Err(QqlError::validation(
                    "QQL-VALIDATION-FUSION-PREFETCH",
                    "FUSION requires PREFETCH",
                    Some(span),
                ));
            }
            *nested = prefetch;
        }
        QueryExpr::Formula {
            prefetch: nested, ..
        } => {
            reject_using(using, span)?;
            *nested = prefetch;
        }
        QueryExpr::Rerank {
            using: target,
            prefetch: nested,
            ..
        } => {
            let using = using.ok_or_else(|| {
                QqlError::validation(
                    "QQL-VALIDATION-RERANK-USING",
                    "RERANK requires USING <vector>",
                    Some(span),
                )
            })?;
            if prefetch.is_empty() {
                return Err(QqlError::validation(
                    "QQL-VALIDATION-RERANK-PREFETCH",
                    "RERANK requires PREFETCH",
                    Some(span),
                ));
            }
            *target = using;
            *nested = prefetch;
        }
        QueryExpr::Points { .. }
        | QueryExpr::OrderBy { .. }
        | QueryExpr::SampleRandom
        | QueryExpr::Hybrid { .. } => {
            reject_using(using, span)?;
            if !prefetch.is_empty() {
                return Err(QqlError::validation(
                    "QQL-VALIDATION-PREFETCH",
                    "this query expression does not accept PREFETCH",
                    Some(span),
                ));
            }
        }
    }
    Ok(())
}

fn reject_using(using: Option<String>, span: Span) -> Result<(), QqlError> {
    if using.is_some() {
        Err(QqlError::validation(
            "QQL-VALIDATION-USING",
            "this query expression does not accept USING",
            Some(span),
        ))
    } else {
        Ok(())
    }
}

fn validate_prefetch_references(
    expression: &QueryExpr,
    ctes: &[Cte],
    span: Span,
) -> Result<(), QqlError> {
    let prefetch = match expression {
        QueryExpr::Nearest { prefetch, .. }
        | QueryExpr::Recommend { prefetch, .. }
        | QueryExpr::Context { prefetch, .. }
        | QueryExpr::Discover { prefetch, .. }
        | QueryExpr::Fusion { prefetch, .. }
        | QueryExpr::Formula { prefetch, .. }
        | QueryExpr::RelevanceFeedback { prefetch, .. }
        | QueryExpr::Rerank { prefetch, .. } => prefetch,
        QueryExpr::Points { .. }
        | QueryExpr::OrderBy { .. }
        | QueryExpr::SampleRandom
        | QueryExpr::Hybrid { .. } => return Ok(()),
    };
    for item in prefetch {
        if let PrefetchSource::Cte(name) = &item.source {
            if !ctes.iter().any(|cte| cte.name.eq_ignore_ascii_case(name)) {
                return Err(QqlError::validation(
                    "QQL-VALIDATION-PREFETCH-CTE",
                    alloc::format!("PREFETCH references unknown CTE '{}'", name),
                    Some(span),
                ));
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn validate_common_clauses(
    expression: &QueryExpr,
    filter: Option<&FilterExpr>,
    params: Option<&crate::ast::SearchParams>,
    score_threshold: Option<f64>,
    group: Option<&GroupSpec>,
    limit: Option<u64>,
    offset: Option<u64>,
    span: Span,
) -> Result<(), QqlError> {
    if let Some(score) = score_threshold {
        if !score.is_finite() {
            return Err(QqlError::validation(
                "QQL-VALIDATION-SCORE",
                "score threshold must be finite",
                Some(span),
            ));
        }
    }
    if matches!(expression, QueryExpr::Points { .. })
        && (filter.is_some()
            || params.is_some()
            || score_threshold.is_some()
            || group.is_some()
            || limit.is_some()
            || offset.is_some())
    {
        return Err(QqlError::validation(
            "QQL-VALIDATION-POINTS-CLAUSE",
            "QUERY POINTS accepts only output selectors",
            Some(span),
        ));
    }
    Ok(())
}

fn required_number(values: &[(String, crate::ast::Value)], key: &str) -> Result<f64, QqlError> {
    values
        .iter()
        .find(|(candidate, _)| candidate.eq_ignore_ascii_case(key))
        .and_then(|(_, value)| match value {
            crate::ast::Value::Int(value) => Some(*value as f64),
            crate::ast::Value::Float(value) => Some(*value),
            _ => None,
        })
        .ok_or_else(|| {
            QqlError::validation(
                "QQL-VALIDATION-FEEDBACK-STRATEGY",
                alloc::format!("feedback strategy requires numeric '{}'", key),
                None,
            )
        })
}
