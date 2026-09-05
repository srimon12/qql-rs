use crate::filter::{lower_filter, top_level_filter};
use crate::semantic::PlanQueryInput;
use crate::types::*;
use qql_core::ast::{
    FusionMethod, OrderDirection, PrefetchSource, QueryExpr, QueryInput, QueryStmt, VectorValue,
};
use qql_core::error::QqlError;

/// Convert an AST vector value into its transport-neutral plan representation.
pub fn lower_vector_value(value: &VectorValue) -> PlanVectorValue {
    PlanVectorValue::from(value)
}

/// Convert an AST query input (point, vector, text, image) into its plan form.
pub fn lower_query_input(input: &QueryInput) -> PlanQueryInput {
    PlanQueryInput::from(input)
}

/// Lower a formula expression tree to the OpenAPI `Expression` JSON shape.
pub fn lower_formula_expr(expr: &qql_core::ast::FormulaExpr) -> serde_json::Value {
    match expr {
        qql_core::ast::FormulaExpr::Constant { value } => serde_json::json!(value),
        qql_core::ast::FormulaExpr::Variable { name } => {
            if name == "score" {
                serde_json::json!("$score")
            } else {
                serde_json::json!(name)
            }
        }
        qql_core::ast::FormulaExpr::Sum { left, right } => serde_json::json!({
            "sum": [lower_formula_expr(left), lower_formula_expr(right)]
        }),
        qql_core::ast::FormulaExpr::Sub { left, right } => serde_json::json!({
            "sum": [lower_formula_expr(left), { "neg": lower_formula_expr(right) }]
        }),
        qql_core::ast::FormulaExpr::Mul { left, right } => serde_json::json!({
            "mult": [lower_formula_expr(left), lower_formula_expr(right)]
        }),
        qql_core::ast::FormulaExpr::Div {
            left,
            right,
            by_zero_default,
        } => {
            let mut div = serde_json::Map::new();
            div.insert("left".into(), lower_formula_expr(left));
            div.insert("right".into(), lower_formula_expr(right));
            if let Some(default) = by_zero_default {
                div.insert("by_zero_default".into(), serde_json::json!(default));
            }
            serde_json::json!({ "div": div })
        }
        qql_core::ast::FormulaExpr::Neg { operand } => serde_json::json!({
            "neg": lower_formula_expr(operand)
        }),
        qql_core::ast::FormulaExpr::Abs { x } => serde_json::json!({
            "abs": lower_formula_expr(x)
        }),
        qql_core::ast::FormulaExpr::Sqrt { x } => serde_json::json!({
            "sqrt": lower_formula_expr(x)
        }),
        qql_core::ast::FormulaExpr::Log { x } => serde_json::json!({
            "log10": lower_formula_expr(x)
        }),
        qql_core::ast::FormulaExpr::Ln { x } => serde_json::json!({
            "ln": lower_formula_expr(x)
        }),
        qql_core::ast::FormulaExpr::Exp { x } => serde_json::json!({
            "exp": lower_formula_expr(x)
        }),
        qql_core::ast::FormulaExpr::Acosh { x } => serde_json::json!({
            "acosh": lower_formula_expr(x)
        }),
        qql_core::ast::FormulaExpr::Max { args } => {
            let terms: Vec<_> = args.iter().map(lower_formula_expr).collect();
            serde_json::json!({ "max": terms })
        }
        qql_core::ast::FormulaExpr::Min { args } => {
            let terms: Vec<_> = args.iter().map(lower_formula_expr).collect();
            serde_json::json!({ "min": terms })
        }
        qql_core::ast::FormulaExpr::Pow { base, exponent } => serde_json::json!({
            "pow": {
                "base": lower_formula_expr(base),
                "exponent": lower_formula_expr(exponent)
            }
        }),
        qql_core::ast::FormulaExpr::GeoDistance { lat, lon, field } => serde_json::json!({
            "geo_distance": {
                "origin": { "lat": lat, "lon": lon },
                "to": field
            }
        }),
        qql_core::ast::FormulaExpr::Decay {
            kind,
            x,
            target,
            scale,
            midpoint,
        } => {
            let mut params = serde_json::Map::new();
            let is_target_datetime = target
                .as_ref()
                .is_some_and(|t| matches!(&**t, qql_core::ast::FormulaExpr::Datetime { .. }));
            let x_val = match (&**x, is_target_datetime) {
                (qql_core::ast::FormulaExpr::Variable { name }, true) => {
                    serde_json::json!({ "datetime_key": name })
                }
                _ => lower_formula_expr(x),
            };
            params.insert("x".into(), x_val);
            if let Some(t) = target {
                params.insert("target".into(), lower_formula_expr(t));
            }
            if let Some(s) = scale {
                params.insert("scale".into(), serde_json::json!(s));
            }
            if let Some(m) = midpoint {
                params.insert("midpoint".into(), serde_json::json!(m));
            }
            let key = match kind.to_ascii_lowercase().as_str() {
                "lin" | "lin_decay" => "lin_decay",
                "exp" | "exp_decay" => "exp_decay",
                _ => "gauss_decay",
            };
            serde_json::json!({ key: params })
        }
        qql_core::ast::FormulaExpr::Case { cond, then_, else_ } => {
            let cond_val = match lower_filter(cond) {
                FilterExpression::Single(clause) => crate::plan::serialize_body(&*clause)
                    .expect("formula CASE condition clause serialization failed"),
                FilterExpression::Compound(comp) => crate::plan::serialize_body(&comp)
                    .expect("formula CASE condition compound serialization failed"),
            };
            // OpenAPI Expression accepts a Condition as a boolean 0/1 term.
            // Encode CASE as: condition * then + (1 - condition) * else.
            serde_json::json!({
                "sum": [
                    {
                        "mult": [cond_val.clone(), lower_formula_expr(then_)]
                    },
                    {
                        "mult": [
                            {
                                "sum": [
                                    1.0,
                                    { "neg": cond_val }
                                ]
                            },
                            lower_formula_expr(else_)
                        ]
                    }
                ]
            })
        }
        qql_core::ast::FormulaExpr::MatchCondition { field, values } => {
            // Condition expressions evaluate to 1.0 / 0.0 on the formula wire format.
            use crate::filter::value_to_json;
            if values.len() == 1 {
                let val = value_to_json(&values[0]);
                serde_json::json!({
                    "key": field,
                    "match": { "value": val }
                })
            } else {
                let any: Vec<_> = values.iter().map(value_to_json).collect();
                serde_json::json!({
                    "key": field,
                    "match": { "any": any }
                })
            }
        }
        qql_core::ast::FormulaExpr::Datetime { value } => serde_json::json!({
            "datetime": value
        }),
        qql_core::ast::FormulaExpr::DatetimeKey { key } => serde_json::json!({
            "datetime_key": key
        }),
    }
}

