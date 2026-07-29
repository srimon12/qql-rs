use std::collections::HashMap;

use qdrant_edge::external::ordered_float::OrderedFloat;
use qdrant_edge::{
    Condition, ContextPair as EdgeContextPair, ContextQuery, DecayKind, Direction, DiscoverQuery,
    Filter, Fusion, GeoPoint, JsonPath, Mmr, NamedQuery, OrderBy, OrderByInterface,
    PayloadSelectorExclude, PayloadSelectorInclude, Prefetch, QueryEnum, QueryRequest,
    RecommendQuery, Sample, ScoringQuery, SearchParams, VectorInternal, WithPayloadInterface,
    WithVector,
};

use qql_core::error::QqlError;
use qql_plan::types::{
    PayloadSelectorReq, PrefetchRequest, QueryRequest as PlanQueryRequest, QueryVariant,
    SearchParamsRequest, VectorSelectorReq,
};
use qql_plan::{PlanQueryInput, PlanVectorValue};

pub(crate) fn convert_query_request(request: &PlanQueryRequest) -> Result<QueryRequest, QqlError> {
    if request.shard_key.is_some() {
        return Err(unsupported_shard());
    }
    Ok(QueryRequest {
        prefetches: request
            .prefetch
            .iter()
            .map(convert_prefetch)
            .collect::<Result<_, _>>()?,
        query: Some(convert_query(&request.query, request.using.as_deref())?),
        filter: convert_filter(request.filter.as_ref())?,
        score_threshold: request
            .score_threshold
            .map(|score| OrderedFloat(score as f32)),
        limit: usize::try_from(request.limit.unwrap_or(10)).map_err(limit_error)?,
        offset: usize::try_from(request.offset.unwrap_or(0)).map_err(limit_error)?,
        params: request
            .params
            .as_ref()
            .map(convert_search_params)
            .transpose()?,
        with_vector: request
            .with_vector
            .as_ref()
            .map(convert_with_vector)
            .unwrap_or(WithVector::Bool(false)),
        with_payload: request
            .with_payload
            .as_ref()
            .map(convert_with_payload)
            .transpose()?
            .unwrap_or(WithPayloadInterface::Bool(true)),
    })
}

fn convert_prefetch(request: &PrefetchRequest) -> Result<Prefetch, QqlError> {
    Ok(Prefetch {
        prefetches: request
            .prefetch
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(convert_prefetch)
            .collect::<Result<_, _>>()?,
        query: request
            .query
            .as_ref()
            .map(|query| convert_query(query, request.using.as_deref()))
            .transpose()?,
        limit: usize::try_from(request.limit.unwrap_or(10)).map_err(limit_error)?,
        params: request
            .params
            .as_ref()
            .map(convert_search_params)
            .transpose()?,
        filter: convert_filter(request.filter.as_ref())?,
        score_threshold: request
            .score_threshold
            .map(|score| OrderedFloat(score as f32)),
    })
}

