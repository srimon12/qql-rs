use super::*;
use qdrant_edge::OrderByInterface;
use qql_plan::types::{
    DiscoverQuery as PlanDiscover, FormulaDefault, FormulaQuery, NearestQuery, OrderByQuery,
    PlanFormula, RecommendQuery, RelevanceFeedbackInput,
};
use qql_plan::{PlanPointId, PlanQueryInput, PlanVectorValue};

/// Minimal grouped request over a dense nearest query on `docs`.
fn groups_request(field: &str, limit: u64, group_size: u64) -> qql_plan::types::QueryGroupsRequest {
    qql_plan::types::QueryGroupsRequest {
        query: QueryVariant::Nearest(NearestQuery {
            nearest: PlanQueryInput::Vector(PlanVectorValue::Dense(vec![1.0, 0.0, 0.0])),
            mmr: None,
        }),
        using: Some("dense".to_string()),
        prefetch: Vec::new(),
        filter: None,
        params: None,
        score_threshold: None,
        with_payload: None,
        with_vector: None,
        group_by: field.to_string(),
        group_size,
        limit,
        with_lookup: None,
        lookup_from: None,
        shard_key: None,
        timeout: None,
        consistency: None,
        group_offset: None,
    }
}

#[test]
fn test_nearest_dense_conversion() {
    let query = QueryVariant::Nearest(NearestQuery {
        nearest: PlanQueryInput::Vector(PlanVectorValue::Dense(vec![1.0, 2.0, 3.0])),
        mmr: None,
    });
    let result = convert_query(&query, Some("dense"));
    assert!(result.is_ok());
    match result.unwrap() {
        ScoringQuery::Vector(QueryEnum::Nearest(named)) => {
            assert_eq!(named.query, VectorInternal::Dense(vec![1.0, 2.0, 3.0]));
            assert_eq!(named.using, Some("dense".to_string()));
        }
        other => panic!("expected Nearest, got {other:?}"),
    }
}

#[test]
fn test_nearest_sparse_conversion() {
    let query = QueryVariant::Nearest(NearestQuery {
        nearest: PlanQueryInput::Vector(PlanVectorValue::Sparse {
            indices: vec![0, 2],
            values: vec![0.5, 0.8],
        }),
        mmr: None,
    });
    let result = convert_query(&query, Some("sparse"));
    assert!(result.is_ok());
}

#[test]
fn test_nearest_mmr_conversion() {
    let query = QueryVariant::Nearest(NearestQuery {
        nearest: PlanQueryInput::Vector(PlanVectorValue::Dense(vec![1.0, 2.0, 3.0])),
        mmr: Some(qql_plan::types::MmrQueryParams {
            diversity: 0.4,
            candidates_limit: 100,
        }),
    });
    let result = convert_query(&query, Some("dense"));
    assert!(result.is_ok());
    match result.unwrap() {
        ScoringQuery::Mmr(mmr) => {
            assert_eq!(mmr.lambda, OrderedFloat(0.4));
            assert_eq!(mmr.candidates_limit, 100);
            assert_eq!(mmr.vector, VectorInternal::Dense(vec![1.0, 2.0, 3.0]));
            assert_eq!(mmr.using, "dense");
        }
        other => panic!("expected Mmr, got {other:?}"),
    }
}

#[test]
fn test_recommend_best_score_conversion() {
    let query = QueryVariant::Recommend {
        recommend: RecommendQuery {
            positive: vec![PlanQueryInput::Vector(PlanVectorValue::Dense(vec![
                1.0, 0.0, 0.0,
            ]))],
            negative: vec![PlanQueryInput::Vector(PlanVectorValue::Dense(vec![
                0.0, 1.0, 0.0,
            ]))],
            strategy: Some("best_score".to_string()),
        },
    };
    let result = convert_query(&query, Some("dense"));
    assert!(result.is_ok());
    match result.unwrap() {
        ScoringQuery::Vector(QueryEnum::RecommendBestScore(named)) => {
            assert_eq!(named.query.positives.len(), 1);
            assert_eq!(named.query.negatives.len(), 1);
            assert_eq!(named.using, Some("dense".to_string()));
        }
        other => panic!("expected RecommendBestScore, got {other:?}"),
    }
}

