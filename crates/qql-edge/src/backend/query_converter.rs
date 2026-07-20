//! AST query, search params, prefetch, and payload filter transformation to qdrant-edge models.

use qdrant_edge::{EdgeShard, Filter as EdgeFilter, VectorInternal, WithPayloadInterface};

use super::conversions::{edge_err, to_edge_id};
use qql::pipeline::{
    PrefetchQuery, QueryPointsRequest, SearchParams as QqlSearchParams,
    WithPayload as QqlWithPayload, WithVector as QqlWithVector,
};
use qql_core::error::QqlError;

pub(crate) fn convert_search_params(p: &QqlSearchParams) -> qdrant_edge::SearchParams {
    qdrant_edge::SearchParams {
        hnsw_ef: p.hnsw_ef.map(|v| v as usize),
        exact: p.exact.unwrap_or(false),
        quantization: p
            .quantization
            .as_ref()
            .map(|q| qdrant_edge::QuantizationSearchParams {
                ignore: q.ignore.unwrap_or(false),
                rescore: q.rescore,
                oversampling: q.oversampling,
            }),
        indexed_only: p.indexed_only.unwrap_or(false),
        acorn: p.acorn.as_ref().map(|a| qdrant_edge::AcornSearchParams {
            enable: a.enable,
            max_selectivity: None,
        }),
    }
}

pub(crate) fn convert_with_vector(wv: QqlWithVector) -> qdrant_edge::WithVector {
    if !wv.vectors.is_empty() {
        qdrant_edge::WithVector::Selector(wv.vectors)
    } else {
        qdrant_edge::WithVector::Bool(wv.enable.unwrap_or(false))
    }
}

pub(crate) fn convert_with_payload(wp: &QqlWithPayload) -> Result<WithPayloadInterface, QqlError> {
    if !wp.include.is_empty() {
        let keys = wp
            .include
            .iter()
            .map(|s| {
                s.parse::<qdrant_edge::JsonPath>().map_err(|e| {
                    QqlError::runtime(format!("invalid payload include path '{s}': {e:?}"))
                })
            })
            .collect::<Result<Vec<_>, QqlError>>()?;
        Ok(WithPayloadInterface::Selector(
            qdrant_edge::PayloadSelector::Include(qdrant_edge::PayloadSelectorInclude {
                include: keys,
            }),
        ))
    } else if !wp.exclude.is_empty() {
        let keys = wp
            .exclude
            .iter()
            .map(|s| {
                s.parse::<qdrant_edge::JsonPath>().map_err(|e| {
                    QqlError::runtime(format!("invalid payload exclude path '{s}': {e:?}"))
                })
            })
            .collect::<Result<Vec<_>, QqlError>>()?;
        Ok(WithPayloadInterface::Selector(
            qdrant_edge::PayloadSelector::Exclude(qdrant_edge::PayloadSelectorExclude {
                exclude: keys,
            }),
        ))
    } else {
        Ok(WithPayloadInterface::Bool(wp.enable.unwrap_or(false)))
    }
}

pub(crate) fn resolve_vector_input(
    input: qql::pipeline::VectorInput,
    shard: &EdgeShard,
    using: Option<&str>,
) -> Result<VectorInternal, QqlError> {
    match input {
        qql::pipeline::VectorInput::Dense(v) => Ok(VectorInternal::Dense(v)),
        qql::pipeline::VectorInput::Id(pid) => {
            let edge_id = to_edge_id(pid)?;
            let records = shard
                .retrieve(&[edge_id], None, Some(qdrant_edge::WithVector::Bool(true)))
                .map_err(edge_err)?;

            let record = records.into_iter().next().ok_or_else(|| {
                QqlError::runtime(format!("point not found for recommendation: {edge_id:?}"))
            })?;

            let vector_struct = record
                .vector
                .ok_or_else(|| QqlError::runtime(format!("point has no vector: {edge_id:?}")))?;

            let internal = vector_struct;
            match internal {
                qdrant_edge::VectorStructInternal::Single(v) => Ok(VectorInternal::Dense(v)),
                qdrant_edge::VectorStructInternal::MultiDense(v) => {
                    Ok(VectorInternal::MultiDense(v))
                }
                qdrant_edge::VectorStructInternal::Named(mut map) => {
                    let key = using.unwrap_or("");
                    if let Some(v) = map.remove(key).or_else(|| map.remove("")) {
                        Ok(v)
                    } else {
                        Err(QqlError::runtime(format!(
                            "vector '{key}' not found in point {edge_id:?}"
                        )))
                    }
                }
            }
        }
        qql::pipeline::VectorInput::Document { .. } => Err(QqlError::runtime(
            "unresolved document input in edge query — please check embedding pipeline config",
        )),
    }
}