fn convert_query(query: &QueryVariant, using: Option<&str>) -> Result<ScoringQuery, QqlError> {
    match query {
        QueryVariant::Nearest(nearest) => {
            let vector = plan_input_to_vector_internal(&nearest.nearest)?;
            if let Some(mmr) = &nearest.mmr {
                Ok(ScoringQuery::Mmr(Mmr {
                    vector,
                    using: using.unwrap_or("").into(),
                    lambda: OrderedFloat(mmr.diversity as f32),
                    candidates_limit: usize::try_from(mmr.candidates_limit).map_err(limit_error)?,
                }))
            } else {
                Ok(ScoringQuery::Vector(QueryEnum::Nearest(NamedQuery {
                    query: vector,
                    using: using.map(str::to_string),
                })))
            }
        }
        QueryVariant::Recommend { recommend } => {
            let positives: Vec<VectorInternal> = recommend
                .positive
                .iter()
                .map(plan_input_to_vector_internal)
                .collect::<Result<_, _>>()?;
            let negatives: Vec<VectorInternal> = recommend
                .negative
                .iter()
                .map(plan_input_to_vector_internal)
                .collect::<Result<_, _>>()?;
            let reco = RecommendQuery::new(positives, negatives);
            let strategy = recommend.strategy.as_deref().unwrap_or("average_vector");
            let query_enum = match strategy {
                "best_score" => QueryEnum::RecommendBestScore(NamedQuery {
                    query: reco,
                    using: using.map(str::to_string),
                }),
                "sum_scores" => QueryEnum::RecommendSumScores(NamedQuery {
                    query: reco,
                    using: using.map(str::to_string),
                }),
                "average_vector" => {
                    return Err(
                        crate::backend::unsupported::EdgeUnsupported::RecommendAverageVector
                            .error(),
                    );
                }
                other => {
                    return Err(edge_error(format!(
                        "unsupported recommend strategy '{other}'"
                    )));
                }
            };
            Ok(ScoringQuery::Vector(query_enum))
        }
        QueryVariant::Context { context } => {
            let pairs: Vec<EdgeContextPair<VectorInternal>> = context
                .iter()
                .map(|pair| {
                    let positive = plan_input_to_vector_internal(&pair.positive)?;
                    let negative = plan_input_to_vector_internal(&pair.negative)?;
                    Ok(EdgeContextPair { positive, negative })
                })
                .collect::<Result<_, _>>()?;
            Ok(ScoringQuery::Vector(QueryEnum::Context(NamedQuery {
                query: ContextQuery::new(pairs),
                using: using.map(str::to_string),
            })))
        }
        QueryVariant::Discover { discover } => {
            let target = plan_input_to_vector_internal(&discover.target)?;
            let pairs: Vec<EdgeContextPair<VectorInternal>> = discover
                .context
                .iter()
                .map(|pair| {
                    let positive = plan_input_to_vector_internal(&pair.positive)?;
                    let negative = plan_input_to_vector_internal(&pair.negative)?;
                    Ok(EdgeContextPair { positive, negative })
                })
                .collect::<Result<_, _>>()?;
            Ok(ScoringQuery::Vector(QueryEnum::Discover(NamedQuery {
                query: DiscoverQuery::new(target, pairs),
                using: using.map(str::to_string),
            })))
        }
        QueryVariant::OrderBy { order_by } => {
            let direction = match order_by.direction.as_deref() {
                None | Some("asc") => Direction::Asc,
                Some("desc") => Direction::Desc,
                Some(other) => {
                    return Err(edge_error(format!(
                        "unsupported order_by direction '{other}'"
                    )));
                }
            };
            let key: JsonPath =
                serde_json::from_value(serde_json::Value::String(order_by.key.clone()))
                    .map_err(|e| edge_error(format!("invalid order_by key: {e}")))?;
            Ok(ScoringQuery::OrderBy(OrderBy {
                key,
                direction: Some(direction),
                start_from: None,
            }))
        }
        QueryVariant::Sample { sample } => match sample.as_str() {
            "random" => Ok(ScoringQuery::Sample(Sample::Random)),
            other => Err(edge_error(format!("unsupported sample method '{other}'"))),
        },
        QueryVariant::Fusion { fusion } => match fusion.as_str() {
            "rrf" => Ok(ScoringQuery::Fusion(Fusion::Rrf {
                k: 2,
                weights: None,
            })),
            "dbsf" => Ok(ScoringQuery::Fusion(Fusion::Dbsf)),
            other => Err(edge_error(format!("unsupported fusion method '{other}'"))),
        },
        QueryVariant::Rrf(rrf) => Ok(ScoringQuery::Fusion(Fusion::Rrf {
            k: usize::try_from(rrf.rrf.k.unwrap_or(2)).map_err(limit_error)?,
            weights: rrf.rrf.weights.as_ref().map(|weights| {
                weights
                    .iter()
                    .map(|weight| OrderedFloat(*weight as f32))
                    .collect()
            }),
        })),
        QueryVariant::Formula(formula) => {
            let formula_json = serde_json::to_value(&formula.formula)
                .map_err(|e| edge_error(format!("serialize formula: {e}")))?;
            let expression = json_to_expression(&formula_json)?;
            let defaults = formula
                .defaults
                .as_ref()
                .map(|d| {
                    d.iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect::<HashMap<_, _>>()
                })
                .unwrap_or_default();

            let edge_formula = qdrant_edge::Formula {
                formula: expression,
                defaults,
            };
            let parsed = edge_formula
                .try_into()
                .map_err(|e: qdrant_edge::OperationError| {
                    edge_error(format!("failed to parse formula: {e}"))
                })?;
            Ok(ScoringQuery::Formula(parsed))
        }
        QueryVariant::RelevanceFeedback { relevance_feedback } => {
            let target = plan_input_to_vector_internal(&relevance_feedback.target)?;
            let feedback: Vec<qdrant_edge::FeedbackItem<VectorInternal>> = relevance_feedback
                .feedback
                .iter()
                .map(|item| {
                    let vector = plan_input_to_vector_internal(&item.example)?;
                    Ok(qdrant_edge::FeedbackItem {
                        vector,
                        score: OrderedFloat(item.score as f32),
                    })
                })
                .collect::<Result<_, _>>()?;
            let coefficients = qdrant_edge::NaiveFeedbackStrategy {
                a: OrderedFloat(relevance_feedback.strategy.naive.a as f32),
                b: OrderedFloat(relevance_feedback.strategy.naive.b as f32),
                c: OrderedFloat(relevance_feedback.strategy.naive.c as f32),
            };
            Ok(ScoringQuery::Vector(QueryEnum::FeedbackNaive(NamedQuery {
                query: qdrant_edge::FeedbackNaiveQuery {
                    target,
                    feedback,
                    coefficients,
                },
                using: using.map(str::to_string),
            })))
        }
    }
}