#[test]
fn test_recommend_sum_scores_conversion() {
    let query = QueryVariant::Recommend {
        recommend: RecommendQuery {
            positive: vec![PlanQueryInput::Vector(PlanVectorValue::Dense(vec![
                1.0, 0.0, 0.0,
            ]))],
            negative: vec![],
            strategy: Some("sum_scores".to_string()),
        },
    };
    let result = convert_query(&query, Some("dense"));
    assert!(result.is_ok());
    match result.unwrap() {
        ScoringQuery::Vector(QueryEnum::RecommendSumScores(named)) => {
            assert_eq!(named.query.positives.len(), 1);
        }
        other => panic!("expected RecommendSumScores, got {other:?}"),
    }
}

#[test]
fn test_recommend_omitted_strategy_rejected_as_average_vector() {
    let query = QueryVariant::Recommend {
        recommend: RecommendQuery {
            positive: vec![PlanQueryInput::Vector(PlanVectorValue::Dense(vec![
                1.0, 0.0, 0.0,
            ]))],
            negative: vec![],
            strategy: None,
        },
    };
    let error = convert_query(&query, Some("dense")).expect_err("default strategy");
    assert_eq!(error.code, "QQL-EDGE-UNSUPPORTED-RECOMMEND-STRATEGY");
    assert!(
        error.message.contains("average_vector"),
        "{}",
        error.message
    );
}

#[test]
fn test_recommend_average_vector_rejected() {
    let query = QueryVariant::Recommend {
        recommend: RecommendQuery {
            positive: vec![PlanQueryInput::Vector(PlanVectorValue::Dense(vec![
                1.0, 0.0, 0.0,
            ]))],
            negative: vec![],
            strategy: Some("average_vector".to_string()),
        },
    };
    let result = convert_query(&query, Some("dense"));
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("average_vector"));
}

#[test]
fn test_recommend_point_rejected() {
    let query = QueryVariant::Recommend {
        recommend: RecommendQuery {
            positive: vec![PlanQueryInput::Point(PlanPointId::Number(42))],
            negative: vec![],
            strategy: None,
        },
    };
    let result = convert_query(&query, None);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("point-id"));
}

#[test]
fn test_context_conversion() {
    let query = QueryVariant::Context {
        context: vec![qql_plan::types::ContextPair {
            positive: PlanQueryInput::Vector(PlanVectorValue::Dense(vec![1.0, 0.0, 0.0])),
            negative: PlanQueryInput::Vector(PlanVectorValue::Dense(vec![0.0, 1.0, 0.0])),
        }],
    };
    let result = convert_query(&query, Some("dense"));
    assert!(result.is_ok());
    match result.unwrap() {
        ScoringQuery::Vector(QueryEnum::Context(named)) => {
            assert_eq!(named.query.pairs.len(), 1);
            assert_eq!(named.using, Some("dense".to_string()));
        }
        other => panic!("expected Context, got {other:?}"),
    }
}

#[test]
fn test_discover_conversion() {
    let query = QueryVariant::Discover {
        discover: PlanDiscover {
            target: PlanQueryInput::Vector(PlanVectorValue::Dense(vec![1.0, 0.0, 0.0])),
            context: vec![qql_plan::types::ContextPair {
                positive: PlanQueryInput::Vector(PlanVectorValue::Dense(vec![0.5, 0.5, 0.0])),
                negative: PlanQueryInput::Vector(PlanVectorValue::Dense(vec![0.0, 0.0, 1.0])),
            }],
        },
    };
    let result = convert_query(&query, Some("dense"));
    assert!(result.is_ok());
    match result.unwrap() {
        ScoringQuery::Vector(QueryEnum::Discover(named)) => {
            assert_eq!(named.query.pairs.len(), 1);
            assert_eq!(named.using, Some("dense".to_string()));
        }
        other => panic!("expected Discover, got {other:?}"),
    }
}

#[test]
fn test_order_by_conversion() {
    let query = QueryVariant::OrderBy {
        order_by: OrderByQuery {
            key: "created_at".to_string(),
            direction: Some("desc".to_string()),
            start_from: None,
        },
    };
    let result = convert_query(&query, None);
    assert!(result.is_ok());
    match result.unwrap() {
        ScoringQuery::OrderBy(order_by) => {
            assert_eq!(order_by.key.to_string(), "created_at");
            assert_eq!(order_by.direction, Some(Direction::Desc));
        }
        other => panic!("expected OrderBy, got {other:?}"),
    }
}

