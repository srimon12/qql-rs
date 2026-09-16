//! Typed semantic primitives for the plan layer.
//!
//! These types remain typed until a transport boundary. REST serialization
//! matches the OpenAPI wire format. gRPC converts them directly to protobuf
//! without reverse-engineering JSON shapes.
//!
//! Conversions (`Display` / `From` / `Serialize` / `Deserialize`) live in
//! [`crate::semantic_conv`]; this module keeps the type definitions.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

// ── Point ID ────────────────────────────────────────────────────

/// Transport-neutral point ID: unsigned integer or string (typically UUID).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PlanPointId {
    /// Unsigned 64-bit point ID.
    Number(u64),
    /// String point ID, typically a UUID.
    String(String),
}

// ── Shard key ───────────────────────────────────────────────────

/// Transport-neutral custom shard key. Serializes as a JSON string or number
/// so REST matches OpenAPI `ExtendedPointId`-style shard keys.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PlanShardKey {
    /// Keyword / UUID shard key.
    Keyword(String),
    /// Numeric shard key.
    Number(u64),
}

impl PlanShardKey {
    /// Keyword text, when this key is a string.
    pub fn as_keyword(&self) -> Option<&str> {
        match self {
            Self::Keyword(s) => Some(s.as_str()),
            Self::Number(_) => None,
        }
    }
}

// ── Vector value ────────────────────────────────────────────────

/// Transport-neutral vector value: dense, sparse, or multi-dense rows.
#[derive(Debug, Clone, PartialEq)]
pub enum PlanVectorValue {
    /// Single dense `f32` vector.
    Dense(Vec<f32>),
    /// Sparse vector with paired `indices` and `values` arrays.
    Sparse {
        /// Row indices of the non-zero entries.
        indices: Vec<u32>,
        /// Values at the indexed rows.
        values: Vec<f32>,
    },
    /// Multi-dense vector: one row per input token or patch.
    MultiDense(Vec<Vec<f32>>),
    /// Per-point document inference (OpenAPI `Document` passthrough).
    Document {
        /// Document text to embed.
        text: String,
        /// Embedding model; empty when the executor fills it in.
        model: Option<String>,
        /// Opaque inference options, serialized as `options` when present.
        options: Option<serde_json::Map<String, serde_json::Value>>,
    },
    /// Per-point image inference (OpenAPI `Image` passthrough).
    Image {
        /// Image URL or base64 payload.
        image: String,
        /// Image embedding model; empty when the executor fills it in.
        model: Option<String>,
        /// Opaque inference options, serialized as `options` when present.
        options: Option<serde_json::Map<String, serde_json::Value>>,
    },
    /// Per-point custom inference (OpenAPI `InferenceObject` passthrough).
    Object {
        /// Arbitrary model input, serialized as `object`.
        object: serde_json::Value,
        /// Embedding model; empty when the executor fills it in.
        model: Option<String>,
        /// Opaque inference options, serialized as `options` when present.
        options: Option<serde_json::Map<String, serde_json::Value>>,
    },
    /// Parameter placeholder for vector (`:name`).
    Param(String),
    /// Positional parameter placeholder (`?N` or `?`).
    PositionalParam(usize),
}

// ── Returned vectors ────────────────────────────────────────────

/// Vector(s) attached to a returned point: one unnamed vector or a named set.
///
/// Serializes as the OpenAPI `VectorStructOutput` shape — the value itself for
/// [`PlanVectorStruct::Single`], a name → value map for
/// [`PlanVectorStruct::Named`]. Named entries use `BTreeMap` so serialization
/// order is deterministic. Deserialization never produces parameter
/// placeholders: responses always carry concrete vectors.
#[derive(Debug, Clone, PartialEq)]
pub enum PlanVectorStruct {
    /// Unnamed single vector (dense, sparse, or multi-dense).
    Single(PlanVectorValue),
    /// Named vectors, keyed by vector name.
    Named(BTreeMap<String, PlanVectorValue>),
}

// ── Facet / group values ────────────────────────────────────────

/// One facet value as returned by Qdrant: keyword, integer, or bool
/// (OpenAPI `FacetValue`). Serializes untagged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PlanFacetValue {
    /// Keyword (string) facet value.
    Keyword(String),
    /// Signed integer facet value.
    Integer(i64),
    /// Boolean facet value.
    Bool(bool),
}

/// Group key of a grouped query result (OpenAPI `GroupId`): keyword,
/// unsigned integer, or signed integer. Serializes untagged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PlanGroupId {
    /// Keyword (string) group key.
    Keyword(String),
    /// Unsigned integer group key.
    Unsigned(u64),
    /// Signed integer group key.
    Signed(i64),
}

// ── Query / vector input ────────────────────────────────────────

