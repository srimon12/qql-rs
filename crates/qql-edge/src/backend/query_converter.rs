use qdrant_edge::{
    NamedQuery, QueryEnum, SearchParams, VectorInternal, WithPayloadInterface, WithVector,
};

use qql_core::error::QqlError;
use qql_plan::types::{
    PayloadSelectorReq, QueryRequest, QueryVariant, SearchParamsRequest, VectorSelectorReq,
};

pub(crate) fn convert_query_request(
    req: &QueryRequest,
) -> Result<qdrant_edge::SearchRequest, QqlError> {
    let query = extract_query(&req.query, req)?;
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

fn extract_query(qv: &QueryVariant, req: &QueryRequest) -> Result<QueryEnum, QqlError> {
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
                    using: req.using.clone(),
                }))
            } else {
                Err(QqlError::execution(
                    "QQL-EDGE",
                    "nearest query requires a dense vector",
                    None,
                ))
            }
        }
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
