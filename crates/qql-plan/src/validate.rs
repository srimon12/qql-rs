//! Plan-time query validation: USING kinds, RRF params, recommend average dims.

use qql_core::ast::{QueryExpr, QueryInput, VectorKind, VectorTarget, VectorValue};
use qql_core::error::QqlError;

pub(crate) fn validate_query_stmt(query: &qql_core::ast::QueryStmt) -> Result<(), QqlError> {
    for cte in &query.ctes {
        validate_query_stmt(&cte.query)?;
    }
    validate_query_expr(&query.expression)?;

    let has_rrf_params = query
        .params
        .as_ref()
        .is_some_and(|params| params.rrf_k.is_some() || params.rrf_weights.is_some());
    let accepts_rrf_params = matches!(
        &query.expression,
        QueryExpr::Fusion {
            method: qql_core::ast::FusionMethod::Rrf,
            ..
        } | QueryExpr::Hybrid {
            fusion: qql_core::ast::FusionMethod::Rrf,
            ..
        }
    );
    if has_rrf_params && !accepts_rrf_params {
        return Err(QqlError::validation(
            "QQL-PLAN-RRF-PARAMS",
            "rrf_k and rrf_weights are valid only with RRF fusion",
            None,
        ));
    }
    if let Some(weights) = query
        .params
        .as_ref()
        .and_then(|params| params.rrf_weights.as_ref())
    {
        let prefetch_count = match &query.expression {
            QueryExpr::Fusion { prefetch, .. }
            | QueryExpr::Rerank { prefetch, .. }
            | QueryExpr::CrossRerank { prefetch, .. } => prefetch.len(),
            QueryExpr::Hybrid { .. } => 2,
            _ => 0,
        };
        if prefetch_count > 0 && weights.len() != prefetch_count {
            return Err(QqlError::validation(
                "QQL-PLAN-RRF-WEIGHTS",
                format!(
                    "rrf_weights contains {} values but fusion has {} prefetches",
                    weights.len(),
                    prefetch_count
                ),
                None,
            ));
        }
    }
    Ok(())
}

pub(crate) fn validate_query_expr(expression: &QueryExpr) -> Result<(), QqlError> {
    validate_query_target_kinds(expression)?;
    validate_recommend_average_dims(expression)?;
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

    match expression {
        QueryExpr::Fusion { .. } if prefetch.is_empty() => {
            return Err(QqlError::validation(
                "QQL-PLAN-FUSION-PREFETCH",
                "FUSION requires at least one prefetch",
                None,
            ));
        }
        QueryExpr::Rerank { using: None, .. } => {
            return Err(QqlError::validation(
                "QQL-PLAN-RERANK-USING",
                "RERANK requires a non-empty USING vector name",
                None,
            ));
        }
        QueryExpr::Rerank { .. } if prefetch.is_empty() => {
            return Err(QqlError::validation(
                "QQL-PLAN-RERANK-PREFETCH",
                "RERANK requires at least one prefetch",
                None,
            ));
        }
        _ => {}
    }

    for item in prefetch {
        if let qql_core::ast::PrefetchSource::Query(query) = &item.source {
            validate_query_stmt(query)?;
        }
    }
    Ok(())
}

