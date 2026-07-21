use crate::filter::{lower_filter, point_id_req};
use crate::types::*;
use qql_core::ast::{
    FusionMethod, OrderDirection, PrefetchSource, QueryExpr, QueryInput, QueryStmt, VectorValue,
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
            let formula_val = serde_json::to_value(expression.as_ref()).unwrap_or_default();
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
                formula: formula_val,
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
        QueryExpr::Points { .. } => QueryVariant::Nearest(NearestQuery {
            nearest: serde_json::Value::Array(Vec::new()),
            mmr: None,
        }),
    }
}

pub fn lower_prefetch(prefetch: &qql_core::ast::Prefetch) -> PrefetchRequest {
    let query = match &prefetch.source {
        PrefetchSource::Cte(_name) => None,
        PrefetchSource::Query(query) => Some(lower_query_expr(&query.expression)),
    };
    PrefetchRequest {
        query,
        using: match &prefetch.source {
            PrefetchSource::Query(q) => expression_using(&q.expression).cloned(),
            PrefetchSource::Cte(_) => None,
        },
        filter: prefetch.filter.as_ref().map(|f| lower_filter(f)),
        params: None,
        score_threshold: prefetch.score_threshold,
        limit: None,
        lookup_from: prefetch.lookup.as_ref().map(|l| LookupRequest {
            collection: l.collection.clone(),
            vector: l.vector.clone(),
        }),
        prefetch: None,
    }
}

fn lower_output_selector(
    output: &qql_core::ast::QueryOutput,
) -> (Option<PayloadSelectorReq>, Option<VectorSelectorReq>) {
    let with_payload = output.payload.as_ref().map(|p| match p {
        qql_core::ast::PayloadSelector::All => PayloadSelectorReq::All(true),
        qql_core::ast::PayloadSelector::None => PayloadSelectorReq::All(false),
        qql_core::ast::PayloadSelector::Include(fields) => PayloadSelectorReq::Include {
            include: fields.clone(),
        },
        qql_core::ast::PayloadSelector::Exclude(fields) => PayloadSelectorReq::Exclude {
            exclude: fields.clone(),
        },
    });
    let with_vector = output.vectors.as_ref().map(|v| match v {
        qql_core::ast::VectorSelector::All => VectorSelectorReq::All(true),
        qql_core::ast::VectorSelector::None => VectorSelectorReq::All(false),
        qql_core::ast::VectorSelector::Names(names) => VectorSelectorReq::Names(names.clone()),
    });
    (with_payload, with_vector)
}

pub fn lower_query_request(query: &QueryStmt) -> QueryRequest {
    let (with_payload, with_vector) = lower_output_selector(&query.output);
    let (query_variant, using, prefetch) = build_query_with_prefetch(query);

    QueryRequest {
        query: query_variant,
        using,
        prefetch,
        filter: query.filter.as_ref().map(|f| lower_filter(f)),
        params: query.params.as_ref().and_then(lower_search_params),
        score_threshold: query.score_threshold,
        with_payload,
        with_vector,
        limit: query.page.limit,
        offset: query.page.offset,
        lookup_from: None,
    }
}

pub fn lower_query_groups_request(query: &QueryStmt) -> QueryGroupsRequest {
    let group = query
        .group
        .as_ref()
        .expect("group required for groups query");
    let (with_payload, with_vector) = lower_output_selector(&query.output);
    let (query_variant, using, prefetch) = build_query_with_prefetch(query);

    QueryGroupsRequest {
        query: query_variant,
        using,
        prefetch,
        filter: query.filter.as_ref().map(|f| lower_filter(f)),
        params: query.params.as_ref().and_then(lower_search_params),
        score_threshold: query.score_threshold,
        with_payload,
        with_vector,
        group_by: group.field.clone(),
        group_size: group.size.unwrap_or(3),
        limit: query.page.limit.unwrap_or(10),
        with_lookup: group
            .lookup
            .as_ref()
            .map(|coll| WithLookupValue::Collection(coll.clone())),
        lookup_from: None,
    }
}