/// Semantic query input — preserves point / dense / sparse / multi / document /
/// image distinctions that JSON shape inference cannot recover.
#[derive(Debug, Clone, PartialEq)]
pub enum PlanQueryInput {
    /// Point-ID input resolved by the backend.
    Point(PlanPointId),
    /// Inline dense/sparse/multi-dense vector input.
    Vector(PlanVectorValue),
    /// Server-side or client-pre-embed document. `model: None` serializes as
    /// `{"text": …, "model": ""}` — a placeholder the executor's embedding
    /// resolution must replace before dispatch. Offline `compile_statement`
    /// output on an un-prepared plan therefore requires preparation.
    Document {
        /// Document text to embed.
        text: String,
        /// Embedding model; empty when the executor fills it in.
        model: Option<String>,
        /// Opaque inference options, serialized as `options` when present.
        options: Option<serde_json::Map<String, serde_json::Value>>,
    },
    /// OpenAPI `Image` inference input (image URL or base64 + model).
    /// Prefer resolving to a dense [`PlanQueryInput::Vector`] client-side when
    /// the host has an image embedder; otherwise the wire form is preserved.
    /// Like [`PlanQueryInput::Document`], `model: None` serializes as
    /// `{"image": …, "model": ""}` pending executor resolution.
    Image {
        /// Image URL or base64 payload.
        image: String,
        /// Image embedding model; empty when the executor fills it in.
        model: Option<String>,
        /// Opaque inference options, serialized as `options` when present.
        options: Option<serde_json::Map<String, serde_json::Value>>,
    },
    /// OpenAPI `InferenceObject` input (arbitrary model payload + model).
    /// Passthrough only: the executor never interprets `object`.
    Object {
        /// Arbitrary model input, serialized as `object`.
        object: serde_json::Value,
        /// Embedding model; empty when the executor fills it in.
        model: Option<String>,
        /// Opaque inference options, serialized as `options` when present.
        options: Option<serde_json::Map<String, serde_json::Value>>,
    },
}

// ── Point vectors (upsert body) ─────────────────────────────────

/// Vectors carried by one point: single unnamed vector or named set.
#[derive(Debug, Clone, PartialEq)]
pub enum PlanPointVectors {
    /// Single unnamed vector (single-vector collection).
    Unnamed(PlanVectorValue),
    /// Named vectors as `(name, value)` pairs.
    Named(Vec<(String, PlanVectorValue)>),
    /// Parameter placeholder for unnamed or named vector (`:name`).
    Param(String),
    /// Positional parameter placeholder (`?N` or `?`).
    PositionalParam(usize),
}

#[cfg(test)]
mod tests {
    use super::*;
    use qql_core::ast::Value;