fn plan_input_to_vector_internal(input: &PlanQueryInput) -> Result<VectorInternal, QqlError> {
    match input {
        PlanQueryInput::Vector(PlanVectorValue::Dense(values)) if !values.is_empty() => {
            Ok(VectorInternal::Dense(values.clone()))
        }
        PlanQueryInput::Vector(PlanVectorValue::Sparse { indices, values }) => {
            Ok(VectorInternal::Sparse(qdrant_edge::SparseVector {
                indices: indices.clone(),
                values: values.clone(),
            }))
        }
        PlanQueryInput::Vector(PlanVectorValue::MultiDense(rows)) => {
            if rows.is_empty() {
                return Err(edge_error("multidense query vector cannot be empty"));
            }
            let vec = qdrant_edge::Vector::new_multi(rows.clone())
                .map_err(|e| edge_error(format!("invalid multidense query vector: {e}")))?;
            Ok(vec.0)
        }
        PlanQueryInput::Vector(PlanVectorValue::Dense(_)) => {
            Err(edge_error("dense query vector cannot be empty"))
        }
        PlanQueryInput::Point(_) => {
            Err(crate::backend::unsupported::EdgeUnsupported::PointReferenceQuery.error())
        }
        PlanQueryInput::Document { .. } => Err(edge_error(
            "text input reached edge execution without client-side embedding",
        )),
        PlanQueryInput::Image { .. } => Err(edge_error(
            "image input reached edge execution without client-side embedding",
        )),
    }
}