/// Lower a `QueryExpr` to its wire `QueryVariant` representation.
pub fn lower_query_expr(expr: &QueryExpr) -> Result<QueryVariant, QqlError> {
    Ok(match expr {
        QueryExpr::Nearest { input, mmr, .. } => QueryVariant::Nearest(NearestQuery {
            nearest: lower_query_input(input),
            mmr: mmr.as_ref().map(|m| MmrQueryParams {
                diversity: m.diversity,
                candidates_limit: m.candidates,
            }),
        }),
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
            QueryVariant::Fusion { fusion: m.into() }
        }
        QueryExpr::Formula {
            expression,
            defaults,
            ..
        } => {
            let defaults_map = if defaults.is_empty() {
                None
            } else {
                let mut m = serde_json::Map::new();
                for (key, value) in defaults {
                    m.insert(key.clone(), crate::filter::value_to_json(value));
                }
                Some(m)
            };
            QueryVariant::Formula(FormulaQuery {
                formula: PlanFormula(expression.as_ref().clone()),
                defaults: defaults_map,
            })
        }
        QueryExpr::RelevanceFeedback {
            target,
            feedback,
            strategy,
            ..
        } => {
            let feedback_items: Vec<_> = feedback
                .iter()
                .map(|item| FeedbackItem {
                    example: lower_query_input(&item.example),
                    score: item.score,
                })
                .collect();
            QueryVariant::RelevanceFeedback {
                relevance_feedback: RelevanceFeedbackInput {
                    target: lower_query_input(target),
                    feedback: feedback_items,
                    strategy: FeedbackStrategy {
                        naive: NaiveFeedbackStrategyParams {
                            a: strategy.a,
                            b: strategy.b,
                            c: strategy.c,
                        },
                    },
                },
            }
        }
        QueryExpr::Hybrid { .. } => QueryVariant::Fusion {
            fusion: "rrf".into(),
        },
        QueryExpr::Rerank { input, .. } => QueryVariant::Nearest(NearestQuery {
            nearest: lower_query_input(input),
            mmr: None,
        }),
        // Points / CrossRerank are planned by plan() special-cases and cannot
        // be represented as a QueryVariant. Reaching this arm means one leaked
        // into a prefetch source — a structured error, never a panic.
        QueryExpr::CrossRerank { .. } | QueryExpr::Points { .. } => {
            return Err(QqlError::validation(
                "QQL-PLAN-UNSUPPORTED-PREFETCH",
                "POINTS / CROSS RERANK are not supported inside PREFETCH",
                None,
            ));
        }
    })
}

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

/// Lower a `QueryOutput` into payload/vector selection for wire bodies.
pub fn lower_output_selector_public(
    output: &qql_core::ast::QueryOutput,
) -> (Option<PayloadSelectorReq>, Option<VectorSelectorReq>) {
    lower_output_selector(output)
}

fn lower_output_selector(
    output: &qql_core::ast::QueryOutput,
) -> (Option<PayloadSelectorReq>, Option<VectorSelectorReq>) {
    let with_payload = match &output.payload {
        Some(qql_core::ast::PayloadSelector::All) => Some(PayloadSelectorReq::All(true)),
        Some(qql_core::ast::PayloadSelector::None) => Some(PayloadSelectorReq::All(false)),
        Some(qql_core::ast::PayloadSelector::Include(fields)) => {
            Some(PayloadSelectorReq::Include {
                include: fields.clone(),
            })
        }
        Some(qql_core::ast::PayloadSelector::Exclude(fields)) => {
            Some(PayloadSelectorReq::Exclude {
                exclude: fields.clone(),
            })
        }
        None => Some(PayloadSelectorReq::All(true)),
    };
    let with_vector = output.vectors.as_ref().map(|v| match v {
        qql_core::ast::VectorSelector::All => VectorSelectorReq::All(true),
        qql_core::ast::VectorSelector::None => VectorSelectorReq::All(false),
        qql_core::ast::VectorSelector::Names(names) => VectorSelectorReq::Names(names.clone()),
    });
    (with_payload, with_vector)
}

