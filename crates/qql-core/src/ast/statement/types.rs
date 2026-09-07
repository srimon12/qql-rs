//! Primitive types, identifiers, vectors, and selectors for QQL statements.

use alloc::string::String;
use alloc::vec::Vec;

/// Memory placement of a storage component (`cold` / `cached` / `pinned`).
///
/// Mirrors Qdrant 1.19 `Memory`. Data is always persisted on disk; this only
/// controls how the component is held in RAM. `Pinned` is not valid for
/// payload storage — parsers reject it for `payload_memory`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum MemoryPlacement {
    /// Prefer disk; load on demand.
    Cold,
    /// Cache in RAM when hot.
    Cached,
    /// Keep in RAM (invalid for payload storage).
    Pinned,
}

impl MemoryPlacement {
    /// Canonical lowercase keyword for this placement.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cold => "cold",
            Self::Cached => "cached",
            Self::Pinned => "pinned",
        }
    }

    /// Parse a placement string (case-insensitive). Unknown values → `None`.
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "cold" => Some(Self::Cold),
            "cached" => Some(Self::Cached),
            "pinned" => Some(Self::Pinned),
            _ => None,
        }
    }
}

impl core::fmt::Display for MemoryPlacement {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Dense / sparse vector storage datatype (OpenAPI `Datatype`).
///
/// Aliases accepted at parse: `f32`→`Float32`, `f16`→`Float16`, `u8`→`Uint8`,
/// `t4`→`Turbo4`. Sparse indexes reject `Turbo4` (unsupported upstream).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum VectorDatatype {
    /// 32-bit IEEE float (default dense storage).
    Float32,
    /// 16-bit IEEE half float.
    Float16,
    /// 8-bit unsigned integer.
    Uint8,
    /// 4-bit Turbo quantized storage (dense vectors only).
    Turbo4,
}

impl VectorDatatype {
    /// Canonical lowercase keyword for this datatype.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Float32 => "float32",
            Self::Float16 => "float16",
            Self::Uint8 => "uint8",
            Self::Turbo4 => "turbo4",
        }
    }

    /// Parse a datatype string including short aliases. Unknown → `None`.
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "float32" | "f32" => Some(Self::Float32),
            "float16" | "f16" => Some(Self::Float16),
            "uint8" | "u8" => Some(Self::Uint8),
            "turbo4" | "t4" => Some(Self::Turbo4),
            _ => None,
        }
    }

    /// Sparse indexes support float32 / float16 / uint8 only (not turbo4).
    pub fn parse_sparse(s: &str) -> Option<Self> {
        match Self::parse(s) {
            Some(Self::Turbo4) => None,
            other => other,
        }
    }
}

impl core::fmt::Display for VectorDatatype {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Point identifier: unsigned integer or unique string.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PointId {
    /// Unsigned 64-bit integer ID.
    Number(u64),
    /// Arbitrary unique string ID.
    String(String),
    /// Parameter placeholder (`:name`).
    Param(
        String,
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "Option::is_none")
        )]
        Option<alloc::boxed::Box<crate::error::Span>>,
    ),
    /// Positional parameter placeholder (`?`).
    PositionalParam(
        usize,
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "Option::is_none")
        )]
        Option<alloc::boxed::Box<crate::error::Span>>,
    ),
}

impl PointId {
    /// Construct an unlocated named parameter placeholder.
    pub fn param(name: impl Into<String>) -> Self {
        Self::Param(name.into(), None)
    }

    /// Construct a located named parameter placeholder.
    pub fn param_with_span(name: impl Into<String>, span: crate::error::Span) -> Self {
        Self::Param(name.into(), Some(alloc::boxed::Box::new(span)))
    }

    /// Construct an unlocated positional parameter placeholder.
    pub fn positional_param(idx: usize) -> Self {
        Self::PositionalParam(idx, None)
    }

    /// Construct a located positional parameter placeholder.
    pub fn positional_param_with_span(idx: usize, span: crate::error::Span) -> Self {
        Self::PositionalParam(idx, Some(alloc::boxed::Box::new(span)))
    }