fn json_to_expression(value: &serde_json::Value) -> Result<qdrant_edge::Expression, QqlError> {
    match value {
        serde_json::Value::Number(n) => {
            let value = n
                .as_f64()
                .ok_or_else(|| edge_error(format!("formula number is not representable: {n}")))?;
            Ok(qdrant_edge::Expression::Constant(value as f32))
        }
        serde_json::Value::String(s) if s == "$score" => {
            Ok(qdrant_edge::Expression::Variable("score".to_string()))
        }
        serde_json::Value::String(s) => Ok(qdrant_edge::Expression::Variable(s.clone())),
        serde_json::Value::Object(obj) => {
            if let Some(arr) = obj.get("sum").and_then(|v| v.as_array()) {
                let exprs: Vec<qdrant_edge::Expression> = arr
                    .iter()
                    .map(json_to_expression)
                    .collect::<Result<_, _>>()?;
                return Ok(qdrant_edge::Expression::Sum(exprs));
            }
            if let Some(arr) = obj.get("mult").and_then(|v| v.as_array()) {
                let exprs: Vec<qdrant_edge::Expression> = arr
                    .iter()
                    .map(json_to_expression)
                    .collect::<Result<_, _>>()?;
                return Ok(qdrant_edge::Expression::Mult(exprs));
            }
            if let Some(val) = obj.get("neg") {
                let expr = json_to_expression(val)?;
                return Ok(qdrant_edge::Expression::Neg(Box::new(expr)));
            }
            if let Some(div_obj) = obj.get("div") {
                let left = div_obj
                    .get("left")
                    .ok_or_else(|| edge_error("div missing 'left'"))?;
                let right = div_obj
                    .get("right")
                    .ok_or_else(|| edge_error("div missing 'right'"))?;
                let by_zero_default = div_obj
                    .get("by_zero_default")
                    .and_then(|v| v.as_f64())
                    .map(|v| v as f32);
                let left_expr = json_to_expression(left)?;
                let right_expr = json_to_expression(right)?;
                return Ok(qdrant_edge::Expression::Div {
                    left: Box::new(left_expr),
                    right: Box::new(right_expr),
                    by_zero_default,
                });
            }
            if let Some(val) = obj.get("abs") {
                let expr = json_to_expression(val)?;
                return Ok(qdrant_edge::Expression::Abs(Box::new(expr)));
            }
            if let Some(val) = obj.get("sqrt") {
                let expr = json_to_expression(val)?;
                return Ok(qdrant_edge::Expression::Sqrt(Box::new(expr)));
            }
            if let Some(val) = obj.get("log10") {
                let expr = json_to_expression(val)?;
                return Ok(qdrant_edge::Expression::Log10(Box::new(expr)));
            }
            if let Some(val) = obj.get("ln") {
                let expr = json_to_expression(val)?;
                return Ok(qdrant_edge::Expression::Ln(Box::new(expr)));
            }
            if let Some(val) = obj.get("exp") {
                let expr = json_to_expression(val)?;
                return Ok(qdrant_edge::Expression::Exp(Box::new(expr)));
            }
            if let Some(pow_obj) = obj.get("pow") {
                let base = pow_obj
                    .get("base")
                    .ok_or_else(|| edge_error("pow missing 'base'"))?;
                let exponent = pow_obj
                    .get("exponent")
                    .ok_or_else(|| edge_error("pow missing 'exponent'"))?;
                let base_expr = json_to_expression(base)?;
                let exponent_expr = json_to_expression(exponent)?;
                return Ok(qdrant_edge::Expression::Pow {
                    base: Box::new(base_expr),
                    exponent: Box::new(exponent_expr),
                });
            }
            if let Some(gd_obj) = obj.get("geo_distance") {
                let origin = gd_obj
                    .get("origin")
                    .ok_or_else(|| edge_error("geo_distance missing 'origin'"))?;
                let lat = origin
                    .get("lat")
                    .and_then(|v| v.as_f64())
                    .ok_or_else(|| edge_error("geo_distance origin missing 'lat'"))?;
                let lon = origin
                    .get("lon")
                    .and_then(|v| v.as_f64())
                    .ok_or_else(|| edge_error("geo_distance origin missing 'lon'"))?;
                let to_str = gd_obj
                    .get("to")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| edge_error("geo_distance missing 'to'"))?;
                let to: JsonPath =
                    serde_json::from_value(serde_json::Value::String(to_str.to_string()))
                        .map_err(|e| edge_error(format!("invalid geo_distance 'to': {e}")))?;
                return Ok(qdrant_edge::Expression::GeoDistance {
                    origin: GeoPoint {
                        lat: OrderedFloat(lat),
                        lon: OrderedFloat(lon),
                    },
                    to,
                });
            }

            for (decay_key, decay_val) in obj.iter() {
                let kind = match decay_key.as_str() {
                    "lin_decay" => Some(DecayKind::Lin),
                    "exp_decay" => Some(DecayKind::Exp),
                    "gauss_decay" => Some(DecayKind::Gauss),
                    _ => None,
                };
                if let Some(kind) = kind {
                    let x_val = decay_val
                        .get("x")
                        .ok_or_else(|| edge_error(format!("{decay_key} missing 'x'")))?;
                    let x = json_to_expression(x_val)?;
                    let target = decay_val
                        .get("target")
                        .map(json_to_expression)
                        .transpose()?
                        .map(Box::new);
                    let midpoint = decay_val
                        .get("midpoint")
                        .and_then(|v| v.as_f64())
                        .map(|v| v as f32);
                    let scale = decay_val
                        .get("scale")
                        .and_then(|v| v.as_f64())
                        .map(|v| v as f32);
                    return Ok(qdrant_edge::Expression::Decay {
                        kind,
                        x: Box::new(x),
                        target,
                        midpoint,
                        scale,
                    });
                }
            }

            if obj.contains_key("key") && obj.contains_key("match") {
                let condition: Condition = serde_json::from_value(value.clone())
                    .map_err(|e| edge_error(format!("invalid formula condition: {e}")))?;
                return Ok(qdrant_edge::Expression::Condition(Box::new(condition)));
            }

            if let Some(dt) = obj.get("datetime").and_then(|v| v.as_str()) {
                return Ok(qdrant_edge::Expression::Datetime(dt.to_string()));
            }

            if let Some(dtk) = obj.get("datetime_key").and_then(|v| v.as_str()) {
                let path: JsonPath =
                    serde_json::from_value(serde_json::Value::String(dtk.to_string()))
                        .map_err(|e| edge_error(format!("invalid datetime_key: {e}")))?;
                return Ok(qdrant_edge::Expression::DatetimeKey(path));
            }

            Err(edge_error(format!(
                "unsupported formula expression: {value}"
            )))
        }
        other => Err(edge_error(format!("unsupported formula value: {other}"))),
    }
}

