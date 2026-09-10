//! Typed semantic primitives for the plan layer.
//!
//! These types remain typed until a transport boundary. REST serialization
//! matches the OpenAPI wire format. gRPC converts them directly to protobuf
//! without reverse-engineering JSON shapes.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Deserialize, Serialize, Serializer};

// ── Point ID ────────────────────────────────────────────────────

/// Transport-neutral point ID: unsigned integer or string (typically UUID).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PlanPointId {
    /// Unsigned 64-bit point ID.
    Number(u64),
    /// String point ID, typically a UUID.
    String(String),
}

impl core::fmt::Display for PlanPointId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            PlanPointId::Number(n) => write!(f, "{n}"),
            PlanPointId::String(s) => write!(f, "{s}"),
        }
    }
}

impl From<&qql_core::ast::PointId> for PlanPointId {
    // INVARIANT: `Param` / `PositionalParam` arms panic. `plan()` gates with
    // `ensure_no_unbound_params` and `plan_template()` with
    // `validate_no_unbound_scalar_params` (which covers point IDs), so direct
    // callers must preserve that order.
    fn from(id: &qql_core::ast::PointId) -> Self {
        match id {
            qql_core::ast::PointId::Number(n) => PlanPointId::Number(*n),
            qql_core::ast::PointId::String(s) => PlanPointId::String(s.clone()),
            qql_core::ast::PointId::Param(name, _) => {
                panic!("invariant violation: unbound parameter :{name} reached PlanPointId");
            }
            qql_core::ast::PointId::PositionalParam(idx, _) => {
                panic!(
                    "invariant violation: unbound positional parameter ?{idx} reached PlanPointId"
                );
            }
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

impl core::fmt::Display for PlanShardKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Keyword(s) => write!(f, "{s}"),
            Self::Number(n) => write!(f, "{n}"),
        }
    }
}

impl From<&qql_core::ast::ShardKey> for PlanShardKey {
    // INVARIANT: `Param` / `PositionalParam` arms panic. `plan()` gates with
    // `ensure_no_unbound_params` and `plan_template()` with
    // `validate_no_unbound_scalar_params` (both cover shard keys, including
    // DDL key fields and `shard_keys` lists), and `bind_stmt` substitutes
    // placeholders before re-planning — so direct callers must preserve that
    // order, exactly like `PlanPointId::from`.
    fn from(key: &qql_core::ast::ShardKey) -> Self {
        match key {
            qql_core::ast::ShardKey::Keyword(s) => Self::Keyword(s.clone()),
            qql_core::ast::ShardKey::Number(n) => Self::Number(*n),
            qql_core::ast::ShardKey::Param(name, _) => {
                panic!("invariant violation: unbound parameter :{name} reached PlanShardKey");
            }
            qql_core::ast::ShardKey::PositionalParam(idx, _) => {
                panic!(
                    "invariant violation: unbound positional parameter ?{idx} reached PlanShardKey"
                );
            }
        }
    }
}

impl Serialize for PlanShardKey {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Number(n) => serializer.serialize_u64(*n),
            Self::Keyword(s) => serializer.serialize_str(s),
        }
    }
}

impl<'de> serde::Deserialize<'de> for PlanShardKey {
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
            Wire::Number(n) => Self::Number(n),
            Wire::String(s) => Self::Keyword(s),
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
    /// Parameter placeholder for vector (`:name`).
    Param(String),
    /// Positional parameter placeholder (`?N` or `?`).
    PositionalParam(usize),
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
            qql_core::ast::VectorValue::Param(name, _) => PlanVectorValue::Param(name.clone()),
            qql_core::ast::VectorValue::PositionalParam(idx, _) => {
                PlanVectorValue::PositionalParam(*idx)
            }
        }
    }
}