    fn dict(pairs: Vec<(&str, Value)>) -> Value {
        Value::Dict(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
    }

    #[test]
    fn from_value_dense_float_list_and_f32array_lower_identically() {
        // Python list-bound vectors (flat list[float] → Value::List) and
        // buffer-bound vectors (numpy / array.array → Value::F32Array) must
        // lower to the same PlanVectorValue: the host heuristic packs long
        // flat float lists as F32Array, so both spellings have to meet here.
        let floats: Vec<Value> = (0..384).map(|i| Value::Float(i as f64 * 0.001)).collect();
        let packed: Vec<f32> = (0..384).map(|i| (i as f64 * 0.001) as f32).collect();
        assert_eq!(
            PlanVectorValue::from_value(&Value::List(floats)),
            PlanVectorValue::from_value(&Value::F32Array(packed.clone()))
        );
        assert_eq!(
            PlanVectorValue::from_value(&Value::F32Array(packed.clone())),
            Some(PlanVectorValue::Dense(packed))
        );
    }

    #[test]
    fn from_value_flat_multivector_chunks() {
        let v = dict(vec![
            (
                "data",
                Value::List(vec![
                    Value::Float(0.1),
                    Value::Float(0.2),
                    Value::Float(0.3),
                    Value::Float(0.4),
                ]),
            ),
            ("dim", Value::Int(2)),
        ]);
        assert_eq!(
            PlanVectorValue::from_value(&v),
            Some(PlanVectorValue::MultiDense(vec![
                vec![0.1, 0.2],
                vec![0.3, 0.4]
            ]))
        );
        // F32Array data takes the same path without per-element boxing.
        let v = dict(vec![
            ("data", Value::F32Array(vec![0.1, 0.2])),
            ("dim", Value::Int(2)),
        ]);
        assert_eq!(
            PlanVectorValue::from_value(&v),
            Some(PlanVectorValue::MultiDense(vec![vec![0.1, 0.2]]))
        );
        // Nested lists stay equivalent to the flat spelling.
        let nested = Value::List(vec![
            Value::List(vec![Value::Float(0.1), Value::Float(0.2)]),
            Value::List(vec![Value::Float(0.3), Value::Float(0.4)]),
        ]);
        let flat2 = dict(vec![
            (
                "data",
                Value::List(vec![
                    Value::Float(0.1),
                    Value::Float(0.2),
                    Value::Float(0.3),
                    Value::Float(0.4),
                ]),
            ),
            ("dim", Value::Int(2)),
        ]);
        assert_eq!(
            PlanVectorValue::from_value(&nested),
            PlanVectorValue::from_value(&flat2)
        );
    }

    #[test]
    fn from_value_flat_multivector_rejects_bad_shapes() {
        // Missing dim.
        assert_eq!(
            PlanVectorValue::from_value(&dict(vec![("data", Value::List(vec![]))])),
            None
        );
        // Length not a multiple of dim.
        assert_eq!(
            PlanVectorValue::from_value(&dict(vec![
                ("data", Value::List(vec![Value::Float(0.1)])),
                ("dim", Value::Int(2)),
            ])),
            None
        );
        // Non-positive dim.
        assert_eq!(
            PlanVectorValue::from_value(&dict(vec![
                ("data", Value::List(vec![Value::Float(0.1)])),
                ("dim", Value::Int(0)),
            ])),
            None
        );
        // Mixed flat + sparse keys fail closed.
        assert_eq!(
            PlanVectorValue::from_value(&dict(vec![
                ("data", Value::List(vec![Value::Float(0.1)])),
                ("dim", Value::Int(1)),
                ("indices", Value::List(vec![Value::Int(0)])),
            ])),
            None
        );
    }

    #[test]
    fn from_value_sparse_accepts_f32array_values() {
        let v = dict(vec![
            ("indices", Value::List(vec![Value::Int(1), Value::Int(5)])),
            ("values", Value::F32Array(vec![0.5, 0.8])),
        ]);
        assert_eq!(
            PlanVectorValue::from_value(&v),
            Some(PlanVectorValue::Sparse {
                indices: vec![1, 5],
                values: vec![0.5, 0.8]
            })
        );
    }

    #[test]
    fn vector_struct_round_trips_every_output_shape() {
        for (json, typed) in [
            (
                serde_json::json!([0.5, 0.25]),
                PlanVectorStruct::Single(PlanVectorValue::Dense(vec![0.5, 0.25])),
            ),
            (
                serde_json::json!([[0.5, 0.25], [0.125, 0.0]]),
                PlanVectorStruct::Single(PlanVectorValue::MultiDense(vec![
                    vec![0.5, 0.25],
                    vec![0.125, 0.0],
                ])),
            ),
            (
                serde_json::json!({"indices": [1, 5], "values": [0.5, 0.75]}),
                PlanVectorStruct::Single(PlanVectorValue::Sparse {
                    indices: vec![1, 5],
                    values: vec![0.5, 0.75],
                }),
            ),
            (
                serde_json::json!({
                    "dense": [0.5, 0.25],
                    "sparse": {"indices": [2], "values": [0.5]},
                }),
                PlanVectorStruct::Named(BTreeMap::from([
                    ("dense".to_string(), PlanVectorValue::Dense(vec![0.5, 0.25])),
                    (
                        "sparse".to_string(),
                        PlanVectorValue::Sparse {
                            indices: vec![2],
                            values: vec![0.5],
                        },
                    ),
                ])),
            ),
        ] {
            let parsed: PlanVectorStruct = serde_json::from_value(json.clone()).unwrap();
            assert_eq!(parsed, typed, "parse {json}");
            assert_eq!(serde_json::to_value(&typed).unwrap(), json, "serialize");
        }
    }

    #[test]
    fn vector_struct_rejects_unmodelled_shapes() {
        // Inference document/image objects are not part of `VectorStructOutput`.
        assert!(
            serde_json::from_value::<PlanVectorStruct>(
                serde_json::json!({"text": "hello", "model": "m"})
            )
            .is_err()
        );
        assert!(serde_json::from_value::<PlanVectorStruct>(serde_json::json!(null)).is_err());
        assert!(serde_json::from_value::<PlanVectorStruct>(serde_json::json!("dense")).is_err());
    }

    #[test]
    fn facet_and_group_values_serialize_untagged() {
        assert_eq!(
            serde_json::to_value(PlanFacetValue::Keyword("books".into())).unwrap(),
            serde_json::json!("books")
        );
        assert_eq!(
            serde_json::to_value(PlanFacetValue::Integer(-3)).unwrap(),
            serde_json::json!(-3)
        );
        assert_eq!(
            serde_json::to_value(PlanFacetValue::Bool(true)).unwrap(),
            serde_json::json!(true)
        );
        assert_eq!(
            serde_json::from_value::<PlanFacetValue>(serde_json::json!(7)).unwrap(),
            PlanFacetValue::Integer(7)
        );

        for (json, typed) in [
            (serde_json::json!("a"), PlanGroupId::Keyword("a".into())),
            (serde_json::json!(7), PlanGroupId::Unsigned(7)),
            (serde_json::json!(-3), PlanGroupId::Signed(-3)),
        ] {
            let parsed: PlanGroupId = serde_json::from_value(json.clone()).unwrap();
            assert_eq!(parsed, typed);
            assert_eq!(serde_json::to_value(&typed).unwrap(), json);
        }
    }
}