pub(crate) fn convert_search_params(
    params: &SearchParamsRequest,
) -> Result<SearchParams, QqlError> {
    if params.acorn.is_some() {
        return Err(crate::backend::unsupported::EdgeUnsupported::Acorn.error());
    }
    Ok(SearchParams {
        hnsw_ef: params
            .hnsw_ef
            .map(usize::try_from)
            .transpose()
            .map_err(limit_error)?,
        exact: params.exact.unwrap_or(false),
        quantization: params.quantization.as_ref().map(|quantization| {
            qdrant_edge::QuantizationSearchParams {
                ignore: quantization.ignore.unwrap_or(false),
                rescore: quantization.rescore,
                oversampling: quantization.oversampling,
            }
        }),
        indexed_only: params.indexed_only.unwrap_or(false),
        acorn: None,
    })
}

pub(crate) fn convert_with_payload(
    selector: &PayloadSelectorReq,
) -> Result<WithPayloadInterface, QqlError> {
    match selector {
        PayloadSelectorReq::All(value) => Ok(WithPayloadInterface::Bool(*value)),
        PayloadSelectorReq::Include { include } => Ok(PayloadSelectorInclude::new(
            include
                .iter()
                .map(|path| parse_json_path(path))
                .collect::<Result<_, _>>()?,
        )
        .into()),
        PayloadSelectorReq::Exclude { exclude } => Ok(PayloadSelectorExclude::new(
            exclude
                .iter()
                .map(|path| parse_json_path(path))
                .collect::<Result<_, _>>()?,
        )
        .into()),
    }
}

