//! Plan query requests → qdrant-edge request conversion.
//!
//! Split by concern: formula lowering lives in [`formula`], search params and
//! output selectors in [`selectors`]; both keep the same
//! `backend::query_converter::…` call paths through the re-exports below.

use std::collections::HashMap;

use qdrant_edge::external::ordered_float::OrderedFloat;
use qdrant_edge::{
    ContextPair as EdgeContextPair, ContextQuery, Direction, DiscoverQuery, Fusion, JsonPath, Mmr,
    NamedQuery, OrderBy, Prefetch, QueryEnum, QueryRequest, RecommendQuery, Sample, ScoringQuery,
    VectorInternal, WithPayloadInterface, WithVector,
};

use qql_core::error::QqlError;
use qql_plan::types::{
    FilterExpression, PayloadSelectorReq, PrefetchRequest, QueryRequest as PlanQueryRequest,
    QueryVariant, SearchParamsRequest, VectorSelectorReq,
};
use qql_plan::{PlanQueryInput, PlanVectorValue};

use super::error_map::{EdgeOp, edge_err};

mod formula;
mod selectors;

use formula::{formula_default_to_json, plan_formula_to_edge};
pub(crate) use selectors::{
    convert_order_by_interface, convert_search_params, convert_with_payload, convert_with_vector,
    parse_json_path,
};

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
    lookup_from: Option<&qql_plan::types::LookupRequest>,
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
    if lookup_from.is_some() {
        return Err(crate::backend::unsupported::EdgeUnsupported::PointReferenceQuery.error());
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
        request.lookup_from.as_ref(),
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
        request.lookup_from.as_ref(),
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
    if request.lookup_from.is_some() {
        return Err(crate::backend::unsupported::EdgeUnsupported::PointReferenceQuery.error());
    }
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
            let query_enum = match recommend.strategy.as_deref() {
                Some("best_score") => QueryEnum::RecommendBestScore(NamedQuery {
                    query: reco,
                    using: using.map(str::to_string),
                }),
                Some("sum_scores") => QueryEnum::RecommendSumScores(NamedQuery {
                    query: reco,
                    using: using.map(str::to_string),
                }),
                None | Some("average_vector") => {
                    return Err(
                        crate::backend::unsupported::EdgeUnsupported::RecommendAverageVector
                            .error(),
                    );
                }
                Some(other) => {
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
        // Edge has no inference service; custom objects stay rejected like
        // the other inference inputs.
        PlanQueryInput::Object { .. } => Err(edge_error(
            "object input reached edge execution without client-side embedding",
        )),
        PlanQueryInput::Vector(
            PlanVectorValue::Document { .. }
            | PlanVectorValue::Image { .. }
            | PlanVectorValue::Object { .. },
        ) => Err(edge_error(
            "inference vector reached edge execution without client-side embedding",
        )),
    }
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
mod tests;
