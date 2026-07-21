use crate::filter::{lower_filter, point_id_req};
use crate::types::*;
use qql_core::ast::{
    FusionMethod, OrderDirection, PrefetchSource, QueryExpr, QueryInput, QueryStmt,
    VectorValue,
};

pub fn lower_vector_value(value: &VectorValue) -> serde_json::Value {
    match value {
        VectorValue::Dense(values) => serde_json::Value::Array(
            values
                .iter()
                .map(|v| serde_json::Value::from(*v as f64))
                .collect(),
        ),
        VectorValue::Sparse { indices, values } => serde_json::json!({
            "indices": indices,
            "values": values,
        }),
        VectorValue::MultiDense(rows) => serde_json::Value::Array(
            rows.iter()
                .map(|row| {
                    serde_json::Value::Array(
                        row.iter()
                            .map(|v| serde_json::Value::from(*v as f64))
                            .collect(),
                    )
                })
                .collect(),
        ),
    }
}

pub fn lower_query_input(input: &QueryInput) -> serde_json::Value {
    match input {
        QueryInput::Text { text, model } => {
            if let Some(model) = model {
                serde_json::json!({"text": text, "model": model})
            } else {
                serde_json::json!(text)
            }
        }
        QueryInput::Vector(v) => lower_vector_value(v),
        QueryInput::Point(id) => point_id_req(id),
    }
}

pub fn lower_query_expr(expr: &QueryExpr) -> QueryVariant {
    match expr {
        QueryExpr::Nearest { input, .. } => QueryVariant::Nearest {
            nearest: lower_query_input(input),
        },
        QueryExpr::Recommend {
            positive,
            negative,
            strategy,
            ..
        } => {
            let pos: Vec<_> = positive.iter().map(lower_query_input).collect();
            let neg: Vec<_> = negative.iter().map(lower_query_input).collect();
            let s = strategy.map(|s| match s {
                qql_core::ast::RecommendStrategy::AverageVector => "average_vector".into(),
                qql_core::ast::RecommendStrategy::BestScore => "best_score".into(),
                qql_core::ast::RecommendStrategy::SumScores => "sum_scores".into(),
            });
            QueryVariant::Recommend {
                recommend: RecommendQuery {
                    positive: pos,
                    negative: neg,
                    strategy: s,
                },
            }
        }
        QueryExpr::Context { pairs, .. } => {
            let ctx: Vec<_> = pairs
                .iter()
                .map(|pair| ContextPair {
                    positive: lower_query_input(&pair.positive),
                    negative: lower_query_input(&pair.negative),
                })
                .collect();
            QueryVariant::Context { context: ctx }
        }
        QueryExpr::Discover {
            target, context, ..
        } => {
            let ctx: Vec<_> = context
                .iter()
                .map(|pair| ContextPair {
                    positive: lower_query_input(&pair.positive),
                    negative: lower_query_input(&pair.negative),
                })
                .collect();
            QueryVariant::Discover {
                discover: DiscoverQuery {
                    target: lower_query_input(target),
                    context: ctx,
                },
            }
        }
        QueryExpr::OrderBy { field, direction } => {
            let dir = match direction {
                OrderDirection::Asc => Some("asc".into()),
                OrderDirection::Desc => Some("desc".into()),
            };
            QueryVariant::OrderBy {
                order_by: OrderByQuery {
                    key: field.clone(),
                    direction: dir,
                },
            }
        }
        QueryExpr::SampleRandom => QueryVariant::Sample {
            sample: "random".into(),
        },
        QueryExpr::Fusion { method, .. } => {
            let m = match method {
                FusionMethod::Rrf => "rrf",
                FusionMethod::Dbsf => "dbsf",
            };
            QueryVariant::Fusion {
                fusion: m.into(),
            }
        }
        QueryExpr::Formula { .. } => QueryVariant::Formula {
            formula: serde_json::json!("expression"),
        },
        QueryExpr::RelevanceFeedback { target, .. } => QueryVariant::Recommend {
            recommend: RecommendQuery {
                positive: vec![lower_query_input(target)],
                negative: Vec::new(),
                strategy: None,
            },
        },
        QueryExpr::Mmr { .. } => QueryVariant::Sample {
            sample: "random".into(),
        },
        QueryExpr::Hybrid { .. } => QueryVariant::Fusion {
            fusion: "rrf".into(),
        },
        QueryExpr::Rerank { input, .. } => QueryVariant::Nearest {
            nearest: lower_query_input(input),
        },
        QueryExpr::Points { .. } => QueryVariant::Nearest {
            nearest: serde_json::Value::Array(Vec::new()),
        },
    }
}