fn build_query_with_prefetch(
    query: &QueryStmt,
) -> (QueryVariant, Option<String>, Vec<PrefetchRequest>) {
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
            let candidates = query.page.limit.map(|l| l * 10).unwrap_or(100);

            let dense_prefetch = PrefetchRequest {
                query: Some(QueryVariant::Nearest(NearestQuery {
                    nearest: build_text_input(text, model),
                    mmr: None,
                })),
                using: dense_vector.clone(),
                filter: query.filter.as_ref().map(|f| lower_filter(f)),
                params: query.params.as_ref().and_then(lower_search_params),
                score_threshold: query.score_threshold,
                limit: Some(candidates),
                lookup_from: None,
                prefetch: None,
            };
            let sparse_prefetch = PrefetchRequest {
                query: Some(QueryVariant::Nearest(NearestQuery {
                    nearest: build_text_input(text, &None),
                    mmr: None,
                })),
                using: sparse_vector.clone(),
                filter: query.filter.as_ref().map(|f| lower_filter(f)),
                params: query.params.as_ref().and_then(lower_search_params),
                score_threshold: query.score_threshold,
                limit: Some(candidates),
                lookup_from: None,
                prefetch: None,
            };
            (
                QueryVariant::Fusion {
                    fusion: fusion_name.into(),
                },
                None,
                vec![dense_prefetch, sparse_prefetch],
            )
        }
        QueryExpr::Rerank {
            input,
            model: rerank_model,
            using,
            prefetch,
        } => {
            let pf_requests: Vec<PrefetchRequest> = prefetch.iter().map(lower_prefetch).collect();
            let nearest_input = match input {
                QueryInput::Text { text, .. } => {
                    serde_json::json!({"text": text, "model": rerank_model})
                }
                _ => lower_query_input(input),
            };
            (
                QueryVariant::Nearest(NearestQuery {
                    nearest: nearest_input,
                    mmr: None,
                }),
                Some(using.clone()),
                pf_requests,
            )
        }
        _ => {
            let variant = lower_query_expr(&query.expression);
            let using = expression_using(&query.expression).cloned();
            let prefetches: Vec<PrefetchRequest> = expression_prefetch(&query.expression)
                .iter()
                .map(lower_prefetch)
                .collect();
            (variant, using, prefetches)
        }
    }
}

fn build_text_input(text: &str, model: &Option<String>) -> serde_json::Value {
    if let Some(model) = model {
        serde_json::json!({"text": text, "model": model})
    } else {
        serde_json::json!(text)
    }
}

pub fn lower_search_params(params: &qql_core::ast::SearchParams) -> Option<SearchParamsRequest> {
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
        | QueryExpr::RelevanceFeedback { using, .. } => using.as_ref(),
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
        | QueryExpr::Rerank { prefetch, .. } => prefetch,
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use qql_core::parser::Parser;

    fn parse_route(source: &str) -> serde_json::Value {
        let s = Parser::parse(source).unwrap();
        let r = crate::routing::route(&s);
        r.body_json().unwrap()
    }

    #[test]
    fn hybrid_expands_to_prefetches() {
        let json = parse_route(
            "QUERY HYBRID TEXT 'ai search' DENSE dense SPARSE sparse FUSION RRF FROM docs LIMIT 10;",
        );
        assert_eq!(json["query"]["fusion"], "rrf");
        assert!(json["query"].get("nearest").is_none());
        let pf = json["prefetch"].as_array().unwrap();
        assert_eq!(pf.len(), 2);
    }

    #[test]
    fn nearest_text_is_string() {
        let json = parse_route("QUERY 'hello world' FROM docs LIMIT 5;");
        assert_eq!(json["query"]["nearest"], "hello world");
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
            "QUERY MMR TEXT 'query' DIVERSITY 0.4 CANDIDATES 100 FROM docs USING dense LIMIT 5;",
        );
        assert_eq!(json["query"]["nearest"], "query");
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
            "QUERY NEAREST POINT 42 FROM docs USING dense PREFETCH (QUERY TEXT 'x' FROM docs USING dense LIMIT 50) LIMIT 10;",
        );
        let pf = &json["prefetch"][0];
        assert_eq!(pf["query"]["nearest"], "x");
    }

    #[test]
    fn query_request_no_group_fields() {
        let json = parse_route("QUERY 'hello' FROM docs LIMIT 5;");
        assert!(json.get("group_by").is_none());
        assert!(json.get("group_size").is_none());
    }

    #[test]
    fn grouped_request_has_group_fields() {
        let json = parse_route(
            "QUERY 'news' FROM docs GROUP BY topic SIZE 5 LOOKUP FROM topics LIMIT 20;",
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
            "WITH a AS (QUERY 'x' FROM docs USING dense LIMIT 100) QUERY FUSION RRF FROM docs PREFETCH (a) LIMIT 10;",
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
            "QUERY RERANK TEXT 'travel tips' MODEL 'colbert-v2' FROM docs USING colbert PREFETCH (QUERY 'travel tips' FROM docs USING dense LIMIT 50) LIMIT 10;",
        );
        assert_eq!(json["using"], "colbert");
        let nearest = &json["query"]["nearest"];
        assert_eq!(nearest["text"], "travel tips");
        assert_eq!(nearest["model"], "colbert-v2");
        assert_eq!(json["prefetch"].as_array().unwrap().len(), 1);
    }
}