    /// Extract the parameter source span if present.
    pub fn param_span(&self) -> Option<crate::error::Span> {
        match self {
            Self::Param(_, span) | Self::PositionalParam(_, span) => span.as_deref().copied(),
            _ => None,
        }
    }
}

/// A vector value: dense, sparse, or multivector.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum VectorValue {
    /// Single dense vector.
    Dense(Vec<f32>),
    /// Sparse vector — `indices` positions paired with `values` weights.
    Sparse {
        /// Row positions of the non-zero entries.
        indices: Vec<u32>,
        /// Weights at the indexed rows.
        values: Vec<f32>,
    },
    /// Multivector bag of dense vectors (ColBERT-style MaxSim).
    MultiDense(Vec<Vec<f32>>),
}

/// Vector payload attached to an upsert point.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PointVectors {
    /// One unnamed vector value.
    Unnamed(VectorValue),
    /// Vector values keyed by vector name.
    Named(Vec<(String, VectorValue)>),
}

/// Explicit `AS` role for a `USING` vector target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum VectorKind {
    /// Single dense embedding.
    Dense,
    /// Sparse (e.g. BM25) embedding.
    Sparse,
}

/// `USING <name> [AS <kind>]` resolution target.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct VectorTarget {
    /// Named vector to embed into.
    pub name: String,
    /// Explicit `AS DENSE` / `AS SPARSE` role; schema-resolved when `None`.
    pub kind: Option<VectorKind>,
    /// Multivector (ColBERT-style) dense target. Filled at parse only via
    /// `AS MULTI`, or at execution prep from collection schema
    /// (`multivector_config`). Not a third `VectorKind` — still dense.
    #[cfg_attr(feature = "serde", serde(default))]
    pub multi: bool,
}

/// `ORDER BY` sort direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum OrderDirection {
    /// `ASC` — ascending (the default).
    Asc,
    /// `DESC` — descending.
    Desc,
}

/// Read consistency for Qdrant point reads.
///
/// OpenAPI `ReadConsistency` / proto `ReadConsistency`: either a replica
/// **factor** `N`, or a named mode (`majority` / `quorum` / `all`).
/// REST: query param on `/points/query` etc. gRPC: `read_consistency` field.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ReadConsistency {
    /// Send requests to N nodes; keep points present on all of them.
    Factor(u64),
    /// N/2+1 random requests; points present on all of them.
    Majority,
    /// All nodes; points present on a majority.
    Quorum,
    /// All nodes; points present on all of them.
    All,
}

/// `WITH PAYLOAD` payload projection selector.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PayloadSelector {
    /// `true` — return all payload fields.
    All,
    /// `false` — return no payload.
    None,
    /// `INCLUDE (fields)` — only the listed fields.
    Include(Vec<String>),
    /// `EXCLUDE (fields)` — everything except the listed fields.
    Exclude(Vec<String>),
}

/// `WITH VECTOR` vector projection selector.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum VectorSelector {
    /// `true` — return all vectors.
    All,
    /// `false` — return no vectors.
    None,
    /// Return only the named vectors.
    Names(Vec<String>),
}

/// Distance metric of a `VECTOR(size, distance)` definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum VectorDistance {
    /// `COSINE` — cosine similarity distance.
    Cosine,
    /// `DOT` — dot product distance.
    Dot,
    /// `EUCLID` — Euclidean distance.
    Euclid,
    /// `MANHATTAN` — Manhattan (L1) distance.
    Manhattan,
}

/// Comparator for multivector (late-interaction) scoring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MultivectorComparator {
    /// `max_sim` — maximum pairwise vector similarity.
    MaxSim,
}

/// `WITH QUANTIZATION (type = …)` quantization family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum QuantizationType {
    /// `scalar` — scalar (int8-style) quantization.
    Scalar,
    /// `binary` — binary quantization.
    Binary,
    /// `product` — product quantization.
    Product,
    /// `turbo` — Turbo quantization (`bits` 1, 1.5, 2, or 4).
    Turbo,
}