/// ACORN params are a first-class `SearchParams` field on qdrant-edge
/// 0.8; the typed converter must pass them through, not reject them.
#[test]
fn test_acorn_search_params_conversion() {
    let params = SearchParamsRequest {
        hnsw_ef: None,
        exact: None,
        acorn: Some(qql_plan::types::AcornSearchParams {
            enable: true,
            max_selectivity: Some(0.4),
        }),
        indexed_only: None,
        quantization: None,
        idf: None,
    };
    let converted = convert_search_params(&params).expect("acorn conversion");
    let acorn = converted.acorn.expect("acorn present");
    assert!(acorn.enable);
    assert_eq!(acorn.max_selectivity, Some(OrderedFloat(0.4)));

    let disabled = convert_search_params(&SearchParamsRequest {
        acorn: Some(qql_plan::types::AcornSearchParams {
            enable: false,
            max_selectivity: None,
        }),
        ..params
    })
    .expect("acorn conversion");
    assert_eq!(
        disabled.acorn,
        Some(qdrant_edge::AcornSearchParams {
            enable: false,
            max_selectivity: None,
        })
    );
}

/// A grouped request lowers onto `GroupRequest` with the plan `limit` as
/// the group count and `group_size` as the hits per group.
#[test]
fn test_group_request_conversion() {
    let request = groups_request("district", 5, 3);
    let converted = convert_query_groups_request(&request).expect("group conversion");
    assert_eq!(converted.group_by.to_string(), "district");
    assert_eq!(converted.groups, 5);
    assert_eq!(converted.group_size, 3);
    assert_eq!(converted.query.limit, 5);
    assert_eq!(converted.query.offset, 0);
    assert_eq!(
        converted.query.with_payload,
        WithPayloadInterface::Bool(true),
        "payloads default on"
    );
    assert_eq!(converted.query.with_vector, WithVector::Bool(false));
    assert!(matches!(
        converted.query.query,
        Some(ScoringQuery::Vector(QueryEnum::Nearest(_)))
    ));
}

/// `GROUP BY … LOOKUP FROM <collection>` is the only grouped sub-feature
/// qdrant-edge cannot represent; it must fail on its own code.
#[test]
fn test_group_lookup_rejected() {
    let mut request = groups_request("district", 5, 3);
    request.with_lookup = Some(qql_plan::types::WithLookupValue::Collection(
        "districts".to_string(),
    ));
    let error = convert_query_groups_request(&request).expect_err("lookup must fail");
    assert_eq!(error.code, "QQL-EDGE-UNSUPPORTED-GROUP-LOOKUP");

    let mut request = groups_request("district", 5, 3);
    request.lookup_from = Some(qql_plan::types::LookupRequest {
        collection: "other".to_string(),
        vector: None,
        shard_key: None,
    });
    let error = convert_query_groups_request(&request).expect_err("lookup_from must fail");
    assert_eq!(error.code, "QQL-EDGE-UNSUPPORTED-GROUP-LOOKUP");
}

/// Hydration selectors mirror the plain query defaults.
#[test]
fn test_group_output_defaults() {
    let request = groups_request("district", 5, 3);
    let (with_payload, with_vector) = convert_group_output(&request).expect("output selectors");
    assert_eq!(with_payload, WithPayloadInterface::Bool(true));
    assert_eq!(with_vector, WithVector::Bool(false));
}

#[test]
fn test_sample_random_conversion() {
    let query = QueryVariant::Sample {
        sample: "random".to_string(),
    };
    let result = convert_query(&query, None);
    assert!(result.is_ok());
    match result.unwrap() {
        ScoringQuery::Sample(sample) => {
            assert_eq!(sample, Sample::Random);
        }
        other => panic!("expected Sample, got {other:?}"),
    }
}
#[test]
fn test_unsupported_sample_rejected() {
    let query = QueryVariant::Sample {
        sample: "reservoir".to_string(),
    };
    let result = convert_query(&query, None);
    assert!(result.is_err());
}

#[test]
fn test_formula_simple_conversion() {
    let expr = PlanFormula::Sum {
        left: Box::new(PlanFormula::Constant(1.0)),
        right: Box::new(PlanFormula::Variable("$score".to_string())),
    };
    let query = QueryVariant::Formula(FormulaQuery {
        formula: expr,
        defaults: None,
    });
    let result = convert_query(&query, None);
    assert!(
        result.is_ok(),
        "formula conversion should succeed: {:?}",
        result.err()
    );
}

