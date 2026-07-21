use std::collections::HashMap;

use qdrant_edge::{VectorInternal, VectorStructInternal};
use serde_json::Value;

use qql_core::error::QqlError;

pub trait ToEdgeVector {
    fn to_edge_vector(self) -> Result<VectorStructInternal, QqlError>;
}

impl ToEdgeVector for serde_json::Value {
    fn to_edge_vector(self) -> Result<VectorStructInternal, QqlError> {
        parse_vector_struct(self)
    }
}

fn err(msg: impl Into<std::borrow::Cow<'static, str>>) -> QqlError {
    QqlError::execution("QQL-EDGE", msg, None)
}

pub(crate) fn parse_vector_struct(
    val: serde_json::Value,
) -> Result<VectorStructInternal, QqlError> {
    match val {
        Value::Array(arr) => parse_array_vector(arr),
        Value::Object(obj) => parse_object_vector(obj),
        _ => Err(err(format!("unsupported vector shape: {val:?}"))),
    }
}

pub(crate) fn parse_array_vector(arr: Vec<Value>) -> Result<VectorStructInternal, QqlError> {
    if arr.is_empty() {
        return Err(err("empty vector array provided"));
    }
    let is_multi = arr.first().is_some_and(|first| first.is_array());
    if is_multi {
        let multi: Vec<Vec<f32>> = serde_json::from_value(Value::Array(arr))
            .map_err(|e| err(format!("invalid multivector: {e}")))?;
        if multi.iter().any(|sub| sub.is_empty()) {
            return Err(err("empty multivector sub-array provided"));
        }
        let vec = qdrant_edge::Vector::new_multi(multi)
            .map_err(|e| err(format!("invalid multivector: {e}")))?;
        Ok(qdrant_edge::Vectors::from(vec).into())
    } else {
        let dense: Vec<f32> = serde_json::from_value(Value::Array(arr))
            .map_err(|e| err(format!("invalid dense vector: {e}")))?;
        if dense.is_empty() {
            return Err(err("dense vector cannot be empty"));
        }
        let mut map = HashMap::with_capacity(1);
        map.insert(String::new(), VectorInternal::Dense(dense));
        Ok(VectorStructInternal::Named(map))
    }
}

pub(crate) fn parse_object_vector(
    obj: serde_json::Map<String, Value>,
) -> Result<VectorStructInternal, QqlError> {
    if obj.contains_key("indices") && obj.contains_key("values") {
        let sv: qdrant_edge::SparseVector = serde_json::from_value(Value::Object(obj))
            .map_err(|e| err(format!("invalid sparse vector: {e}")))?;
        let mut map = HashMap::with_capacity(1);
        map.insert(String::new(), VectorInternal::Sparse(sv));
        Ok(VectorStructInternal::Named(map))
    } else {
        let mut map = HashMap::with_capacity(obj.len());
        for (k, v) in obj {
            let vec_internal = match v {
                Value::Object(sparse_obj)
                    if sparse_obj.contains_key("indices") && sparse_obj.contains_key("values") =>
                {
                    let sv: qdrant_edge::SparseVector =
                        serde_json::from_value(Value::Object(sparse_obj))
                            .map_err(|e| err(format!("invalid sparse vector in named: {e}")))?;
                    VectorInternal::Sparse(sv)
                }
                Value::Array(arr) => {
                    if arr.first().is_some_and(|f| f.is_array()) {
                        let multi: Vec<Vec<f32>> = serde_json::from_value(Value::Array(arr))
                            .map_err(|e| err(format!("invalid multivector in named: {e}")))?;
                        let vec = qdrant_edge::Vector::new_multi(multi)
                            .map_err(|e| err(format!("invalid multivector in named: {e}")))?;
                        vec.0
                    } else {
                        let dense: Vec<f32> = serde_json::from_value(Value::Array(arr))
                            .map_err(|e| err(format!("invalid dense vector in named: {e}")))?;
                        VectorInternal::Dense(dense)
                    }
                }
                _ => return Err(err(format!("invalid named vector format for key '{k}'"))),
            };
            map.insert(k, vec_internal);
        }
        Ok(VectorStructInternal::Named(map))
    }
}
