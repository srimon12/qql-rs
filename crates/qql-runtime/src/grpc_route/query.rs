//! Plan query / point converters → proto query messages.
//!
//! Covers `QueryPoints` / `QueryPointGroups` (variant, prefetch, filter,
//! selectors, consistency, shard key) plus point IDs, payload/vector
//! selectors, search params, and point-vectors helpers.

use qql_core::error::QqlError;
use qql_plan::types::{FilterExpression, PayloadSelectorReq, VectorSelectorReq, WithLookupValue};
use qql_plan::{PlanPointId, PlanPointVectors, PlanQueryInput, PlanVectorValue};

use crate::qdrant_grpc::qdrant;

use super::common::{shard_key_selector, to_point_id};
use super::filter::{to_filter, to_filter_opt};
use super::formula::ast_formula_to_grpc;
use super::values::to_qdrant_value;

pub(crate) fn to_query_points(
    req: &qql_plan::types::QueryRequest,
    collection: &str,
) -> Result<qdrant::QueryPoints, QqlError> {
    Ok(qdrant::QueryPoints {
        collection_name: collection.into(),
        prefetch: req
            .prefetch
            .iter()
            .map(to_prefetch)
            .collect::<Result<Vec<_>, _>>()?,
        query: Some(to_query_variant(&req.query)?),
        using: req.using.clone(),
        filter: to_filter_opt(req.filter.as_ref())?,
        params: req.params.as_ref().map(to_search_params),
        score_threshold: req.score_threshold.map(|s| s as f32),
        limit: req.limit,
        offset: req.offset,
        with_payload: req.with_payload.as_ref().map(to_payload_selector),
        with_vectors: req.with_vector.as_ref().map(to_vectors_selector),
        shard_key_selector: shard_key_selector(&req.shard_key),
        // Proto QueryPoints.timeout / read_consistency (not SearchParams body).
        timeout: req.timeout,
        read_consistency: req.consistency.as_ref().map(to_read_consistency),
        ..Default::default()
    })
}

pub(crate) fn to_query_groups(
    req: &qql_plan::types::QueryGroupsRequest,
    collection: &str,
) -> Result<qdrant::QueryPointGroups, QqlError> {
    Ok(qdrant::QueryPointGroups {
        collection_name: collection.into(),
        prefetch: req
            .prefetch
            .iter()
            .map(to_prefetch)
            .collect::<Result<Vec<_>, _>>()?,
        query: Some(to_query_variant(&req.query)?),
        using: req.using.clone(),
        filter: to_filter_opt(req.filter.as_ref())?,
        params: req.params.as_ref().map(to_search_params),
        score_threshold: req.score_threshold.map(|s| s as f32),
        with_payload: req.with_payload.as_ref().map(to_payload_selector),
        with_vectors: req.with_vector.as_ref().map(to_vectors_selector),
        group_by: req.group_by.clone(),
        group_size: Some(req.group_size),
        limit: Some(req.limit),
        with_lookup: req.with_lookup.as_ref().map(|wv| match wv {
            WithLookupValue::Collection(c) => qdrant::WithLookup {
                collection: c.clone(),
                ..Default::default()
            },
            WithLookupValue::Full(wl) => qdrant::WithLookup {
                collection: wl.collection.clone(),
                with_payload: wl.with_payload.as_ref().map(to_payload_selector),
                with_vectors: wl.with_vectors.as_ref().map(to_vectors_selector),
            },
        }),
        shard_key_selector: shard_key_selector(&req.shard_key),
        timeout: req.timeout,
        read_consistency: req.consistency.as_ref().map(to_read_consistency),
        ..Default::default()
    })
}

pub(crate) fn to_read_consistency(
    c: &qql_plan::types::ReadConsistencyParam,
) -> qdrant::ReadConsistency {
    use qdrant::read_consistency::Value as RcValue;
    use qql_plan::types::ReadConsistencyParam;
    let value = match c {
        ReadConsistencyParam::Factor(n) => RcValue::Factor(*n),
        ReadConsistencyParam::Majority => {
            RcValue::Type(qdrant::ReadConsistencyType::Majority as i32)
        }
        ReadConsistencyParam::Quorum => RcValue::Type(qdrant::ReadConsistencyType::Quorum as i32),
        ReadConsistencyParam::All => RcValue::Type(qdrant::ReadConsistencyType::All as i32),
    };
    qdrant::ReadConsistency { value: Some(value) }
}

