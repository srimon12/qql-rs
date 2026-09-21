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

/// Owned variant of [`lower_prefetch_with_ctes`]: moves inline query bodies
/// and lookup strings instead of cloning them.
///
/// CTE-referenced sources still clone the referenced CTE body: a CTE may be
/// referenced by several prefetches, so it cannot be moved out once. The hot
/// path (inline `PREFETCH (QUERY …)` stages carrying vectors) moves with zero
/// copies; the CTE-reference path is cold (no vectors in the reference itself).
pub fn lower_prefetch_owned(
    prefetch: qql_core::ast::Prefetch,
    ctes: &[qql_core::ast::Cte],
) -> Result<PrefetchRequest, QqlError> {
    lower_prefetch_with_ctes_owned(prefetch, ctes)
}

/// Owned prefetch lowering over borrowed CTE bodies (see
/// [`lower_prefetch_owned`]).
pub fn lower_prefetch_with_ctes_owned(
    prefetch: qql_core::ast::Prefetch,
    ctes: &[qql_core::ast::Cte],
) -> Result<PrefetchRequest, QqlError> {
    use crate::filter::top_level_filter_owned;
    use crate::params::lower_search_params_owned;

    let source_query: qql_core::ast::QueryStmt = match prefetch.source {
        PrefetchSource::Cte(name) => {
            let cte = ctes.iter().find(|c| c.name.eq_ignore_ascii_case(&name));
            let Some(cte) = cte else {
                return Err(QqlError::validation(
                    "QQL-PLAN-PREFETCH-CTE",
                    format!("PREFETCH references unknown CTE '{name}'"),
                    None,
                ));
            };
            // Cold: shared CTE body cloned once per referencing prefetch.
            (*cte.query).clone()
        }
        PrefetchSource::Query(query) => *query,
    };

    if source_query.group.is_some() {
        return Err(QqlError::validation(
            "QQL-PLAN-PREFETCH-GROUP",
            "GROUP BY is not supported inside PREFETCH",
            None,
        ));
    }

    let qql_core::ast::QueryStmt {
        expression,
        filter: source_filter_ast,
        params: source_params_ast,
        score_threshold: source_score,
        page,
        ctes: source_ctes,
        ..
    } = source_query;
    let source_limit = page.limit;
    let (variant, using, nested) = build_query_with_prefetch_owned_parts(
        expression,
        source_ctes,
        source_filter_ast.as_deref(),
        source_params_ast.as_ref(),
        source_limit,
        source_score,
    )?;
    let source_filter = source_filter_ast
        .map(|f| top_level_filter_owned(*f))
        .transpose()?;
    let source_params = source_params_ast
        .map(lower_search_params_owned)
        .transpose()?
        .flatten();

    // Outer PREFETCH WHERE / SCORE THRESHOLD override source-query values when set.
    let filter = prefetch
        .filter
        .map(|f| top_level_filter_owned(*f))
        .transpose()?
        .or(source_filter);
    let score_threshold = prefetch.score_threshold.or(source_score);

    Ok(PrefetchRequest {
        query: Some(variant),
        using,
        filter,
        params: source_params,
        score_threshold,
        limit: source_limit,
        lookup_from: prefetch.lookup.map(|l| LookupRequest {
            collection: l.collection,
            vector: l.vector,
            shard_key: l.shard_key.map(crate::semantic::PlanShardKey::from),
        }),
        prefetch: if nested.is_empty() {
            None
        } else {
            Some(nested)
        },
    })
}