pub(crate) fn validate_query_target_kinds(expression: &QueryExpr) -> Result<(), QqlError> {
    let (target, inputs): (Option<&VectorTarget>, Vec<&QueryInput>) = match expression {
        QueryExpr::Nearest { input, using, .. } => (using.as_ref(), vec![input]),
        QueryExpr::Recommend {
            positive,
            negative,
            using,
            ..
        } => (
            using.as_ref(),
            positive.iter().chain(negative.iter()).collect(),
        ),
        QueryExpr::Context { pairs, using, .. } => (
            using.as_ref(),
            pairs
                .iter()
                .flat_map(|pair| [&pair.positive, &pair.negative])
                .collect(),
        ),
        QueryExpr::Discover {
            target,
            context,
            using,
            ..
        } => {
            let mut inputs = vec![target];
            inputs.extend(
                context
                    .iter()
                    .flat_map(|pair| [&pair.positive, &pair.negative]),
            );
            (using.as_ref(), inputs)
        }
        QueryExpr::RelevanceFeedback {
            target,
            feedback,
            using,
            ..
        } => {
            let mut inputs = vec![target];
            inputs.extend(feedback.iter().map(|item| &item.example));
            (using.as_ref(), inputs)
        }
        QueryExpr::Rerank { input, using, .. } => {
            if using
                .as_ref()
                .and_then(|target| target.kind)
                .is_some_and(|kind| kind != VectorKind::Dense)
            {
                return Err(query_kind_error("RERANK requires a dense vector target"));
            }
            (using.as_ref(), vec![input])
        }
        _ => return Ok(()),
    };

    let Some(target_kind) = target.and_then(|target| target.kind) else {
        return Ok(());
    };
    for input in inputs {
        let input_kind = match input {
            QueryInput::Vector(VectorValue::Dense(_) | VectorValue::MultiDense(_)) => {
                Some(VectorKind::Dense)
            }
            QueryInput::Vector(VectorValue::Sparse { .. }) => Some(VectorKind::Sparse),
            QueryInput::Vector(
                VectorValue::Document { .. }
                | VectorValue::Image { .. }
                | VectorValue::Object { .. },
            )
            | QueryInput::Vector(VectorValue::Param(..) | VectorValue::PositionalParam(..))
            | QueryInput::Text { .. }
            | QueryInput::Image { .. }
            | QueryInput::Object { .. }
            | QueryInput::Point(_)
            | QueryInput::Param(..)
            | QueryInput::PositionalParam(..) => None,
        };
        if input_kind.is_some_and(|kind| kind != target_kind) {
            return Err(query_kind_error(
                "query input vector type does not match the USING vector kind",
            ));
        }
    }
    Ok(())
}

/// The recommend `average_vector` strategy (also the server default) folds all
/// positive/negative examples into a single query vector, so every inline
/// example vector must share the same shape. Upstream Qdrant rejects
/// mismatched dimensions (#10374); we fail fast at plan time. Point-ID / TEXT
/// examples have no known shape here and are skipped (resolved later).
pub(crate) fn validate_recommend_average_dims(expression: &QueryExpr) -> Result<(), QqlError> {
    let QueryExpr::Recommend {
        positive,
        negative,
        strategy,
        ..
    } = expression
    else {
        return Ok(());
    };
    // Only average_vector requires a shared shape; best_score / sum_scores
    // score each example independently.
    if matches!(
        strategy,
        Some(qql_core::ast::RecommendStrategy::BestScore)
            | Some(qql_core::ast::RecommendStrategy::SumScores)
    ) {
        return Ok(());
    }

    // Dense → (1, dim); MultiDense → (rows, row dim). Ragged rows have no
    // single dimension, so their shape is unknown here — skip them and let the
    // backend validate. Sparse examples have no average semantics and are
    // skipped as well.
    let shape_of = |value: &VectorValue| -> Option<(usize, usize)> {
        match value {
            VectorValue::Dense(dims) => Some((1, dims.len())),
            VectorValue::MultiDense(rows) => {
                let dim = rows.first().map_or(0, Vec::len);
                rows.iter()
                    .all(|row| row.len() == dim)
                    .then_some((rows.len(), dim))
            }
            VectorValue::Sparse { .. }
            | VectorValue::Document { .. }
            | VectorValue::Image { .. }
            | VectorValue::Object { .. }
            | VectorValue::Param(..)
            | VectorValue::PositionalParam(..) => None,
        }
    };

    let mut expected: Option<(usize, usize)> = None;
    for input in positive.iter().chain(negative.iter()) {
        let QueryInput::Vector(value) = input else {
            continue;
        };
        let Some(shape) = shape_of(value) else {
            continue;
        };
        match expected {
            None => expected = Some(shape),
            Some(prev) if prev == shape => {}
            Some(prev) => {
                return Err(QqlError::validation(
                    "QQL-PLAN-RECOMMEND-AVERAGE",
                    alloc::format!(
                        "average_vector examples must share one dimension: found ({}, {}) and ({}, {}) rows x dims",
                        prev.0,
                        prev.1,
                        shape.0,
                        shape.1
                    ),
                    None,
                ));
            }
        }
    }
    Ok(())
}

pub(crate) fn query_kind_error(message: &'static str) -> QqlError {
    QqlError::validation("QQL-PLAN-VECTOR-KIND", message, None)
}