#[test]
fn test_formula_with_defaults() {
    let expr = PlanFormula::Mul {
        left: Box::new(PlanFormula::Variable("$score".to_string())),
        right: Box::new(PlanFormula::Constant(2.0)),
    };
    let defaults =
        std::collections::BTreeMap::from([("score".to_string(), FormulaDefault::Float(0.0))]);
    let query = QueryVariant::Formula(FormulaQuery {
        formula: expr,
        defaults: Some(defaults),
    });
    let result = convert_query(&query, None);
    assert!(result.is_ok(), "formula with defaults: {:?}", result.err());
}

/// MAX / MIN / ACOSH parse in QQL but the pinned qdrant-edge has no
/// matching Expression variants — must fail closed with the catalog code.
#[test]
fn test_formula_nary_acosh_fail_closed() {
    for expr in [
        PlanFormula::Max(vec![PlanFormula::Constant(1.0), PlanFormula::Constant(2.0)]),
        PlanFormula::Min(vec![PlanFormula::Constant(1.0)]),
        PlanFormula::Acosh(Box::new(PlanFormula::Constant(1.0))),
    ] {
        let query = QueryVariant::Formula(FormulaQuery {
            formula: expr,
            defaults: None,
        });
        let result = convert_query(&query, None);
        let err = result.expect_err("n-ary/acosh formula must fail closed offline");
        assert_eq!(
            err.code, "QQL-EDGE-UNSUPPORTED-FORMULA-FUNCTION",
            "wrong code for {err}"
        );
    }
}

/// A typed `$score` reference must map to the reserved score variable,
/// not to a payload path named `score`.
#[test]
fn test_formula_score_variable_is_not_a_payload_var() {
    let query = QueryVariant::Formula(FormulaQuery {
        formula: PlanFormula::Variable("$score".to_string()),
        defaults: None,
    });
    match convert_query(&query, None).expect("formula conversion") {
        ScoringQuery::Formula(parsed) => {
            assert!(
                parsed.payload_vars.is_empty(),
                "expected reserved score variable, got payload vars {:?}",
                parsed.payload_vars
            );
        }
        other => panic!("expected Formula, got {other:?}"),
    }
}

#[test]
fn test_formula_decay_datetime_key_and_case_conversion() {
    let decay = PlanFormula::Decay {
        kind: qql_plan::types::PlanDecayKind::Exp,
        x: Box::new(PlanFormula::DatetimeKey("judgment_date".to_string())),
        target: Some(Box::new(PlanFormula::Datetime(
            "2026-09-04T00:00:00Z".to_string(),
        ))),
        scale: Some(630720000.0),
        midpoint: Some(0.5),
    };
    let query = QueryVariant::Formula(FormulaQuery {
        formula: decay,
        defaults: None,
    });
    assert!(convert_query(&query, None).is_ok(), "datetime decay");

    let condition = qql_plan::types::FilterExpression::Single(Box::new(
        qql_plan::types::FilterClause::Field(Box::new(qql_plan::types::FieldCondition {
            key: "status".to_string(),
            r#match: Some(qql_plan::types::MatchValue::Value {
                value: serde_json::json!("active"),
            }),
            ..Default::default()
        })),
    ));
    let case = PlanFormula::Case {
        cond: Box::new(PlanFormula::Condition(condition)),
        then_: Box::new(PlanFormula::Constant(1.0)),
        else_: Box::new(PlanFormula::Constant(0.0)),
    };
    let query = QueryVariant::Formula(FormulaQuery {
        formula: case,
        defaults: None,
    });
    assert!(convert_query(&query, None).is_ok(), "case condition");
}

#[test]
fn test_relevance_feedback_conversion() {
    let query = QueryVariant::RelevanceFeedback {
        relevance_feedback: RelevanceFeedbackInput {
            target: PlanQueryInput::Vector(PlanVectorValue::Dense(vec![1.0, 0.0, 0.0])),
            feedback: vec![qql_plan::types::FeedbackItem {
                example: PlanQueryInput::Vector(PlanVectorValue::Dense(vec![0.5, 0.5, 0.0])),
                score: 0.8,
            }],
            strategy: qql_plan::types::FeedbackStrategy {
                naive: qql_plan::types::NaiveFeedbackStrategyParams {
                    a: 1.0,
                    b: 0.5,
                    c: 0.5,
                },
            },
        },
    };
    let result = convert_query(&query, Some("dense"));
    assert!(result.is_ok());
    match result.unwrap() {
        ScoringQuery::Vector(QueryEnum::FeedbackNaive(named)) => {
            assert_eq!(named.using, Some("dense".to_string()));
            assert_eq!(named.query.feedback.len(), 1);
            assert_eq!(named.query.coefficients.a, OrderedFloat(1.0));
        }
        other => panic!("expected FeedbackNaive, got {other:?}"),
    }
}