pub fn lower_prefetch(prefetch: &qql_core::ast::Prefetch) -> PrefetchRequest {
    let query = match &prefetch.source {
        PrefetchSource::Cte(_name) => None,
        PrefetchSource::Query(query) => Some(lower_query_expr(&query.expression)),
    };
    PrefetchRequest {
        query,
        filter: prefetch.filter.as_ref().map(|f| lower_filter(f)),
        score_threshold: prefetch.score_threshold,
        lookup: prefetch.lookup.as_ref().map(|l| LookupRequest {
            collection: l.collection.clone(),
            vector: l.vector.clone(),
        }),
    }
}

pub fn lower_query_request(query: &QueryStmt) -> QueryRequest {
    QueryRequest {
        query: lower_query_expr(&query.expression),
        using: expression_using(&query.expression).cloned(),
        prefetch: expression_prefetch(&query.expression)
            .iter()
            .map(lower_prefetch)
            .collect(),
        filter: query.filter.as_ref().map(|f| lower_filter(f)),
        params: query.params.as_ref().and_then(lower_search_params),
        score_threshold: query.score_threshold,
        group_by: query.group.as_ref().map(|g| g.field.clone()),
        group_size: query.group.as_ref().and_then(|g| g.size),
        group_request: query
            .group
            .as_ref()
            .and_then(|g| g.lookup.as_ref().map(|l| GroupRequest {
                with_lookup: l.clone(),
            })),
        with_payload: query.output.payload.as_ref().map(|p| match p {
            qql_core::ast::PayloadSelector::All => PayloadSelectorReq::All(true),
            qql_core::ast::PayloadSelector::None => PayloadSelectorReq::All(false),
            qql_core::ast::PayloadSelector::Include(fields) => PayloadSelectorReq::Include {
                include: fields.clone(),
            },
            qql_core::ast::PayloadSelector::Exclude(fields) => PayloadSelectorReq::Exclude {
                exclude: fields.clone(),
            },
        }),
        with_vector: query.output.vectors.as_ref().map(|v| match v {
            qql_core::ast::VectorSelector::All => VectorSelectorReq::All(true),
            qql_core::ast::VectorSelector::None => VectorSelectorReq::All(false),
            qql_core::ast::VectorSelector::Names(names) => VectorSelectorReq::Names(names.clone()),
        }),
        limit: query.page.limit,
        offset: query.page.offset,
    }
}

pub fn lower_search_params(
    params: &qql_core::ast::SearchParams,
) -> Option<SearchParamsRequest> {
    let mut has = false;
    let r = SearchParamsRequest {
        hnsw_ef: params.hnsw_ef,
        exact: params.exact,
        acorn: params.acorn,
        indexed_only: params.indexed_only,
        quantization: params.quantization.as_ref().map(|q| {
            has = true;
            QuantizationSearchRequest {
                ignore: q.ignore,
                rescore: q.rescore,
                oversampling: q.oversampling,
            }
        }),
    };
    if has
        || r.hnsw_ef.is_some()
        || r.exact.is_some()
        || r.acorn.is_some()
        || r.indexed_only.is_some()
    {
        Some(r)
    } else {
        None
    }
}

fn expression_using(expr: &QueryExpr) -> Option<&String> {
    match expr {
        QueryExpr::Nearest { using, .. }
        | QueryExpr::Recommend { using, .. }
        | QueryExpr::Context { using, .. }
        | QueryExpr::Discover { using, .. }
        | QueryExpr::RelevanceFeedback { using, .. }
        | QueryExpr::Mmr { using, .. } => using.as_ref(),
        QueryExpr::Rerank { using, .. } => Some(using),
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
        | QueryExpr::Mmr { prefetch, .. }
        | QueryExpr::Rerank { prefetch, .. } => prefetch,
        _ => &[],
    }
}
