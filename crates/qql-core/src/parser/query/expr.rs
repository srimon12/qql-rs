use crate::ast::{OrderDirection, QueryExpr, Value};
use crate::error::QqlError;
use crate::parser::AstLowerer;
use crate::token::TokenKind;
use alloc::boxed::Box;
use alloc::vec::Vec;

impl<'a> AstLowerer<'a> {
    pub(crate) fn parse_query_expression(&mut self) -> Result<QueryExpr, QqlError> {
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
            let start_from = self.parse_optional_order_start()?;
            return Ok(QueryExpr::OrderBy {
                field,
                direction,
                start_from,
            });
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
            let expression = self.parse_formula_expr(crate::parser::formula::PRECEDENCE_LOWEST)?;
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
        // CROSS RERANK (pair scorer) before bare RERANK (late-interaction).
        if self.peek_word("CROSS")? {
            return self.parse_cross_rerank();
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

    /// Parse an optional `START FROM <value>` paging origin after `ORDER BY`.
    ///
    /// OpenAPI `OrderBy.start_from` is a starting payload value: an integer, a
    /// float, or a datetime string. Parameter placeholders (`:name` / `?`)
    /// bind like any other scalar. Anything else fails closed.
    pub(crate) fn parse_optional_order_start(&mut self) -> Result<Option<Value>, QqlError> {
        if self.peek()?.kind != TokenKind::Start {
            return Ok(None);
        }
        self.advance()?;
        self.expect(TokenKind::From)?;
        let span = self.peek()?.span;
        match self.parse_value()? {
            value @ (Value::Int(_) | Value::Float(_) | Value::Str(_)) => Ok(Some(value)),
            value @ (Value::Param(..) | Value::PositionalParam(..)) => Ok(Some(value)),
            _ => Err(QqlError::parse(
                "QQL-PARSE-ORDER-START",
                "ORDER BY START FROM requires an integer, float, datetime string, or placeholder",
                span,
            )),
        }
    }
}
