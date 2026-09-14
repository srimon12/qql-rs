use crate::ast::{
    Cte, FilterExpr, FusionMethod, GroupSpec, LookupSpec, Prefetch, PrefetchSource, QueryExpr,
    QueryInput, VectorTarget,
};
use crate::error::{QqlError, Span};
use crate::parser::{AstLowerer, ascii_equal};
use crate::token::TokenKind;
use alloc::string::String;
use alloc::vec::Vec;

#[derive(Debug)]
pub(crate) struct HybridUsing {
    pub(crate) dense_vector: Option<String>,
    pub(crate) sparse_vector: Option<String>,
    pub(crate) fusion: FusionMethod,
}

impl<'a> AstLowerer<'a> {
    pub(crate) fn parse_prefetch_list(&mut self) -> Result<Vec<Prefetch>, QqlError> {
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
                let shard_key = if self.peek()?.kind == TokenKind::Shard {
                    self.advance()?;
                    Some(self.parse_shard_key_atom()?)
                } else {
                    None
                };
                Some(LookupSpec {
                    collection,
                    vector,
                    shard_key,
                })
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

    pub(crate) fn peek_word(&mut self, word: &str) -> Result<bool, QqlError> {
        let token = self.peek()?;
        Ok(token.kind.is_keyword_or_identifier() && ascii_equal(token.text, word))
    }

    pub(crate) fn expect_word(&mut self, word: &str) -> Result<(), QqlError> {
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

    pub(crate) fn is_query_clause_start(&mut self) -> Result<bool, QqlError> {
        Ok(matches!(
            self.peek()?.kind,
            TokenKind::Using
                | TokenKind::Prefetch
                | TokenKind::Where
                | TokenKind::Shard
                | TokenKind::Params
                | TokenKind::Score
                | TokenKind::Group
                | TokenKind::With
                | TokenKind::Limit
                | TokenKind::Offset
        ))
    }
}

pub(crate) fn expand_using_hybrid(
    expression: &mut QueryExpr,
    hybrid: HybridUsing,
    span: Span,
) -> Result<(), QqlError> {
    match expression {
        QueryExpr::Nearest {
            input:
                QueryInput::Text {
                    text,
                    model,
                    text_param,
                    options,
                },
            using: None,
            prefetch,
            mmr: None,
        } if prefetch.is_empty() => {
            if !options.is_empty() {
                return Err(QqlError::validation(
                    "QQL-VALIDATION-HYBRID",
                    "USING HYBRID cannot carry OPTIONS; query the dense and sparse vectors separately",
                    Some(span),
                ));
            }
            *expression = QueryExpr::Hybrid {
                text: core::mem::take(text),
                model: model.take(),
                dense_vector: hybrid.dense_vector,
                sparse_vector: hybrid.sparse_vector,
                fusion: hybrid.fusion,
                text_param: text_param.take(),
            };
            Ok(())
        }
        QueryExpr::Nearest { mmr: Some(_), .. } => Err(QqlError::validation(
            "QQL-VALIDATION-HYBRID",
            "USING HYBRID cannot combine with MMR; use dense nearest with MMR or HYBRID without MMR",
            Some(span),
        )),
        QueryExpr::Hybrid { .. } => Err(QqlError::validation(
            "QQL-VALIDATION-HYBRID",
            "USING HYBRID is redundant with QUERY HYBRID; use one form only",
            Some(span),
        )),
        _ => Err(QqlError::validation(
            "QQL-VALIDATION-HYBRID",
            "USING HYBRID requires a text nearest query (QUERY TEXT '…' or QUERY '…')",
            Some(span),
        )),
    }
}

pub(crate) fn attach_pipeline(
    expression: &mut QueryExpr,
    using: Option<VectorTarget>,
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
            *target = Some(using);
            *nested = prefetch;
        }
        QueryExpr::CrossRerank {
            prefetch: nested, ..
        } => {
            // Pair scorer: no vector USING; PREFETCH supplies candidates.
            reject_using(using, span)?;
            if prefetch.is_empty() {
                return Err(QqlError::validation(
                    "QQL-VALIDATION-CROSS-RERANK-PREFETCH",
                    "CROSS RERANK requires PREFETCH (candidate stage)",
                    Some(span),
                ));
            }
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

pub(crate) fn reject_using(using: Option<VectorTarget>, span: Span) -> Result<(), QqlError> {
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

pub(crate) fn validate_prefetch_references(
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
        | QueryExpr::Rerank { prefetch, .. }
        | QueryExpr::CrossRerank { prefetch, .. } => prefetch,
        QueryExpr::Points { .. }
        | QueryExpr::OrderBy { .. }
        | QueryExpr::SampleRandom
        | QueryExpr::Hybrid { .. } => return Ok(()),
    };
    for item in prefetch {
        if let PrefetchSource::Cte(name) = &item.source
            && !ctes.iter().any(|cte| cte.name.eq_ignore_ascii_case(name))
        {
            return Err(QqlError::validation(
                "QQL-VALIDATION-PREFETCH-CTE",
                alloc::format!("PREFETCH references unknown CTE '{}'", name),
                Some(span),
            ));
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn validate_common_clauses(
    expression: &QueryExpr,
    filter: Option<&FilterExpr>,
    params: Option<&crate::ast::SearchParams>,
    score_threshold: Option<f64>,
    group: Option<&GroupSpec>,
    limit: Option<u64>,
    offset: Option<u64>,
    span: Span,
) -> Result<(), QqlError> {
    if let Some(score) = score_threshold
        && !score.is_finite()
    {
        return Err(QqlError::validation(
            "QQL-VALIDATION-SCORE",
            "score threshold must be finite",
            Some(span),
        ));
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
            "QUERY POINTS accepts only output selectors and SHARD",
            Some(span),
        ));
    }
    Ok(())
}
