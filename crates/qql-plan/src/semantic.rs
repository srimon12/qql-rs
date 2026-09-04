//! Typed semantic primitives for the plan layer.
//!
//! These types remain typed until a transport boundary. REST serialization
//! matches the OpenAPI wire format. gRPC converts them directly to protobuf
//! without reverse-engineering JSON shapes.

use alloc::string::String;
use alloc::vec::Vec;
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Serialize, Serializer};

// ── Point ID ────────────────────────────────────────────────────

/// Transport-neutral point ID: unsigned integer or string (typically UUID).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanPointId {
    /// Unsigned 64-bit point ID.
    Number(u64),
    /// String point ID, typically a UUID.
    String(String),
}

impl From<&qql_core::ast::PointId> for PlanPointId {
    fn from(id: &qql_core::ast::PointId) -> Self {
        match id {
            qql_core::ast::PointId::Number(n) => PlanPointId::Number(*n),
            qql_core::ast::PointId::String(s) => PlanPointId::String(s.clone()),
        }
    }
}

impl Serialize for PlanPointId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            PlanPointId::Number(n) => serializer.serialize_u64(*n),
            PlanPointId::String(s) => serializer.serialize_str(s),
        }
    }
}

impl<'de> serde::Deserialize<'de> for PlanPointId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        #[serde(untagged)]
        enum Wire {
            Number(u64),
            String(String),
        }
        Ok(match Wire::deserialize(deserializer)? {
            Wire::Number(n) => PlanPointId::Number(n),
            Wire::String(s) => PlanPointId::String(s),
        })
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
}

impl From<&qql_core::ast::VectorValue> for PlanVectorValue {
    fn from(v: &qql_core::ast::VectorValue) -> Self {
        match v {
            qql_core::ast::VectorValue::Dense(d) => PlanVectorValue::Dense(d.clone()),
            qql_core::ast::VectorValue::Sparse { indices, values } => PlanVectorValue::Sparse {
                indices: indices.clone(),
                values: values.clone(),
            },
            qql_core::ast::VectorValue::MultiDense(rows) => {
                PlanVectorValue::MultiDense(rows.clone())
            }
        }
    }
}

/// Streams `f32` as JSON `f64` without allocating an intermediate `Vec<f64>`.
struct F64Elems<'a>(&'a [f32]);

impl Serialize for F64Elems<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for v in self.0 {
            seq.serialize_element(&(*v as f64))?;
        }
        seq.end()
    }
}

impl Serialize for PlanVectorValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            PlanVectorValue::Dense(values) => F64Elems(values).serialize(serializer),
            PlanVectorValue::Sparse { indices, values } => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("indices", indices)?;
                map.serialize_entry("values", values)?;
                map.end()
            }
            PlanVectorValue::MultiDense(rows) => {
                let mut seq = serializer.serialize_seq(Some(rows.len()))?;
                for row in rows {
                    seq.serialize_element(&F64Elems(row))?;
                }
                seq.end()
            }
        }
    }
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
    /// Server-side or client-pre-embed document. `model: None` serializes as a
    /// bare string for REST compatibility with historical QQL output.
    Document {
        /// Document text to embed.
        text: String,
        /// Embedding model; empty when the executor fills it in.
        model: Option<String>,
    },
    /// OpenAPI `Image` inference input (image URL or base64 + model).
    /// Prefer resolving to a dense [`PlanQueryInput::Vector`] client-side when
    /// the host has an image embedder; otherwise the wire form is preserved.
    Image {
        /// Image URL or base64 payload.
        image: String,
        /// Image embedding model; empty when the executor fills it in.
        model: Option<String>,
    },
}

impl From<&qql_core::ast::QueryInput> for PlanQueryInput {
    fn from(input: &qql_core::ast::QueryInput) -> Self {
        match input {
            qql_core::ast::QueryInput::Point(id) => PlanQueryInput::Point(PlanPointId::from(id)),
            qql_core::ast::QueryInput::Vector(v) => {
                PlanQueryInput::Vector(PlanVectorValue::from(v))
            }
            qql_core::ast::QueryInput::Text { text, model } => PlanQueryInput::Document {
                text: text.clone(),
                model: model.clone(),
            },
            qql_core::ast::QueryInput::Image { source, model } => PlanQueryInput::Image {
                image: source.clone(),
                model: model.clone(),
            },
        }
    }
}

impl Serialize for PlanQueryInput {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            PlanQueryInput::Point(id) => id.serialize(serializer),
            PlanQueryInput::Vector(v) => v.serialize(serializer),
            PlanQueryInput::Document { text, model } => {
                // OpenAPI Document requires both "text" and "model" fields.
                // Planning validation rejects model-less Documents before we
                // reach serialization; keep a safe fallback to prevent a bare
                // string from ever leaking to the REST wire.
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("text", text)?;
                map.serialize_entry("model", &model.as_deref().unwrap_or(""))?;
                map.end()
            }
            PlanQueryInput::Image { image, model } => {
                // OpenAPI Image requires both "image" and "model" fields.
                // gRPC proto Image uses a Value for the image field; REST
                // always serializes as an object with two string members.
                // Planning validation rejects model-less Images before we
                // reach serialization; keep a safe fallback for the model key.
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("image", image)?;
                map.serialize_entry("model", &model.as_deref().unwrap_or(""))?;
                map.end()
            }
        }
    }
}

// ── Point vectors (upsert body) ─────────────────────────────────

/// Vectors carried by one point: single unnamed vector or named set.
#[derive(Debug, Clone, PartialEq)]
pub enum PlanPointVectors {
    /// Single unnamed vector (single-vector collection).
    Unnamed(PlanVectorValue),
    /// Named vectors as `(name, value)` pairs.
    Named(Vec<(String, PlanVectorValue)>),
}

impl From<&qql_core::ast::PointVectors> for PlanPointVectors {
    fn from(v: &qql_core::ast::PointVectors) -> Self {
        match v {
            qql_core::ast::PointVectors::Unnamed(vv) => {
                PlanPointVectors::Unnamed(PlanVectorValue::from(vv))
            }
            qql_core::ast::PointVectors::Named(entries) => PlanPointVectors::Named(
                entries
                    .iter()
                    .map(|(n, vv)| (n.clone(), PlanVectorValue::from(vv)))
                    .collect(),
            ),
        }
    }
}

impl Serialize for PlanPointVectors {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            PlanPointVectors::Unnamed(v) => v.serialize(serializer),
            PlanPointVectors::Named(entries) => {
                let mut map = serializer.serialize_map(Some(entries.len()))?;
                for (name, value) in entries {
                    map.serialize_entry(name, value)?;
                }
                map.end()
            }
        }
    }
}

// ── Formula (typed; REST Serialize via plan lowering) ───────────

/// Plan-owned formula tree. Keeps AST semantics; REST wire uses snake_case
/// OpenAPI keys via custom serialization in `crate::query::serialize_formula`.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanFormula(pub qql_core::ast::FormulaExpr);

impl Serialize for PlanFormula {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Delegate to JSON intermediate that already matches OpenAPI Expression.
        let value = crate::query::lower_formula_expr(&self.0);
        value.serialize(serializer)
    }
}