pub(crate) fn convert_query_variant(
    qv: qql::pipeline::QueryVariant,
    shard: &EdgeShard,
    using: Option<&str>,
) -> Result<qdrant_edge::ScoringQuery, QqlError> {
    let using_string = using.map(String::from);
    match qv {
        qql::pipeline::QueryVariant::Nearest(v) => {
            let nq = qdrant_edge::NamedQuery {
                query: VectorInternal::Dense(v),
                using: using_string,
            };
            Ok(qdrant_edge::ScoringQuery::Vector(qdrant_edge::QueryEnum::Nearest(nq)))
        }
        qql::pipeline::QueryVariant::Sparse(indices, values) => {
            let sv = qdrant_edge::SparseVector::new(indices, values)
                .map_err(|e| QqlError::runtime(format!("invalid sparse vector: {e}")))?;
            let nq = qdrant_edge::NamedQuery {
                query: VectorInternal::Sparse(sv),
                using: using_string,
            };
            Ok(qdrant_edge::ScoringQuery::Vector(qdrant_edge::QueryEnum::Nearest(nq)))
        }
        qql::pipeline::QueryVariant::Recommend(rec) => {
            let positives = rec
                .positive
                .into_iter()
                .map(|p| resolve_vector_input(p, shard, using))
                .collect::<Result<Vec<_>, QqlError>>()?;
            let negatives = rec
                .negative
                .into_iter()
                .map(|p| resolve_vector_input(p, shard, using))
                .collect::<Result<Vec<_>, QqlError>>()?;

            let query = qdrant_edge::RecommendQuery {
                positives,
                negatives,
            };
            let nq = qdrant_edge::NamedQuery {
                query,
                using: using_string,
            };
            let query_enum = match rec.strategy {
                Some(qql::pipeline::RecommendStrategyType::SumScores) => {
                    qdrant_edge::QueryEnum::RecommendSumScores(nq)
                }
                _ => qdrant_edge::QueryEnum::RecommendBestScore(nq),
            };
            Ok(qdrant_edge::ScoringQuery::Vector(query_enum))
        }
        qql::pipeline::QueryVariant::Context(ctx) => {
            let mut pairs = Vec::with_capacity(ctx.pairs.len());
            for pair in ctx.pairs {
                let positive = pair
                    .positive
                    .map(|p| resolve_vector_input(p, shard, using))
                    .transpose()?;
                let negative = pair
                    .negative
                    .map(|p| resolve_vector_input(p, shard, using))
                    .transpose()?;

                if let (Some(pos), Some(neg)) = (positive, negative) {
                    pairs.push(qdrant_edge::ContextPair {
                        positive: pos,
                        negative: neg,
                    });
                } else {
                    return Err(QqlError::runtime(
                        "context pair must contain both positive and negative examples",
                    ));
                }
            }
            let nq = qdrant_edge::NamedQuery {
                query: qdrant_edge::ContextQuery::new(pairs),
                using: using_string,
            };
            Ok(qdrant_edge::ScoringQuery::Vector(qdrant_edge::QueryEnum::Context(nq)))
        }
        qql::pipeline::QueryVariant::Discover(disc) => {
            let target = resolve_vector_input(disc.target, shard, using)?;
            let mut pairs = Vec::with_capacity(disc.context.pairs.len());
            for pair in disc.context.pairs {
                let positive = pair
                    .positive
                    .map(|p| resolve_vector_input(p, shard, using))
                    .transpose()?;
                let negative = pair
                    .negative
                    .map(|p| resolve_vector_input(p, shard, using))
                    .transpose()?;
                if let (Some(pos), Some(neg)) = (positive, negative) {
                    pairs.push(qdrant_edge::ContextPair {
                        positive: pos,
                        negative: neg,
                    });
                } else {
                    return Err(QqlError::runtime(
                        "context pair must contain both positive and negative examples",
                    ));
                }
            }
            let nq = qdrant_edge::NamedQuery {
                query: qdrant_edge::DiscoverQuery::new(target, pairs),
                using: using_string,
            };
            Ok(qdrant_edge::ScoringQuery::Vector(qdrant_edge::QueryEnum::Discover(nq)))
        }
        qql::pipeline::QueryVariant::OrderBy(input) => {
            let direction = match input.direction {
                qql::pipeline::OrderByDirection::Asc => qdrant_edge::Direction::Asc,
                qql::pipeline::OrderByDirection::Desc => qdrant_edge::Direction::Desc,
            };
            let key = input
                .key
                .parse::<qdrant_edge::JsonPath>()
                .map_err(|e| QqlError::runtime(format!("invalid JSON path: {e:?}")))?;
            Ok(qdrant_edge::ScoringQuery::OrderBy(qdrant_edge::OrderBy {
                key,
                direction: Some(direction),
                start_from: None,
            }))
        }
        qql::pipeline::QueryVariant::Sample => {
            Ok(qdrant_edge::ScoringQuery::Sample(qdrant_edge::Sample::Random))
        }
        qql::pipeline::QueryVariant::Fusion(ft) => {
            let fusion = match ft {
                qql::pipeline::FusionType::Rrf => qdrant_edge::Fusion::Rrf {
                    k: 60,
                    weights: None,
                },
                qql::pipeline::FusionType::Dbsf => qdrant_edge::Fusion::Dbsf,
            };
            Ok(qdrant_edge::ScoringQuery::Fusion(fusion))
        }
        qql::pipeline::QueryVariant::Rrf(config) => {
            let weights = if config.weights.is_empty() {
                None
            } else {
                Some(
                    config
                        .weights
                        .iter()
                        .map(|&w| qdrant_edge::external::ordered_float::OrderedFloat(w))
                        .collect(),
                )
            };
            Ok(qdrant_edge::ScoringQuery::Fusion(qdrant_edge::Fusion::Rrf {
                k: config.k.unwrap_or(60) as usize,
                weights,
            }))
        }
        qql::pipeline::QueryVariant::Formula { .. } => Err(QqlError::runtime(
            "SCORE BOOST / Formula query variant in edge mode is not supported natively by qdrant-edge 0.7 — please use gRPC/REST backend for server-side formula rescoring",
        )),
        qql::pipeline::QueryVariant::MMR {
            input,
            diversity,
            candidates,
        } => {
            let vec_input = match *input {
                qql::pipeline::QueryVariant::Nearest(v) => VectorInternal::Dense(v),
                qql::pipeline::QueryVariant::Sparse(indices, values) => {
                    let sv = qdrant_edge::SparseVector::new(indices, values)
                        .map_err(|e| QqlError::runtime(format!("invalid sparse vector in MMR: {e}")))?;
                    VectorInternal::Sparse(sv)
                }
                _ => {
                    return Err(QqlError::runtime(
                        "MMR in edge mode requires dense or sparse vector input",
                    ))
                }
            };
            let mmr = qdrant_edge::Mmr {
                vector: vec_input,
                using: using_string.unwrap_or_default(),
                lambda: qdrant_edge::external::ordered_float::OrderedFloat(diversity),
                candidates_limit: candidates as usize,
            };
            Ok(qdrant_edge::ScoringQuery::Mmr(mmr))
        }
        _ => Err(QqlError::runtime(format!(
            "query variant not supported in edge mode: {qv:?}"
        ))),
    }
}

