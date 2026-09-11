use std::collections::HashMap;

use qdrant_edge::{VectorInternal, VectorStructInternal};

use qql_core::error::QqlError;
use qql_plan::{PlanPointVectors, PlanVectorValue};

pub trait ToEdgeVector {
    fn to_edge_vector(self) -> Result<VectorStructInternal, QqlError>;
}

impl ToEdgeVector for PlanPointVectors {
    fn to_edge_vector(self) -> Result<VectorStructInternal, QqlError> {
        match self {
            PlanPointVectors::Unnamed(v) => plan_vector_to_edge(v, None),
            PlanPointVectors::Named(entries) => {
                let mut map = HashMap::with_capacity(entries.len());
                for (name, v) in entries {
                    map.insert(name, plan_vector_value_internal(v)?);
                }
                Ok(VectorStructInternal::Named(map))
            }
            PlanPointVectors::Param(name) => Err(err(format!(
                "unbound parameter ':{name}' reached edge vector parsing"
            ))),
            PlanPointVectors::PositionalParam(idx) => Err(err(format!(
                "unbound positional parameter ?{idx} reached edge vector parsing"
            ))),
        }
    }
}

fn plan_vector_value_internal(v: PlanVectorValue) -> Result<VectorInternal, QqlError> {
    match v {
        PlanVectorValue::Dense(d) => Ok(VectorInternal::Dense(d)),
        PlanVectorValue::Sparse { indices, values } => {
            Ok(VectorInternal::Sparse(qdrant_edge::SparseVector {
                indices,
                values,
            }))
        }
        PlanVectorValue::MultiDense(rows) => {
            if rows.is_empty() {
                return Err(err("empty multivector"));
            }
            let vec = qdrant_edge::Vector::new_multi(rows)
                .map_err(|e| err(format!("invalid multivector: {e}")))?;
            Ok(vec.0)
        }
        PlanVectorValue::Param(name) => Err(err(format!(
            "unbound parameter ':{name}' reached edge vector execution"
        ))),
        PlanVectorValue::PositionalParam(idx) => Err(err(format!(
            "unbound positional parameter ?{idx} reached edge vector execution"
        ))),
    }
}

fn plan_vector_to_edge(
    v: PlanVectorValue,
    name: Option<String>,
) -> Result<VectorStructInternal, QqlError> {
    match &v {
        PlanVectorValue::MultiDense(rows) => {
            let vec = qdrant_edge::Vector::new_multi(rows.clone())
                .map_err(|e| err(format!("invalid multivector: {e}")))?;
            Ok(qdrant_edge::Vectors::from(vec).into())
        }
        _ => {
            let internal = plan_vector_value_internal(v)?;
            let mut map = HashMap::with_capacity(1);
            map.insert(name.unwrap_or_default(), internal);
            Ok(VectorStructInternal::Named(map))
        }
    }
}

fn err(msg: impl Into<std::borrow::Cow<'static, str>>) -> QqlError {
    QqlError::execution("QQL-EDGE-VECTOR", msg, None)
}
