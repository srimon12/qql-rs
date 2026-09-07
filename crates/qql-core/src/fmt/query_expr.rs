//! Formatting for QUERY expressions, inputs, search params, and prefetch clauses.

use super::query::render_query_body_inner;
use crate::ast::{
    ContextPair, FusionMethod, OrderDirection, Prefetch, PrefetchSource, QueryExpr, QueryInput,
    RecommendStrategy, SearchParams, VectorKind, VectorTarget, escape_string,
};
use crate::fmt::expr::{
    render_f64, render_name, render_placeholder, render_point_id, render_read_consistency,
    render_value, render_vector_value,
};
use crate::fmt::filter::render_filter;
use crate::fmt::formula::render_formula;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt::Write;

pub(crate) fn render_query_expr(expression: &QueryExpr) -> String {
    match expression {
        QueryExpr::Points { ids } => format!(
            "POINTS ({})",
            ids.iter()
                .map(render_point_id)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        QueryExpr::Nearest {
            input,
            mmr: Some(mmr),
            ..
        } => format!(
            "MMR {} DIVERSITY {} CANDIDATES {}",
            render_query_input(input, false),
            render_f64(mmr.diversity),
            mmr.candidates
        ),
        QueryExpr::Nearest { input, .. } => render_query_input(input, true),
        QueryExpr::Recommend {
            positive,
            negative,
            strategy,
            ..
        } => {
            let mut out = format!(
                "RECOMMEND POSITIVE ({})",
                positive
                    .iter()
                    .map(render_recommend_input)
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            if !negative.is_empty() {
                let _ = write!(
                    out,
                    " NEGATIVE ({})",
                    negative
                        .iter()
                        .map(render_recommend_input)
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            if let Some(strategy) = strategy {
                let _ = write!(out, " STRATEGY {}", render_recommend_strategy(*strategy));
            }
            out
        }
        QueryExpr::Context { pairs, .. } => format!(
            "CONTEXT ({})",
            pairs
                .iter()
                .map(render_context_pair)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        QueryExpr::Discover {
            target, context, ..
        } => format!(
            "DISCOVER TARGET {} CONTEXT ({})",
            render_query_input(target, true),
            context
                .iter()
                .map(render_context_pair)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        QueryExpr::OrderBy { field, direction } => format!(
            "ORDER BY {} {}",
            render_name(field),
            match direction {
                OrderDirection::Asc => "ASC",
                OrderDirection::Desc => "DESC",
            }
        ),
        QueryExpr::SampleRandom => "SAMPLE RANDOM".into(),
        QueryExpr::Fusion { method, .. } => format!("FUSION {}", render_fusion_method(*method)),
        QueryExpr::Formula {
            expression,
            defaults,
            ..
        } => {
            let mut out = format!("FORMULA {}", render_formula(expression));
            if !defaults.is_empty() {
                let entries: Vec<String> = defaults
                    .iter()
                    .map(|(key, value)| format!("{} = {}", render_name(key), render_value(value)))
                    .collect();
                let _ = write!(out, " DEFAULTS ({})", entries.join(", "));
            }
            out
        }
        QueryExpr::RelevanceFeedback {
            target,
            feedback,
            strategy,
            ..
        } => {
            let items: Vec<String> = feedback
                .iter()
                .map(|item| {
                    format!(
                        "({}, {})",
                        render_query_input(&item.example, true),
                        render_f64(item.score)
                    )
                })
                .collect();
            format!(
                "RELEVANCE FEEDBACK TARGET {} FEEDBACK ({}) STRATEGY NAIVE (a = {}, b = {}, c = {})",
                render_query_input(target, true),
                items.join(", "),
                render_f64(strategy.a),
                render_f64(strategy.b),
                render_f64(strategy.c)
            )
        }
        QueryExpr::Hybrid {
            text,
            model,
            dense_vector,
            sparse_vector,
            fusion,
            text_param,
        } => {
            let rendered_text = if let Some(param) = text_param {
                render_placeholder(param).to_string()
            } else {
                format!("'{}'", escape_string(text))
            };
            let mut out = format!("HYBRID TEXT {}", rendered_text);
            if let Some(model) = model {
                let _ = write!(out, " MODEL '{}'", escape_string(model));
            }
            if let Some(vector) = dense_vector {
                let _ = write!(out, " DENSE {}", render_name(vector));
            }
            if let Some(vector) = sparse_vector {
                let _ = write!(out, " SPARSE {}", render_name(vector));
            }
            let _ = write!(out, " FUSION {}", render_fusion_method(*fusion));
            out
        }
        QueryExpr::Rerank { input, model, .. } => format!(
            "RERANK {} MODEL '{}'",
            render_query_input(input, false),
            escape_string(model)
        ),
        QueryExpr::CrossRerank {
            query,
            model,
            field,
            query_param,
            ..
        } => {
            let rendered_query = if let Some(param) = query_param {
                render_placeholder(param).to_string()
            } else {
                format!("'{}'", escape_string(query))
            };
            let mut out = format!(
                "CROSS RERANK TEXT {} MODEL '{}'",
                rendered_query,
                escape_string(model)
            );
            if let Some(field) = field {
                let _ = write!(out, " ON FIELD {}", render_name(field));
            }
            out
        }
    }
}

pub(crate) fn query_expr_using(expression: &QueryExpr) -> Option<&VectorTarget> {
    match expression {
        QueryExpr::Nearest { using, .. }
        | QueryExpr::Recommend { using, .. }
        | QueryExpr::Context { using, .. }
        | QueryExpr::Discover { using, .. }
        | QueryExpr::RelevanceFeedback { using, .. }
        | QueryExpr::Rerank { using, .. } => using.as_ref(),
        _ => None,
    }
}

pub(crate) fn query_expr_prefetch(expression: &QueryExpr) -> &[Prefetch] {
    match expression {
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

pub(crate) fn render_prefetch(prefetch: &Prefetch) -> String {
    let mut out = match &prefetch.source {
        PrefetchSource::Cte(name) => render_name(name),
        PrefetchSource::Query(query) => render_query_body_inner(query),
    };
    if let Some(filter) = &prefetch.filter {
        let _ = write!(out, " WHERE {}", render_filter(filter));
    }
    if let Some(score) = prefetch.score_threshold {
        let _ = write!(out, " SCORE THRESHOLD {}", render_f64(score));
    }
    if let Some(lookup) = &prefetch.lookup {
        let _ = write!(out, " LOOKUP FROM {}", render_name(&lookup.collection));
        if let Some(vector) = &lookup.vector {
            let _ = write!(out, " VECTOR {}", render_name(vector));
        }
    }
    out
}

pub(crate) fn render_query_input(input: &QueryInput, allow_bare: bool) -> String {
    match input {
        QueryInput::Text {
            text,
            model: None,
            text_param,
        } if allow_bare => {
            if let Some(param) = text_param {
                render_placeholder(param).to_string()
            } else {
                format!("'{}'", escape_string(text))
            }
        }
        QueryInput::Text {
            text,
            model,
            text_param,
        } => {
            let rendered_text = if let Some(param) = text_param {
                render_placeholder(param).to_string()
            } else {
                format!("'{}'", escape_string(text))
            };
            let mut out = format!("TEXT {}", rendered_text);
            if let Some(model) = model {
                let _ = write!(out, " MODEL '{}'", escape_string(model));
            }
            out
        }
        QueryInput::Image { source, model } => {
            let mut out = format!("IMAGE '{}'", escape_string(source));
            if let Some(model) = model {
                let _ = write!(out, " MODEL '{}'", escape_string(model));
            }
            out
        }
        QueryInput::Vector(value) => format!("VECTOR {}", render_vector_value(value)),
        QueryInput::Point(point) => format!("POINT {}", render_point_id(point)),
        QueryInput::Param(name, _) => format!(":{}", name),
        QueryInput::PositionalParam(..) => "?".to_string(),
    }
}

pub(crate) fn render_recommend_input(input: &QueryInput) -> String {
    match input {
        QueryInput::Point(point) => render_point_id(point),
        other => render_query_input(other, true),
    }
}

pub(crate) fn render_context_pair(pair: &ContextPair) -> String {
    format!(
        "POSITIVE {} NEGATIVE {}",
        render_query_input(&pair.positive, true),
        render_query_input(&pair.negative, true)
    )
}

pub(crate) fn render_vector_target(target: &VectorTarget) -> String {
    let mut out = render_name(&target.name);
    if target.multi {
        out.push_str(" AS MULTI");
    } else if let Some(kind) = target.kind {
        out.push_str(match kind {
            VectorKind::Dense => " AS DENSE",
            VectorKind::Sparse => " AS SPARSE",
        });
    }
    out
}

pub(crate) fn render_recommend_strategy(strategy: RecommendStrategy) -> &'static str {
    match strategy {
        RecommendStrategy::AverageVector => "average_vector",
        RecommendStrategy::BestScore => "best_score",
        RecommendStrategy::SumScores => "sum_scores",
    }
}

pub(crate) fn render_fusion_method(method: FusionMethod) -> &'static str {
    match method {
        FusionMethod::Rrf => "RRF",
        FusionMethod::Dbsf => "DBSF",
    }
}

pub(crate) fn render_search_params(params: &SearchParams) -> String {
    let mut parts = Vec::new();
    if let Some(value) = params.hnsw_ef {
        parts.push(format!("hnsw_ef = {}", value));
    }
    if let Some(value) = params.exact {
        parts.push(format!("exact = {}", value));
    }
    if let Some(value) = params.acorn {
        parts.push(format!("acorn = {}", value));
    }
    if let Some(value) = params.max_selectivity {
        parts.push(format!("max_selectivity = {}", render_f64(value)));
    }
    if let Some(value) = params.indexed_only {
        parts.push(format!("indexed_only = {}", value));
    }
    if let Some(value) = params.rrf_k {
        parts.push(format!("rrf_k = {}", value));
    }
    if let Some(values) = &params.rrf_weights {
        parts.push(format!(
            "rrf_weights = [{}]",
            values
                .iter()
                .map(|v| render_f64(*v))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if let Some(quantization) = &params.quantization {
        let mut entries = Vec::new();
        if let Some(value) = quantization.ignore {
            entries.push(format!("ignore: {}", value));
        }
        if let Some(value) = quantization.rescore {
            entries.push(format!("rescore: {}", value));
        }
        if let Some(value) = quantization.oversampling {
            entries.push(format!("oversampling: {}", render_f64(value)));
        }
        parts.push(format!("quantization = {{{}}}", entries.join(", ")));
    }
    if let Some(idf) = &params.idf {
        match &idf.corpus {
            None => parts.push("idf = 'global'".into()),
            Some(filter) => parts.push(format!("idf = WHERE {}", render_filter(filter))),
        }
    }
    if let Some(value) = params.timeout {
        parts.push(format!("timeout = {}", value));
    }
    if let Some(consistency) = &params.consistency {
        parts.push(format!(
            "consistency = {}",
            render_read_consistency(consistency)
        ));
    }
    parts.join(", ")
}