pub(crate) fn convert_with_vector(selector: &VectorSelectorReq) -> WithVector {
    match selector {
        VectorSelectorReq::All(value) => WithVector::Bool(*value),
        VectorSelectorReq::Names(names) => WithVector::Selector(names.clone()),
    }
}

pub(crate) fn convert_order_by_interface(
    order_by: &qql_plan::types::OrderByQuery,
) -> Result<OrderByInterface, QqlError> {
    let direction = match order_by.direction.as_deref() {
        None | Some("asc") => Some(Direction::Asc),
        Some("desc") => Some(Direction::Desc),
        Some(other) => {
            return Err(edge_error(format!(
                "unsupported order_by direction '{other}'"
            )));
        }
    };
    let key: JsonPath = serde_json::from_value(serde_json::Value::String(order_by.key.clone()))
        .map_err(|e| edge_error(format!("invalid order_by key: {e}")))?;
    Ok(OrderByInterface::Struct(OrderBy {
        key,
        direction,
        start_from: None,
    }))
}

fn convert_filter(filter: Option<&impl serde::Serialize>) -> Result<Option<Filter>, QqlError> {
    let Some(filter) = filter else {
        return Ok(None);
    };
    let mut value = serde_json::to_value(filter)
        .map_err(|error| edge_error(format!("invalid filter: {error}")))?;
    if value.get("key").is_some() {
        value = serde_json::json!({ "must": [value] });
    }
    serde_json::from_value(value)
        .map(Some)
        .map_err(|error| edge_error(format!("invalid filter format: {error}")))
}

fn parse_json_path(path: &str) -> Result<JsonPath, QqlError> {
    serde_json::from_value(serde_json::Value::String(path.to_string()))
        .map_err(|error| edge_error(format!("invalid payload path '{path}': {error}")))
}

fn limit_error(error: std::num::TryFromIntError) -> QqlError {
    edge_error(format!("limit is too large for this platform: {error}"))
}

fn unsupported_shard() -> QqlError {
    crate::backend::unsupported::EdgeUnsupported::ShardRouting.error()
}

fn edge_error(message: impl Into<String>) -> QqlError {
    QqlError::execution("QQL-EDGE-QUERY", message.into(), None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use qql_plan::types::{
        DiscoverQuery as PlanDiscover, FormulaQuery, NearestQuery, OrderByQuery, PlanFormula,
        RecommendQuery, RelevanceFeedbackInput,
    };
    use qql_plan::{PlanPointId, PlanQueryInput, PlanVectorValue};

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
        assert!(result.unwrap_err().to_string().contains("point-reference"));
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
        let expr = qql_core::ast::FormulaExpr::Sum {
            left: Box::new(qql_core::ast::FormulaExpr::Constant { value: 1.0 }),
            right: Box::new(qql_core::ast::FormulaExpr::Variable {
                name: "score".to_string(),
            }),
        };
        let query = QueryVariant::Formula(FormulaQuery {
            formula: PlanFormula(expr),
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
        let expr = qql_core::ast::FormulaExpr::Mul {
            left: Box::new(qql_core::ast::FormulaExpr::Variable {
                name: "score".to_string(),
            }),
            right: Box::new(qql_core::ast::FormulaExpr::Constant { value: 2.0 }),
        };
        let mut defaults = serde_json::Map::new();
        defaults.insert("score".to_string(), serde_json::json!(0.0));
        let query = QueryVariant::Formula(FormulaQuery {
            formula: PlanFormula(expr),
            defaults: Some(defaults),
        });
        let result = convert_query(&query, None);
        assert!(result.is_ok(), "formula with defaults: {:?}", result.err());
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
        let result = plan_input_to_vector_internal(&PlanQueryInput::Vector(
            PlanVectorValue::MultiDense(vec![vec![1.0, 2.0], vec![3.0, 4.0]]),
        ));
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
}
