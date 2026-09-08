//! Prefetch / hybrid / rerank candidate-stage lowering.

use crate::filter::top_level_filter;
use crate::params::lower_search_params;
use crate::query::{lower_query_expr, lower_query_input};
use crate::semantic::PlanQueryInput;
use crate::types::*;
use qql_core::ast::{FusionMethod, PrefetchSource, QueryExpr, QueryInput, QueryStmt};
use qql_core::error::QqlError;

/// Lower a `PREFETCH` clause with no CTE context (inline query sources only).
pub fn lower_prefetch(prefetch: &qql_core::ast::Prefetch) -> Result<PrefetchRequest, QqlError> {
    lower_prefetch_with_ctes(prefetch, &[])
}

/// Lower a `PREFETCH` clause, resolving `CTE` sources against `ctes`.
pub fn lower_prefetch_with_ctes(
    prefetch: &qql_core::ast::Prefetch,
    ctes: &[qql_core::ast::Cte],
) -> Result<PrefetchRequest, QqlError> {
    let source_query: &QueryStmt = match &prefetch.source {
        PrefetchSource::Cte(name) => ctes
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(name))
            .map(|c| c.query.as_ref())
            .ok_or_else(|| {
                QqlError::validation(
                    "QQL-PLAN-PREFETCH-CTE",
                    format!("PREFETCH references unknown CTE '{name}'"),
                    None,
                )
            })?,
        PrefetchSource::Query(query) => query.as_ref(),
    };

    // PrefetchRequest cannot represent grouping (no group_by / group_size
    // fields). Reject explicitly instead of silently dropping the group.
    if source_query.group.is_some() {
        return Err(QqlError::validation(
            "QQL-PLAN-PREFETCH-GROUP",
            "GROUP BY is not supported inside PREFETCH",
            None,
        ));
    }

    let (query, using, nested_prefetch, source_filter, source_params, source_limit, source_score) = {
        let (variant, using, nested) = build_query_with_prefetch(source_query)?;
        (
            Some(variant),
            using,
            if nested.is_empty() {
                None
            } else {
                Some(nested)
            },
            source_query.filter.as_ref().map(|f| top_level_filter(f)),
            match source_query.params.as_ref() {
                Some(p) => lower_search_params(p)?,
                None => None,
            },
            source_query.page.limit,
            source_query.score_threshold,
        )
    };

    // Outer PREFETCH WHERE / SCORE THRESHOLD override source-query values when set.
    let filter = prefetch
        .filter
        .as_ref()
        .map(|f| top_level_filter(f))
        .or(source_filter);
    let score_threshold = prefetch.score_threshold.or(source_score);

    Ok(PrefetchRequest {
        query,
        using,
        filter,
        params: source_params,
        score_threshold,
        limit: source_limit,
        lookup_from: prefetch.lookup.as_ref().map(|l| LookupRequest {
            collection: l.collection.clone(),
            vector: l.vector.clone(),
        }),
        prefetch: nested_prefetch,
    })
}

