//! Search-params and request-option lowering for QUERY / PREFETCH.

use crate::filter::top_level_filter;
use crate::types::*;
use qql_core::error::QqlError;

/// True when a plan filter has no clauses (serde can produce this for objects
/// with only unknown keys). Empty IDF corpora are rejected as plan errors.
fn filter_expression_is_empty(filter: &FilterExpression) -> bool {
    match filter {
        FilterExpression::Compound(c) => {
            c.must.is_empty()
                && c.must_not.is_empty()
                && c.should.is_empty()
                && c.min_should.is_none()
        }
        FilterExpression::Single(clause) => match clause.as_ref() {
            FilterClause::Filter(c) => {
                c.must.is_empty()
                    && c.must_not.is_empty()
                    && c.should.is_empty()
                    && c.min_should.is_none()
            }
            _ => false,
        },
    }
}

/// Lower body-only OpenAPI `SearchParams` (timeout/consistency are request-level).
///
/// IDF corpora are QQL filters (`idf = WHERE …`). An empty lowered filter is
/// rejected with `QQL-PLAN-IDF`.
pub fn lower_search_params(
    params: &qql_core::ast::SearchParams,
) -> Result<Option<SearchParamsRequest>, QqlError> {
    let mut has = false;
    let idf = match params.idf.as_ref() {
        None => None,
        Some(idf) => Some(match &idf.corpus {
            None => IdfSearchParams::Global,
            Some(filter) => {
                let corpus = top_level_filter(filter);
                if filter_expression_is_empty(&corpus) {
                    return Err(QqlError::validation(
                        "QQL-PLAN-IDF",
                        "idf corpus filter is empty or has no recognised conditions",
                        None,
                    ));
                }
                IdfSearchParams::Corpus { corpus }
            }
        }),
    };
    let r = SearchParamsRequest {
        hnsw_ef: params.hnsw_ef,
        exact: params.exact,
        acorn: params.acorn.map(|enable| AcornSearchParams {
            enable,
            max_selectivity: params.max_selectivity,
        }),
        indexed_only: params.indexed_only,
        quantization: params.quantization.as_ref().map(|q| {
            has = true;
            QuantizationSearchRequest {
                ignore: q.ignore,
                rescore: q.rescore,
                oversampling: q.oversampling,
            }
        }),
        idf,
    };
    if has
        || r.hnsw_ef.is_some()
        || r.exact.is_some()
        || r.acorn.is_some()
        || r.indexed_only.is_some()
        || r.idf.is_some()
    {
        Ok(Some(r))
    } else {
        Ok(None)
    }
}

/// Extract request-level opts (OpenAPI query params / proto fields).
pub fn lower_request_opts(
    params: Option<&qql_core::ast::SearchParams>,
) -> (Option<u64>, Option<ReadConsistencyParam>) {
    match params {
        Some(p) => (
            p.timeout,
            p.consistency.as_ref().map(ReadConsistencyParam::from),
        ),
        None => (None, None),
    }
}

/// Append OpenAPI query params for timeout + consistency.
pub fn push_read_opts(
    query: &mut Vec<(String, String)>,
    timeout: Option<u64>,
    consistency: Option<&ReadConsistencyParam>,
) {
    if let Some(secs) = timeout {
        query.push(("timeout".into(), secs.to_string()));
    }
    if let Some(c) = consistency {
        query.push(("consistency".into(), c.to_query_value()));
    }
}
