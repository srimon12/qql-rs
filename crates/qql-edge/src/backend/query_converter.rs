use qdrant_edge::external::ordered_float::OrderedFloat;
use qdrant_edge::{
    Fusion, NamedQuery, PayloadSelectorExclude, PayloadSelectorInclude, Prefetch, QueryEnum,
    QueryRequest, ScoringQuery, SearchParams, VectorInternal, WithPayloadInterface, WithVector,
};

use qql_core::error::QqlError;
use qql_plan::types::{
    PayloadSelectorReq, PrefetchRequest, QueryRequest as PlanQueryRequest, QueryVariant,
    SearchParamsRequest, VectorSelectorReq,
};

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
            use qql_plan::{PlanQueryInput, PlanVectorValue};
            let vector = match &nearest.nearest {
                PlanQueryInput::Vector(PlanVectorValue::Dense(values)) if !values.is_empty() => {
                    VectorInternal::Dense(values.clone())
                }
                PlanQueryInput::Vector(PlanVectorValue::Sparse { indices, values }) => {
                    VectorInternal::Sparse(qdrant_edge::SparseVector {
                        indices: indices.clone(),
                        values: values.clone(),
                    })
                }
                PlanQueryInput::Vector(PlanVectorValue::MultiDense(_)) => {
                    return Err(edge_error(
                        "multidense queries are not supported in edge mode",
                    ));
                }
                PlanQueryInput::Point(_) => {
                    return Err(edge_error(
                        "point-reference queries are not supported in edge mode; provide a vector",
                    ));
                }
                PlanQueryInput::Document { .. } => {
                    return Err(edge_error(
                        "text input reached edge execution without client-side embedding",
                    ));
                }
                PlanQueryInput::Vector(PlanVectorValue::Dense(_)) => {
                    return Err(edge_error("dense query vector cannot be empty"));
                }
            };
            Ok(ScoringQuery::Vector(QueryEnum::Nearest(NamedQuery {
                query: vector,
                using: using.map(str::to_string),
            })))
        }
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
        QueryVariant::Recommend { .. } => Err(edge_error(
            "recommendation queries are not yet supported in edge mode",
        )),
        _ => Err(edge_error("query variant is not supported in edge mode")),
    }
}

pub(crate) fn convert_search_params(
    params: &SearchParamsRequest,
) -> Result<SearchParams, QqlError> {
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

fn convert_filter(
    filter: Option<&impl serde::Serialize>,
) -> Result<Option<qdrant_edge::Filter>, QqlError> {
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

fn parse_json_path(path: &str) -> Result<qdrant_edge::JsonPath, QqlError> {
    serde_json::from_value(serde_json::Value::String(path.to_string()))
        .map_err(|error| edge_error(format!("invalid payload path '{path}': {error}")))
}

fn limit_error(error: std::num::TryFromIntError) -> QqlError {
    edge_error(format!("limit is too large for this platform: {error}"))
}

fn unsupported_shard() -> QqlError {
    QqlError::execution(
        "QQL-EDGE-UNSUPPORTED-SHARD",
        "SHARD routing is available only with clustered Qdrant backends, not qql-edge",
        None,
    )
}

fn edge_error(message: impl Into<String>) -> QqlError {
    QqlError::execution("QQL-EDGE", message.into(), None)
}
