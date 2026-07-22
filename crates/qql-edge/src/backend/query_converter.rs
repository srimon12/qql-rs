use qdrant_edge::{
    NamedQuery, QueryEnum, SearchParams, VectorInternal, WithPayloadInterface, WithVector,
};

use qql_core::error::QqlError;
use qql_plan::types::{
    PayloadSelectorReq, QueryRequest, QueryVariant, SearchParamsRequest, VectorSelectorReq,
};

pub(crate) fn convert_query_request_with_shard(
    req: &QueryRequest,
    shard: &qdrant_edge::EdgeShard,
) -> Result<qdrant_edge::SearchRequest, QqlError> {
    if let QueryVariant::Recommend { recommend: ref rq } = req.query {
        if let Some(pos_val) = rq.positive.first() {
            if let Ok(point_id) = crate::backend::conversions::to_edge_id(pos_val.clone()) {
                if let Ok(records) = shard.retrieve(
                    &[point_id],
                    Some(WithPayloadInterface::Bool(true)),
                    Some(WithVector::Bool(true)),
                ) {
                    if let Some(record) = records.into_iter().next() {
                        if let Some(vector_struct) = record.vector {
                            let dense_vec = match vector_struct {
                                qdrant_edge::VectorStructInternal::Single(v) => Some(v),
                                qdrant_edge::VectorStructInternal::Named(map) => {
                                    let vec_name = req.using.as_deref().unwrap_or("dense");
                                    map.get(vec_name).and_then(|vi| match vi {
                                        VectorInternal::Dense(v) => Some(v.clone()),
                                        _ => None,
                                    })
                                }
                                _ => None,
                            };
                            if let Some(vec) = dense_vec {
                                let query = QueryEnum::Nearest(NamedQuery {
                                    query: VectorInternal::Dense(vec),
                                    using: req.using.clone(),
                                });
                                let filter = req
                                    .filter
                                    .as_ref()
                                    .and_then(|f| serde_json::to_value(f).ok())
                                    .and_then(|v| serde_json::from_value(v).ok());
                                return Ok(qdrant_edge::SearchRequest {
                                    query,
                                    filter,
                                    params: req.params.as_ref().map(convert_search_params),
                                    limit: req.limit.unwrap_or(10) as usize,
                                    offset: req.offset.unwrap_or(0) as usize,
                                    with_payload: req
                                        .with_payload
                                        .as_ref()
                                        .map(convert_with_payload)
                                        .or(Some(WithPayloadInterface::Bool(true))),
                                    with_vector: Some(req.with_vector.as_ref().map(convert_with_vector).unwrap_or(WithVector::Bool(false))),
                                    score_threshold: req.score_threshold.map(|s| s as f32),
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    convert_query_request(req)
}

pub(crate) fn convert_query_request(
    req: &QueryRequest,
) -> Result<qdrant_edge::SearchRequest, QqlError> {
    let query = extract_query(&req.query, req, None)?;
    let filter = req
        .filter
        .as_ref()
        .and_then(|f| serde_json::to_value(f).ok())
        .and_then(|v| serde_json::from_value(v).ok());
    let params = req.params.as_ref().map(convert_search_params);
    let limit = req.limit.unwrap_or(10) as usize;
    let with_payload = req
        .with_payload
        .as_ref()
        .map(convert_with_payload)
        .or(Some(WithPayloadInterface::Bool(true)));
    let with_vector = req.with_vector.as_ref().map(convert_with_vector);

    Ok(qdrant_edge::SearchRequest {
        query,
        filter,
        params,
        limit,
        offset: req.offset.unwrap_or(0) as usize,
        with_payload,
        with_vector: Some(with_vector.unwrap_or(WithVector::Bool(false))),
        score_threshold: req.score_threshold.map(|s| s as f32),
    })
}

fn extract_query(
    qv: &QueryVariant,
    req: &QueryRequest,
    using_override: Option<String>,
) -> Result<QueryEnum, QqlError> {
    let effective_using = using_override.or_else(|| req.using.clone());
    match qv {
        QueryVariant::Nearest(nq) => {
            let val = &nq.nearest;
            if let Some(arr) = val.as_array() {
                let data: Vec<f32> = arr
                    .iter()
                    .filter_map(|v| v.as_f64().map(|f| f as f32))
                    .collect();
                if data.is_empty() {
                    return Err(QqlError::execution("QQL-EDGE", "empty dense vector", None));
                }
                Ok(QueryEnum::Nearest(NamedQuery {
                    query: VectorInternal::Dense(data),
                    using: effective_using,
                }))
            } else if let Some(obj) = val.as_object() {
                let indices: Vec<u32> = obj
                    .get("indices")
                    .and_then(|i| i.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_u64().map(|n| n as u32))
                            .collect()
                    })
                    .unwrap_or_default();
                let values: Vec<f32> = obj
                    .get("values")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_f64().map(|f| f as f32))
                            .collect()
                    })
                    .unwrap_or_default();
                Ok(QueryEnum::Nearest(NamedQuery {
                    query: VectorInternal::Sparse(qdrant_edge::SparseVector { indices, values }),
                    using: effective_using,
                }))
            } else {
                Err(QqlError::execution(
                    "QQL-EDGE",
                    "nearest query requires a dense or sparse vector",
                    None,
                ))
            }
        }
        QueryVariant::Fusion { .. } => {
            if let Some(pref) = req.prefetch.first() {
                if let Some(ref q) = pref.query {
                    extract_query(q, req, pref.using.clone())
                } else {
                    Err(QqlError::execution(
                        "QQL-EDGE",
                        "prefetch stream missing query",
                        None,
                    ))
                }
            } else {
                Err(QqlError::execution(
                    "QQL-EDGE",
                    "fusion query requires at least one prefetch stream",
                    None,
                ))
            }
        }
        QueryVariant::Recommend { .. } => Err(QqlError::execution(
            "QQL-EDGE",
            "recommendation query requires point vector resolution",
            None,
        )),
        _ => Err(QqlError::execution(
            "QQL-EDGE",
            "unsupported query variant in edge mode",
            None,
        )),
    }
}

pub(crate) fn convert_search_params(p: &SearchParamsRequest) -> SearchParams {
    SearchParams {
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
        acorn: None,
    }
}

pub(crate) fn convert_with_payload(ps: &PayloadSelectorReq) -> WithPayloadInterface {
    match ps {
        PayloadSelectorReq::All(b) => WithPayloadInterface::Bool(*b),
        PayloadSelectorReq::Include { include } => {
            let paths: Vec<qdrant_edge::JsonPath> = include
                .iter()
                .map(|s| serde_json::from_value(serde_json::Value::String(s.clone())).unwrap())
                .collect();
            WithPayloadInterface::Fields(paths)
        }
        PayloadSelectorReq::Exclude { exclude } => {
            let paths: Vec<qdrant_edge::JsonPath> = exclude
                .iter()
                .map(|s| serde_json::from_value(serde_json::Value::String(s.clone())).unwrap())
                .collect();
            WithPayloadInterface::Fields(paths)
        }
    }
}

pub(crate) fn convert_with_vector(vs: &VectorSelectorReq) -> WithVector {
    match vs {
        VectorSelectorReq::All(b) => WithVector::Bool(*b),
        VectorSelectorReq::Names(_) => WithVector::Bool(true),
    }
}