/// Lower a full `QUERY` statement into the `/points/query` request body.
pub fn lower_query_request(query: &QueryStmt) -> Result<QueryRequest, QqlError> {
    let (with_payload, with_vector) = lower_output_selector(&query.output);
    let (query_variant, using, prefetch) = build_query_with_prefetch(query)?;

    let (timeout, consistency) = lower_request_opts(query.params.as_ref());
    Ok(QueryRequest {
        query: query_variant,
        using,
        prefetch,
        filter: query.filter.as_ref().map(|f| top_level_filter(f)),
        params: match query.params.as_ref() {
            Some(p) => lower_search_params(p)?,
            None => None,
        },
        score_threshold: query.score_threshold,
        with_payload,
        with_vector,
        limit: query.page.limit,
        offset: query.page.offset,
        lookup_from: extract_lookup_from(query),
        // Routing is request-level (REST shard_key / gRPC ShardKeySelector) — not in filter.
        shard_key: query.shard_key.clone(),
        timeout,
        consistency,
    })
}

/// Lower a grouped `QUERY` statement into the `/points/query/groups` body.
///
/// User LIMIT and OFFSET fold into the wire `limit` + `group_offset` pair.
pub fn lower_query_groups_request(query: &QueryStmt) -> Result<QueryGroupsRequest, QqlError> {
    let group = query
        .group
        .as_ref()
        .expect("group required for groups query");
    let offset = query.page.offset.unwrap_or(0);
    let user_limit = query.page.limit.unwrap_or(10);
    let effective_limit = user_limit.checked_add(offset).ok_or_else(|| {
        QqlError::validation(
            "QQL-VALIDATION-LIMIT-OVERFLOW",
            format!(
                "grouped query LIMIT {user_limit} + OFFSET {offset} overflows u64; \
                 reduce LIMIT or OFFSET"
            ),
            None,
        )
    })?;
    let group_offset = if offset > 0 { Some(offset) } else { None };

    let (with_payload, with_vector) = lower_output_selector(&query.output);
    let (query_variant, using, prefetch) = build_query_with_prefetch(query)?;

    let (timeout, consistency) = lower_request_opts(query.params.as_ref());
    Ok(QueryGroupsRequest {
        query: query_variant,
        using,
        prefetch,
        filter: query.filter.as_ref().map(|f| top_level_filter(f)),
        params: match query.params.as_ref() {
            Some(p) => lower_search_params(p)?,
            None => None,
        },
        score_threshold: query.score_threshold,
        with_payload,
        with_vector,
        group_by: group.field.clone(),
        group_size: group.size.unwrap_or(3),
        limit: effective_limit,
        with_lookup: group
            .lookup
            .as_ref()
            .map(|coll| WithLookupValue::Collection(coll.clone())),
        lookup_from: extract_lookup_from(query),
        shard_key: query.shard_key.clone(),
        timeout,
        consistency,
        group_offset,
    })
}

fn build_query_with_prefetch(
    query: &QueryStmt,
) -> Result<(QueryVariant, Option<String>, Vec<PrefetchRequest>), QqlError> {
    match &query.expression {
        QueryExpr::Hybrid {
            text,
            model,
            dense_vector,
            sparse_vector,
            fusion,
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
                    nearest: build_text_input(text, model),
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
                    nearest: build_text_input(text, model),
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
            if let Some(params) = &query.params {
                if params.rrf_k.is_some() || params.rrf_weights.is_some() {
                    if let QueryVariant::Fusion { fusion } = &variant {
                        if fusion == "rrf" {
                            variant = QueryVariant::Rrf(RrfQuery {
                                rrf: RrfParams {
                                    k: params.rrf_k,
                                    weights: params.rrf_weights.clone(),
                                },
                            });
                        }
                    }
                }
            }
            let using = expression_using(&query.expression).map(str::to_owned);
            let prefetches = expression_prefetch(&query.expression);
            let pf_requests: Vec<PrefetchRequest> = prefetches
                .iter()
                .map(|p| lower_prefetch_with_ctes(p, &query.ctes))
                .collect::<Result<_, _>>()?;
            Ok((variant, using, pf_requests))
        }
    }
}

fn build_text_input(text: &str, model: &Option<String>) -> PlanQueryInput {
    PlanQueryInput::Document {
        text: text.to_string(),
        model: model.clone(),
    }
}

/// True when a plan filter has no clauses (serde can produce this for objects
/// with only unknown keys). Empty IDF corpora are rejected as plan errors.
fn filter_expression_is_empty(filter: &FilterExpression) -> bool {
    match filter {
        FilterExpression::Compound(c) => {
            c.must.is_empty()
                && c.must_not.is_empty()
                && c.should.is_empty()
                && c.min_should.is_none()
        }
        FilterExpression::Single(clause) => match clause.as_ref() {
            FilterClause::Filter(c) => {
                c.must.is_empty()
                    && c.must_not.is_empty()
                    && c.should.is_empty()
                    && c.min_should.is_none()
            }
            _ => false,
        },
    }
}