pub(crate) fn to_prefetch(
    pf: &qql_plan::types::PrefetchRequest,
) -> Result<qdrant::PrefetchQuery, QqlError> {
    Ok(qdrant::PrefetchQuery {
        prefetch: match pf.prefetch.as_ref() {
            Some(pfs) => pfs
                .iter()
                .map(to_prefetch)
                .collect::<Result<Vec<_>, QqlError>>()?,
            None => Vec::new(),
        },
        query: pf.query.as_ref().map(to_query_variant).transpose()?,
        using: pf.using.clone(),
        filter: to_filter_opt(pf.filter.as_ref())?,
        params: pf.params.as_ref().map(to_search_params),
        score_threshold: pf.score_threshold.map(|s| s as f32),
        limit: pf.limit,
        lookup_from: pf.lookup_from.as_ref().map(|l| qdrant::LookupLocation {
            collection_name: l.collection.clone(),
            vector_name: l.vector.clone(),
            ..Default::default()
        }),
    })
}

/// Convert plan RRF parameters to the gRPC `Rrf` message.
///
/// The pinned proto stores `k` as `uint32` and `weights` as `repeated float`.
/// Values that cannot be represented exactly (`k > u32::MAX`, an `f64` weight
/// that does not round-trip through `f32`) return a structured error instead
/// of being silently dropped or silently losing precision.
pub(crate) fn to_grpc_rrf(rrf: &qql_plan::types::RrfQuery) -> Result<qdrant::Rrf, QqlError> {
    let k = match rrf.rrf.k {
        Some(k) => {
            let k32 = u32::try_from(k).map_err(|_| {
                QqlError::validation(
                    "QQL-GRPC-RRF-K",
                    format!(
                        "rrf_k = {k} cannot be represented on the gRPC path: the bundled Qdrant proto stores k as uint32 (max {})",
                        u32::MAX
                    ),
                    None,
                )
            })?;
            Some(k32)
        }
        None => None,
    };
    let weights = match rrf.rrf.weights.as_ref() {
        Some(weights) => {
            let mut out = Vec::with_capacity(weights.len());
            for w in weights {
                let w32 = *w as f32;
                if (w32 as f64) != *w {
                    return Err(QqlError::validation(
                        "QQL-GRPC-RRF-WEIGHT",
                        format!(
                            "rrf_weights value {w} cannot be represented exactly on the gRPC path: the bundled Qdrant proto stores weights as f32"
                        ),
                        None,
                    ));
                }
                out.push(w32);
            }
            out
        }
        None => Vec::new(),
    };
    Ok(qdrant::Rrf { k, weights })
}

