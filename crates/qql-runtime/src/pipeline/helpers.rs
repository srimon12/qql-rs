use qql_core::ast;
use qql_core::error::QqlError;

use super::{PointId, QuantizationSearchParams, QueryState, SearchParams, VectorInput};

pub fn is_uuid(s: &str) -> bool {
    if s.len() != 36 {
        return false;
    }
    let bytes = s.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if i == 8 || i == 13 || i == 18 || i == 23 {
            if b != b'-' {
                return false;
            }
        } else {
            if !(b.is_ascii_digit() || (b'a'..=b'f').contains(&b) || (b'A'..=b'F').contains(&b)) {
                return false;
            }
        }
    }
    true
}

pub fn to_point_id(val: &ast::Value) -> Result<PointId, QqlError> {
    match val {
        ast::Value::Str(s) => {
            if let Ok(num) = s.parse::<u64>() {
                Ok(PointId::Num(num))
            } else {
                Ok(PointId::Uuid(s.to_string()))
            }
        }
        ast::Value::Int(i) => {
            if *i < 0 {
                return Err(QqlError::execution("QQL-EXECUTION", 
                    "unsupported vector input type: negative integer", None));
            }
            Ok(PointId::Num(*i as u64))
        }
        ast::Value::Float(f) => {
            let v = *f;
            if v < 0.0 || v > (1u64 << 53) as f64 || v != (v as u64) as f64 {
                return Err(QqlError::execution("QQL-EXECUTION", 
                    "unsupported vector input type: non-integer or oversized float", None));
            }
            Ok(PointId::Num(v as u64))
        }
        _ => Err(QqlError::execution("QQL-EXECUTION", format!(
            "unsupported vector input type: {:?}",
            val
        ), None)),
    }
}

#[allow(dead_code)]
pub fn point_id_to_value(pid: &PointId) -> ast::Value {
    match pid {
        PointId::Num(n) => ast::Value::Int(*n as i64),
        PointId::Uuid(s) => ast::Value::Str(s.clone()),
    }
}

pub async fn build_vector_input(
    state: &QueryState,
    val: &ast::Value,
) -> Result<VectorInput, QqlError> {
    match val {
        ast::Value::Str(s) => {
            if !is_uuid(s) && s.parse::<u64>().is_err() {
                if state.local_embed {
                    let embedder = state.embedder.as_ref().ok_or_else(|| {
                        QqlError::execution("QQL-EXECUTION", "local embedding requested but no Embedder provided", None)
                    })?;
                    let dense_vector =
                        embedder
                            .embed_dense(s, &state.dense_model)
                            .await
                            .map_err(|e| {
                                QqlError::execution("QQL-EXECUTION", format!("failed to embed target query: {}", e), None)
                            })?;
                    return Ok(VectorInput::Dense(dense_vector));
                }
                let mut options = state.cloud_model_options.clone();
                if let Some(ref doc_options) = state.doc_options {
                    options.extend(doc_options.clone());
                }
                return Ok(VectorInput::Document {
                    text: s.to_string(),
                    model: state.dense_model.clone(),
                    options,
                });
            }
            let pid = to_point_id(val)?;
            Ok(VectorInput::Id(pid))
        }
        ast::Value::Int(_) | ast::Value::Float(_) => {
            let pid = to_point_id(val)?;
            Ok(VectorInput::Id(pid))
        }
        _ => Err(QqlError::execution("QQL-EXECUTION", format!(
            "unsupported vector input type: {:?}",
            val
        ), None)),
    }
}

pub fn build_search_params(params: &ast::SearchParams) -> Option<SearchParams> {
    let mut result = SearchParams {
        hnsw_ef: None,
        exact: None,
        acorn: None,
        indexed_only: None,
        quantization: None,
    };

    let mut has_any = false;

    if let Some(ef) = params.hnsw_ef {
        if ef > 0 {
            result.hnsw_ef = Some(ef);
            has_any = true;
        }
    }
    if params.exact == Some(true) {
        result.exact = Some(true);
        has_any = true;
    }
    if params.acorn == Some(true) {
        result.acorn = Some(crate::pipeline::AcornSearchParams { enable: true });
        has_any = true;
    }
    if params.indexed_only == Some(true) {
        result.indexed_only = Some(true);
        has_any = true;
    }
    if let Some(ref q) = params.quantization {
        result.quantization = Some(QuantizationSearchParams {
            ignore: q.ignore,
            rescore: q.rescore,
            oversampling: q.oversampling,
        });
        has_any = true;
    }

    if has_any {
        Some(result)
    } else {
        None
    }
}