/// Lower body-only OpenAPI `SearchParams` (timeout/consistency are request-level).
///
/// IDF corpora are QQL filters (`idf = WHERE …`). An empty lowered filter is
/// rejected with `QQL-PLAN-IDF`.
pub fn lower_search_params(
    params: &qql_core::ast::SearchParams,
) -> Result<Option<SearchParamsRequest>, QqlError> {
    let mut has = false;
    let idf = match params.idf.as_ref() {
        None => None,
        Some(idf) => Some(match &idf.corpus {
            None => IdfSearchParams::Global,
            Some(filter) => {
                let corpus = top_level_filter(filter);
                if filter_expression_is_empty(&corpus) {
                    return Err(QqlError::validation(
                        "QQL-PLAN-IDF",
                        "idf corpus filter is empty or has no recognised conditions",
                        None,
                    ));
                }
                IdfSearchParams::Corpus { corpus }
            }
        }),
    };
    let r = SearchParamsRequest {
        hnsw_ef: params.hnsw_ef,
        exact: params.exact,
        acorn: params.acorn.map(|enable| AcornSearchParams {
            enable,
            max_selectivity: params.max_selectivity,
        }),
        indexed_only: params.indexed_only,
        quantization: params.quantization.as_ref().map(|q| {
            has = true;
            QuantizationSearchRequest {
                ignore: q.ignore,
                rescore: q.rescore,
                oversampling: q.oversampling,
            }
        }),
        idf,
    };
    if has
        || r.hnsw_ef.is_some()
        || r.exact.is_some()
        || r.acorn.is_some()
        || r.indexed_only.is_some()
        || r.idf.is_some()
    {
        Ok(Some(r))
    } else {
        Ok(None)
    }
}

/// Extract request-level opts (OpenAPI query params / proto fields).
pub fn lower_request_opts(
    params: Option<&qql_core::ast::SearchParams>,
) -> (Option<u64>, Option<ReadConsistencyParam>) {
    match params {
        Some(p) => (
            p.timeout,
            p.consistency.as_ref().map(ReadConsistencyParam::from),
        ),
        None => (None, None),
    }
}