impl PlanVectorValue {
    /// Convert a `qql_core::ast::Value` to `PlanVectorValue` if possible.
    pub fn from_value(val: &qql_core::ast::Value) -> Option<Self> {
        match val {
            qql_core::ast::Value::F32Array(v) => Some(PlanVectorValue::Dense(v.clone())),
            qql_core::ast::Value::List(items) => {
                if items.is_empty() {
                    return Some(PlanVectorValue::Dense(Vec::new()));
                }
                if let qql_core::ast::Value::List(_) = &items[0] {
                    let mut rows = Vec::with_capacity(items.len());
                    for item in items {
                        if let qql_core::ast::Value::List(sub) = item {
                            let mut row = Vec::with_capacity(sub.len());
                            for f in sub {
                                match f {
                                    qql_core::ast::Value::Float(x) => row.push(*x as f32),
                                    qql_core::ast::Value::Int(i) => row.push(*i as f32),
                                    _ => return None,
                                }
                            }
                            rows.push(row);
                        } else {
                            return None;
                        }
                    }
                    Some(PlanVectorValue::MultiDense(rows))
                } else {
                    let mut dense = Vec::with_capacity(items.len());
                    for item in items {
                        match item {
                            qql_core::ast::Value::Float(x) => dense.push(*x as f32),
                            qql_core::ast::Value::Int(i) => dense.push(*i as f32),
                            _ => return None,
                        }
                    }
                    Some(PlanVectorValue::Dense(dense))
                }
            }
            qql_core::ast::Value::Dict(entries) => {
                let mut has_data = false;
                let mut has_dim = false;
                let mut has_indices = false;
                let mut has_values = false;
                for (k, _) in entries {
                    if k.eq_ignore_ascii_case("data") {
                        has_data = true;
                    } else if k.eq_ignore_ascii_case("dim") {
                        has_dim = true;
                    } else if k.eq_ignore_ascii_case("indices") {
                        has_indices = true;
                    } else if k.eq_ignore_ascii_case("values") {
                        has_values = true;
                    }
                }
                if has_data || has_dim {
                    if has_indices || has_values {
                        return None;
                    }
                    let mut flat: Option<Vec<f32>> = None;
                    let mut dim: Option<usize> = None;
                    for (k, v) in entries {
                        if k.eq_ignore_ascii_case("data") {
                            flat = match v {
                                qql_core::ast::Value::F32Array(items) => Some(items.clone()),
                                qql_core::ast::Value::List(items) => {
                                    let mut out = Vec::with_capacity(items.len());
                                    for item in items {
                                        match item {
                                            qql_core::ast::Value::Float(x) => out.push(*x as f32),
                                            qql_core::ast::Value::Int(i) => out.push(*i as f32),
                                            _ => return None,
                                        }
                                    }
                                    Some(out)
                                }
                                _ => return None,
                            };
                        } else if k.eq_ignore_ascii_case("dim") {
                            match v {
                                qql_core::ast::Value::Int(n) if *n > 0 => {
                                    dim = usize::try_from(*n).ok();
                                }
                                _ => return None,
                            }
                        } else {
                            return None;
                        }
                    }
                    let (flat, dim) = match (flat, dim) {
                        (Some(flat), Some(dim)) => (flat, dim),
                        _ => return None,
                    };
                    if flat.is_empty() || dim == 0 || flat.len() % dim != 0 {
                        return None;
                    }
                    return Some(PlanVectorValue::MultiDense(
                        flat.chunks_exact(dim).map(<[f32]>::to_vec).collect(),
                    ));
                }
                let mut indices = None;
                let mut values = None;
                for (k, v) in entries {
                    if k == "indices"
                        && let qql_core::ast::Value::List(idx_list) = v
                    {
                        let mut idxs = Vec::with_capacity(idx_list.len());
                        for idx in idx_list {
                            if let qql_core::ast::Value::Int(i) = idx {
                                idxs.push(*i as u32);
                            } else {
                                return None;
                            }
                        }
                        indices = Some(idxs);
                    } else if k == "values" {
                        match v {
                            qql_core::ast::Value::F32Array(items) => {
                                values = Some(items.clone());
                            }
                            qql_core::ast::Value::List(val_list) => {
                                let mut vals = Vec::with_capacity(val_list.len());
                                for val in val_list {
                                    match val {
                                        qql_core::ast::Value::Float(x) => vals.push(*x as f32),
                                        qql_core::ast::Value::Int(i) => vals.push(*i as f32),
                                        _ => return None,
                                    }
                                }
                                values = Some(vals);
                            }
                            _ => return None,
                        }
                    }
                }
                if let (Some(indices), Some(values)) = (indices, values) {
                    Some(PlanVectorValue::Sparse { indices, values })
                } else {
                    None
                }
            }
            _ => None,
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
            PlanVectorValue::Param(name) => serializer.serialize_str(&format!(":{name}")),
            PlanVectorValue::PositionalParam(idx) => serializer.serialize_str(&format!("?{idx}")),
        }
    }
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

impl Serialize for PlanVectorStruct {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            PlanVectorStruct::Single(value) => value.serialize(serializer),
            PlanVectorStruct::Named(vectors) => {
                let mut map = serializer.serialize_map(Some(vectors.len()))?;
                for (name, value) in vectors {
                    map.serialize_entry(name, value)?;
                }
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for PlanVectorStruct {
    /// Parse the OpenAPI `VectorStructOutput` shapes: dense array, array of
    /// dense rows, sparse `{indices, values}` object, or an object of named
    /// `VectorOutput` entries. Anything else (including inference `Document` /
    /// `Image` objects, which the output schema does not allow) fails.
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        /// One unnamed `VectorOutput`.
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum ValueWire {
            // Dense first: an empty array is an empty dense vector, not a
            // zero-row multi-dense one. Nested rows only match `MultiDense`.
            Dense(Vec<f32>),
            MultiDense(Vec<Vec<f32>>),
            Sparse { indices: Vec<u32>, values: Vec<f32> },
        }

        impl ValueWire {
            fn into_value(self) -> PlanVectorValue {
                match self {
                    ValueWire::Dense(values) => PlanVectorValue::Dense(values),
                    ValueWire::MultiDense(rows) => PlanVectorValue::MultiDense(rows),
                    ValueWire::Sparse { indices, values } => {
                        PlanVectorValue::Sparse { indices, values }
                    }
                }
            }
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Wire {
            Dense(Vec<f32>),
            MultiDense(Vec<Vec<f32>>),
            Sparse { indices: Vec<u32>, values: Vec<f32> },
            Named(BTreeMap<String, ValueWire>),
        }

        Ok(match Wire::deserialize(deserializer)? {
            Wire::Dense(values) => PlanVectorStruct::Single(PlanVectorValue::Dense(values)),
            Wire::MultiDense(rows) => PlanVectorStruct::Single(PlanVectorValue::MultiDense(rows)),
            Wire::Sparse { indices, values } => {
                PlanVectorStruct::Single(PlanVectorValue::Sparse { indices, values })
            }
            Wire::Named(vectors) => PlanVectorStruct::Named(
                vectors
                    .into_iter()
                    .map(|(name, value)| (name, value.into_value()))
                    .collect(),
            ),
        })
    }
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
    },
}

impl From<&qql_core::ast::QueryInput> for PlanQueryInput {
    fn from(input: &qql_core::ast::QueryInput) -> Self {
        match input {
            qql_core::ast::QueryInput::Point(id) => PlanQueryInput::Point(PlanPointId::from(id)),
            qql_core::ast::QueryInput::Vector(v) => {
                PlanQueryInput::Vector(PlanVectorValue::from(v))
            }
            qql_core::ast::QueryInput::Text { text, model, .. } => PlanQueryInput::Document {
                text: text.clone(),
                model: model.clone(),
            },
            qql_core::ast::QueryInput::Image { source, model } => PlanQueryInput::Image {
                image: source.clone(),
                model: model.clone(),
            },
            qql_core::ast::QueryInput::Param(name, _) => {
                PlanQueryInput::Vector(PlanVectorValue::Param(name.clone()))
            }
            qql_core::ast::QueryInput::PositionalParam(idx, _) => {
                PlanQueryInput::Vector(PlanVectorValue::PositionalParam(*idx))
            }
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
                // Model-less plans serialize "model": "" as a placeholder the
                // executor's embedding resolution replaces before dispatch.
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("text", text)?;
                map.serialize_entry("model", &model.as_deref().unwrap_or(""))?;
                map.end()
            }
            PlanQueryInput::Image { image, model } => {
                // OpenAPI Image requires both "image" and "model" fields.
                // gRPC proto Image uses a Value for the image field; REST
                // always serializes as an object with two string members.
                // Model-less plans keep "" as a placeholder for the executor.
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
    /// Parameter placeholder for unnamed or named vector (`:name`).
    Param(String),
    /// Positional parameter placeholder (`?N` or `?`).
    PositionalParam(usize),
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
            qql_core::ast::PointVectors::Param(name, _) => PlanPointVectors::Param(name.clone()),
            qql_core::ast::PointVectors::PositionalParam(idx, _) => {
                PlanPointVectors::PositionalParam(*idx)
            }
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
            PlanPointVectors::Param(name) => serializer.serialize_str(&format!(":{name}")),
            PlanPointVectors::PositionalParam(idx) => serializer.serialize_str(&format!("?{idx}")),
        }
    }
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