pub(crate) fn to_query_variant(
    qv: &qql_plan::types::QueryVariant,
) -> Result<qdrant::Query, QqlError> {
    use qdrant::query::Variant;
    use qql_plan::types::QueryVariant;

    let variant = match qv {
        QueryVariant::Nearest(nq) => Variant::Nearest(to_vector_input(&nq.nearest)),
        QueryVariant::Recommend { recommend } => Variant::Recommend(qdrant::RecommendInput {
            positive: recommend.positive.iter().map(to_vector_input).collect(),
            negative: recommend.negative.iter().map(to_vector_input).collect(),
            strategy: recommend.strategy.as_deref().map(|s| match s {
                "average_vector" => qdrant::RecommendStrategy::AverageVector as i32,
                "best_score" => qdrant::RecommendStrategy::BestScore as i32,
                "sum_scores" => qdrant::RecommendStrategy::SumScores as i32,
                _ => qdrant::RecommendStrategy::AverageVector as i32,
            }),
        }),
        QueryVariant::Context { context } => Variant::Context(qdrant::ContextInput {
            pairs: context
                .iter()
                .map(|p| qdrant::ContextInputPair {
                    positive: Some(to_vector_input(&p.positive)),
                    negative: Some(to_vector_input(&p.negative)),
                })
                .collect(),
        }),
        QueryVariant::Discover { discover } => Variant::Discover(qdrant::DiscoverInput {
            target: Some(to_vector_input(&discover.target)),
            context: Some(qdrant::ContextInput {
                pairs: discover
                    .context
                    .iter()
                    .map(|p| qdrant::ContextInputPair {
                        positive: Some(to_vector_input(&p.positive)),
                        negative: Some(to_vector_input(&p.negative)),
                    })
                    .collect(),
            }),
        }),
        QueryVariant::OrderBy { order_by } => {
            let dir = order_by.direction.as_deref().map(|d| match d {
                "asc" => qdrant::Direction::Asc as i32,
                "desc" => qdrant::Direction::Desc as i32,
                _ => qdrant::Direction::Asc as i32,
            });
            Variant::OrderBy(qdrant::OrderBy {
                key: order_by.key.clone(),
                direction: dir,
                ..Default::default()
            })
        }
        QueryVariant::Sample { .. } => Variant::Sample(0),
        QueryVariant::Fusion { fusion } => {
            let val = match fusion.as_str() {
                "rrf" => qdrant::Fusion::Rrf as i32,
                "dbsf" => qdrant::Fusion::Dbsf as i32,
                other => {
                    return Err(QqlError::validation(
                        "QQL-GRPC-FUSION",
                        format!("unsupported fusion method '{other}' on the gRPC path"),
                        None,
                    ));
                }
            };
            Variant::Fusion(val)
        }
        QueryVariant::Rrf(rrf) => Variant::Rrf(to_grpc_rrf(rrf)?),
        QueryVariant::Formula(fq) => Variant::Formula(qdrant::Formula {
            expression: ast_formula_to_grpc(&fq.formula.0),
            defaults: fq
                .defaults
                .clone()
                .unwrap_or_default()
                .into_iter()
                .map(|(k, v)| (k, to_qdrant_value(v)))
                .collect(),
        }),
        QueryVariant::RelevanceFeedback { relevance_feedback } => {
            let feedback = relevance_feedback
                .feedback
                .iter()
                .map(|item| qdrant::FeedbackItem {
                    example: Some(to_vector_input(&item.example)),
                    score: item.score as f32,
                })
                .collect();
            let strategy = Some(qdrant::FeedbackStrategy {
                variant: Some(qdrant::feedback_strategy::Variant::Naive(
                    qdrant::NaiveFeedbackStrategy {
                        a: relevance_feedback.strategy.naive.a as f32,
                        b: relevance_feedback.strategy.naive.b as f32,
                        c: relevance_feedback.strategy.naive.c as f32,
                    },
                )),
            });
            Variant::RelevanceFeedback(qdrant::RelevanceFeedbackInput {
                target: Some(to_vector_input(&relevance_feedback.target)),
                feedback,
                strategy,
            })
        }
    };
    Ok(qdrant::Query {
        variant: Some(variant),
    })
}

pub(crate) fn to_vector_input(input: &PlanQueryInput) -> qdrant::VectorInput {
    use qdrant::vector_input::Variant;
    match input {
        PlanQueryInput::Point(id) => qdrant::VectorInput {
            variant: Some(Variant::Id(to_point_id(id))),
        },
        PlanQueryInput::Vector(PlanVectorValue::Dense(data)) => qdrant::VectorInput {
            variant: Some(Variant::Dense(qdrant::DenseVector { data: data.clone() })),
        },
        PlanQueryInput::Vector(PlanVectorValue::Sparse { indices, values }) => {
            qdrant::VectorInput {
                variant: Some(Variant::Sparse(qdrant::SparseVector {
                    indices: indices.clone(),
                    values: values.clone(),
                })),
            }
        }
        PlanQueryInput::Vector(PlanVectorValue::MultiDense(rows)) => qdrant::VectorInput {
            variant: Some(Variant::MultiDense(qdrant::MultiDenseVector {
                vectors: rows
                    .iter()
                    .map(|row| qdrant::DenseVector { data: row.clone() })
                    .collect(),
            })),
        },
        PlanQueryInput::Document { text, model } => qdrant::VectorInput {
            variant: Some(Variant::Document(qdrant::Document {
                text: text.clone(),
                model: model.clone().unwrap_or_default(),
                ..Default::default()
            })),
        },
        PlanQueryInput::Image { image, model } => qdrant::VectorInput {
            variant: Some(Variant::Image(qdrant::Image {
                image: Some(qdrant::Value {
                    kind: Some(qdrant::value::Kind::StringValue(image.clone())),
                }),
                model: model.clone().unwrap_or_default(),
                ..Default::default()
            })),
        },
    }
}

pub(crate) fn to_payload_selector(ps: &PayloadSelectorReq) -> qdrant::WithPayloadSelector {
    match ps {
        PayloadSelectorReq::All(b) => qdrant::WithPayloadSelector {
            selector_options: Some(qdrant::with_payload_selector::SelectorOptions::Enable(*b)),
        },
        PayloadSelectorReq::Include { include } => qdrant::WithPayloadSelector {
            selector_options: Some(qdrant::with_payload_selector::SelectorOptions::Include(
                qdrant::PayloadIncludeSelector {
                    fields: include.clone(),
                },
            )),
        },
        PayloadSelectorReq::Exclude { exclude } => qdrant::WithPayloadSelector {
            selector_options: Some(qdrant::with_payload_selector::SelectorOptions::Exclude(
                qdrant::PayloadExcludeSelector {
                    fields: exclude.clone(),
                },
            )),
        },
    }
}