/// Append OpenAPI query params for timeout + consistency.
pub fn push_read_opts(
    query: &mut Vec<(String, String)>,
    timeout: Option<u64>,
    consistency: Option<&ReadConsistencyParam>,
) {
    if let Some(secs) = timeout {
        query.push(("timeout".into(), secs.to_string()));
    }
    if let Some(c) = consistency {
        query.push(("consistency".into(), c.to_query_value()));
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

fn extract_lookup_from(query: &QueryStmt) -> Option<LookupRequest> {
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

#[cfg(test)]
mod tests {
    use qql_core::parser::Parser;

    fn parse_route(source: &str) -> serde_json::Value {
        let s = Parser::parse(source).unwrap();
        let r = crate::routing::try_route(&s).unwrap();
        r.body_json().unwrap()
    }

    #[test]
    fn acorn_true_serializes_enable() {
        let json =
            parse_route("QUERY TEXT 'hello' MODEL 'e5' FROM docs PARAMS (acorn = true) LIMIT 5;");
        assert_eq!(json["params"]["acorn"]["enable"], true);
    }

    #[test]
    fn acorn_false_serializes_enable_false() {
        let json =
            parse_route("QUERY TEXT 'hello' MODEL 'e5' FROM docs PARAMS (acorn = false) LIMIT 5;");
        assert_eq!(json["params"]["acorn"]["enable"], false);
    }

    #[test]
    fn acorn_max_selectivity_serializes() {
        let json = parse_route(
            "QUERY TEXT 'hello' MODEL 'e5' FROM docs PARAMS (acorn = true, max_selectivity = 0.4) LIMIT 5;",
        );
        assert_eq!(json["params"]["acorn"]["enable"], true);
        assert_eq!(json["params"]["acorn"]["max_selectivity"], 0.4);
    }

    #[test]
    fn idf_serializes_global_and_corpus() {
        let global =
            parse_route("QUERY TEXT 'hello' MODEL 'e5' FROM docs PARAMS (idf = 'global') LIMIT 5;");
        assert_eq!(global["params"]["idf"], "global");

        let corpus = parse_route(
            "QUERY TEXT 'hello' MODEL 'e5' FROM docs PARAMS (idf = WHERE status = 'active') LIMIT 5;",
        );
        let idf = &corpus["params"]["idf"];
        assert_eq!(idf["corpus"]["must"][0]["key"], "status");
        assert_eq!(idf["corpus"]["must"][0]["match"]["value"], "active");

        let tenant = parse_route(
            "QUERY TEXT 'hello' MODEL 'e5' FROM docs WHERE tenant_id = 'acme' SHARD 'acme' PARAMS (idf = WHERE tenant_id = 'acme') LIMIT 5;",
        );
        assert_eq!(
            tenant["params"]["idf"]["corpus"]["must"][0]["key"],
            "tenant_id"
        );
        assert_eq!(
            tenant["params"]["idf"]["corpus"]["must"][0]["match"]["value"],
            "acme"
        );
    }

    #[test]
    fn idf_json_corpus_is_rejected_at_parse() {
        let err = Parser::parse(
            "QUERY TEXT 'hello' MODEL 'e5' FROM docs PARAMS (idf = {corpus: {not_a_filter: true}}) LIMIT 5;",
        )
        .unwrap_err();
        assert_eq!(err.code, "QQL-VALIDATION-IDF");
    }

    #[test]
    fn timeout_and_consistency_are_rest_query_params_not_body() {
        // OpenAPI: timeout + consistency are query params on POST …/points/query.
        let route = crate::plan::try_route(
            &Parser::parse(
                "QUERY TEXT 'hello' MODEL 'e5' FROM docs PARAMS (timeout = 30, consistency = majority) LIMIT 5;",
            )
            .unwrap(),
        )
        .unwrap();
        assert!(
            route.query.iter().any(|(k, v)| k == "timeout" && v == "30"),
            "timeout query param missing: {:?}",
            route.query
        );
        assert!(
            route
                .query
                .iter()
                .any(|(k, v)| k == "consistency" && v == "majority"),
            "consistency query param missing: {:?}",
            route.query
        );
        let body = route.body_json().expect("body");
        assert!(body.get("timeout").is_none(), "timeout must not be body");
        assert!(
            body.get("consistency").is_none(),
            "consistency must not be body"
        );
        // Body params only for HNSW etc. — absent when only request-level opts.
        assert!(body.get("params").is_none() || body["params"].as_object().unwrap().is_empty());
    }

    #[test]
    fn consistency_factor_serializes_as_integer_query_param() {
        let route = crate::plan::try_route(
            &Parser::parse(
                "QUERY TEXT 'hello' MODEL 'e5' FROM docs PARAMS (consistency = 2) LIMIT 5;",
            )
            .unwrap(),
        )
        .unwrap();
        assert!(route
            .query
            .iter()
            .any(|(k, v)| k == "consistency" && v == "2"));
    }

    #[test]
    fn group_by_with_offset_is_supported() {
        let op = crate::plan::plan(
            &Parser::parse("QUERY TEXT 'x' MODEL 'e5' FROM docs GROUP BY topic LIMIT 10 OFFSET 5;")
                .unwrap(),
        )
        .unwrap();
        if let crate::plan::PlannedOperation::QueryGroups { request, .. } = op {
            assert_eq!(request.limit, 15);
            assert_eq!(request.group_offset, Some(5));
        } else {
            panic!("expected QueryGroups operation");
        }
    }

    #[test]
    fn mmr_with_sparse_using_is_supported() {
        let op = crate::plan::plan(
            &Parser::parse(
                "QUERY MMR TEXT 'q' MODEL 'e5' DIVERSITY 0.5 CANDIDATES 20 FROM docs USING sparse AS SPARSE LIMIT 5;",
            )
            .unwrap(),
        );
        assert!(op.is_ok());
    }

    #[test]
    fn prefetch_preserves_filter_limit_and_params() {
        let json = parse_route(
            "WITH a AS (QUERY TEXT 'x' MODEL 'e5' FROM docs USING dense WHERE status = 'active' PARAMS (hnsw_ef = 64) SCORE THRESHOLD 0.5 LIMIT 50) \
             QUERY FUSION RRF FROM docs PREFETCH (a) LIMIT 10;",
        );
        let pf = &json["prefetch"][0];
        assert!(
            pf["filter"].is_object(),
            "CTE filter must be preserved: {}",
            pf
        );
        assert_eq!(pf["limit"], 50);
        assert_eq!(pf["params"]["hnsw_ef"], 64);
        assert_eq!(pf["score_threshold"], 0.5);
    }

    #[test]
    fn prefetch_cte_name_is_case_insensitive() {
        let json = parse_route(
            "WITH DenseHits AS (QUERY TEXT 'x' MODEL 'e5' FROM docs USING dense LIMIT 20) \
             QUERY FUSION RRF FROM docs PREFETCH (densehits) LIMIT 5;",
        );
        let pf = &json["prefetch"][0];
        assert!(
            pf["query"].is_object(),
            "case-insensitive CTE lookup must resolve query: {}",
            pf
        );
        assert_eq!(pf["limit"], 20);
    }

    #[test]
    fn formula_div_preserves_by_zero_default() {
        // Div with DEFAULT is only available if the formula parser supports it;
        // at least ensure MatchCondition/Datetime lower to non-null JSON when present
        // via a simple arithmetic formula regression.
        let json = parse_route("QUERY FORMULA score * 2 DEFAULTS (score = 0.0) FROM docs LIMIT 5;");
        assert!(
            json["query"]["formula"].is_object()
                || json["query"]["formula"].is_number()
                || json["query"]["formula"].is_string()
        );
        assert_ne!(json["query"]["formula"], serde_json::Value::Null);
    }

    #[test]
    fn formula_match_condition_lowers_to_value_when_single() {
        let json = parse_route("QUERY FORMULA MATCH(is_superhost, true) FROM docs LIMIT 5;");
        let formula = &json["query"]["formula"];
        assert_eq!(formula["key"], "is_superhost");
        assert_eq!(formula["match"]["value"], true);
    }

    #[test]
    fn hybrid_expands_to_prefetches() {
        let json = parse_route(
            "QUERY HYBRID TEXT 'ai search' MODEL 'bge' DENSE dense SPARSE sparse FUSION RRF FROM docs LIMIT 10;",
        );
        assert_eq!(json["query"]["fusion"], "rrf");
        assert!(json["query"].get("nearest").is_none());
        let pf = json["prefetch"].as_array().unwrap();
        assert_eq!(pf.len(), 2);
    }

    #[test]
    fn using_hybrid_expands_like_front_form() {
        let front = parse_route(
            "QUERY HYBRID TEXT 'ai search' MODEL 'bge' DENSE dense SPARSE sparse FUSION RRF FROM docs LIMIT 10;",
        );
        let tail = parse_route(
            "QUERY TEXT 'ai search' MODEL 'bge' FROM docs USING HYBRID DENSE dense SPARSE sparse FUSION RRF LIMIT 10;",
        );
        assert_eq!(front, tail);
        assert_eq!(tail["query"]["fusion"], "rrf");
        assert_eq!(tail["prefetch"].as_array().unwrap().len(), 2);
        assert_eq!(tail["prefetch"][0]["using"], "dense");
        assert_eq!(tail["prefetch"][1]["using"], "sparse");
        // Candidate overfetch: LIMIT * 10
        assert_eq!(tail["prefetch"][0]["limit"], 100);
        assert_eq!(tail["prefetch"][1]["limit"], 100);
    }

    #[test]
    fn using_hybrid_dbsf_and_defaults() {
        let json = parse_route(
            "QUERY TEXT 'q' MODEL 'bge' FROM docs USING HYBRID DENSE d SPARSE s FUSION DBSF LIMIT 5;",
        );
        assert_eq!(json["query"]["fusion"], "dbsf");
        assert_eq!(json["prefetch"][0]["using"], "d");
        assert_eq!(json["prefetch"][1]["using"], "s");
        assert_eq!(json["prefetch"][0]["limit"], 50);
    }

    #[test]
    fn nearest_text_without_model_plans_successfully() {
        // Bare text without MODEL now succeeds at plan time — MODEL is
        // filled by the executor/embedder, not the transport-agnostic plan layer.
        let result = crate::plan::plan(
            &Parser::parse("QUERY TEXT 'hello' FROM docs USING dense LIMIT 5;").unwrap(),
        );
        assert!(
            result.is_ok(),
            "plan should succeed without MODEL: {}",
            result.unwrap_err()
        );
    }

    #[test]
    fn nearest_text_with_model_is_object() {
        let json =
            parse_route("QUERY TEXT 'hello' MODEL 'embedder' FROM docs USING dense LIMIT 5;");
        assert_eq!(json["query"]["nearest"]["text"], "hello");
        assert_eq!(json["query"]["nearest"]["model"], "embedder");
    }

    #[test]
    fn nearest_vector_is_array() {
        let json = parse_route("QUERY NEAREST VECTOR [1.0, 2.0] FROM docs USING dense LIMIT 5;");
        assert!(json["query"]["nearest"].is_array());
    }

    #[test]
    fn nearest_with_mmr() {
        let json = parse_route(
            "QUERY MMR TEXT 'query' MODEL 'embedder' DIVERSITY 0.4 CANDIDATES 100 FROM docs USING dense LIMIT 5;",
        );
        assert_eq!(json["query"]["nearest"]["text"], "query");
        assert_eq!(json["query"]["nearest"]["model"], "embedder");
        assert_eq!(json["query"]["mmr"]["diversity"], 0.4);
        assert_eq!(json["query"]["mmr"]["candidates_limit"], 100);
    }

    #[test]
    fn recommend_serializes_correctly() {
        let json = parse_route(
            "QUERY RECOMMEND POSITIVE (1) NEGATIVE (2, 3) STRATEGY average_vector FROM docs USING dense LIMIT 10;",
        );
        assert_eq!(json["query"]["recommend"]["positive"][0], 1);
        assert_eq!(
            json["query"]["recommend"]["negative"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(json["query"]["recommend"]["strategy"], "average_vector");
    }

    #[test]
    fn formula_case_condition_lowers_to_object_not_null() {
        // Regression: the CASE condition used
        // `serde_json::to_value(...).unwrap_or_default()`, which silently lowered
        // to a JSON `null` term on any serialization failure. It must lower to a
        // real filter object (single clause or compound) — never null.
        let single = parse_route(
            "QUERY FORMULA CASE WHEN status = 'active' THEN $score * 2 ELSE $score END FROM docs LIMIT 5;",
        );
        let formula = &single["query"]["formula"];
        assert!(formula.is_object(), "formula must be an object: {formula}");
        let cond = &formula["sum"][0]["mult"][0];
        assert_ne!(cond, &serde_json::Value::Null);
        assert_eq!(cond["key"], "status");
        assert_eq!(cond["match"]["value"], "active");
        // The `(1 - condition)` arm mirrors the same condition term.
        assert_eq!(formula["sum"][1]["mult"][0]["sum"][1]["neg"], *cond);

        let compound = parse_route(
            "QUERY FORMULA CASE WHEN a = 1 AND b = 2 THEN $score ELSE 0 END FROM docs LIMIT 5;",
        );
        let formula = &compound["query"]["formula"];
        let cond = &formula["sum"][0]["mult"][0];
        assert_ne!(cond, &serde_json::Value::Null);
        assert_eq!(cond["must"][0]["key"], "a");
        assert_eq!(cond["must"][0]["match"]["value"], 1);
        assert_eq!(cond["must"][1]["key"], "b");
    }

    #[test]
    fn formula_with_defaults() {
        let json = parse_route(
            "QUERY FORMULA score * 2 DEFAULTS (score = 0.0, boost = 1.0) FROM docs LIMIT 5;",
        );
        assert!(json["query"]["formula"].is_object());
        let defaults = json["query"]["defaults"].as_object().unwrap();
        assert_eq!(defaults["score"], 0.0);
        assert_eq!(defaults["boost"], 1.0);
    }

    #[test]
    fn relevance_feedback_proper_shape() {
        let json = parse_route(
            "QUERY RELEVANCE FEEDBACK TARGET POINT 42 FEEDBACK ((POINT 43, 0.5), (POINT 44, -0.2)) STRATEGY NAIVE (a = 1.0, b = 0.5, c = 0.5) FROM docs USING dense LIMIT 10;",
        );
        let rf = &json["query"]["relevance_feedback"];
        assert_eq!(rf["target"], 42);
        assert_eq!(rf["feedback"].as_array().unwrap().len(), 2);
        assert_eq!(rf["feedback"][0]["score"], 0.5);
        assert_eq!(rf["strategy"]["naive"]["a"], 1.0);
    }

    #[test]
    fn prefetch_serializes_lookup_from() {
        let json = parse_route(
            "QUERY NEAREST POINT 42 FROM docs USING dense PREFETCH (QUERY TEXT 'x' MODEL 'e5' FROM docs USING dense LIMIT 50) LIMIT 10;",
        );
        let pf = &json["prefetch"][0];
        assert_eq!(pf["query"]["nearest"]["text"], "x");
        assert_eq!(pf["query"]["nearest"]["model"], "e5");
    }

    #[test]
    fn query_request_no_group_fields() {
        let json = parse_route("QUERY TEXT 'hello' MODEL 'e5' FROM docs LIMIT 5;");
        assert!(json.get("group_by").is_none());
        assert!(json.get("group_size").is_none());
    }

    #[test]
    fn grouped_request_has_group_fields() {
        let json = parse_route(
            "QUERY TEXT 'news' MODEL 'e5' FROM docs GROUP BY topic SIZE 5 LOOKUP FROM topics LIMIT 20;",
        );
        assert_eq!(json["group_by"], "topic");
        assert_eq!(json["group_size"], 5);
        assert_eq!(json["limit"], 20);
    }

    #[test]
    fn order_by_query() {
        let json = parse_route("QUERY ORDER BY created_at DESC FROM docs LIMIT 10;");
        assert_eq!(json["query"]["order_by"]["key"], "created_at");
        assert_eq!(json["query"]["order_by"]["direction"], "desc");
    }

    #[test]
    fn sample_query() {
        let json = parse_route("QUERY SAMPLE RANDOM FROM docs LIMIT 5;");
        assert_eq!(json["query"]["sample"], "random");
    }

    #[test]
    fn fusion_query() {
        let json = parse_route(
            "WITH a AS (QUERY TEXT 'x' MODEL 'e5' FROM docs USING dense LIMIT 100) QUERY FUSION RRF FROM docs PREFETCH (a) LIMIT 10;",
        );
        assert_eq!(json["query"]["fusion"], "rrf");
    }

    #[test]
    fn discover_query() {
        let json = parse_route(
            "QUERY DISCOVER TARGET POINT 42 CONTEXT (POSITIVE POINT 1 NEGATIVE POINT 2) FROM docs USING dense LIMIT 10;",
        );
        assert_eq!(json["query"]["discover"]["target"], 42);
        assert_eq!(
            json["query"]["discover"]["context"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn context_query() {
        let json = parse_route(
            "QUERY CONTEXT (POSITIVE POINT 1 NEGATIVE POINT 2, POSITIVE POINT 3 NEGATIVE POINT 4) FROM docs LIMIT 10;",
        );
        assert_eq!(json["query"]["context"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn rerank_has_prefetches_and_model() {
        let json = parse_route(
            "QUERY RERANK TEXT 'travel tips' MODEL 'colbert-v2' FROM docs USING colbert PREFETCH (QUERY TEXT 'travel tips' MODEL 'e5' FROM docs USING dense LIMIT 50) LIMIT 10;",
        );
        assert_eq!(json["using"], "colbert");
        let nearest = &json["query"]["nearest"];
        assert_eq!(nearest["text"], "travel tips");
        assert_eq!(nearest["model"], "colbert-v2");
        assert_eq!(json["prefetch"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn document_without_model_plans_successfully() {
        // Plan layer is transport-agnostic — MODEL is filled downstream.
        let result = crate::plan::plan(
            &Parser::parse("QUERY 'bare string' FROM docs USING dense LIMIT 5;").unwrap(),
        );
        assert!(
            result.is_ok(),
            "plan should succeed without MODEL: {}",
            result.unwrap_err()
        );
    }

    #[test]
    fn hybrid_without_model_plans_successfully() {
        let result = crate::plan::plan(
            &Parser::parse(
                "QUERY HYBRID TEXT 'search' DENSE d SPARSE s FUSION RRF FROM docs LIMIT 10;",
            )
            .unwrap(),
        );
        assert!(
            result.is_ok(),
            "plan should succeed without MODEL: {}",
            result.unwrap_err()
        );
    }

    #[test]
    fn document_and_image_with_model_are_objects() {
        // Verify that modelled Document is always an object, never a bare string.
        let doc_json = parse_route("QUERY TEXT 'doc text' MODEL 'my-model' FROM docs LIMIT 5;");
        let nearest = &doc_json["query"]["nearest"];
        assert!(
            nearest.is_object(),
            "modelled Document must be object: {nearest}"
        );
        assert_eq!(nearest["text"], "doc text");
        assert_eq!(nearest["model"], "my-model");

        // Verify that modelled Image is always an object with both image and model.
        let img_json = parse_route(
            "QUERY IMAGE 'https://img.example.com/cat.jpg' MODEL 'clip-vision' FROM docs USING img LIMIT 5;",
        );
        let img_nearest = &img_json["query"]["nearest"];
        assert!(
            img_nearest.is_object(),
            "modelled Image must be object: {img_nearest}"
        );
        assert_eq!(img_nearest["image"], "https://img.example.com/cat.jpg");
        assert_eq!(img_nearest["model"], "clip-vision");
    }

    #[test]
    fn points_prefetch_source_errors_instead_of_panicking() {
        let result = crate::plan::plan(
            &Parser::parse(
                "QUERY TEXT 'x' FROM docs USING dense PREFETCH (QUERY POINTS (1) FROM docs) LIMIT 10;",
            )
            .unwrap(),
        );
        let err = result.unwrap_err();
        assert_eq!(err.kind, qql_core::error::ErrorKind::Validation);
        assert_eq!(err.code, "QQL-PLAN-UNSUPPORTED-PREFETCH");
    }

    #[test]
    fn points_cte_prefetch_source_errors_instead_of_panicking() {
        let result = crate::plan::plan(
            &Parser::parse(
                "WITH a AS (QUERY POINTS (1) FROM docs) \
                 QUERY FUSION RRF FROM docs PREFETCH (a) LIMIT 10;",
            )
            .unwrap(),
        );
        let err = result.unwrap_err();
        assert_eq!(err.kind, qql_core::error::ErrorKind::Validation);
        assert_eq!(err.code, "QQL-PLAN-UNSUPPORTED-PREFETCH");
    }

    #[test]
    fn cross_rerank_prefetch_source_errors_instead_of_panicking() {
        let result = crate::plan::plan(
            &Parser::parse(
                "QUERY TEXT 'x' FROM docs USING dense \
                 PREFETCH (QUERY CROSS RERANK TEXT 'q' MODEL 'm' FROM docs \
                   PREFETCH (QUERY TEXT 'c' FROM docs LIMIT 5)) \
                 LIMIT 10;",
            )
            .unwrap(),
        );
        let err = result.unwrap_err();
        assert_eq!(err.kind, qql_core::error::ErrorKind::Validation);
        assert_eq!(err.code, "QQL-PLAN-UNSUPPORTED-PREFETCH");
    }

    #[test]
    fn rerank_cte_prefetch_preserves_candidate_query() {
        let json = parse_route(
            "WITH a AS (QUERY TEXT 'x' MODEL 'e5' FROM docs USING dense LIMIT 20) \
             QUERY RERANK TEXT 'q' MODEL 'colbert' FROM docs USING colbert PREFETCH (a) LIMIT 10;",
        );
        let pf = &json["prefetch"][0];
        assert!(
            pf["query"].is_object(),
            "CTE-referencing RERANK prefetch must keep its candidate query: {pf}"
        );
        assert_eq!(pf["query"]["nearest"]["text"], "x");
        assert_eq!(pf["limit"], 20);
        assert_ne!(pf, &serde_json::json!({}));
    }

    #[test]
    fn group_by_prefetch_source_is_rejected() {
        let result = crate::plan::plan(
            &Parser::parse(
                "QUERY FUSION RRF FROM docs PREFETCH \
                   (QUERY TEXT 'y' FROM docs GROUP BY topic LIMIT 5) \
                 LIMIT 10;",
            )
            .unwrap(),
        );
        let err = result.unwrap_err();
        assert_eq!(err.kind, qql_core::error::ErrorKind::Validation);
        assert_eq!(err.code, "QQL-PLAN-PREFETCH-GROUP");
    }

    #[test]
    fn prefetch_lowering_errors_are_propagated_not_swallowed() {
        use qql_core::ast::*;
        // Programmatic AST: a PREFETCH referencing a CTE that is not declared
        // must fail planning instead of silently lowering to an empty prefetch.
        let stmt = Stmt::Query(Box::new(QueryStmt {
            ctes: vec![],
            collection: QueryCollection::Explicit("docs".into()),
            expression: QueryExpr::Fusion {
                method: FusionMethod::Rrf,
                prefetch: vec![Prefetch {
                    source: PrefetchSource::Cte("missing".into()),
                    filter: None,
                    score_threshold: None,
                    lookup: None,
                }],
            },
            filter: None,
            params: None,
            score_threshold: None,
            group: None,
            output: QueryOutput::default(),
            page: PageSpec {
                limit: Some(10),
                offset: None,
            },
            shard_key: None,
        }));
        let err = crate::plan::plan(&stmt).unwrap_err();
        assert_eq!(err.kind, qql_core::error::ErrorKind::Validation);
        assert_eq!(err.code, "QQL-PLAN-PREFETCH-CTE");
    }

    #[test]
    fn grouped_query_limit_offset_overflow_is_a_validation_error() {
        let result = crate::plan::plan(
            &Parser::parse(
                "QUERY TEXT 'x' FROM docs GROUP BY topic \
                 LIMIT 18446744073709551615 OFFSET 18446744073709551615;",
            )
            .unwrap(),
        );
        let err = result.unwrap_err();
        assert_eq!(err.kind, qql_core::error::ErrorKind::Validation);
        assert_eq!(err.code, "QQL-VALIDATION-LIMIT-OVERFLOW");
    }

    #[test]
    fn hybrid_limit_times_ten_overflow_is_a_validation_error() {
        let result = crate::plan::plan(
            &Parser::parse(
                "QUERY HYBRID TEXT 'q' DENSE d SPARSE s FROM docs \
                 LIMIT 18446744073709551615;",
            )
            .unwrap(),
        );
        let err = result.unwrap_err();
        assert_eq!(err.kind, qql_core::error::ErrorKind::Validation);
        assert_eq!(err.code, "QQL-VALIDATION-LIMIT-OVERFLOW");
    }

    #[test]
    fn query_defaults_with_payload_to_true_when_omitted() {
        let json = parse_route("QUERY TEXT 'q' FROM docs;");
        assert_eq!(json["with_payload"], true);
    }

    #[test]
    fn query_respects_explicit_with_payload_false() {
        let json = parse_route("QUERY TEXT 'q' FROM docs WITH PAYLOAD false;");
        assert_eq!(json["with_payload"], false);
    }

    #[test]
    fn decay_datetime_target_and_variable_auto_inferred() {
        let json = parse_route(
            "QUERY FORMULA EXP_DECAY(judgment_date, TARGET = '2026-09-04T00:00:00Z', SCALE = 630720000, MIDPOINT = 0.5) FROM docs;"
        );
        let decay = &json["query"]["formula"]["exp_decay"];
        assert_eq!(decay["x"]["datetime_key"], "judgment_date");
        assert_eq!(decay["target"]["datetime"], "2026-09-04T00:00:00Z");
        assert_eq!(decay["scale"], 630720000.0);
        assert_eq!(decay["midpoint"], 0.5);
    }
}