#[test]
fn test_multidense_accepted() {
    let result =
        plan_input_to_vector_internal(&PlanQueryInput::Vector(PlanVectorValue::MultiDense(vec![
            vec![1.0, 2.0],
            vec![3.0, 4.0],
        ])));
    assert!(result.is_ok(), "edge must accept MultiDense query vectors");
    match result.unwrap() {
        VectorInternal::MultiDense(_) => {}
        other => panic!("expected MultiDense, got {other:?}"),
    }
}

#[test]
fn test_empty_dense_rejected() {
    let result =
        plan_input_to_vector_internal(&PlanQueryInput::Vector(PlanVectorValue::Dense(vec![])));
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("empty"));
}

#[test]
fn test_convert_order_by_interface() {
    let ob = OrderByQuery {
        key: "price".to_string(),
        direction: Some("asc".to_string()),
        start_from: None,
    };
    let result = convert_order_by_interface(&ob).unwrap();
    match result {
        OrderByInterface::Struct(s) => {
            assert_eq!(s.key.to_string(), "price");
            assert_eq!(s.direction, Some(Direction::Asc));
        }
        _ => panic!("expected Struct variant"),
    }
}

/// Build a `PlanQueryRequest` with the given filter over a simple dense
/// nearest query.
fn request_with_filter(filter: qql_plan::types::FilterExpression) -> PlanQueryRequest {
    PlanQueryRequest {
        query: QueryVariant::Nearest(NearestQuery {
            nearest: PlanQueryInput::Vector(PlanVectorValue::Dense(vec![1.0, 2.0, 3.0])),
            mmr: None,
        }),
        using: Some("dense".to_string()),
        prefetch: Vec::new(),
        filter: Some(filter),
        params: None,
        score_threshold: None,
        with_payload: None,
        with_vector: None,
        limit: Some(10),
        offset: None,
        lookup_from: None,
        shard_key: None,
        timeout: None,
        consistency: None,
    }
}

fn city_match() -> qql_plan::types::FilterClause {
    qql_plan::types::FilterClause::Field(Box::new(qql_plan::types::FieldCondition {
        key: "city".to_string(),
        r#match: Some(qql_plan::types::MatchValue::Value {
            value: serde_json::json!("NYC"),
        }),
        range: None,
        geo_bounding_box: None,
        geo_radius: None,
        geo_polygon: None,
        values_count: None,
        is_empty: None,
        is_null: None,
    }))
}

#[test]
fn test_single_condition_filter_is_wrapped_in_must() {
    // A bare condition filter (`{"key": ...}`) must be wrapped as
    // `{"must": [...]}` to match the qdrant-edge Filter schema. This pins
    // the behaviour of the single canonical JSON→Filter converter now
    // shared by the query, scroll, count, and mutation paths.
    let request = request_with_filter(qql_plan::types::FilterExpression::Single(Box::new(
        city_match(),
    )));
    let converted = convert_query_request(&request).expect("conversion succeeds");
    let filter_value = serde_json::to_value(converted.filter.as_ref().expect("filter present"))
        .expect("filter serializes");
    assert_eq!(
        filter_value,
        serde_json::json!({
            "must": [{ "key": "city", "match": { "value": "NYC" } }]
        })
    );
}

#[test]
fn test_compound_filter_passes_through_unchanged() {
    // A full filter object (`{"must": [...]}`) is already in Filter
    // schema and must not be double-wrapped.
    let request = request_with_filter(qql_plan::types::FilterExpression::Compound(
        qql_plan::types::FilterCompound {
            must: vec![city_match()],
            must_not: vec![],
            should: vec![],
            min_should: None,
        },
    ));
    let converted = convert_query_request(&request).expect("conversion succeeds");
    let filter_value = serde_json::to_value(converted.filter.as_ref().expect("filter present"))
        .expect("filter serializes");
    assert_eq!(
        filter_value,
        serde_json::json!({
            "must": [{ "key": "city", "match": { "value": "NYC" } }]
        })
    );
}
