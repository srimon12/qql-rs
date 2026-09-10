use std::collections::HashMap;

use qdrant_edge::external::ordered_float::OrderedFloat;
use qdrant_edge::{
    ContextPair as EdgeContextPair, ContextQuery, DecayKind, Direction, DiscoverQuery, Fusion,
    GeoPoint, JsonPath, Mmr, NamedQuery, OrderBy, OrderByInterface, PayloadSelectorExclude,
    PayloadSelectorInclude, Prefetch, QueryEnum, QueryRequest, RecommendQuery, Sample,
    ScoringQuery, SearchParams, VectorInternal, WithPayloadInterface, WithVector,
};

use qql_core::error::QqlError;
use qql_plan::types::{
    FilterExpression, FormulaDefault, PayloadSelectorReq, PlanDecayKind, PlanFormula,
    PrefetchRequest, QueryRequest as PlanQueryRequest, QueryVariant, SearchParamsRequest,
    VectorSelectorReq,
};
use qql_plan::{PlanQueryInput, PlanVectorValue};

use super::error_map::{EdgeOp, edge_err};
use super::filter_converter::convert_formula_condition;

/// Fields shared by the single-query and grouped-query request bodies, so the
/// two converters cannot drift.
struct SharedQueryFields<'a> {
    query: &'a QueryVariant,
    using: Option<&'a str>,
    prefetch: &'a [PrefetchRequest],
    filter: Option<&'a FilterExpression>,
    params: Option<&'a SearchParamsRequest>,
    score_threshold: Option<f64>,
    with_payload: Option<&'a PayloadSelectorReq>,
    with_vector: Option<&'a VectorSelectorReq>,
    limit: u64,
    offset: u64,
}

/// Reject the request-level fields qdrant-edge has no slot for.
fn reject_request_level(
    shard_key: Option<&qql_plan::semantic::PlanShardKey>,
    timeout: Option<u64>,
    consistency: Option<&qql_plan::types::ReadConsistencyParam>,
) -> Result<(), QqlError> {
    if shard_key.is_some() {
        return Err(unsupported_shard());
    }
    if timeout.is_some() {
        return Err(crate::backend::unsupported::EdgeUnsupported::Timeout.error());
    }
    if consistency.is_some() {
        return Err(crate::backend::unsupported::EdgeUnsupported::Consistency.error());
    }
    Ok(())
}

fn convert_shared_query(fields: SharedQueryFields<'_>) -> Result<QueryRequest, QqlError> {
    Ok(QueryRequest {
        prefetches: fields
            .prefetch
            .iter()
            .map(convert_prefetch)
            .collect::<Result<_, _>>()?,
        query: Some(convert_query(fields.query, fields.using)?),
        filter: super::convert_edge_filter(fields.filter)?,
        score_threshold: fields.score_threshold.map(|score| score as f32),
        limit: usize::try_from(fields.limit).map_err(limit_error)?,
        offset: usize::try_from(fields.offset).map_err(limit_error)?,
        params: fields.params.map(convert_search_params).transpose()?,
        with_vector: fields
            .with_vector
            .map(convert_with_vector)
            .unwrap_or(WithVector::Bool(false)),
        with_payload: fields
            .with_payload
            .map(convert_with_payload)
            .transpose()?
            .unwrap_or(WithPayloadInterface::Bool(true)),
    })
}

pub(crate) fn convert_query_request(request: &PlanQueryRequest) -> Result<QueryRequest, QqlError> {
    reject_request_level(
        request.shard_key.as_ref(),
        request.timeout,
        request.consistency.as_ref(),
    )?;
    convert_shared_query(SharedQueryFields {
        query: &request.query,
        using: request.using.as_deref(),
        prefetch: &request.prefetch,
        filter: request.filter.as_ref(),
        params: request.params.as_ref(),
        score_threshold: request.score_threshold,
        with_payload: request.with_payload.as_ref(),
        with_vector: request.with_vector.as_ref(),
        limit: request.limit.unwrap_or(10),
        offset: request.offset.unwrap_or(0),
    })
}