pub(crate) fn build_query_with_prefetch(
    query: &QueryStmt,
) -> Result<(QueryVariant, Option<String>, Vec<PrefetchRequest>), QqlError> {
    match &query.expression {
        QueryExpr::Hybrid {
            text,
            model,
            dense_vector,
            sparse_vector,
            fusion,
            ..
        } => {
            let fusion_name = match fusion {
                FusionMethod::Rrf => "rrf",
                FusionMethod::Dbsf => "dbsf",
            };
            let candidates = match query.page.limit {
                Some(l) => l.checked_mul(10).ok_or_else(|| {
                    QqlError::validation(
                        "QQL-VALIDATION-LIMIT-OVERFLOW",
                        format!(
                            "hybrid query LIMIT {l} overflows the candidate limit \
                             (LIMIT * 10); reduce LIMIT"
                        ),
                        None,
                    )
                })?,
                None => 100,
            };

            let hybrid_params = match query.params.as_ref() {
                Some(p) => lower_search_params(p)?,
                None => None,
            };
            let dense_prefetch = PrefetchRequest {
                query: Some(QueryVariant::Nearest(NearestQuery {
                    nearest: build_text_input(text, model, dense_vector.as_deref()),
                    mmr: None,
                })),
                using: dense_vector.clone(),
                filter: query.filter.as_ref().map(|f| top_level_filter(f)),
                params: hybrid_params.clone(),
                score_threshold: query.score_threshold,
                limit: Some(candidates),
                lookup_from: None,
                prefetch: None,
            };
            let sparse_prefetch = PrefetchRequest {
                query: Some(QueryVariant::Nearest(NearestQuery {
                    nearest: build_text_input(text, model, sparse_vector.as_deref()),
                    mmr: None,
                })),
                using: sparse_vector.clone(),
                filter: query.filter.as_ref().map(|f| top_level_filter(f)),
                params: hybrid_params,
                score_threshold: query.score_threshold,
                limit: Some(candidates),
                lookup_from: None,
                prefetch: None,
            };
            let variant = if let Some(params) = &query.params {
                if params.rrf_k.is_some() || params.rrf_weights.is_some() {
                    QueryVariant::Rrf(RrfQuery {
                        rrf: RrfParams {
                            k: params.rrf_k,
                            weights: params.rrf_weights.clone(),
                        },
                    })
                } else {
                    QueryVariant::Fusion {
                        fusion: fusion_name.into(),
                    }
                }
            } else {
                QueryVariant::Fusion {
                    fusion: fusion_name.into(),
                }
            };
            Ok((variant, None, vec![dense_prefetch, sparse_prefetch]))
        }
        QueryExpr::Rerank {
            input,
            model: rerank_model,
            using,
            prefetch,
        } => {
            let using = using.as_ref().ok_or_else(|| {
                QqlError::validation(
                    "QQL-PLAN-RERANK-USING",
                    "RERANK requires USING vector name",
                    None,
                )
            })?;
            if using.name.is_empty() {
                return Err(QqlError::validation(
                    "QQL-PLAN-RERANK-USING",
                    "RERANK requires non-empty USING vector name",
                    None,
                ));
            }
            if prefetch.is_empty() {
                return Err(QqlError::validation(
                    "QQL-PLAN-RERANK-PREFETCH",
                    "RERANK requires at least one PREFETCH",
                    None,
                ));
            }
            let pf_requests: Vec<PrefetchRequest> = prefetch
                .iter()
                .map(|p| lower_prefetch_with_ctes(p, &query.ctes))
                .collect::<Result<_, _>>()?;
            let nearest_input = match input {
                QueryInput::Text { text, .. } => PlanQueryInput::Document {
                    text: text.clone(),
                    model: Some(rerank_model.clone()),
                },
                _ => lower_query_input(input),
            };
            Ok((
                QueryVariant::Nearest(NearestQuery {
                    nearest: nearest_input,
                    mmr: None,
                }),
                Some(using.name.clone()),
                pf_requests,
            ))
        }
        _ => {
            let mut variant = lower_query_expr(&query.expression)?;
            if let Some(params) = &query.params
                && (params.rrf_k.is_some() || params.rrf_weights.is_some())
                && let QueryVariant::Fusion { fusion } = &variant
                && fusion == "rrf"
            {
                variant = QueryVariant::Rrf(RrfQuery {
                    rrf: RrfParams {
                        k: params.rrf_k,
                        weights: params.rrf_weights.clone(),
                    },
                });
            }
            let using = expression_using(&query.expression).map(str::to_owned);
            if let QueryVariant::Nearest(nearest) = &mut variant
                && let PlanQueryInput::Document { model, .. } = &mut nearest.nearest
                && model.as_deref().unwrap_or("").is_empty()
            {
                *model = default_model_for_using(using.as_deref()).or_else(|| model.clone());
            }
            let prefetches = expression_prefetch(&query.expression);
            let pf_requests: Vec<PrefetchRequest> = prefetches
                .iter()
                .map(|p| lower_prefetch_with_ctes(p, &query.ctes))
                .collect::<Result<_, _>>()?;
            Ok((variant, using, pf_requests))
        }
    }
}

/// Single definition of the `USING bm25` model default: an unspecified
/// `USING bm25` target resolves to Qdrant's server-side BM25 model — the same
/// model `qql-embed`'s sparse pipeline is wire-compatible with. All lowering
/// sites must go through this helper so the default cannot drift.
pub(crate) fn default_model_for_using(using: Option<&str>) -> Option<String> {
    using
        .is_some_and(|u| u.eq_ignore_ascii_case("bm25"))
        .then(|| "Qdrant/bm25".to_string())
}

fn build_text_input(text: &str, model: &Option<String>, using: Option<&str>) -> PlanQueryInput {
    let resolved_model = match model {
        Some(m) if !m.is_empty() => Some(m.clone()),
        _ => default_model_for_using(using).or_else(|| model.clone()),
    };
    PlanQueryInput::Document {
        text: text.to_string(),
        model: resolved_model,
    }
}

fn expression_using(expr: &QueryExpr) -> Option<&str> {
    match expr {
        QueryExpr::Nearest { using, .. }
        | QueryExpr::Recommend { using, .. }
        | QueryExpr::Context { using, .. }
        | QueryExpr::Discover { using, .. }
        | QueryExpr::RelevanceFeedback { using, .. }
        | QueryExpr::Rerank { using, .. } => using.as_ref().map(|target| target.name.as_str()),
        _ => None,
    }
}

fn expression_prefetch(expr: &QueryExpr) -> &[qql_core::ast::Prefetch] {
    match expr {
        QueryExpr::Nearest { prefetch, .. }
        | QueryExpr::Recommend { prefetch, .. }
        | QueryExpr::Context { prefetch, .. }
        | QueryExpr::Discover { prefetch, .. }
        | QueryExpr::Fusion { prefetch, .. }
        | QueryExpr::Formula { prefetch, .. }
        | QueryExpr::RelevanceFeedback { prefetch, .. }
        | QueryExpr::Rerank { prefetch, .. }
        | QueryExpr::CrossRerank { prefetch, .. } => prefetch,
        _ => &[],
    }
}

pub(crate) fn extract_lookup_from(query: &QueryStmt) -> Option<LookupRequest> {
    for pf in expression_prefetch(&query.expression) {
        if let Some(l) = &pf.lookup {
            return Some(LookupRequest {
                collection: l.collection.clone(),
                vector: l.vector.clone(),
            });
        }
    }
    None
}
