//! Search params, output selectors, and order-by lowering for edge queries.
//!
//! Split out of `query_converter` so each file stays near the repo's size
//! target. Module path and callers are unchanged (re-exported by the parent).

use qdrant_edge::external::ordered_float::OrderedFloat;
use qdrant_edge::{
    Direction, JsonPath, OrderBy, OrderByInterface, PayloadSelectorExclude, PayloadSelectorInclude,
    SearchParams, WithPayloadInterface, WithVector,
};

use qql_core::error::QqlError;
use qql_plan::types::{OrderByQuery, PayloadSelectorReq, SearchParamsRequest, VectorSelectorReq};

use super::{edge_error, limit_error};

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
                    corpus: crate::backend::convert_edge_filter(Some(corpus))?.unwrap(),
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
    order_by: &OrderByQuery,
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
    let start_from = order_by
        .start_from
        .as_ref()
        .map(|value| {
            use qdrant_edge::StartFrom;
            if let Some(n) = value.as_i64() {
                Ok(StartFrom::Integer(n))
            } else if let Some(n) = value.as_u64() {
                i64::try_from(n)
                    .map(StartFrom::Integer)
                    .map_err(|_| edge_error(format!("order_by start_from {n} exceeds i64")))
            } else if let Some(f) = value.as_f64() {
                Ok(StartFrom::Float(f))
            } else if let Some(text) = value.as_str() {
                text.parse::<qdrant_edge::DateTimeWrapper>()
                    .map(StartFrom::Datetime)
                    .map_err(|_| {
                        edge_error(format!(
                            "order_by start_from '{text}' must be an integer, float, or ISO-8601 datetime"
                        ))
                    })
            } else {
                Err(edge_error(format!(
                    "order_by start_from {value} must be an integer, float, or ISO-8601 datetime"
                )))
            }
        })
        .transpose()?;
    Ok(OrderByInterface::Struct(OrderBy {
        key,
        direction,
        start_from,
    }))
}

pub(crate) fn parse_json_path(path: &str) -> Result<JsonPath, QqlError> {
    serde_json::from_value(serde_json::Value::String(path.to_string()))
        .map_err(|error| edge_error(format!("invalid payload path '{path}': {error}")))
}
