use super::pipeline::HybridUsing;
use crate::ast::{
    ContextPair, FeedbackItem, FeedbackStrategy, FusionMethod, MmrConfig, QueryExpr, QueryInput,
    RecommendStrategy,
};
use crate::error::QqlError;
use crate::parser::{AstLowerer, ascii_equal};
use crate::token::TokenKind;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

impl<'a> AstLowerer<'a> {
    pub(crate) fn parse_recommend(&mut self) -> Result<QueryExpr, QqlError> {
        self.expect(TokenKind::Recommend)?;
        self.expect_word("POSITIVE")?;
        let positive = self.parse_recommend_examples()?;
        let negative = if self.peek_word("NEGATIVE")? {
            self.advance()?;
            self.parse_recommend_examples()?
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

    /// Parse a `RECOMMEND` example list.
    ///
    /// Bare strings and integers keep their historical meaning of point IDs;
    /// every other spelling (`VECTOR […]`, `TEXT '…'`, `POINT n`, `?`, `:`)
    /// is a full query input, mirroring `fmt::render_recommend_input`.
    fn parse_recommend_examples(&mut self) -> Result<Vec<QueryInput>, QqlError> {
        self.expect(TokenKind::Lparen)?;
        let mut examples = Vec::new();
        if self.peek()?.kind == TokenKind::Rparen {
            return Err(QqlError::parse(
                "QQL-PARSE-POINT-IDS",
                "recommend example list cannot be empty",
                self.peek()?.span,
            ));
        }
        loop {
            let example = match self.peek()?.kind {
                TokenKind::String | TokenKind::Integer => self
                    .parse_point_id("recommend example list")
                    .map(QueryInput::Point)?,
                _ => self.parse_query_input()?,
            };
            examples.push(example);
            if self.peek()?.kind != TokenKind::Comma {
                break;
            }
            self.advance()?;
        }
        self.expect(TokenKind::Rparen)?;
        Ok(examples)
    }

    pub(crate) fn parse_recommend_strategy(&mut self) -> Result<RecommendStrategy, QqlError> {
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

    pub(crate) fn parse_context_pairs(&mut self) -> Result<Vec<ContextPair>, QqlError> {
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
        Ok(pairs)
    }

    pub(crate) fn parse_discover(&mut self) -> Result<QueryExpr, QqlError> {
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

    pub(crate) fn parse_relevance_feedback(&mut self) -> Result<QueryExpr, QqlError> {
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
        let strategy = self.parse_feedback_strategy()?;
        Ok(QueryExpr::RelevanceFeedback {
            target,
            feedback,
            strategy,
            using: None,
            prefetch: Vec::new(),
        })
    }

    fn parse_feedback_strategy(&mut self) -> Result<FeedbackStrategy, QqlError> {
        self.expect(TokenKind::Lparen)?;
        let a = self.parse_feedback_param("A")?;
        self.feedback_separator()?;
        let b = self.parse_feedback_param("B")?;
        self.feedback_separator()?;
        let c = self.parse_feedback_param("C")?;
        if self.peek()?.kind == TokenKind::Comma {
            return Err(QqlError::parse(
                "QQL-PARSE-FEEDBACK-STRATEGY",
                "feedback strategy accepts exactly three parameters (a, b, c) in order",
                self.peek()?.span,
            ));
        }
        self.expect(TokenKind::Rparen)?;
        Ok(FeedbackStrategy { a, b, c })
    }

    fn feedback_separator(&mut self) -> Result<(), QqlError> {
        let tok = self.peek()?;
        if tok.kind == TokenKind::Comma {
            self.advance()?;
            Ok(())
        } else {
            Err(QqlError::parse(
                "QQL-PARSE-FEEDBACK-STRATEGY",
                "feedback strategy expects exactly three parameters (a, b, c) in order",
                tok.span,
            ))
        }
    }

    fn parse_feedback_param(&mut self, name: &str) -> Result<f64, QqlError> {
        let key = self.peek()?;
        if !(key.is_keyword_or_identifier() && ascii_equal(key.text, name)) {
            return Err(QqlError::parse(
                "QQL-PARSE-FEEDBACK-STRATEGY",
                alloc::format!("feedback strategy expects '{name}' here (order: a, b, c)"),
                key.span,
            ));
        }
        self.advance()?;
        self.expect(TokenKind::Equals)?;
        self.parse_numeric_literal()
    }

    pub(crate) fn parse_mmr(&mut self) -> Result<QueryExpr, QqlError> {
        self.expect_word("MMR")?;
        let input = self.parse_query_input()?;
        self.expect_word("DIVERSITY")?;
        let div_tok = self.peek()?;
        let diversity = self.parse_numeric_literal()?;
        if !(0.0..=1.0).contains(&diversity) {
            return Err(QqlError::validation(
                "QQL-VALIDATION-MMR",
                "MMR diversity must be between 0 and 1",
                Some(div_tok.span),
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

    pub(crate) fn parse_hybrid(&mut self) -> Result<QueryExpr, QqlError> {
        self.expect(TokenKind::Hybrid)?;
        let input_tok = self.peek()?;
        let input = self.parse_query_input()?;
        let QueryInput::Text {
            text,
            model,
            text_param,
        } = input
        else {
            return Err(QqlError::validation(
                "QQL-VALIDATION-HYBRID",
                "HYBRID shorthand requires a text input",
                Some(input_tok.span),
            ));
        };
        let HybridUsing {
            dense_vector,
            sparse_vector,
            fusion,
        } = self.parse_hybrid_modifiers()?;
        Ok(QueryExpr::Hybrid {
            text,
            model,
            dense_vector,
            sparse_vector,
            fusion,
            text_param,
        })
    }

    pub(crate) fn parse_hybrid_using(&mut self) -> Result<HybridUsing, QqlError> {
        self.expect(TokenKind::Hybrid)?;
        self.parse_hybrid_modifiers()
    }

    fn parse_hybrid_modifiers(&mut self) -> Result<HybridUsing, QqlError> {
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
        Ok(HybridUsing {
            dense_vector,
            sparse_vector,
            fusion,
        })
    }

    pub(crate) fn parse_cross_rerank(&mut self) -> Result<QueryExpr, QqlError> {
        self.advance()?;
        self.expect(TokenKind::Rerank)?;
        let (query, query_param) = if self.peek_word("TEXT")? {
            self.advance()?;
            if self.peek()?.kind == TokenKind::Colon {
                self.advance()?;
                let name = self.parse_param_name()?;
                (String::new(), Some(alloc::format!(":{}", name)))
            } else if self.peek()?.kind == TokenKind::Question {
                self.advance()?;
                let idx = self.next_positional_param();
                (String::new(), Some(alloc::format!("?{}", idx)))
            } else {
                (self.parse_string()?, None)
            }
        } else if self.peek()?.kind == TokenKind::Colon {
            self.advance()?;
            let name = self.parse_param_name()?;
            (String::new(), Some(alloc::format!(":{}", name)))
        } else if self.peek()?.kind == TokenKind::Question {
            self.advance()?;
            let idx = self.next_positional_param();
            (String::new(), Some(alloc::format!("?{}", idx)))
        } else if self.peek()?.kind == TokenKind::String {
            (self.parse_string()?, None)
        } else {
            return Err(QqlError::parse(
                "QQL-PARSE-CROSS-RERANK",
                "CROSS RERANK requires TEXT '…' or a string query",
                self.peek()?.span,
            ));
        };
        let model = self.parse_required_model_string()?;
        let field = if self.peek()?.kind == TokenKind::On {
            self.advance()?;
            self.expect(TokenKind::Field)?;
            Some(self.parse_identifier()?)
        } else {
            None
        };
        Ok(QueryExpr::CrossRerank {
            query,
            model,
            field,
            prefetch: Vec::new(),
            query_param,
        })
    }

    pub(crate) fn parse_rerank(&mut self) -> Result<QueryExpr, QqlError> {
        self.expect(TokenKind::Rerank)?;
        let input = if self.peek_word("TEXT")? {
            self.advance()?;
            QueryInput::Text {
                text: self.parse_string()?,
                model: None,
                text_param: None,
            }
        } else if self.peek()?.kind == TokenKind::Vector {
            self.advance()?;
            self.parse_vector_value().map(QueryInput::Vector)?
        } else if self.peek_word("POINT")? {
            self.advance()?;
            self.parse_point_id("POINT").map(QueryInput::Point)?
        } else {
            return Err(QqlError::parse(
                "QQL-PARSE-RERANK",
                "RERANK input requires TEXT '…', VECTOR […], or POINT <id>",
                self.peek()?.span,
            ));
        };
        let model = self.parse_required_model_string()?;
        Ok(QueryExpr::Rerank {
            input,
            model,
            using: None,
            prefetch: Vec::new(),
        })
    }
}