/// Lower a grouped query request into a `qdrant-edge` [`GroupRequest`].
///
/// `groups` is the plan's `limit` (the executor trims `group_offset`
/// client-side, exactly as for REST/gRPC) and `group_size` is the points per
/// group. The base query keeps the caller's output selectors; the edge backend
/// hydrates the distilled hits from a `retrieve` because qdrant-edge's
/// grouping driver only requests the `group_by` field for candidates.
pub(crate) fn convert_query_groups_request(
    request: &qql_plan::types::QueryGroupsRequest,
) -> Result<qdrant_edge::GroupRequest, QqlError> {
    if request.with_lookup.is_some() || request.lookup_from.is_some() {
        return Err(crate::backend::unsupported::EdgeUnsupported::GroupLookup.error());
    }
    reject_request_level(
        request.shard_key.as_ref(),
        request.timeout,
        request.consistency.as_ref(),
    )?;
    let query = convert_shared_query(SharedQueryFields {
        query: &request.query,
        using: request.using.as_deref(),
        prefetch: &request.prefetch,
        filter: request.filter.as_ref(),
        params: request.params.as_ref(),
        score_threshold: request.score_threshold,
        with_payload: request.with_payload.as_ref(),
        with_vector: request.with_vector.as_ref(),
        // The grouping driver shapes its own candidate limit/offset
        // (`shape_candidates_query` overwrites both on every request).
        limit: request.limit,
        offset: 0,
    })?;
    Ok(qdrant_edge::GroupRequest::new(
        query,
        parse_json_path(&request.group_by)?,
        usize::try_from(request.limit).map_err(limit_error)?,
        usize::try_from(request.group_size).map_err(limit_error)?,
    ))
}

