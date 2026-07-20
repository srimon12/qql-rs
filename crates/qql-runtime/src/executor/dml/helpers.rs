use serde_json;

use crate::executor::PointId;
use qql_core::ast::Value;
use qql_core::error::QqlError;

use crate::executor::helpers::value_to_json;

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

pub(crate) fn extract_provided_vectors(
    values_list: &[Vec<(String, Value)>],
) -> Result<Vec<Option<serde_json::Value>>, QqlError> {
    let mut batch = Vec::with_capacity(values_list.len());
    for row in values_list {
        let mut vectors = serde_json::Map::new();
        for (k, v) in row {
            if !is_vector_key(k) {
                continue;
            }
            let vec_name = if k == "vector" || k == "_v" {
                "" // unnamed single vector
            } else {
                k.strip_prefix("_v_").unwrap_or(k)
            };

            match v {
                Value::Dict(items) => {
                    // Named vectors: {"dense": [...], "sparse": {...}}
                    for (nk, nv) in items {
                        vectors.insert(nk.clone(), value_to_json(nv));
                    }
                }
                Value::List(_items) => {
                    let json_val = value_to_json(v);
                    if vec_name.is_empty() {
                        vectors.insert(crate::executor::DENSE_VECTOR_NAME.to_string(), json_val);
                    } else {
                        vectors.insert(vec_name.to_string(), json_val);
                    }
                }
                _ => {
                    vectors.insert(vec_name.to_string(), value_to_json(v));
                }
            }
        }
        batch.push(if vectors.is_empty() {
            None
        } else {
            Some(serde_json::Value::Object(vectors))
        });
    }
    Ok(batch)
}