/// Owned variant of [`build_query_with_prefetch`]: consumes the expression and
/// CTE vec, moving every vector buffer.
///
/// `filter` / `params` / `limit` / `score` arrive as borrowed views because the
/// caller still owns them for the outer request: hybrid arms lower the filter
/// via the borrowed path (small-string clones only — filters carry no vector
/// buffers), while all vector inputs move out of `expression`.
pub fn build_query_with_prefetch_owned_parts(
    expression: QueryExpr,
    ctes: Vec<qql_core::ast::Cte>,
    filter: Option<&qql_core::ast::FilterExpr>,
    params: Option<&qql_core::ast::SearchParams>,
    limit: Option<u64>,
    score_threshold: Option<f64>,
) -> Result<(QueryVariant, Option<String>, Vec<PrefetchRequest>), QqlError> {
    use crate::query::lower_query_expr_owned;
    match expression {
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
            let candidates = match limit {
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

            let hybrid_params = match params {
                Some(p) => lower_search_params(p)?,
                None => None,
            };
            // Small-metadata clones only (no vector buffers in filters/params).
            let dense_filter = filter.map(top_level_filter).transpose()?;
            let sparse_filter = filter.map(top_level_filter).transpose()?;
            let dense_prefetch = PrefetchRequest {
                query: Some(QueryVariant::Nearest(NearestQuery {
                    nearest: build_text_input_owned(
                        text.clone(),
                        model.clone(),
                        dense_vector.as_deref(),
                    ),
                    mmr: None,
                })),
                using: dense_vector.clone(),
                filter: dense_filter,
                params: hybrid_params.clone(),
                score_threshold,
                limit: Some(candidates),
                lookup_from: None,
                prefetch: None,
            };
            let sparse_prefetch = PrefetchRequest {
                query: Some(QueryVariant::Nearest(NearestQuery {
                    nearest: build_text_input_owned(text, model, sparse_vector.as_deref()),
                    mmr: None,
                })),
                using: sparse_vector,
                filter: sparse_filter,
                params: hybrid_params,
                score_threshold,
                limit: Some(candidates),
                lookup_from: None,
                prefetch: None,
            };
            let variant = if let Some(params) = params {
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
            let using = using.ok_or_else(|| {
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
                .into_iter()
                .map(|p| lower_prefetch_with_ctes_owned(p, &ctes))
                .collect::<Result<_, _>>()?;
            let nearest_input = match input {
                QueryInput::Text { text, .. } => PlanQueryInput::Document {
                    text,
                    model: Some(rerank_model),
                    options: None,
                },
                other => crate::query::lower_query_input_owned(other),
            };
            Ok((
                QueryVariant::Nearest(NearestQuery {
                    nearest: nearest_input,
                    mmr: None,
                }),
                Some(using.name),
                pf_requests,
            ))
        }
        other => {
            let using_name = expression_using(&other).map(str::to_owned);
            let (expr_body, prefetch_asts) = split_prefetch_owned(other);
            let mut variant = lower_query_expr_owned(expr_body)?;
            if let Some(params) = params
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
            let using = using_name;
            if let QueryVariant::Nearest(nearest) = &mut variant
                && let PlanQueryInput::Document { model, .. } = &mut nearest.nearest
                && model.as_deref().unwrap_or("").is_empty()
            {
                *model = default_model_for_using(using.as_deref()).or_else(|| model.clone());
            }
            let pf_requests: Vec<PrefetchRequest> = prefetch_asts
                .into_iter()
                .map(|p| lower_prefetch_with_ctes_owned(p, &ctes))
                .collect::<Result<_, _>>()?;
            Ok((variant, using, pf_requests))
        }
    }
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
            source_query
                .filter
                .as_ref()
                .map(|f| top_level_filter(f))
                .transpose()?,
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
        .transpose()?
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
            shard_key: l.shard_key.as_ref().map(PlanShardKey::from),
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
                filter: query
                    .filter
                    .as_ref()
                    .map(|f| top_level_filter(f))
                    .transpose()?,
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
                filter: query
                    .filter
                    .as_ref()
                    .map(|f| top_level_filter(f))
                    .transpose()?,
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
                    options: None,
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
        options: None,
    }
}

/// Owned variant of [`build_text_input`]: moves text / model strings.
fn build_text_input_owned(
    text: String,
    model: Option<String>,
    using: Option<&str>,
) -> PlanQueryInput {
    let resolved_model = match model {
        Some(m) if !m.is_empty() => Some(m),
        other => default_model_for_using(using).or(other),
    };
    PlanQueryInput::Document {
        text,
        model: resolved_model,
        options: None,
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
                shard_key: l.shard_key.as_ref().map(PlanShardKey::from),
            });
        }
    }
    None
}