/// Output selectors for a grouped request, with the same defaults the plain
/// query path applies (payloads on, vectors off). Used to hydrate the final
/// group hits after qdrant-edge's candidate fetch.
pub(crate) fn convert_group_output(
    request: &qql_plan::types::QueryGroupsRequest,
) -> Result<(WithPayloadInterface, WithVector), QqlError> {
    Ok((
        request
            .with_payload
            .as_ref()
            .map(convert_with_payload)
            .transpose()?
            .unwrap_or(WithPayloadInterface::Bool(true)),
        request
            .with_vector
            .as_ref()
            .map(convert_with_vector)
            .unwrap_or(WithVector::Bool(false)),
    ))
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
        filter: super::convert_edge_filter(request.filter.as_ref())?,
        score_threshold: request.score_threshold.map(|score| score as f32),
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
            let expression = plan_formula_to_edge(&formula.formula)?;
            let defaults = formula
                .defaults
                .as_ref()
                .map(|defaults| {
                    defaults
                        .iter()
                        .map(|(key, value)| (key.clone(), formula_default_to_json(value)))
                        .collect::<HashMap<_, _>>()
                })
                .unwrap_or_default();

            let edge_formula = qdrant_edge::Formula {
                formula: expression,
                defaults,
            };
            let parsed = edge_formula
                .try_into()
                .map_err(|e: qdrant_edge::OperationError| edge_err(EdgeOp::Formula, None, e))?;
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
        PlanQueryInput::Vector(PlanVectorValue::Param(name)) => Err(edge_error(format!(
            "unbound parameter ':{name}' reached edge query execution"
        ))),
        PlanQueryInput::Vector(PlanVectorValue::PositionalParam(idx)) => Err(edge_error(format!(
            "unbound positional parameter ?{idx} reached edge query execution"
        ))),
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

fn plan_formula_to_edge(expr: &PlanFormula) -> Result<qdrant_edge::Expression, QqlError> {
    use qdrant_edge::Expression;
    Ok(match expr {
        PlanFormula::Constant(value) => Expression::Constant(*value as f32),
        PlanFormula::Variable(name) => Expression::Variable(name.clone()),
        PlanFormula::Sum { left, right } => Expression::Sum(vec![
            plan_formula_to_edge(left)?,
            plan_formula_to_edge(right)?,
        ]),
        PlanFormula::Sub { left, right } => Expression::Sum(vec![
            plan_formula_to_edge(left)?,
            Expression::Neg(Box::new(plan_formula_to_edge(right)?)),
        ]),
        PlanFormula::Mul { left, right } => Expression::Mult(vec![
            plan_formula_to_edge(left)?,
            plan_formula_to_edge(right)?,
        ]),
        PlanFormula::Div {
            left,
            right,
            by_zero_default,
        } => Expression::Div {
            left: Box::new(plan_formula_to_edge(left)?),
            right: Box::new(plan_formula_to_edge(right)?),
            by_zero_default: by_zero_default.map(|value| value as f32),
        },
        PlanFormula::Neg(operand) => Expression::Neg(Box::new(plan_formula_to_edge(operand)?)),
        PlanFormula::Abs(x) => Expression::Abs(Box::new(plan_formula_to_edge(x)?)),
        PlanFormula::Sqrt(x) => Expression::Sqrt(Box::new(plan_formula_to_edge(x)?)),
        PlanFormula::Log10(x) => Expression::Log10(Box::new(plan_formula_to_edge(x)?)),
        PlanFormula::Ln(x) => Expression::Ln(Box::new(plan_formula_to_edge(x)?)),
        PlanFormula::Exp(x) => Expression::Exp(Box::new(plan_formula_to_edge(x)?)),
        // MAX / MIN / ACOSH are parseable QQL but the pinned qdrant-edge has
        // no matching Expression variants — fail with the catalog error.
        PlanFormula::Acosh(_) | PlanFormula::Max(_) | PlanFormula::Min(_) => {
            return Err(crate::backend::unsupported::EdgeUnsupported::FormulaNary.error());
        }
        PlanFormula::Pow { base, exponent } => Expression::Pow {
            base: Box::new(plan_formula_to_edge(base)?),
            exponent: Box::new(plan_formula_to_edge(exponent)?),
        },
        PlanFormula::GeoDistance { lat, lon, field } => Expression::GeoDistance {
            origin: GeoPoint {
                lat: OrderedFloat(*lat),
                lon: OrderedFloat(*lon),
            },
            to: parse_json_path(field)?,
        },
        PlanFormula::Decay {
            kind,
            x,
            target,
            scale,
            midpoint,
        } => Expression::Decay {
            kind: match kind {
                PlanDecayKind::Lin => DecayKind::Lin,
                PlanDecayKind::Exp => DecayKind::Exp,
                PlanDecayKind::Gauss => DecayKind::Gauss,
            },
            x: Box::new(plan_formula_to_edge(x)?),
            target: target
                .as_ref()
                .map(|target| plan_formula_to_edge(target).map(Box::new))
                .transpose()?,
            midpoint: midpoint.map(|value| value as f32),
            scale: scale.map(|value| value as f32),
        },
        PlanFormula::Condition(filter) => {
            Expression::Condition(Box::new(convert_formula_condition(filter)?))
        }
        PlanFormula::Case { cond, then_, else_ } => {
            // Same weighting as the REST lowering: cond * then + (1 - cond) * else.
            let condition = plan_formula_to_edge(cond)?;
            let one_minus = Expression::Sum(vec![
                Expression::Constant(1.0),
                Expression::Neg(Box::new(condition.clone())),
            ]);
            Expression::Sum(vec![
                Expression::Mult(vec![condition, plan_formula_to_edge(then_)?]),
                Expression::Mult(vec![one_minus, plan_formula_to_edge(else_)?]),
            ])
        }
        PlanFormula::Datetime(value) => Expression::Datetime(value.clone()),
        PlanFormula::DatetimeKey(key) => Expression::DatetimeKey(parse_json_path(key)?),
    })
}

/// Convert a typed `DEFAULTS` binding to the edge API's JSON value domain.
fn formula_default_to_json(value: &FormulaDefault) -> serde_json::Value {
    match value {
        FormulaDefault::Null => serde_json::Value::Null,
        FormulaDefault::Bool(value) => serde_json::Value::Bool(*value),
        FormulaDefault::Int(value) => serde_json::Value::Number((*value).into()),
        FormulaDefault::Float(value) => serde_json::Number::from_f64(*value)
            .map_or(serde_json::Value::Null, serde_json::Value::Number),
        FormulaDefault::String(value) => serde_json::Value::String(value.clone()),
        FormulaDefault::List(values) => {
            serde_json::Value::Array(values.iter().map(formula_default_to_json).collect())
        }
        FormulaDefault::Object(entries) => serde_json::Value::Object(
            entries
                .iter()
                .map(|(key, value)| (key.clone(), formula_default_to_json(value)))
                .collect(),
        ),
    }
}

pub(crate) fn convert_search_params(
    params: &SearchParamsRequest,
) -> Result<SearchParams, QqlError> {
    let idf = params
        .idf
        .as_ref()
        .map(|idf| match idf {
            qql_plan::types::IdfSearchParams::Global => {
                Ok(qdrant_edge::IdfParams::Scope(qdrant_edge::IdfScope::Global))
            }
            qql_plan::types::IdfSearchParams::Corpus { corpus } => Ok(
                qdrant_edge::IdfParams::Corpus(qdrant_edge::IdfCorpusParams {
                    corpus: super::convert_edge_filter(Some(corpus))?.unwrap(),
                }),
            ),
        })
        .transpose()?;
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
        acorn: params
            .acorn
            .as_ref()
            .map(|acorn| qdrant_edge::AcornSearchParams {
                enable: acorn.enable,
                max_selectivity: acorn.max_selectivity.map(OrderedFloat),
            }),
        idf,
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

pub(crate) fn parse_json_path(path: &str) -> Result<JsonPath, QqlError> {
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
        DiscoverQuery as PlanDiscover, FormulaDefault, FormulaQuery, NearestQuery, OrderByQuery,
        PlanFormula, RecommendQuery, RelevanceFeedbackInput,
    };
    use qql_plan::{PlanPointId, PlanQueryInput, PlanVectorValue};

    /// Minimal grouped request over a dense nearest query on `docs`.
    fn groups_request(
        field: &str,
        limit: u64,
        group_size: u64,
    ) -> qql_plan::types::QueryGroupsRequest {
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
}