pub(crate) fn to_vectors_selector(vs: &VectorSelectorReq) -> qdrant::WithVectorsSelector {
    match vs {
        VectorSelectorReq::All(b) => qdrant::WithVectorsSelector {
            selector_options: Some(qdrant::with_vectors_selector::SelectorOptions::Enable(*b)),
        },
        VectorSelectorReq::Names(names) => qdrant::WithVectorsSelector {
            selector_options: Some(qdrant::with_vectors_selector::SelectorOptions::Include(
                qdrant::VectorsSelector {
                    names: names.clone(),
                },
            )),
        },
    }
}

pub(crate) fn to_search_params(
    params: &qql_plan::types::SearchParamsRequest,
) -> qdrant::SearchParams {
    use qql_plan::types::IdfSearchParams;
    qdrant::SearchParams {
        hnsw_ef: params.hnsw_ef,
        exact: params.exact,
        indexed_only: params.indexed_only,
        quantization: params
            .quantization
            .as_ref()
            .map(|q| qdrant::QuantizationSearchParams {
                ignore: q.ignore,
                rescore: q.rescore,
                oversampling: q.oversampling,
            }),
        acorn: params.acorn.as_ref().map(|a| qdrant::AcornSearchParams {
            enable: Some(a.enable),
            max_selectivity: a.max_selectivity,
        }),
        idf: params.idf.as_ref().map(|idf| match idf {
            IdfSearchParams::Global => qdrant::IdfParams { corpus: None },
            IdfSearchParams::Corpus { corpus } => qdrant::IdfParams {
                corpus: to_filter(corpus).ok(),
            },
        }),
    }
}

pub(crate) fn plan_vector_to_proto(v: &PlanVectorValue) -> qdrant::Vector {
    match v {
        PlanVectorValue::Dense(data) => qdrant::Vector {
            vector: Some(qdrant::vector::Vector::Dense(qdrant::DenseVector {
                data: data.clone(),
            })),
            ..Default::default()
        },
        PlanVectorValue::Sparse { indices, values } => qdrant::Vector {
            vector: Some(qdrant::vector::Vector::Sparse(qdrant::SparseVector {
                indices: indices.clone(),
                values: values.clone(),
            })),
            ..Default::default()
        },
        PlanVectorValue::MultiDense(rows) => qdrant::Vector {
            vector: Some(qdrant::vector::Vector::MultiDense(
                qdrant::MultiDenseVector {
                    vectors: rows
                        .iter()
                        .map(|row| qdrant::DenseVector { data: row.clone() })
                        .collect(),
                },
            )),
            ..Default::default()
        },
    }
}

pub(crate) fn to_vectors(vectors: &PlanPointVectors) -> Option<qdrant::Vectors> {
    match vectors {
        PlanPointVectors::Unnamed(v) => Some(qdrant::Vectors {
            vectors_options: Some(qdrant::vectors::VectorsOptions::Vector(
                plan_vector_to_proto(v),
            )),
        }),
        PlanPointVectors::Named(entries) => {
            let mut map = std::collections::HashMap::new();
            for (name, v) in entries {
                map.insert(name.clone(), plan_vector_to_proto(v));
            }
            Some(qdrant::Vectors {
                vectors_options: Some(qdrant::vectors::VectorsOptions::Vectors(
                    qdrant::NamedVectors { vectors: map },
                )),
            })
        }
    }
}

pub(crate) fn points_and_filter_selector(
    points: Option<&Vec<PlanPointId>>,
    filter: Option<&FilterExpression>,
) -> Result<Option<qdrant::PointsSelector>, QqlError> {
    if let Some(points) = points {
        Ok(Some(qdrant::PointsSelector {
            points_selector_one_of: Some(qdrant::points_selector::PointsSelectorOneOf::Points(
                qdrant::PointsIdsList {
                    ids: points.iter().map(to_point_id).collect(),
                },
            )),
        }))
    } else if let Some(f) = filter {
        Ok(Some(qdrant::PointsSelector {
            points_selector_one_of: Some(qdrant::points_selector::PointsSelectorOneOf::Filter(
                to_filter(f)?,
            )),
        }))
    } else {
        Ok(None)
    }
}