/// Owned lookup scan over a borrowed expression: clones only the two small
/// routing strings of the first lookup found (no vector buffers involved).
pub(crate) fn extract_lookup_from_owned(expr: &QueryExpr) -> Option<LookupRequest> {
    for pf in expression_prefetch(expr) {
        if let Some(l) = &pf.lookup {
            return Some(LookupRequest {
                collection: l.collection.clone(),
                vector: l.vector.clone(),
                shard_key: l.shard_key.as_ref().map(PlanShardKey::from),
            });
        }
    }
    None
}

/// Split an owned expression into its body (prefetch vec emptied) plus the
/// moved-out prefetch stages. `lower_query_expr_owned` ignores prefetch
/// fields, so the emptied body lowers identically while stages move.
fn split_prefetch_owned(expr: QueryExpr) -> (QueryExpr, Vec<qql_core::ast::Prefetch>) {
    use qql_core::ast::Prefetch;
    fn take(prefetch: Vec<Prefetch>) -> Vec<Prefetch> {
        prefetch
    }
    match expr {
        QueryExpr::Nearest {
            input,
            using,
            prefetch,
            mmr,
        } => {
            let stages = take(prefetch);
            (
                QueryExpr::Nearest {
                    input,
                    using,
                    prefetch: Vec::new(),
                    mmr,
                },
                stages,
            )
        }
        QueryExpr::Recommend {
            positive,
            negative,
            strategy,
            using,
            prefetch,
        } => {
            let stages = take(prefetch);
            (
                QueryExpr::Recommend {
                    positive,
                    negative,
                    strategy,
                    using,
                    prefetch: Vec::new(),
                },
                stages,
            )
        }
        QueryExpr::Context {
            pairs,
            using,
            prefetch,
        } => {
            let stages = take(prefetch);
            (
                QueryExpr::Context {
                    pairs,
                    using,
                    prefetch: Vec::new(),
                },
                stages,
            )
        }
        QueryExpr::Discover {
            target,
            context,
            using,
            prefetch,
        } => {
            let stages = take(prefetch);
            (
                QueryExpr::Discover {
                    target,
                    context,
                    using,
                    prefetch: Vec::new(),
                },
                stages,
            )
        }
        QueryExpr::Fusion { method, prefetch } => {
            let stages = take(prefetch);
            (
                QueryExpr::Fusion {
                    method,
                    prefetch: Vec::new(),
                },
                stages,
            )
        }
        QueryExpr::Formula {
            expression,
            defaults,
            prefetch,
        } => {
            let stages = take(prefetch);
            (
                QueryExpr::Formula {
                    expression,
                    defaults,
                    prefetch: Vec::new(),
                },
                stages,
            )
        }
        QueryExpr::RelevanceFeedback {
            target,
            feedback,
            strategy,
            using,
            prefetch,
        } => {
            let stages = take(prefetch);
            (
                QueryExpr::RelevanceFeedback {
                    target,
                    feedback,
                    strategy,
                    using,
                    prefetch: Vec::new(),
                },
                stages,
            )
        }
        QueryExpr::Rerank {
            input,
            model,
            using,
            prefetch,
        } => {
            let stages = take(prefetch);
            (
                QueryExpr::Rerank {
                    input,
                    model,
                    using,
                    prefetch: Vec::new(),
                },
                stages,
            )
        }
        QueryExpr::CrossRerank {
            query,
            model,
            field,
            prefetch,
            query_param,
        } => {
            let stages = take(prefetch);
            (
                QueryExpr::CrossRerank {
                    query,
                    model,
                    field,
                    prefetch: Vec::new(),
                    query_param,
                },
                stages,
            )
        }
        other => (other, Vec::new()),
    }
}