pub(crate) fn convert_prefetch_query(
    req: PrefetchQuery,
    shard: &EdgeShard,
) -> Result<qdrant_edge::Prefetch, QqlError> {
    let prefetches = req
        .prefetches
        .into_iter()
        .map(|p| convert_prefetch_query(p, shard))
        .collect::<Result<Vec<_>, QqlError>>()?;

    let query = req
        .query
        .map(|q| convert_query_variant(q, shard, req.using.as_deref()))
        .transpose()?;

    let filter = req
        .filter
        .map(|f| serde_json::from_value::<EdgeFilter>(f.0))
        .transpose()
        .map_err(|e| QqlError::runtime(format!("invalid filter: {e}")))?;

    let params = req.params.as_ref().map(convert_search_params);

    Ok(qdrant_edge::Prefetch {
        prefetches,
        limit: req.limit.unwrap_or(0) as usize,
        query,
        params,
        filter,
        score_threshold: req
            .score_threshold
            .map(qdrant_edge::external::ordered_float::OrderedFloat),
    })
}

pub(crate) fn convert_query_request(
    req: QueryPointsRequest,
    shard: &EdgeShard,
) -> Result<qdrant_edge::QueryRequest, QqlError> {
    let prefetches = req
        .prefetches
        .into_iter()
        .map(|p| convert_prefetch_query(p, shard))
        .collect::<Result<Vec<_>, QqlError>>()?;

    let query = req
        .query
        .map(|q| convert_query_variant(q, shard, req.using.as_deref()))
        .transpose()?;

    let filter = req
        .filter
        .map(|f| serde_json::from_value::<EdgeFilter>(f.0))
        .transpose()
        .map_err(|e| QqlError::runtime(format!("invalid filter: {e}")))?;

    let params = req.params.as_ref().map(convert_search_params);
    let with_vector = req.with_vector.map(convert_with_vector).unwrap_or_default();
    let with_payload = req
        .with_payload
        .as_ref()
        .map(convert_with_payload)
        .transpose()?
        .unwrap_or_default();

    Ok(qdrant_edge::QueryRequest {
        prefetches,
        limit: req.limit as usize,
        offset: req.offset as usize,
        with_vector,
        with_payload,
        query,
        filter,
        score_threshold: req
            .score_threshold
            .map(qdrant_edge::external::ordered_float::OrderedFloat),
        params,
    })
}
