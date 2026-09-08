pub(crate) mod expr;
pub(crate) mod expr_advanced;
pub(crate) mod expr_input;
pub(crate) mod pipeline;

use super::{AstLowerer, ascii_equal};
use crate::ast::{
    Cte, GroupSpec, PageSpec, QueryCollection, QueryOutput, QueryStmt, Stmt, VectorKind,
    VectorTarget,
};
use crate::error::QqlError;
use crate::token::TokenKind;
use alloc::boxed::Box;
use alloc::vec::Vec;
use pipeline::{
    attach_pipeline, expand_using_hybrid, validate_common_clauses, validate_prefetch_references,
};

impl<'a> AstLowerer<'a> {
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

        // USING HYBRID [DENSE n] [SPARSE n] [FUSION …]  — expands to QueryExpr::Hybrid
        // USING <name> [AS DENSE|SPARSE|MULTI]         — single named-vector target
        let using = if self.peek()?.kind == TokenKind::Using {
            self.advance()?;
            if self.peek()?.kind == TokenKind::Hybrid {
                let hybrid_using = self.parse_hybrid_using()?;
                expand_using_hybrid(&mut expression, hybrid_using, expression_span)?;
                None
            } else {
                let name = self.parse_identifier()?;
                let (kind, multi) = if self.peek()?.kind == TokenKind::As {
                    self.advance()?;
                    match self.peek()?.kind {
                        TokenKind::Dense => {
                            self.advance()?;
                            (Some(VectorKind::Dense), false)
                        }
                        TokenKind::Sparse => {
                            self.advance()?;
                            (Some(VectorKind::Sparse), false)
                        }
                        // Slight additive form: AS MULTI | AS MULTIVECTOR → dense multivector.
                        // Matched as bare words so we avoid a new reserved keyword.
                        _ if ascii_equal(self.peek()?.text, "MULTI")
                            || ascii_equal(self.peek()?.text, "MULTIVECTOR") =>
                        {
                            self.advance()?;
                            (Some(VectorKind::Dense), true)
                        }
                        _ => {
                            return Err(QqlError::parse(
                                "QQL-PARSE-VECTOR-KIND",
                                "USING <vector> AS requires DENSE, SPARSE, or MULTI",
                                self.peek()?.span,
                            ));
                        }
                    }
                } else {
                    (None, false)
                };
                Some(VectorTarget { name, kind, multi })
            }
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
            Some(self.parse_shard_key_atom()?)
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

        let (limit, limit_param, limit_span) = if self.peek()?.kind == TokenKind::Limit {
            self.advance()?;
            if let Some((param, span)) = self.parse_placeholder_param()? {
                (None, Some(param), Some(span))
            } else {
                (Some(self.parse_positive_u64("LIMIT")?), None, None)
            }
        } else {
            (None, None, None)
        };

        let (offset, offset_param, offset_span) = if self.peek()?.kind == TokenKind::Offset {
            self.advance()?;
            if let Some((param, span)) = self.parse_placeholder_param()? {
                (None, Some(param), Some(span))
            } else {
                (Some(self.parse_non_negative_u64("OFFSET")?), None, None)
            }
        } else {
            (None, None, None)
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
            page: PageSpec {
                limit,
                offset,
                limit_param,
                offset_param,
                limit_span,
                offset_span,
            },
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
            let query = self.parse_query_stmt(false, ctes.clone())?;
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
}
