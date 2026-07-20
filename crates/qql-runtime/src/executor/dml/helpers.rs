use crate::executor::PointId;
use qql_core::ast::Value;
use qql_core::error::QqlError;

pub(crate) fn extract_point_id(row: &[(String, Value)]) -> Result<PointId, QqlError> {
    let id_val = row.iter().find(|(k, _)| k == "id");
    match id_val {
        Some((_, Value::Int(i))) => {
            if *i < 0 {
                Err(QqlError::runtime("negative ID not supported"))
            } else {
                Ok(PointId::Num(*i as u64))
            }
        }
        Some((_, Value::Str(s))) => {
            if let Ok(num) = s.parse::<u64>() {
                Ok(PointId::Num(num))
            } else {
                Ok(PointId::Uuid(s.clone()))
            }
        }
        Some((_, Value::Float(f))) => {
            let v = *f;
            if v < 0.0 || v > (1u64 << 53) as f64 || v != (v as u64) as f64 {
                Err(QqlError::runtime(
                    "unsupported point ID: non-integer or oversized float",
                ))
            } else {
                Ok(PointId::Num(v as u64))
            }
        }
        _ => Err(QqlError::runtime(
            "INSERT requires an 'id' field in VALUES (unsigned integer or UUID string)",
        )),
    }
}

pub(crate) fn is_vector_key(key: &str) -> bool {
    key == "vector" || key == "_v" || key.starts_with("_v_")
}

pub(crate) fn has_vector_keys(values_list: &[Vec<(String, Value)>]) -> bool {
    for row in values_list {
        if row.iter().any(|(k, _)| is_vector_key(k)) {
            return true;
        }
    }
    false
}

