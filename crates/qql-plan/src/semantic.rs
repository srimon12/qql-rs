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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanPointId {
    Number(u64),
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

// ── Vector value ────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum PlanVectorValue {
    Dense(Vec<f32>),
    Sparse { indices: Vec<u32>, values: Vec<f32> },
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

impl Serialize for PlanVectorValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            PlanVectorValue::Dense(values) => {
                let mut seq = serializer.serialize_seq(Some(values.len()))?;
                for v in values {
                    seq.serialize_element(&(*v as f64))?;
                }
                seq.end()
            }
            PlanVectorValue::Sparse { indices, values } => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("indices", indices)?;
                map.serialize_entry("values", values)?;
                map.end()
            }
            PlanVectorValue::MultiDense(rows) => {
                let mut seq = serializer.serialize_seq(Some(rows.len()))?;
                for row in rows {
                    let floats: Vec<f64> = row.iter().map(|v| *v as f64).collect();
                    seq.serialize_element(&floats)?;
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
    Point(PlanPointId),
    Vector(PlanVectorValue),
    /// Server-side or client-pre-embed document. `model: None` serializes as a
    /// bare string for REST compatibility with historical QQL output.
    Document {
        text: String,
        model: Option<String>,
    },
    /// OpenAPI `Image` inference input (image URL or base64 + model).
    /// Prefer resolving to a dense [`PlanQueryInput::Vector`] client-side when
    /// the host has an image embedder; otherwise the wire form is preserved.
    Image {
        image: String,
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

#[derive(Debug, Clone, PartialEq)]
pub enum PlanPointVectors {
    Unnamed(PlanVectorValue),
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
